//! [re:fdc386] ([re:e77da1]..[re:cf9835]): the 33-mode waveshaper
//! shared by Maul, Driver and Shaped Distortion, with its helpers
//! `MultiModeDistortion::*` ([re:b49436]..[re:84df0c]), [re:b019ad]
//! ([re:6a42db]..[re:d97c2a]) and the ring-mod oscillators ([re:c8aa5e]).
//!
//! [re:2e09a0] corrects its steps with [re:031640] and a ramp table, but
//! those tables are uninitialised and nothing in the original fills them, so
//! at runtime they are zero: the oscillators reduce to naive waveforms whose
//! steps land a few samples after the phase wraps. That is what is ported.

use super::blocks::Rng;
use super::math::sat;
use crate::analog::{analog, lut_pos, sin_tab, tri_tab};

/// Mode names ([re:d44a2f], 33 items).
pub const MODES: [&str; 33] = [
    "Thru", "Tanh", "Diode", "Op-amp", "JFET", "Transistor", "OTA", "Tube", "Diff", "Half Rect", "Tri", "RM - Sin", "RM - Tri", "RM - Saw", "RM - Sqr", "RM - White", "RM - Pink", "OD Asym",
    "OD Soft", "OD Shred", "OD Tannin", "Clip", "Clip Hard", "Clip Rect", "Clip Half", "Clip Half Rect", "Clip Full Rect", "Poly 1", "Poly 2", "Poly 3", "Poly 4", "Bit Redux", "SR Redux",
];

#[inline]
fn lut(t: &[f32], v: f32) -> f32 {
    let (i, f) = lut_pos(v);
    (t[i + 1] - t[i]) * f + t[i]
}

/// [re:faa14d].
fn hard_limit(x: f32, lo: f32, hi: f32) -> f32 {
    let v = x * 20.1;
    let r = if v <= hi {
        if -lo <= v {
            return x;
        }
        sat(v + lo) - lo
    } else {
        sat(v - hi) + hi
    };
    r * 0.049751244
}

fn overdrive_asym(x: f32) -> f32 {
    let f = x * 16.0;
    if 2.0 < f {
        return (3.0 - 1.0 / (f + 1.0 + -2.0)) * 0.0625;
    }
    let f = if f < -15.0 { -1.0 / (f + -1.0 + 15.0) + -16.0 } else { f };
    f * 0.0625
}

fn distortion_soft(x: f32) -> f32 {
    let s = if x < 0.0 { -1.0 } else { 1.0 };
    -1.0 / (x + s) + s
}

fn clip_hard(x: f32) -> f32 {
    let c = ((x + 1.5).abs() - (x - 1.5).abs()) * 0.5;
    c * -0.14814815 * c * c + c
}

fn half_rectify(x: f32, g: f32) -> f32 {
    let a = (g / 30.0).max(0.0);
    let a = (a - 1.0) * (a - 1.0);
    (((1.0 - a) + (a + x).abs()) - (x - 1.0).abs()) * 0.5
}

fn full_rectify(x: f32, g: f32) -> f32 {
    let a = (g / 30.0).max(0.0);
    let b = (1.0 - a) * (1.0 - a);
    let c = ((x + 1.0).abs() - (x - 1.0).abs()) * 0.5;
    c * b + c.abs() * (1.0 - b)
}

fn tri_shaper(x: f32) -> f32 {
    let t = tri_tab();
    let v = x * 0.5;
    let p = (v - v.floor()) * 2048.0;
    let i = p.floor() as i32;
    let k = (1024 + i) as usize;
    let r = (t[k + 1] - t[k]) * (p - i as f32) + t[k];
    r + r
}

fn hard_clip(x: f32) -> f32 {
    x.max(-1.0).min(1.0)
}

/// [re:b019ad] running state.
#[derive(Debug, Clone, Copy)]
struct TubeState {
    alt: f32,
    drive: [f32; 2],
    s_in: f32,
    s_out: f32,
    s_bias: f32,
    s_dc: f32,
}

impl Default for TubeState {
    fn default() -> Self {
        TubeState { alt: 1e-8, drive: [0.0; 2], s_in: 0.0, s_out: 0.0, s_bias: 0.0, s_dc: 0.0 }
    }
}

