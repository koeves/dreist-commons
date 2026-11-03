//! [re:31606c] ([re:dde9bc] .. [re:70a7fa]), the `VintageSampler` synthesis mode's
//! output stage: 6× RBJ low-pass, 26 kHz sample-and-hold with 12-bit
//! quantization, 4× RBJ high-pass feeding a ring modulator (aliasing images,
//! mixed in at 0.2), then a 2-pole OTA ladder and a soft clipper.
//!
//! Filter corner frequencies are fixed fractions of the sample rate
//! (`13000/44100` and `10000/44100`), as in the original.

use crate::analog::{analog, lerp, lut_pos};

/// Direct-form-I biquad ([re:09aaff]).
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// RBJ coefficients as [re:f046e6] computes them (f64).
    fn design(w: f64, highpass: bool) -> Biquad {
        let (s, c) = (w.sin(), w.cos());
        let alpha = s * 0.7072135210037231;
        let inv = 1.0 / (alpha + 1.0);
        let (b0, b1) = if highpass {
            let b = (c + 1.0) * inv;
            ((b * 0.5) as f32, -(b as f32))
        } else {
            let b = (1.0 - c) * inv;
            ((b * 0.5) as f32, b as f32)
        };
        Biquad { b0, b1, b2: b0, a1: (c * -2.0 * inv) as f32, a2: (inv * (1.0 - alpha)) as f32, ..Default::default() }
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = (self.b2 * self.x2 + self.b1 * self.x1 + self.b0 * x) - (self.a2 * self.y2 + self.a1 * self.y1);
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// [re:d14f2a] ([re:900a9d]): OTA ladder whose output mix
/// selects the second pole, with a 10 Hz high-pass on input and output.
#[derive(Debug, Clone, Copy, Default)]
struct MoogOut {
    hp_coef: f32,
    cutoff: f32,
    x: f32,
    v: [f32; 4],
    o: [f32; 5],
    hp: f32,
    dc: f32,
    denormal: f32,
}

/// Same cutoff warp and resonance compensation as [re:711690].
fn warp(e: f32) -> f32 {
    (((e * 0.393442 + -0.131474) * e + 0.111105) * e + 0.991549) * e
}
fn res_comp(e: f32) -> f32 {
    ((e * -0.22 + -0.0408209) * e + -0.0635353) * e + 1.00175
}

impl MoogOut {
    fn new(sr: f32) -> MoogOut {
        let inv = 1.0 / sr;
        let e = (inv * -62.831856).exp();
        let f = ((inv * 13000.0).abs() + 1.0 - (inv * 13000.0 - 1.0).abs()) * 0.5;
        let g = (((f.abs() + 0.25) - (f - 0.25).abs()) * -std::f32::consts::PI).exp();
        MoogOut { hp_coef: 1.0 - e, cutoff: 1.0 - g, denormal: 1e-8, ..Default::default() }
    }

    fn ota(v: f32) -> f32 {
        lerp(&analog().otalim, lut_pos(v * 32.0))
    }

    fn process(&mut self, input: f32) -> f32 {
        let t = &analog().otalim;
        let d = self.denormal;
        self.denormal = -d;
        let old = self.v;
        for v in self.v.iter_mut() {
            *v -= d;
        }
        self.dc -= d;
        let e = self.cutoff;
        let g = warp(e);
        let a = g * 0.74;
        let b = g * 0.26;
        let hp_state = self.hp - d;
        let x = input - hp_state;
        self.hp = self.hp_coef * x + hp_state;
        let x = self.o[4] * -0.1 * res_comp(e) + x;
        self.x = x;
        let o1 = Self::ota(x);
        let p1 = self.o[0];
        self.o[0] = o1;
        let v2 = ((p1 * b + (old[0] - d)) - self.o[1] * g) + o1 * a;
        self.v[0] = v2;
        let o2 = Self::ota(v2);
        let p2 = self.o[1];
        self.o[1] = o2;
        let v3 = (((old[1] - d) + p2 * b) - self.o[2] * g) + o2 * a;
        self.v[1] = v3;
        let o3 = Self::ota(v3);
        let p3 = self.o[2];
        self.o[2] = o3;
        let v4 = (((old[2] - d) + p3 * b) - self.o[3] * g) + o3 * a;
        self.v[2] = v4;
        let o4 = Self::ota(v4);
        let p4 = self.o[3];
        self.o[3] = o4;
        let v5 = (((old[3] - d) - g * self.o[4]) + b * p4) + a * o4;
        self.v[3] = v5;
        let o5 = Self::ota(v5);
        self.o[4] = o5;
        // Output mix weights are [0, -0, 1, -0, 0] (constructor): the third OTA.
        let mix = ((((o5 * 0.0 + o3 * 1.0) + o1 * 0.0) + o2 * -0.0) + o4 * -0.0) * 32.0;
        let (i, fr) = lut_pos(mix);
        let y = (t[i + 1] - t[i]) * fr + (t[i] - self.dc);
        self.dc = self.hp_coef * y + self.dc;
        y
    }
}

/// [re:2e09a0] square (`tickSqrPwm`) as used by [re:eb0467] mode 3.
/// The original's band-limiting table [re:031640] lives in zero-filled
/// memory that nothing writes, so every BLEP correction is 0 and each edge
/// lands about 8 samples late through the voice-slot bookkeeping.
#[derive(Debug, Clone)]
struct SquareOsc {
    phase: f32,
    sign: f32,
    pw: f32,
    accum: f32,
    delay: [f32; 4],
    delay_pos: usize,
    last_fm: f32,
    /// Active edge slots, oldest first (at most 32): table index (advances
    /// by 16 per sample) and sign.
    slots: [(i32, f32); 32],
    live: usize,
}

impl SquareOsc {
    /// [re:719524]: reset and run 5 warm-up ticks.
    fn new(freq: f32) -> SquareOsc {
        let mut o = SquareOsc { phase: 0.0, sign: -1.0, pw: 0.5, accum: -0.5, delay: [0.5; 4], delay_pos: 0, last_fm: 0.0, slots: [(0, 0.0); 32], live: 0 };
        for _ in 0..5 {
            let inc = ((freq + 0.0 - o.last_fm + 1.0).abs() - (freq - o.last_fm - 1.0).abs()) * 0.5;
            o.last_fm = 0.0;
            o.tick(inc, 0.5);
        }
        o
    }

    fn edge(&mut self, frac: f32) {
        if self.live < 32 {
            let idx = (frac * 16.0).floor() as i32;
            self.slots[self.live] = (idx, self.sign);
            self.live += 1;
        }
    }

    /// Shared body of the 3- and 4-argument `tickSqrPwm` ([re:d08eff] / [re:fe8643]).
    fn tick(&mut self, inc: f32, pw: f32) -> f32 {
        let delayed = self.delay[self.delay_pos];
        self.delay[self.delay_pos] = pw;
        self.delay_pos = (self.delay_pos + 1) & 3;
        self.phase += inc;
        let mut th = if self.sign >= 0.0 {
            1.0 - self.pw
        } else {
            self.pw = pw;
            pw
        };
        while th <= self.phase {
            self.phase -= th;
            self.sign = -self.sign;
            let frac = self.phase / inc;
            self.edge(frac);
            if self.sign < 0.0 {
                self.pw = pw;
                th = pw;
            } else {
                th = 1.0 - self.pw;
            }
        }
        while self.phase < 0.0 {
            self.sign = -self.sign;
            let frac = self.phase / inc;
            self.edge(frac);
            self.phase += 1.0 - th;
            if self.sign < 0.0 {
                self.pw = pw;
                th = pw;
            } else {
                th = 1.0 - self.pw;
            }
        }
        // ms_step is all zeros: slots only matter for when `accum` flips.
        let mut kept = 0;
        for i in 0..self.live {
            let (idx, s) = self.slots[i];
            if idx > 0x7e {
                self.accum += s;
            } else {
                self.slots[kept] = (idx + 16, s);
                kept += 1;
            }
        }
        self.live = kept;
        delayed + 0.0 + self.accum + -0.5
    }
}

/// [re:eb0467] (mode 3 = square) followed by its [re:4e6c52].
#[derive(Debug, Clone)]
struct RingMod {
    osc: SquareOsc,
    inc: f32,
    target: f32,
    dc_coef: f32,
    dc: f32,
    denormal: f32,
}

impl RingMod {
    fn new(sr: f32, freq: f32) -> RingMod {
        let inv = 1.0 / sr;
        let f = ((freq * inv).abs() + 0.5 - (freq * inv - 0.5).abs()) * 0.5;
        let e = (inv * -62.831856).exp();
        RingMod { osc: SquareOsc::new(f), inc: 0.0, target: f, dc_coef: 1.0 - e, dc: 0.0, denormal: 1e-8 }
    }

    fn reset(&mut self) {
        self.inc = 0.0;
    }

    fn process(&mut self, x: f32) -> f32 {
        self.inc += (self.target - self.inc) * 1.0;
        let y = self.osc.tick(self.inc, 0.5) * x;
        // [re:cbe7e3].
        let s = self.dc - self.denormal;
        self.denormal = -self.denormal;
        let out = y - s;
        self.dc = self.dc_coef * out + s;
        out
    }
}

#[derive(Debug, Clone)]
struct Channel {
    pre: [Biquad; 6],
    post: [Biquad; 4],
    held: f32,
    moog: MoogOut,
    ring: RingMod,
}

#[derive(Debug, Clone)]
pub struct Sp1200 {
    sh_inc: f32,
    sh_phase: f32,
    ch: [Channel; 2],
}

impl Sp1200 {
    pub fn new(sr: f32) -> Sp1200 {
        let inv = 1.0 / sr;
        let ring_freq = sr * 0.29478458;
        let w_lp = inv as f64 * ring_freq as f64 * 6.2831854820251465;
        let w_hp = inv as f64 * ((sr * 0.22675738) as f64 * 6.2831854820251465);
        let ch = Channel { pre: [Biquad::design(w_lp, false); 6], post: [Biquad::design(w_hp, true); 4], held: 0.0, moog: MoogOut::new(sr), ring: RingMod::new(sr, ring_freq) };
        Sp1200 { sh_inc: (inv * 26000.0).min(1.0).max(0.0), sh_phase: 0.0, ch: [ch.clone(), ch] }
    }

    /// [re:7cb9c3].
    pub fn reset(&mut self) {
        self.sh_phase = 0.0;
        for c in &mut self.ch {
            c.pre.iter_mut().chain(c.post.iter_mut()).for_each(Biquad::reset);
            c.held = 0.0;
            c.ring.reset();
        }
    }

    fn finish(c: &mut Channel) -> f32 {
        let mut y = c.held;
        for f in &mut c.post {
            y = f.tick(y);
        }
        let r = c.ring.process(y);
        let m = c.moog.process(r * 0.2 + c.held * 0.8);
        let m = ((m * 1.5 + 2.0).abs() - (m * 1.5 + -2.0).abs()) * 0.5;
        ((m.abs() * 0.0625 + -0.25) * m * m + 1.0) * m
    }

    /// [re:e034fb] (stereo) or
    /// `Process(float)` [re:70a7fa] (mono, `right` = None).
    pub fn process(&mut self, left: f32, right: Option<f32>) -> (f32, f32) {
        let mut l = left;
        for f in &mut self.ch[0].pre {
            l = f.tick(l);
        }
        let mut r = right.unwrap_or(0.0);
        if right.is_some() {
            for f in &mut self.ch[1].pre {
                r = f.tick(r);
            }
        }
        self.sh_phase += self.sh_inc;
        if 1.0 <= self.sh_phase {
            self.sh_phase += -1.0;
            self.ch[0].held = (l * 4096.0).floor() * 0.000244141;
            if right.is_some() {
                self.ch[1].held = (r * 4096.0).floor() * 0.000244141;
            }
        }
        let ol = Self::finish(&mut self.ch[0]);
        let or = if right.is_some() { Self::finish(&mut self.ch[1]) } else { 0.0 };
        (ol, or)
    }
}

/// 12-bit quantize + 8-bit fixed-point linear interpolation used by
/// [re:7ca8fa] before the model.
pub fn vintage_tap(a: f32, b: f32, frac: f32) -> f32 {
    let q0 = ((a + 1.0) * 2047.5).floor() as i32 as u32 & 0xffff;
    let q1 = ((b + 1.0) * 2047.5).floor() as i32 as u32 & 0xffff;
    let u = (frac * 256.0) as i32 as u32 & 0xff;
    let v = ((q1.wrapping_sub(q0).wrapping_mul(u) >> 8).wrapping_add(q0)) & 0xffff;
    v as f32 * 0.0004884005 + -1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vintage_tap_quantizes() {
        assert!((vintage_tap(0.0, 0.0, 0.0) - 0.0).abs() < 0.001);
        assert!((vintage_tap(-1.0, 1.0, 0.5) - 0.0).abs() < 0.002);
        let s = vintage_tap(0.3, 0.3, 0.0);
        assert!((s - 0.3).abs() < 0.001);
    }

    #[test]
    fn model_passes_low_tones() {
        let sr = 44100.0;
        let mut m = Sp1200::new(sr);
        let mut acc = 0.0;
        for i in 0..8192 {
            let x = (i as f32 * 200.0 / sr * std::f32::consts::TAU).sin() * 0.5;
            let (y, _) = m.process(x, None);
            assert!(y.is_finite());
            if i > 4096 {
                acc += y * y;
            }
        }
        assert!((acc / 4096.0).sqrt() > 0.05);
    }
}
