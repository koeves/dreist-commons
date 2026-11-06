//! [re:667451] (Reverb; processing
//! [re:8c7936]) around [re:7ed3b6] ([re:56604a]..[re:f993c3]): input gain →
//! saturator → [re:8147ac] (16 taps per side, cross-fed) and a
//! diffusion stage → [re:ca5ee9] (figure-eight loop of four
//! [re:447ddc] blocks with shelving damping) → early/late mix → output
//! gain → saturator, then a ramped wet/dry mix.
//!
//! [re:f5417b] never advances its write index ([re:8d92b9]), so
//! its delayed reads are always zero and each of the four diffusers is a
//! plain gain of `0.5·(1 + g²)`; that is what the original runs.

use super::blocks::{Gain, Interp, Rng};
use super::math::pow2_at_least;
use super::math::sat;
use crate::analog::sin_tab;
use crate::fx::{Ctx, Node, Port, ports, ports_from, scratch};

const TIMES_L: [f32; 16] = [1000.0, 1580.0, 1750.0, 1930.0, 2000.0, 2180.0, 2150.0, 2280.0, 2240.0, 2340.0, 2275.0, 2380.0, 2320.0, 2415.0, 2345.0, 2432.0];
const TIMES_R: [f32; 16] = [1080.0, 1500.0, 1830.0, 1850.0, 2080.0, 2100.0, 2230.0, 2200.0, 2320.0, 2260.0, 2355.0, 2300.0, 2400.0, 2335.0, 2425.0, 2352.0];

/// [re:8147ac] ([re:20c535]..[re:b1d932]).
struct Early {
    sr: f32,
    buf: [Vec<f32>; 2],
    mask: u32,
    idx: u32,
    prev: [f32; 2],
    tgt: [[f32; 16]; 3],
    cur: [[f32; 16]; 3],
    taps: [[i32; 16]; 2],
    fb: [f32; 2],
}

impl Early {
    fn new() -> Early {
        Early { sr: 44100.0, buf: Default::default(), mask: 0, idx: 0, prev: [0.0; 2], tgt: [[0.0; 16]; 3], cur: [[0.0; 16]; 3], taps: [[-1; 16]; 2], fb: [0.0; 2] }
    }