/// [re:b019ad]: triode stage from a load-line table solved at
/// sample-rate setup, with grid-bias feedback and output coupling.
pub struct Tube {
    table: Box<[f32; 1999]>,
    k_in: f32,
    k_out: f32,
    k_bias: f32,
    gain_bias: f32,
    k_dc: f32,
    sr: f32,
    st: TubeState,
}

impl Default for Tube {
    fn default() -> Self {
        Tube { table: Box::new([0.0; 1999]), k_in: 0.0, k_out: 0.0, k_bias: 0.0, gain_bias: 0.0, k_dc: 0.0, sr: 0.0, st: TubeState::default() }
    }
}

impl Tube {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.k_dc = 1.0 - ((1.0 / sr) * -62.831856).exp();
        self.k_bias = (-1.0 / (sr * 0.10610334)).exp();
        self.k_out = 1.0 - (-1.0 / (sr * 0.0012732396)).exp();
        let r = f32::from_bits(0x48f423fb);
        self.gain_bias = f32::from_bits(0x455ac007);
        let is = f32::from_bits(0x38d1b71a);
        self.k_in = 1.0 - (-1.0 / (sr * 8.499997e-7 * 60.0)).exp();
        let l = is.ln() * 0.16051364;
        for (n, i) in (-1000i32..999).enumerate() {
            let v = i as f32 / 100.0;
            let mut y = 1e-12f32;
            let mut it = 1u32;
            loop {
                let prev = y;
                let ny = prev - (((prev.ln() * 0.16051364 + r * prev) - l) - v) / (0.16051364 / prev + r);
                y = ny.max(1e-11);
                if !(it < 12) {
                    break;
                }
                it += 1;
                if (y - prev).abs() < y * 0.0001 {
                    break;
                }
            }
            let x = (v - r * y).max(-4.0).min(1.0);
            self.table[n] = x * -61.7825 + x * -1.5452 * x + x * 2.7351 * x * x + x * 0.321 * x * x * x + 121.3769;
        }
        self.reset();
    }

    /// `Reset`: one second of silence through the stage.
    pub fn reset(&mut self) {
        let n = self.sr as i32 as usize;
        if n == 0 {
            return;
        }
        let d = self.begin();
        let inc = self.ramp(1.0 / n as f32);
        for _ in 0..n {
            self.tick(0.0, inc);
        }
        self.end(d);
    }

    pub fn set_drive(&mut self, d: f32) {
        self.st.drive[1] = d;
    }

    fn lookup(&self, v: f32) -> f32 {
        let i = v.floor() as i32;
        if i < -999 {
            self.table[1]
        } else if i < 998 {
            let k = (i + 1000) as usize;
            (self.table[k + 1] - self.table[k]) * (v - i as f32) + self.table[k]
        } else {
            self.table[1998]
        }
    }

    /// Block start: the anti-denormal offset flips; returns it.
    fn begin(&mut self) -> f32 {
        let st = &mut self.st;
        let d = st.alt;
        st.alt = -d;
        st.s_in -= d;
        st.s_out -= d;
        st.s_bias -= d;
        st.s_dc -= d;
        d
    }

    fn end(&mut self, d: f32) {
        self.st.s_in += d;
        self.st.s_out += d;
        self.st.s_bias += d;
    }

    /// Per-sample drive increment for a block of 1/`inv_n` samples.
    fn ramp(&self, inv_n: f32) -> f32 {
        (self.st.drive[1] - self.st.drive[0]) * inv_n
    }

    #[inline]
    fn tick(&mut self, x: f32, inc: f32) -> f32 {
        let (k_in, k_bias, k_out, k_dc, gain_bias) = (self.k_in, self.k_bias, self.k_out, self.k_dc, self.gain_bias);
        let st = &mut self.st;
        st.drive[0] += inc;
        st.s_in = ((st.drive[0] * 1.6902758 + 0.46415886) * x - st.s_in) * k_in + st.s_in;
        let v = (st.s_in - st.s_bias) * 100.0;
        let t = self.lookup(v);
        let st = &mut self.st;
        st.s_bias = (1.0 - k_bias) * (0.0 - t) * 1e-5 * gain_bias + st.s_bias * k_bias;
        st.s_out = (t - st.s_out) * k_out + st.s_out;
        let y = (t - st.s_out) - st.s_dc;
        st.s_dc += k_dc * y;
        y
    }

    /// In place.
    pub fn process(&mut self, x: &mut [f32], inv_n: f32) {
        let d = self.begin();
        let inc = self.ramp(inv_n);
        for v in x.iter_mut() {
            *v = self.tick(*v, inc);
        }
        self.end(d);
    }
}

