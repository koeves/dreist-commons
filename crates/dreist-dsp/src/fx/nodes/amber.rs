//! [re:3a3c4e] (Amber Chorus; `InitializePorts` [re:31e04c] builds the
//! LFO tables, `Voice::SetMode` [re:40c94f], `ProcessBuffer` [re:18487c]): a
//! BBD-style multi-tap chorus with six modes. Each tap reads a mono delay
//! line (fed by the summed input, plus feedback in some modes) through a
//! cubic interpolator, then a DC blocker, a low-pass, an optional "bright"
//! bypass and a 2-pole state-variable low-pass, and is panned by Spread.
//!
//! The original adds analogue noise to the LFOs and the delay input from
//! [re:4c4947]/`+0xe0`, but nothing in the binary ever writes those
//! buffers (they are only zeroed), so the noise terms are zero here.

use std::sync::OnceLock;

use super::math::{cubic, pow2_at_least};
use crate::analog::sin_tab;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// `SolveDiodeShaperOmni(x, y)` [re:93f883]: Newton solve of a diode pair
/// clipper (`y` starts at `x`, ≤ 10 steps, 0.1 % tolerance).
fn diode_shaper(x: f32) -> f32 {
    let vcrit1 = f32::from_bits(0x3ef8eb1f);
    let vcrit2 = f32::from_bits(0x3efbb791);
    let mut y = x;
    let mut it = 1u32;
    loop {
        let a = if vcrit2 < -y { vcrit2 } else { 0.0 - y };
        let b = if vcrit1 < y { vcrit1 } else { y };
        let ga = (a / 0.04175).exp() * 2.27e-7;
        let gb = (b / 0.04125).exp() * 2.22e-7;
        let da = ga / 0.04175;
        let db = gb / 0.04125;
        let f7 = (gb + -2.22e-7) - b * db;
        let ny = ((((ga + -2.27e-7) - a * da) - f7) + x * 0.00025641025) / (db + da + 0.00025641025 + 0.001);
        if 9 < it {
            return ny;
        }
        let d = y - ny;
        it += 1;
        y = ny;
        if (ny * 0.001).abs() >= d.abs() {
            return y;
        }
    }
}

/// The six [re:5c92ec] tables (1025 entries, domain [0, 1)),
/// normalised the way the original does it: `(v + min)/(max − min)`.
fn tables() -> &'static [Vec<f32>; 6] {
    static T: OnceLock<[Vec<f32>; 6]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t: [Vec<f32>; 6] = Default::default();
        for v in t.iter_mut() {
            v.resize(1025, 0.0);
        }
        for l in 0..1024usize {
            let x = l as i32 as f32 * 0.0009765625;
            let tri = if x < 0.5 { x + x } else { 2.0 - (x + x) };
            let y = diode_shaper(tri * 8.2 + -4.31);
            t[0][l] = y;
            t[1][l] = y;
            t[2][l] = (x * 6.2831855).sin();
            t[3][l] = tri;
            let w = if x + -1.0 < 0.0 { x + 1.0 } else { x };
            let s = (w * 6.2831855).sin() * 0.5 + 0.5;
            t[4][l] = s;
            t[5][l] = s;
        }
        for v in t.iter_mut() {
            let (mut lo, mut hi) = (v[0], v[0]);
            for &e in &v[1..1024] {
                if e <= lo {
                    lo = e;
                }
                if hi <= e {
                    hi = e;
                }
            }
            let inv = 1.0 / (hi - lo);
            for e in v[..1024].iter_mut() {
                *e = (*e + lo) * inv;
            }
            v[1024] = v[0];
        }
        t
    })
}

/// [re:b8960c] (domain [0, 1], so the shared
/// `vdiff_inv_` is 1).
#[inline]
fn lut(t: &[f32], x: f32) -> f32 {
    let v = x * 1024.0;
    let v = if v < -1024.0 { -1024.0 } else { v.min(1024.0) };
    let i = v.floor() as i32;
    let k = i.clamp(0, 1023) as usize;
    (t[k + 1] - t[k]) * (v - i as f32) + t[k]
}

#[derive(Debug, Clone, Copy, Default)]
struct Tap {
    delay: f32,
    dc: f32,
    lp: f32,
    lp2: f32,
    band: f32,
    low: f32,
}