    /// The original sizes the lines for 10 s; the taps reach at most
    /// 2432/44100 · 10 (Room Size 1) + 0.12 s (Predelay 1) ≈ 0.672 s, and
    /// reads are relative to the write index, so a power of two above 0.75 s
    /// plays the same.
    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        let n = pow2_at_least((sr * 0.75).ceil() as i64);
        self.buf = [vec![0.0; n], vec![0.0; n]];
        self.mask = n as u32 - 1;
        self.idx = 0;
        self.prev = [0.0; 2];
    }

    /// Setup with `(room, decay, _, predelay_ms, _)`.
    fn init(&mut self, room: f32, decay: f32, pre_ms: f32) {
        self.fb[1] = decay * 0.5;
        let s = room * 9.5 + 0.5;
        let sr = self.sr;
        let pre = pre_ms * 0.001 * sr;
        for k in 0..16 {
            let l = (TIMES_L[k] / 44100.0) * sr * s + pre;
            self.tgt[0][k] = l;
            self.tgt[1][k] = (TIMES_R[k] / 44100.0) * sr * s + pre;
            let g = ((1.0 - l / sr) * 0.5 + decay * 0.5).max(0.0).min(1.0);
            self.tgt[2][k] = g * 0.0625;
        }
    }

    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        let inv = 1.0 / n as f32;
        let mut d = [[0.0f32; 16]; 3];
        let mut stat = true;
        for k in 0..16 {
            for j in 0..3 {
                d[j][k] = (self.tgt[j][k] - self.cur[j][k]) * inv;
                if d[j][k].abs() >= 1e-6 {
                    stat = false;
                }
            }
        }
        let dfb = (self.fb[1] - self.fb[0]) * inv;
        let mask = self.mask;
        let [ol, or] = out;
        if self.taps[0][0] < 0 || !(stat && dfb.abs() < 1e-6) {
            for i in 0..n {
                self.fb[0] += dfb;
                self.idx = self.idx.wrapping_add(1) & mask;
                let w = self.idx as usize;
                self.buf[0][w] = self.fb[0] * self.prev[0] + inp[0][i];
                self.buf[1][w] = self.fb[0] * self.prev[1] + inp[1][i];
                let (mut ar, mut al) = (0.0f32, 0.0f32);
                for k in 0..16 {
                    self.cur[0][k] += d[0][k];
                    self.cur[1][k] += d[1][k];
                    self.cur[2][k] += d[2][k];
                    let g = self.cur[2][k];
                    let tl = self.cur[0][k].floor() as i32;
                    let tr = self.cur[1][k].floor() as i32;
                    self.taps[0][k] = tl;
                    self.taps[1][k] = tr;
                    ar += self.buf[1][(self.idx.wrapping_sub(tr as u32) & mask) as usize] * g;
                    al += self.buf[0][(self.idx.wrapping_sub(tl as u32) & mask) as usize] * g;
                }
                ol[i] = ar * 0.2 + al * 0.8;
                or[i] = al * 0.2 + ar * 0.8;
                self.prev = [ol[i], or[i]];
            }
        } else {
            for i in 0..n {
                self.idx = self.idx.wrapping_add(1) & mask;
                let w = self.idx as usize;
                self.buf[0][w] = self.prev[0] * self.fb[0] + inp[0][i];
                self.buf[1][w] = self.fb[0] * self.prev[1] + inp[1][i];
                let mut al = self.buf[0][(self.idx.wrapping_sub(self.taps[0][15] as u32) & mask) as usize] * self.cur[2][15];
                let mut ar = self.cur[2][15] * self.buf[1][(self.idx.wrapping_sub(self.taps[1][15] as u32) & mask) as usize];
                for k in (0..15).rev() {
                    al += self.buf[0][(self.idx.wrapping_sub(self.taps[0][k] as u32) & mask) as usize] * self.cur[2][k];
                    ar += self.cur[2][k] * self.buf[1][(self.idx.wrapping_sub(self.taps[1][k] as u32) & mask) as usize];
                }
                let al = al + 0.0;
                let ar = ar + 0.0;
                ol[i] = ar * 0.2 + al * 0.8;
                or[i] = al * 0.2 + ar * 0.8;
                self.prev = [ol[i], or[i]];
            }
        }
    }
}

/// [re:8fb2cb] ([re:a72cf6]..[re:2bc702]): RBJ shelf (Q 0.7072), frequency
/// and gain glide 1/32 per sample inside [re:793d92].
#[derive(Debug, Clone, Copy, Default)]
struct Shelf {
    sr: f32,
    c: [f32; 5],
    y: [f32; 2],
    x: [f32; 2],
    f: [f32; 2],
    g: [f32; 2],
    high: bool,
}

impl Shelf {
    fn update(&mut self) {
        let a = 10f64.powf(self.g[0] as f64 / 40.0);
        let w = (self.f[0] as f64 * std::f64::consts::TAU) / self.sr as f64;
        let (sn, cs) = (w.sin(), w.cos());
        let ap = a + 1.0;
        let am = a - 1.0;
        let amc = am * cs;
        let s = (a.sqrt() / 0.707213579) * sn;
        let (b0, b1, b2, a0, a1, a2);
        if !self.high {
            b0 = s + (ap - amc);
            b1 = (am - cs * ap) * (a + a);
            b2 = (ap - amc) - s;
            a0 = s + ap + amc;
            a1 = (cs * ap + am) * -2.0;
            a2 = (ap + amc) - s;
        } else {
            b0 = s + ap + amc;
            b1 = (am + cs * ap) * a * -2.0;
            b2 = (ap + amc) - s;
            a0 = s + (ap - amc);
            let t = am - cs * ap;
            a1 = t + t;
            a2 = (ap - amc) - s;
        }
        self.c = [(b0 * a / a0) as f32, (b1 / a0) as f32, (b2 * a / a0) as f32, (a1 / a0) as f32, (a2 / a0) as f32];
    }