/// Step-event slot of a [re:2e09a0] (only its timing matters with the
/// zero step tables).
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    idx: i32,
    amp: f32,
}

/// [re:c8aa5e] reduced to what the original executes (see module docs).
#[derive(Debug, Clone, Copy)]
pub struct Osc {
    /// Live step events (at most 32), oldest first.
    slots: [Slot; 32],
    live: usize,
    ring: [f32; 4],
    ring_idx: usize,
    acc_saw: f32,
    acc: f32,
    slope: f32,
    prev: f32,
    phase_saw: f32,
    phase: f32,
    sign: f32,
    hp: f32,
    /// Pulse width and the width history ring (0.5s).
    pw: f32,
    ring_pw: [f32; 4],
}

impl Default for Osc {
    fn default() -> Self {
        Osc {
            slots: [Slot::default(); 32],
            live: 0,
            ring: [0.0; 4],
            ring_idx: 0,
            acc_saw: -0.5,
            acc: -0.5,
            slope: -1.0,
            prev: 0.0,
            phase_saw: 0.0,
            phase: 0.0,
            sign: -1.0,
            hp: 0.0,
            pw: 0.5,
            ring_pw: [0.5; 4],
        }
    }
}

impl Osc {
    /// Settling init: reset, then five settling ticks for the
    /// saw (0), square (1, via [`Self::tick_sqr_pwm`]) and triangle (3) shapes.
    pub fn init(&mut self, mode: u32, f: f32) {
        *self = Osc::default();
        let fc = ((f + 1.0).abs() - (f - 1.0).abs()) * 0.5;
        for _ in 0..5 {
            match mode {
                0 => {
                    self.tick_saw(fc);
                }
                1 => {
                    // A pulse tick (f, 0, 0.5) before the first transition only
                    // advances the phase.
                    self.phase += fc;
                }
                3 => {
                    self.tick_tri(f);
                }
                _ => {}
            }
        }
    }

    /// Settling init for the ring modulator's shapes: 0 saw, 1 pulse
    /// (pulse tick with (f, 0, 0.5)), 2 parabola (parabola tick with (f, 0)), 3 triangle;
    /// other modes only reset.
    pub fn init_exact(&mut self, mode: u32, f: f32) {
        match mode {
            1 => {
                *self = Osc::default();
                let fc = ((f + 1.0).abs() - (f - 1.0).abs()) * 0.5;
                for _ in 0..5 {
                    self.tick_sqr_pwm(fc, 0.5);
                }
            }
            2 => {
                *self = Osc::default();
                let fc = ((f.abs() + 1.0) - (f - 1.0).abs()) * 0.5;
                for _ in 0..5 {
                    self.tick_prb(fc);
                }
            }
            0 | 3 => self.init(mode, f),
            _ => *self = Osc::default(),
        }
    }

    /// Phase of the sine/noise readers that share the saw phase.
    pub fn phase_saw_mut(&mut self) -> &mut f32 {
        &mut self.phase_saw
    }

    /// Parabola tick [re:fae61a]: integrated saw, smoothed by the previous
    /// increment (×8 gain).
    pub fn tick_prb(&mut self, f: f32) -> f32 {
        let i = self.ring_idx;
        let old = self.ring[i];
        self.acc_saw += old;
        self.ring[i] = f;
        self.ring_idx = (i + 1) & 3;
        let p = self.phase_saw + f;
        self.phase_saw = p;
        if 1.0 <= p {
            self.phase_saw = p + -1.0;
            self.spawn((p + -1.0) / f, -1.0);
        } else if p < 0.0 {
            self.spawn(p / f, 1.0);
            self.phase_saw += 1.0;
        }
        let folded = self.age(126);
        self.acc_saw += folded;
        self.acc = ((0.0 + self.acc_saw) * 8.0 - self.acc) * old.abs() + self.acc;
        self.acc
    }

