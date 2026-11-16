//! [re:81dd8e] ([re:721fab] .. [re:d1b1f2]): tuned feedback comb with a 4-point
//! cubic fractional delay read, soft-clipped write and a damping one-pole in
//! the loop. Two flavours (`type < 0.5` / `>= 0.5`): negative feedback at an
//! octave up, or positive feedback at the pitch with a cutoff-tracking damper.

#[derive(Debug, Clone)]
pub struct Comb {
    inv_sr: f32,
    sr: f32,
    buf: Vec<f32>,
    mask: usize,
    max_delay: f32,
    write: usize,
    smooth_coef: f32,
    damp_coef: f32,
    positive: bool,
    octave: f32,
    res: f32,
    delay: f32,
    feedback: f32,
    dry: f32,
    wet: f32,
    delay_smooth: f32,
    damp: f32,
    denormal: f32,
    /// The delay buffer still holds a previous note's samples: cleared at
    /// the first `process`, so note starts that never use the comb do not
    /// zero its 0.2 s buffer.
    stale: bool,
}

impl Comb {
    pub fn new(sample_rate: f32) -> Comb {
        let mut c = Comb::blank();
        c.set_sample_rate(sample_rate);
        c
    }

    /// Back to the state after `new`, keeping the delay buffer (no
    /// allocation: voices reset their filters at every note start). The
    /// buffer is cleared when the comb next runs.
    pub fn reset(&mut self) {
        let (buf, sr) = (std::mem::take(&mut self.buf), self.sr);
        *self = Comb::blank();
        self.buf = buf;
        self.set_rate(sr);
        self.stale = true;
    }

    fn blank() -> Comb {
        Comb {
            inv_sr: 0.0,
            sr: 0.0,
            buf: Vec::new(),
            mask: 0,
            max_delay: 0.0,
            write: 0,
            smooth_coef: 0.0,
            damp_coef: 0.0,
            positive: true,
            octave: 0.0,
            res: 0.0,
            delay: 1.0,
            feedback: 0.0,
            dry: 1.0,
            wet: 1.0,
            delay_smooth: 1.0,
            damp: 0.0,
            denormal: 1e-8,
            stale: false,
        }
    }