    fn init(&mut self, f: f32, g: f32, high: bool, force: bool) {
        let fresh = self.f[0].abs() <= 1e-6 || self.g[0].abs() <= 1e-6;
        self.f[1] = f;
        self.g[1] = g;
        self.high = high;
        if fresh || force {
            self.f[0] = f;
            self.g[0] = g;
            self.update();
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let df = self.f[1] - self.f[0];
        let dg = self.g[1] - self.g[0];
        if df.abs() >= 1e-6 || dg.abs() >= 1e-6 {
            let sf = df * 0.03125;
            self.f[0] += sf;
            let sg = dg * 0.03125;
            self.g[0] += sg;
            self.update();
            if sf.abs() < 0.01 {
                self.f[0] = self.f[1];
            }
            if sg.abs() < 0.01 {
                self.g[0] = self.g[1];
            }
        }
        let c = self.c;
        let y = ((c[2] * self.x[1] + c[1] * self.x[0] + c[0] * x) - c[3] * self.y[0]) - c[4] * self.y[1];
        self.x[1] = self.x[0];
        self.x[0] = x;
        self.y[1] = self.y[0];
        self.y[0] = y;
        y
    }
}

#[derive(Debug, Clone, Default)]
struct Allpass {
    buf: Vec<f32>,
    mask: u32,
    idx: u32,
    delay: i32,
    coef: f32,
}

impl Allpass {
    fn alloc(&mut self, n: i32) {
        let s = pow2_at_least(n as i64);
        self.buf = vec![0.0; s];
        self.mask = s as u32 - 1;
        self.idx = 0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.idx = self.idx.wrapping_add(1) & self.mask;
        let w = self.idx;
        let v = self.buf[(w.wrapping_sub(self.delay as u32) & self.mask) as usize] - x * self.coef;
        self.buf[w as usize] = self.coef * v + x;
        v
    }
}

/// [re:447ddc] ([re:91c034]..[re:608080]): two LFO-modulated allpasses and an
/// interpolated delay.
#[derive(Debug, Clone, Default)]
struct ApDelay {
    ap: [Allpass; 2],
    base: [i32; 2],
    line: Vec<f32>,
    line_mask: u32,
    line_idx: u32,
    line_delay: f32,
    lfo_rate: f32,
    lfo_phase: f32,
    coef: f32,
}

impl ApDelay {
    /// The original sizes the allpasses and the line for 1 s; their delays
    /// reach at most 300 · 8 · 1.6 / 44100 s ≈ 0.088 s (+1 % modulation), so
    /// a power of two above 0.125 s plays the same.
    fn set_sample_rate(&mut self, sr: f32) {
        self.lfo_rate = Rng::new().white(0.9, 1.5) * (1.0 / sr);
        let n = (sr * 0.125).ceil() as i32;
        self.ap[0].alloc(n);
        self.ap[1].alloc(n);
        let s = pow2_at_least(n as i64);
        self.line = vec![0.0; s];
        self.line_mask = s as u32 - 1;
        self.line_idx = 0;
    }