    /// Pulse tick [re:d08eff]: pulse with width `pw`.
    pub fn tick_sqr_pwm(&mut self, f: f32, pw: f32) -> f32 {
        let i = self.ring_idx;
        let old = self.ring_pw[i];
        self.ring_pw[i] = pw;
        self.ring_idx = (i + 1) & 3;
        self.phase += f;
        let mut th = if self.sign >= 0.0 {
            1.0 - self.pw
        } else {
            self.pw = pw;
            pw
        };
        let mut p = self.phase;
        while th <= p {
            self.phase = p - th;
            self.sign = -self.sign;
            let sgn = self.sign;
            self.spawn((p - th) / f, sgn);
            if sgn < 0.0 {
                self.pw = pw;
                th = pw;
            } else {
                th = 1.0 - self.pw;
            }
            p = self.phase;
        }
        while p < 0.0 {
            self.sign = -self.sign;
            let sgn = self.sign;
            self.spawn(p / f, sgn);
            self.phase = (1.0 - th) + self.phase;
            p = self.phase;
            if sgn < 0.0 {
                self.pw = pw;
                th = pw;
            } else {
                th = 1.0 - self.pw;
            }
        }
        let folded = self.age(126);
        self.acc += folded;
        old + 0.0 + self.acc + -0.5
    }

    /// Spawn a step event at fractional position `frac` (0..1).
    fn spawn(&mut self, frac: f32, amp: f32) {
        if self.live < 32 {
            self.slots[self.live] = Slot { idx: (frac * 16.0).floor() as i32, amp };
            self.live += 1;
        }
    }

    /// Keep the events for which `keep` holds, in order.
    fn retain(&mut self, mut keep: impl FnMut(&mut Slot) -> bool) {
        let mut n = 0;
        for i in 0..self.live {
            let mut s = self.slots[i];
            if keep(&mut s) {
                self.slots[n] = s;
                n += 1;
            }
        }
        self.live = n;
    }

    /// Age the events; those past `limit` are folded into the accumulator.
    fn age(&mut self, limit: i32) -> f32 {
        let mut folded = 0.0;
        self.retain(|s| {
            if s.idx > limit {
                folded += s.amp;
                false
            } else {
                s.idx += 16;
                true
            }
        });
        folded
    }

    pub fn tick_saw(&mut self, f: f32) -> f32 {
        let i = self.ring_idx;
        self.acc_saw += self.ring[i];
        self.ring[i] = f;
        self.ring_idx = (i + 1) & 3;
        let p = self.phase_saw + f;
        self.phase_saw = p;
        if 1.0 <= p {
            self.phase_saw = p - 1.0;
            self.spawn((p - 1.0) / f, -1.0);
        } else if p < 0.0 {
            self.spawn(p / f, 1.0);
            self.phase_saw += 1.0;
        }
        let folded = self.age(126);
        self.acc_saw += folded;
        self.acc_saw
    }

    pub fn tick_sqr(&mut self, f: f32) -> f32 {
        let f2 = f + f;
        let p = self.phase + f2;
        self.phase = p;
        if 1.0 <= p {
            self.phase = p - 1.0;
            self.sign = -self.sign;
            self.spawn((p - 1.0) / f2, self.sign);
        } else if p < 0.0 {
            self.sign = -self.sign;
            self.spawn(p / f2, self.sign);
            self.phase += 1.0;
        }
        let folded = self.age(126);
        self.acc += folded;
        self.acc
    }