    /// Sample-rate setup [re:07aa89]: power-of-two buffer of at least 0.2 s.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.set_rate(sr);
        let len = self.mask + 1;
        self.buf.clear();
        self.buf.resize(len, 0.0);
        self.stale = false;
    }

    /// The rate's constants and buffer length, without touching the buffer.
    fn set_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.inv_sr = 1.0 / sr;
        let want = ((sr * 0.2 - 2.0).abs() + sr * 0.2 + 2.0) * 0.5;
        let want = want.round() as i64;
        let mut len = 1usize;
        if want > 1 {
            let mut v = (want - 1) as u64;
            while v != 0 {
                len *= 2;
                v >>= 1;
            }
        }
        self.mask = len - 1;
        self.max_delay = (len as i32 - 2) as f32;
        let a = self.inv_sr * 376.99112;
        self.smooth_coef = ((a + 1.0) - (a - 1.0).abs()) * 0.5;
        let b = self.inv_sr * 125.66371;
        self.damp_coef = ((b + 1.0) - (b - 1.0).abs()) * 0.5;
    }

    /// Setup [re:af406e].
    pub fn initialize(&mut self, cutoff: f32, res: f32, mode: f32) {
        self.positive = 0.5 <= mode;
        let c = cutoff * 12.0;
        self.octave = (c.abs() - (c - 12.0).abs()) * 0.5;
        self.res = res * 1.01;
    }

    /// `Process` [re:d1b1f2] (`inv_n` = 1/len, capped at 1/32).
    pub fn process(&mut self, inp: &[f32], out: &mut [f32], inv_n: f32) {
        if std::mem::take(&mut self.stale) {
            self.buf.fill(0.0);
        }
        let r = (self.res.abs() + self.res) * 0.5;
        let r2 = r * r;
        let fb_amount = r2 * 1.025;
        // Inverse square root of (1 + 1.025·r²): bit-trick estimate + one Newton step.
        let bits = (fb_amount + 1.0).to_bits();
        let y0 = f32::from_bits(0xbe6a3000u32.wrapping_sub(bits) >> 1);
        let y = (y0 * y0 * (r2 * -0.5125 - 0.5) + 1.5) * y0;
        let sign = (((bits as i32) >> 30) | 1) as f32;
        let norm = y * y * sign;
        let d = self.denormal;
        self.denormal = -d;
        self.delay_smooth -= d;
        let w = self.mask & self.write;
        self.buf[w] -= d;
        self.damp -= d;
        let p = inv_n.min(0.03125);
        let up = crate::fx::exp2(-self.octave);
        let (delay_t, fb_t, dry_t, wet_t, damp_k) = if self.positive {
            let fc = crate::fx::exp2(self.octave - 2.0) * 440.0 * self.inv_sr;
            (up * 0.0022727272 * self.sr, fb_amount, norm * norm * 0.5, norm * 0.5, Some(fc))
        } else {
            (up * 0.0011363636 * self.sr, -fb_amount, norm * norm * 0.5, norm * -0.5, None)
        };
        let d_delay = delay_t - self.delay;
        let d_fb = fb_t - self.feedback;
        let d_dry = dry_t - self.dry;
        let d_wet = wet_t - self.wet;
        let m = self.mask;
        for (x, o) in inp.iter().zip(out.iter_mut()) {
            self.delay += p * 0.08 * d_delay;
            self.dry += p * d_dry;
            self.wet += p * d_wet;
            self.delay_smooth = (self.delay - self.delay_smooth) * self.smooth_coef + self.delay_smooth;
            self.feedback += p * d_fb;
            self.write = (self.write + 1) & m;
            let w = self.write;
            let xin = ((x + 2.0).abs() - (x - 2.0).abs()) * 0.5;
            let dl = self.delay;
            let pos = (w as i32 as f32 + -2.0) + ((dl - self.max_delay).abs() - (dl.abs() + self.max_delay)) * 0.5;
            let fl = pos.floor() as i32;
            let t = pos - fl as f32;
            let at = |k: i32| self.buf[(k as u32 as usize) & m];
            let (a, b, c, e) = (at(fl - 1), at(fl), at(fl + 1), at(fl + 2));
            let yv = ((((b - c) * 0.5 + (e - a) * 0.16666667) * t + ((c + a) * 0.5 - b)) * t + (((e * -0.16666667 + a * -0.33333334) + b * -0.5) + c)) * t + b;
            let hp = yv - self.damp;
            let coef = match damp_k {
                Some(fc) => ((fc.abs() + 0.25) - (fc - 0.25).abs()) * 0.5,
                None => self.damp_coef,
            };
            self.damp = coef * hp + self.damp;
            *o = self.wet * yv + self.dry * self.buf[(w.wrapping_sub(2)) & m];
            let v = hp * self.feedback + ((xin.abs() * 0.0625 + -0.25) * xin * xin + 1.0) * xin;
            let c2 = (v + 1.5).abs() - (v - 1.5).abs();
            let c1 = c2 * 0.5;
            self.buf[w] = c1 * c1 * c2 * -0.074074075 + c1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resonates_and_stays_bounded() {
        let mut c = Comb::new(44100.0);
        c.initialize(0.5, 1.0, 1.0);
        let mut x = vec![0.0f32; 8192];
        x[0] = 1.0;
        let mut y = vec![0.0; 8192];
        for (a, b) in x.chunks(64).zip(y.chunks_mut(64)) {
            c.process(a, b, 1.0 / 64.0);
        }
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 4.0));
        assert!(y[2000..].iter().any(|v| v.abs() > 1e-4), "impulse should ring");
    }
}