pub struct Amber {
    ports: Vec<Port>,
    sr: f32,
    active: bool,
    mode: i32,
    buf: Vec<f32>,
    mask: u32,
    idx: u32,
    phase: [f32; 6],
    /// 0x19c DC blocker, 0x1a0 low-pass, 0x1a4 mode-3 second low-pass.
    k_dc: f32,
    k_lp: f32,
    k_lp3: f32,
    svf_a: f32,
    svf_c: f32,
    k_delay: [f32; 3],
    base: [f32; 3],
    depth: [f32; 3],
    taps: [Tap; 3],
    alt: f32,
}

impl Amber {
    pub fn new() -> Amber {
        // Built here rather than on the audio thread.
        tables();
        let mut ports = ports_from(ports::AMBER_CHORUS);
        // Mode: int ctor over [re:292e24] (6 items).
        ports[1] = Port::new(0.0, 0.0, 5.0);
        Amber {
            ports,
            sr: 0.0,
            active: false,
            mode: -1,
            buf: Vec::new(),
            mask: 0,
            idx: 0,
            phase: [0.0; 6],
            k_dc: 0.0,
            k_lp: 0.0,
            k_lp3: 0.0,
            svf_a: 0.0,
            svf_c: 0.0,
            k_delay: [0.0; 3],
            base: [0.0; 3],
            depth: [0.0; 3],
            taps: [Tap::default(); 3],
            alt: super::ssl_filter::DENORMAL,
        }
    }

    fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        // The mode's coefficients depend on the rate: recompute them.
        self.mode = -1;
        let m = 384000.0f32 * 0.015;
        let n = (((m - 2.0).abs() + m + 2.0) * 0.5).floor() as i32;
        let size = pow2_at_least(n as i64);
        self.buf = vec![0.0; size];
        self.mask = size as u32 - 1;
    }

    /// `SetMode`: coefficients, tap delays and the starting smoothed delays.
    fn set_mode(&mut self, m: i32) {
        if self.mode == m {
            return;
        }
        self.mode = m;
        let inv = 1.0 / self.sr;
        let sr = self.sr;
        let k = |w: f32| 1.0 - (inv * w).exp();
        let svf = |s: &mut Amber| {
            let f = (inv * 9000.0).min(0.25);
            let x = f * 0.5 * 2048.0;
            let i = x.floor() as i32;
            let t = sin_tab();
            let j = (1024 + i) as usize;
            let v = (t[j + 1] - t[j]) * (x - i as f32) + t[j];
            let v = v + v;
            let c = v * 1.41421 * v * 0.16666667;
            s.svf_a = v - c;
            s.svf_c = 1.41421 - c;
        };
        let tab = &tables()[m as usize];
        let ph = self.phase;
        match m {
            2 => {
                self.k_delay = [k(-150.16814); 3];
                self.base = [sr * 0.00281, sr * 0.00279, sr * 0.00272];
                self.depth = [sr * 10.001, sr * 10.0, sr * 10.002];
                self.k_dc = k(-30.159292);
                self.k_lp = k(-18849.557);
                svf(self);
                for j in 0..3 {
                    let v = lut(tab, ph[j]) * 0.050896354 + lut(tab, ph[3 + j]) * 0.0045833117 + 0.075858004;
                    self.taps[j].delay = v * v * v * self.depth[j] + self.base[j];
                }
            }
            3 => {
                self.k_delay[0] = k(-150.16814);
                self.k_delay[1] = k(-150.16814);
                self.k_dc = k(-30.159292);
                self.k_lp = k(-31415.928);
                self.k_lp3 = k(-62203.535);
                svf(self);
                self.base[0] = sr * 0.00181954;
                self.base[1] = sr * 0.00146959;
                self.depth[0] = sr * 0.0036749;
                self.depth[1] = sr * 0.0036395302;
                self.taps[0].delay = sr * 0.00181954 + sr * 0.0036749 * lut(tab, ph[0]);
                self.taps[1].delay = self.base[1] + self.depth[1] * lut(tab, ph[1]);
            }
            4 | 5 => {
                self.k_delay = [k(-370.07962), k(-147.02654), k(-370.07962)];
                self.k_dc = k(-30.159292);
                self.k_lp = k(-9424.778);
                let b = sr * 0.000281;
                self.base = [b; 3];
                self.depth = [sr * 10.0; 3];
                svf(self);
                for j in 0..3 {
                    let v = lut(tab, ph[j]) * 0.008 + lut(tab, ph[3 + j]) * 0.002;
                    self.taps[j].delay = v * self.depth[j] + self.base[j];
                }
            }
            _ => {
                self.k_delay = [k(-370.07962), k(-147.02654), k(-370.07962)];
                self.k_dc = k(-30.159292);
                self.k_lp = k(-9424.778);
                svf(self);
                self.base = [sr * 0.00241, sr * 0.00479, sr * 0.00272];
                self.depth = [sr * 0.00090999994, sr * 0.0005699997, sr * 0.0009900001];
                self.taps[0].delay = sr * 0.00090999994 * lut(tab, ph[0]) + sr * 0.00241;
                self.taps[1].delay = self.base[1] + self.depth[1] * lut(tab, ph[1]);
                self.taps[2].delay = self.base[2] + self.depth[2] * lut(tab, ph[2]);
            }
        }
    }

    /// One tap: smooth the delay toward `target`, read, DC-block, low-pass
    /// (twice in mode 3), "bright" bypass, SVF low-pass.
    #[inline]
    fn tap(&mut self, j: usize, target: f32, bright: f32, three: bool) -> f32 {
        let t = &mut self.taps[j];
        t.delay = (target - t.delay) * self.k_delay[j] + t.delay;
        let w = self.idx;
        let pos = w as i32 as f32 - t.delay;
        let c = cubic(&self.buf, self.mask, pos);
        let e = c - t.dc;
        t.dc = self.k_dc * e + t.dc;
        let be = bright * e;
        t.lp = (e - t.lp) * self.k_lp + t.lp;
        let mut x = t.lp;
        if three {
            t.lp2 = (t.lp - t.lp2) * self.k_lp3 + t.lp2;
            x = t.lp2;
        }
        let mix = x * (1.0 - bright) + be;
        let low = self.svf_a * t.band + t.low;
        t.low = low;
        let h = (mix - low) - self.svf_c * t.band;
        t.band = h * self.svf_a + t.band;
        low
    }
}