    pub fn tick_tri(&mut self, f: f32) -> f32 {
        let f2 = ((f + f + 1.0).abs() - (f + f - 1.0).abs()) * 0.5;
        let i = self.ring_idx;
        let old = self.ring[i];
        self.ring[i] = f2;
        self.ring_idx = (i + 1) & 3;
        self.acc += self.slope * old;
        let p = self.phase + f2;
        self.phase = p;
        if 1.0 <= p {
            self.phase = p - 1.0;
            let s = self.sign;
            self.sign = -s;
            self.spawn((p - 1.0) / f2, s * -2.0);
        } else if p < 0.0 {
            let s = self.sign;
            self.sign = -s;
            self.spawn(p / f2, s * -2.0);
            self.phase += 1.0;
        }
        // Expiring triangle events flip the slope (their ramp integral is 0).
        let mut flips = 0;
        self.retain(|s| {
            if s.idx > 110 {
                flips += 1;
                false
            } else {
                s.idx += 16;
                true
            }
        });
        if flips & 1 == 1 {
            self.slope = -self.slope;
        }
        self.prev = old;
        let y = self.acc - self.hp;
        self.hp += old * 0.1 * y;
        y
    }
}

/// [re:fdc386].
pub struct MultiModeDistortion {
    sr: f32,
    inv_sr: f32,
    target: f32,
    drive: f32,
    mode: u32,
    hold: [f32; 2],
    sr_phase: f32,
    k_dc: f32,
    dc: [f32; 2],
    fresh: bool,
    osc_sin_phase: f32,
    osc_tri: Osc,
    osc_saw: Osc,
    osc_sqr: Osc,
    white: (f32, f32, Rng),
    pink: (f32, f32, Rng),
    tube: [Tube; 2],
}

impl Default for MultiModeDistortion {
    fn default() -> Self {
        MultiModeDistortion {
            sr: 44100.0,
            inv_sr: 1.0 / 44100.0,
            target: 0.0,
            drive: 0.0,
            mode: 1,
            hold: [0.0; 2],
            sr_phase: 0.0,
            k_dc: 0.0,
            dc: [0.0; 2],
            fresh: true,
            osc_sin_phase: 0.0,
            osc_tri: Osc::default(),
            osc_saw: Osc::default(),
            osc_sqr: Osc::default(),
            white: (0.0, -0.5, Rng::with_seed(0)),
            pink: (0.0, -0.5, Rng::with_seed(0)),
            tube: [Tube::default(), Tube::default()],
        }
    }
}