    /// `UpdateDelayTime(n)`: advance the LFO a block and modulate ±1 %.
    fn update(&mut self, n: usize) {
        let mut p = self.lfo_phase;
        if n > 0 {
            for _ in 0..n {
                let v = p + self.lfo_rate;
                p = if 1.0 < v { v - 1.0 } else { v };
            }
            self.lfo_phase = p;
        }
        let t = sin_tab();
        let x = p * 2048.0;
        let i = x.floor() as i32;
        let f = x - i as f32;
        let k = (1024 + i) as usize;
        let s = (t[k + 1] - t[k]) * f + t[k] + 0.5;
        let s = ((s.abs() + 1.0) - (s - 1.0).abs()) * 0.5;
        let k = (1536 + i) as usize;
        let c = (t[k + 1] - t[k]) * f + t[k] + 0.5;
        let c = ((c.abs() + 1.0) - (c - 1.0).abs()) * 0.5;
        let d0 = self.base[0];
        let d1 = self.base[1];
        self.ap[0].delay = (d0 as f32 * 0.01 * (s + s + -1.0)) as i32 + d0;
        self.ap[1].delay = (d1 as f32 * 0.01 * (c + c + -1.0)) as i32 + d1;
        self.ap[0].coef = self.coef;
        self.ap[1].coef = self.coef;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let a = self.ap[0].tick(x);
        let b = self.ap[1].tick(a);
        self.line_idx = self.line_idx.wrapping_add(1) & self.line_mask;
        let w = self.line_idx;
        self.line[w as usize] = b;
        let pos = w as i32 as f32 - self.line_delay;
        let i = pos.floor() as i32 as u32;
        let fr = (w as i32 as f32 - self.line_delay) - i as i32 as f32;
        fr * self.line[(i.wrapping_add(1) & self.line_mask) as usize] + (1.0 - fr) * self.line[(i & self.line_mask) as usize]
    }
}

/// [re:ca5ee9] ([re:f7bfa7]..[re:84d92b]).
struct Henverb {
    sr: f32,
    apd: [ApDelay; 4],
    shelf: [Shelf; 4],
    size: f32,
    fb: f32,
    fb_tgt: f32,
    loop_out: f32,
    coef: f32,
}

impl Henverb {
    fn new() -> Henverb {
        Henverb { sr: 44100.0, apd: Default::default(), shelf: [Shelf::default(); 4], size: 0.0, fb: 0.5, fb_tgt: 0.5, loop_out: 0.0, coef: 0.5 }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        for a in &mut self.apd {
            a.set_sample_rate(sr);
        }
        for s in &mut self.shelf {
            s.sr = sr;
        }
        // Indices: 0, 1 LF; 2, 3 HF.
        self.shelf[2].init(1000.0, -2.0, true, true);
        self.shelf[3].init(1000.0, -2.0, true, true);
        self.shelf[0].init(30.0, -2.0, false, true);
        self.shelf[1].init(30.0, -2.0, false, true);
    }

    /// Setup with `(room, decay², damp_gain, damp_freq, density)`.
    fn init(&mut self, room: f32, fb: f32, damp_gain: f32, damp_freq: f32, coef: f32) {
        self.fb_tgt = fb;
        let size = room * 270.0 + 30.0;
        if (self.size - size).abs() >= 1e-6 || (self.coef - coef).abs() >= 1e-6 {
            self.size = size;
            let s = self.sr / 44100.0;
            let r = |v: f32| (if v < 0.0 { -0.5 } else { 0.5 } + v) as i32;
            self.coef = coef;
            for (a, m) in self.apd.iter_mut().zip([5.5f32, 8.0, 6.0, 7.5]) {
                let d = [r(size * m * s), r(size * 1.3 * m * s), r(size * 1.6 * m * s)];
                a.base = [d[0], d[1]];
                a.line_delay = d[2] as f32;
                a.coef = coef;
            }
        }
        let f = damp_freq * damp_freq * 19500.0 + 500.0;
        let g = damp_gain * 22.0 + -24.0;
        self.shelf[2].init(f, g, true, false);
        self.shelf[3].init(f, g, true, false);
        self.shelf[0].init(30.0, -2.0, false, true);
        self.shelf[1].init(30.0, -2.0, false, true);
    }

    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 4]) {
        let n = inp[0].len();
        let inv = 1.0 / n as f32;
        let start = self.fb;
        let tgt = self.fb_tgt;
        for a in &mut self.apd {
            a.update(n);
        }
        let [o0, o1, o2, o3] = out;
        let mut fb = self.fb;
        let mut lo = self.loop_out;
        for i in 0..n {
            fb += (tgt - start) * inv;
            self.fb = fb;
            let x = (inp[0][i] + inp[1][i]) * 0.5;
            lo += x;
            let t1 = self.apd[0].tick(lo);
            o0[i] = t1;
            let t2 = self.apd[1].tick(t1);
            o1[i] = t2;
            let y = self.shelf[2].tick(t2);
            let y = self.shelf[0].tick(y);
            let x2 = x + y * self.fb;
            let t3 = self.apd[2].tick(x2);
            o2[i] = t3;
            let t4 = self.apd[3].tick(t3);
            o3[i] = t4;
            let z = self.shelf[3].tick(t4);
            let z = self.shelf[1].tick(z);
            fb = self.fb;
            lo = z * fb;
            self.loop_out = lo;
        }
    }
}