impl Default for Amber {
    fn default() -> Self {
        Amber::new()
    }
}

impl Node for Amber {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn prepare_rate(&mut self, sr: f32) {
        if self.sr != sr {
            self.prepare(sr);
        }
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.prepare(ctx.sample_rate);
        }
        let p = |i: usize| self.ports[i].value;
        let [ol, or] = out;
        if p(0) < 0.5 {
            if self.active {
                self.buf.fill(0.0);
            }
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
            self.active = false;
            return;
        }
        self.active = true;
        let m = p(1);
        let mode = (((m.abs() + 5.0) - (m - 5.0).abs()) * 0.5).round() as i32;
        let speed_p = p(2);
        let bright = if 0.5 <= p(4) { 1.0 } else { 0.0 };
        let spread = ((p(3).abs() + 1.0) - (p(3) - 1.0).abs()) * 0.5;
        let wet = p(5);
        self.set_mode(mode);
        let speed = ((speed_p.abs() + 2.0) - (speed_p - 2.0).abs()) * 0.5 * (1.0 / ctx.sample_rate);
        let d = self.alt;
        self.alt = -d;
        for t in &mut self.taps {
            t.delay -= d;
            t.dc -= d;
            t.lp -= d;
            t.lp2 -= d;
            t.band -= d;
            t.low -= d;
        }
        let dry = 1.0 - wet;
        let tab = &tables()[mode as usize];
        let n = inp[0].len();
        match mode {
            0 | 1 => {
                let w = [0.5 - spread * 0.5, spread * 0.5 + 0.5, 0.5];
                let inc = [speed * 4.78, speed * 5.99, speed * 1.214];
                for i in 0..n {
                    self.idx = self.idx.wrapping_add(1) & self.mask;
                    let sum = inp[0][i] + inp[1][i];
                    let (mut al, mut ar) = (0.0f32, 0.0f32);
                    for j in 0..3 {
                        let v = inc[j] + self.phase[j];
                        self.phase[j] = v - v.floor();
                        let target = self.base[j] + self.depth[j] * lut(tab, self.phase[j]);
                        let o = self.tap(j, target, bright, false);
                        al += o * w[j];
                        ar += o * (1.0 - w[j]);
                    }
                    ol[i] = inp[0][i] * dry + wet * al;
                    or[i] = inp[1][i] * dry + ar * wet;
                    let fb = if mode == 1 { ar * 0.2 + al * 0.2 } else { 0.0 };
                    let x = if mode == 1 { 0.0 + fb + sum * 0.5 * 0.9 } else { 0.0 + sum * 0.5 * 0.9 };
                    self.buf[self.idx as usize] = x;
                }
            }
            2 | 4 | 5 => {
                let w = [0.5 - spread * 0.5, 0.5, spread * 0.5 + 0.5];
                let (ra, rb, oa, ob) = if mode == 2 { (0.25, 5.7, [0.33333334, 0.68333334], [0.33333334, 0.69]) } else { (0.6, 4.0, [0.32333335, 0.66], [0.33, 0.65333337]) };
                for i in 0..n {
                    self.idx = self.idx.wrapping_add(1) & self.mask;
                    let sum = inp[0][i] + inp[1][i];
                    let a = if mode == 2 { 0.0 + speed * ra + self.phase[0] } else { self.phase[0] + speed * ra };
                    let b = if mode == 2 { 0.0 + speed * rb + self.phase[3] } else { self.phase[3] + speed * rb };
                    let raw_a = [a, a + oa[0], a + oa[1]];
                    let raw_b = [b, b + ob[0], b + ob[1]];
                    let (mut al, mut ar) = (0.0f32, 0.0f32);
                    for j in 0..3 {
                        self.phase[j] = raw_a[j] - raw_a[j].floor();
                        self.phase[3 + j] = raw_b[j] - raw_b[j].floor();
                        let (ta, tb) = (lut(tab, self.phase[j]), lut(tab, self.phase[3 + j]));
                        let target = if mode == 2 {
                            let v = ta * 0.050896354 + tb * 0.0045833117 + 0.075858004;
                            v * v * v * self.depth[j] + self.base[j]
                        } else {
                            (ta * 0.0004 + tb * 0.00015) * self.depth[j] + self.base[j]
                        };
                        let o = self.tap(j, target, bright, false);
                        if mode == 5 {
                            ar += o * w[j];
                            al += (1.0 - w[j]) * o;
                        } else {
                            al += o * w[j];
                            ar += (1.0 - w[j]) * o;
                        }
                    }
                    let (outl, outr) = if mode == 5 { (ar, al) } else { (al, ar) };
                    ol[i] = inp[0][i] * dry + outl * wet;
                    or[i] = inp[1][i] * dry + outr * wet;
                    let x = match mode {
                        2 => 0.0 + sum * 0.5 * 0.8,
                        4 => 0.0 + ar * 0.15 + al * 0.15 + sum * 0.5 * 0.8,
                        _ => 0.0 + al * 0.3 + ar * 0.3 + sum * 0.5 * 0.8,
                    };
                    self.buf[self.idx as usize] = x;
                }
            }
            _ => {
                let w = [0.5 - spread * 0.5, spread * 0.5 + 0.5];
                for i in 0..n {
                    self.idx = self.idx.wrapping_add(1) & self.mask;
                    let sum = inp[0][i] + inp[1][i];
                    let a = 0.0 + speed * 0.9547 + self.phase[0];
                    let raw = [a, a + 0.5];
                    let (mut al, mut ar) = (0.0f32, 0.0f32);
                    for j in 0..2 {
                        self.phase[j] = raw[j] - raw[j].floor();
                        let target = self.base[j] + self.depth[j] * lut(tab, self.phase[j]);
                        let o = self.tap(j, target, bright, true);
                        al += o * w[j];
                        ar += (1.0 - w[j]) * o;
                    }
                    ol[i] = inp[0][i] * dry + al * wet;
                    or[i] = inp[1][i] * dry + ar * wet;
                    self.buf[self.idx as usize] = 0.0 + sum * 0.5 * 0.577;
                }
            }
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.mode = -1;
        self.buf.fill(0.0);
        self.idx = 0;
        self.phase = [0.0; 6];
        self.taps = [Tap::default(); 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_normalised_like_the_original() {
        let t = tables();
        // Triangle and raised sine have min 0, so they stay in [0, 1].
        assert!(t[3].iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(t[4].iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v)));
        // Plain sine: (s − 1)/2 ∈ [−1, 0].
        assert!(t[2].iter().all(|v| (-1.0 - 1e-6..=1e-6).contains(v)));
        assert_eq!(t[0][1024], t[0][0]);
    }

    #[test]
    fn all_modes_run() {
        let ctx = Ctx::new(48000.0);
        for mode in 0..6 {
            let mut a = Amber::new();
            a.ports[1].value = mode as f32;
            a.ports[5].value = 1.0;
            let x: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
            let mut e = 0.0;
            for _ in 0..10 {
                let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
                a.process(&ctx, [&x, &x], [&mut l, &mut r]);
                assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "mode {mode}");
                e = l.iter().map(|v| v * v).sum::<f32>();
            }
            assert!(e > 1e-3, "mode {mode}: {e}");
        }
    }
}