impl MultiModeDistortion {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.inv_sr = 1.0 / sr;
        self.k_dc = 1.0 - ((1.0 / sr) * -62.831856).exp();
        self.fresh = true;
        for t in &mut self.tube {
            t.set_sample_rate(sr);
        }
    }

    /// Setup with `(drive, type01)`.
    pub fn init(&mut self, drive: f32, type01: f32) {
        self.target = drive;
        let t = type01 * 32.0;
        let m = (((t.abs() + 32.0) - (t - 32.0).abs()) * 0.5).round() as u32;
        if self.mode != m {
            if m == 7 {
                for t in &mut self.tube {
                    t.reset();
                }
            }
            self.hold = [0.0; 2];
            self.sr_phase = 0.0;
            self.dc = [0.0; 2];
        }
        self.mode = m;
        if self.fresh {
            self.fresh = false;
            let f = (self.drive * 6.0 + -3.0).exp2() * self.inv_sr * 440.0;
            let f = ((f.abs() + 0.5) - (f - 0.5).abs()) * 0.5;
            self.osc_sin_phase = 0.0;
            self.osc_saw.init(0, f);
            self.osc_sqr.init(1, f);
            self.osc_tri.init(3, f);
            self.white = (0.0, -0.5, Rng::new());
            self.pink = (0.0, -0.5, Rng::new());
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 || self.mode > 32 {
            return;
        }
        let inv_n = 1.0 / n as f32;
        if self.mode == 0 {
            return;
        }
        if self.mode == 7 {
            let d = self.target;
            for (t, x) in self.tube.iter_mut().zip([&mut *l, &mut *r]) {
                t.set_drive(d);
                t.process(x, inv_n);
            }
            return;
        }
        let step = (self.target - self.drive) * inv_n;
        let a = analog();
        let mode = self.mode;
        for i in 0..n {
            self.drive += step;
            let d = self.drive;
            let (xl, xr) = (l[i], r[i]);
            let rm_f = || {
                let f = (d * 6.0 + -3.0).exp2() * (self.inv_sr * 440.0);
                ((f.abs() + 0.5) - (f - 0.5).abs()) * 0.5
            };
            let (yl, yr) = match mode {
                1 => {
                    let g = d * d * 39.0 + 1.0;
                    (sat(xl * g), sat(g * xr))
                }
                2..=6 => {
                    let (g, t, s): (f32, &[f32], f32) = match mode {
                        2 => (d * d * 4.0 + 1.0, &a.diodeonelim, 32.0),
                        3 => (d * d * 4.0 + 1.0, &a.mooglim, 32.0),
                        4 => (d * d * 3.0 + 1.0, &a.jfetlim, 32.0),
                        5 => (d * d * 4.0 + 1.0, &a.transisterlim, 32.0),
                        _ => (d * d * 4.0 + 1.0, &a.otalim_s, 1.0),
                    };
                    let vl = if s == 1.0 { xl * g } else { xl * g * 32.0 };
                    let vr = if s == 1.0 { g * xr } else { g * xr * 32.0 };
                    (lut(t, vl), lut(t, vr))
                }
                8 | 9 => {
                    let g = d * d * d * 19.0 + 1.0;
                    let lo = if mode == 8 { 19.0 } else { 1.0 };
                    (hard_limit(xl * g, lo, 19.0), hard_limit(g * xr, lo, 19.0))
                }
                10 => {
                    let g = (((d * d * d * 59.0 + 1.0) / 20.0) * 3.321928).exp2();
                    (tri_shaper(xl * g), tri_shaper(g * xr))
                }
                11 => {
                    let f = rm_f();
                    let mut p = self.osc_sin_phase + f;
                    if 1.0 <= p {
                        p += -1.0;
                    }
                    self.osc_sin_phase = p;
                    let x = p * 2048.0;
                    let k = x.floor() as i32;
                    let t = sin_tab();
                    let j = (1024 + k) as usize;
                    let s = (t[j + 1] - t[j]) * (x - k as f32) + t[j];
                    (xl * s, s * xr)
                }
                12..=14 => {
                    let f = rm_f();
                    let s = match mode {
                        12 => self.osc_tri.tick_tri(f),
                        13 => self.osc_saw.tick_saw(f),
                        _ => self.osc_sqr.tick_sqr(f),
                    };
                    (xl * s, s * xr)
                }
                15 | 16 => {
                    let f = rm_f();
                    let st = if mode == 15 { &mut self.white } else { &mut self.pink };
                    let p = f + f + st.0;
                    st.0 = p;
                    if 1.0 <= p {
                        st.0 = p - p.floor();
                        st.1 = if mode == 15 { st.2.white(-0.5, 0.5) } else { st.2.pink_raw() };
                    }
                    let s = st.1;
                    (xl * s, s * xr)
                }
                17 | 18 => {
                    let g = (((d * 59.0 + 1.0) / 20.0) * 3.321928).exp2();
                    if mode == 17 { (overdrive_asym(xl * g), overdrive_asym(g * xr)) } else { (distortion_soft(xl * g), distortion_soft(g * xr)) }
                }
                19 => {
                    let g = d * d * 0.4 * 5.812921 + 1.0;
                    let g = g * g * g;
                    (hard_clip(lut(&a.tanhlim, xl * g * 32.0)), hard_clip(lut(&a.tanhlim, g * xr * 32.0)))
                }
                20 => {
                    let g = d * d * 0.5 * 5.812921 + 1.0;
                    let g = g * g * g;
                    let tg = lut(&a.tanhlim, g * 32.0);
                    let f = |x: f32| {
                        let v = x * g;
                        let w = ((v - (v - lut(&a.tanhlim, v * 32.0)) * 3.0) / tg) * 32.0;
                        lut(&a.tanhlim, w)
                    };
                    (f(xl), f(xr))
                }
                21 => {
                    let g = d * d * d * 19.0 + 1.0;
                    let c = |v: f32| ((v + 1.0).abs() - (v - 1.0).abs()) * 0.5;
                    (c(xl * g), c(g * xr))
                }
                22 => {
                    let g = (((d * d * 59.0 + 1.0) / 60.0) * 3.321928).exp2();
                    (clip_hard(xl * g), clip_hard(g * xr))
                }
                23 => {
                    let g = d * d * d * 9.0 + 1.0;
                    let c = |v: f32| {
                        let a = v.abs();
                        ((a + 1.0) - (a - 1.0).abs()) * 0.5
                    };
                    (c(xl * g), c(g * xr))
                }
                24 => {
                    let g = d * d * d * 19.0 + 1.0;
                    let c = |v: f32| {
                        let h = ((v.abs() + 1.0) - (v - 1.0).abs()) * 0.5;
                        ((h + 1.0).abs() - (h - 1.0).abs()) * 0.5
                    };
                    (c(xl * g), c(g * xr))
                }
                25 | 26 => {
                    let g = (((d * 11.0 + 1.0) / 12.0) * 3.321928).exp2();
                    if mode == 25 { (half_rectify(xl * g, g), half_rectify(xr * g, g)) } else { (full_rectify(xl * g, g), full_rectify(xr * g, g)) }
                }
                27 => {
                    let g = d * d * 1.5 + 1.0;
                    let f = |x: f32| {
                        let v = x * g + 1e-8;
                        (1.0 / (v * 0.5)).sin() * (v * v)
                    };
                    (f(xl), f(xr))
                }
                28 => {
                    let g = d * d * 3.0 + 1.0;
                    let f = |x: f32| {
                        let v = x * g + 1e-8;
                        (1.0 / (v * 0.5)).sin() * v * 0.5 + v * 0.5
                    };
                    (f(xl), f(xr))
                }
                29 => {
                    let g = d * d * 2.5 + 1.0;
                    let f = |x: f32| {
                        let v = x * g;
                        (v * 20.0).sin() * (v * 0.2) + v * 0.8
                    };
                    (f(xl), f(xr))
                }
                30 => {
                    let g = d * d * 2.5 + 1.0;
                    let f = |x: f32| {
                        let v = x * g;
                        (v.powf(3.0) * 40.0).sin() * (v * 0.2) + v * 0.8
                    };
                    (f(xl), f(xr))
                }
                31 => {
                    let lv = ((1.0 - d).powf(2.5) * 16.0 + 2.0).exp2();
                    ((xl * lv).floor() / lv, (xr * lv).floor() / lv)
                }
                _ => {
                    let c = (1.0 - d).powf(3.0) * 0.99 + 0.01 + self.sr_phase;
                    self.sr_phase = c;
                    if 1.0 <= c {
                        self.sr_phase = 0.0;
                        self.hold = [xl, xr];
                    }
                    (self.hold[0], self.hold[1])
                }
            };
            let y0 = yl - self.dc[0];
            let y1 = yr - self.dc[1];
            self.dc[0] += self.k_dc * y0;
            self.dc[1] += self.k_dc * y1;
            l[i] = y0;
            r[i] = y1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_is_finite() {
        let sr = 48000.0;
        for m in 0..33 {
            let mut d = MultiModeDistortion::default();
            d.set_sample_rate(sr);
            for blk in 0..20 {
                d.init(0.7, m as f32 / 32.0);
                let mut l: Vec<f32> = (0..256).map(|i| ((blk * 256 + i) as f32 * 0.05).sin() * 0.8).collect();
                let mut r = l.clone();
                d.process(&mut l, &mut r);
                assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "mode {} {}", m, MODES[m]);
            }
        }
    }

    #[test]
    fn analog_tables_are_odd_ish() {
        let a = analog();
        // Transistor and diode-one tables pass small signals almost linearly.
        let mid = 1024;
        assert!(a.transisterlim[mid].abs() < 1e-3);
        assert!((a.transisterlim[mid + 1] - 0.03125).abs() < 0.01, "{}", a.transisterlim[mid + 1]);
        assert!(a.diodeonelim[mid].abs() < 1e-3);
        assert!(a.mooglim[mid].abs() < 1e-3 && a.mooglim[mid + 40] > 0.0);
    }
}