pub struct FxpReverb {
    ports: Vec<Port>,
    sr: f32,
    gain_in: Gain,
    gain_out: Gain,
    early: Early,
    core: Henverb,
    density: f32,
    late: Interp,
    early_lvl: Interp,
    prev_wet: f32,
    work: Work,
}

/// Per-block work buffers: the gained input (then the wet output), the
/// early reflections, the diffused input and the late taps.
struct Work {
    x: [Vec<f32>; 2],
    early: [Vec<f32>; 2],
    diffused: [Vec<f32>; 2],
    late: [Vec<f32>; 4],
}

impl Work {
    fn new() -> Work {
        Work { x: [scratch(), scratch()], early: [scratch(), scratch()], diffused: [scratch(), scratch()], late: std::array::from_fn(|_| scratch()) }
    }

    fn size(&mut self, n: usize) {
        for b in self.x.iter_mut().chain(&mut self.early).chain(&mut self.diffused).chain(&mut self.late) {
            b.clear();
            b.resize(n, 0.0);
        }
    }
}

impl FxpReverb {
    pub fn new() -> FxpReverb {
        FxpReverb {
            ports: ports_from(ports::REVERB),
            sr: 0.0,
            gain_in: Gain::default(),
            gain_out: Gain::default(),
            early: Early::new(),
            core: Henverb::new(),
            density: 0.5,
            late: Interp::default(),
            early_lvl: Interp::default(),
            prev_wet: 0.0,
            work: Work::new(),
        }
    }
}

impl FxpReverb {
    /// Stream setup on a rate change.
    fn prepare(&mut self, sr: f32) {
        if self.sr != sr {
            self.sr = sr;
            self.core.set_sample_rate(sr);
            self.early.set_sample_rate(sr);
        }
    }
}

impl Default for FxpReverb {
    fn default() -> Self {
        FxpReverb::new()
    }
}

/// Vector wet/dry mix: weights ramp from the previous wet value
/// (4-lane accumulation for the first `n & !3` samples).
fn wet_dry(out: [&mut [f32]; 2], wet: [&[f32]; 2], dry: [&[f32]; 2], w0: f32, w1: f32) {
    let n = out[0].len();
    if n == 0 {
        return;
    }
    let d0 = 1.0 - w0;
    let dd = ((1.0 - w1) - d0) / n as f32;
    let dw = (w1 - w0) / n as f32;
    let [ol, or] = out;
    let n4 = n & !3;
    let mut lw = [dw * 0.0 + w0, dw + w0, dw + dw + w0, dw * 3.0 + w0];
    let mut ld = [dd * 0.0 + d0, d0 + dd, dd + dd + d0, dd * 3.0 + d0];
    let (sw, sd) = (dw * 4.0, dd * 4.0);
    let mut i = 0;
    while i < n4 {
        for k in 0..4 {
            ol[i + k] = dry[0][i + k] * ld[k] + wet[0][i + k] * lw[k];
            or[i + k] = ld[k] * dry[1][i + k] + lw[k] * wet[1][i + k];
            lw[k] += sw;
            ld[k] += sd;
        }
        i += 4;
    }
    if n4 < n {
        let mut d = d0 + (n4 as f32) * dd;
        let mut w = w0 + (n4 as f32) * dw;
        for j in n4..n {
            ol[j] = dry[0][j] * d + wet[0][j] * w;
            or[j] = dry[1][j] * d + w * wet[1][j];
            d += dd;
            w += dw;
        }
    }
}

impl Node for FxpReverb {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn prepare_rate(&mut self, sr: f32) {
        self.prepare(sr);
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.prepare(ctx.sample_rate);
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let p = |i: usize| self.ports[i].value;
        // [re:c59c29].
        self.density = p(3) * 0.39999998 + 0.3;
        self.gain_in.init(p(6) * 1.5835);
        self.gain_out.init(p(7) * 1.5835);
        self.core.init(p(1), p(2) * p(2), p(4), p(5), self.density);
        self.early.init(p(1), p(2), p(8) * p(8) * 119.9 + 0.1);
        self.late.target = p(10);
        self.early_lvl.target = p(9);
        let w = &mut self.work;
        w.size(n);
        let [x0, x1] = &mut w.x;
        x0.copy_from_slice(inp[0]);
        x1.copy_from_slice(inp[1]);
        self.gain_in.process(x0, x1);
        for v in x0.iter_mut().chain(x1.iter_mut()) {
            *v = sat(*v);
        }
        let [erl, err] = &mut w.early;
        self.early.process([x0, x1], [erl, err]);
        // Diffusers: two DualAPFilters per side reduce to gains.
        let g = self.density;
        let ap = |x: f32| {
            let h = x * 0.5;
            let a = 0.0 - h * g;
            let b = 0.0 - a * g;
            b + h
        };
        let [d0, d1] = &mut w.diffused;
        for i in 0..n {
            d0[i] = ap(ap(x0[i]));
            d1[i] = ap(ap(x1[i]));
        }
        let [t0, t1, t2, t3] = &mut w.late;
        self.core.process([d0, d1], [t0, t1, t2, t3]);
        let late = self.late.process(n);
        let early = self.early_lvl.process(n);
        // The wet signal reuses the input buffers.
        let (wl, wr) = (x0, x1);
        let n4 = n & !3;
        for i in 0..n {
            let (p5, p6) = (d0[i], d1[i]);
            let (p9, p10, p11, p12) = (t0[i], t1[i], t2[i], t3[i]);
            if i < n4 {
                wl[i] = erl[i] * early[i] + (p5 + p10 + p11 + p9) * late[i];
                wr[i] = err[i] * early[i] + (p6 + p12 + p11 + p9) * late[i];
            } else {
                wl[i] = erl[i] * early[i] + (p9 + p11 + p10 + p5) * late[i];
                wr[i] = err[i] * early[i] + (p9 + p11 + p12 + p6) * late[i];
            }
        }
        self.gain_out.process(wl, wr);
        for v in wl.iter_mut().chain(wr.iter_mut()) {
            *v = sat(*v);
        }
        let wet = p(0);
        wet_dry(out, [wl, wr], inp, self.prev_wet, wet);
        self.prev_wet = wet;
    }

    fn reset(&mut self) {
        self.prev_wet = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_rings_and_stays_finite() {
        let ctx = Ctx::new(48000.0);
        let mut r = FxpReverb::new();
        r.ports[0].value = 1.0;
        let n = 256;
        let mut late_energy = 0.0f32;
        for blk in 0..200 {
            let mut x = vec![0.0f32; n];
            if blk == 2 {
                x[0] = 1.0;
            }
            let (mut l, mut rr) = (vec![0.0; n], vec![0.0; n]);
            r.process(&ctx, [&x, &x], [&mut l, &mut rr]);
            assert!(l.iter().chain(rr.iter()).all(|v| v.is_finite()));
            if blk > 40 {
                late_energy += l.iter().map(|v| v * v).sum::<f32>();
            }
        }
        assert!(late_energy > 1e-8, "{late_energy}");
    }
}
