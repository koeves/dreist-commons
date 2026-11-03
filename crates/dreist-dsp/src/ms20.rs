//! [re:4480f1] ([re:20ee6f] .. [re:10e482]): 2-pole Sallen-Key style filter with a
//! diode clipper in the resonance loop and an OTA output stage. Mono; the
//! voice filter runs one per channel.

use crate::analog::{analog, lerp, lut_pos};

/// [re:e1a2d5] (`CreateFilterModeMap` [re:8b9ff3]).
pub const MS20_MODES: [&str; 4] = ["LPF", "BPF", "HPF", "Notch"];

#[derive(Debug, Clone)]
pub struct Ms20 {
    inv_sr: f32,
    dc_coef: f32,
    mode: u32,
    octave: f32,
    res: f32,
    y1: f32,
    y2: f32,
    fb_prev: f32,
    a: f32,
    b: f32,
    dc: f32,
    g: f32,
    denormal: f32,
}

impl Ms20 {
    pub fn new(sample_rate: f32) -> Ms20 {
        let mut f = Ms20 { inv_sr: 0.0, dc_coef: 0.0, mode: 0, octave: 0.0, res: 0.0, y1: 0.0, y2: 0.0, fb_prev: 0.0, a: 0.0, b: 0.0, dc: 0.0, g: 0.0, denormal: 1e-8 };
        f.set_sample_rate(sample_rate);
        f
    }

    /// `SetSampleRate` [re:8892c5]: 10 Hz DC blocker on the output.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.inv_sr = 1.0 / sr;
        let e = (self.inv_sr * -62.831856).exp();
        self.dc_coef = ((2.0 - e) - e.abs()) * 0.5;
    }

    /// `Initialize` [re:b4eb83].
    pub fn initialize(&mut self, cutoff: f32, res: f32, mode: f32) {
        self.mode = (mode * 3.0).round() as u32;
        self.octave = cutoff * 12.0 - 6.0;
        self.res = res * 1.01;
    }

    /// `Process` [re:10e482] (`inv_n` = 1/len).
    pub fn process(&mut self, inp: &[f32], out: &mut [f32], inv_n: f32) {
        let t = analog();
        let f = self.octave.exp2() * (self.inv_sr * 440.0);
        let e = (((f.abs() + 1.0) - (f - 1.0).abs()) * -std::f32::consts::PI).exp();
        let s = if self.res == 0.0 { 0.0 } else { self.res.sqrt() };
        let s = s + s;
        let m = (s + 16.0) - (s - 16.0).abs();
        let q = m * 0.125;
        self.a = ((q + 1.0) - (q - 1.0).abs()) * 0.5;
        self.b = 1.0 - self.a;
        let d = self.denormal;
        self.denormal = -d;
        self.y1 -= d;
        self.y2 -= d;
        self.dc -= d;
        if self.mode > 3 {
            return;
        }
        let k = m * 0.5;
        let e = 1.0 - e;
        let g = ((e * 0.25 + 0.33333334) * e + 0.5) * e * e + e;
        let dg = (((g + 1.0) - (g - 1.0).abs()) * 0.5 - self.g) * inv_n;
        let (a, b) = (self.a, self.b);
        for (x, o) in inp.iter().zip(out.iter_mut()) {
            self.g += dg;
            let x = *x;
            let dio = lerp(&t.diodelim, lut_pos(self.y2 * 32.0));
            let fb = b * self.fb_prev * k + a * dio * k;
            let u = match self.mode {
                2 => -self.y1 - fb,
                _ => (x - self.y1) - fb,
            } * 32.0;
            self.fb_prev = dio;
            self.y1 = lerp(&t.tanhlim, lut_pos(u)) * self.g + self.y1;
            let v = match self.mode {
                0 => (dio * k - self.y2) + self.y1,
                _ => (-x - self.y2) + dio * k + self.y1,
            } * 32.0;
            self.y2 = lerp(&t.tanhlim, lut_pos(v)) * self.g + self.y2;
            let w = match self.mode {
                0 | 1 => dio * 32.0,
                _ => (dio + x) * 32.0,
            };
            let (i, fr) = lut_pos(w);
            let r = (t.otalim[i] - self.dc) + fr * (t.otalim[i + 1] - t.otalim[i]);
            self.dc = self.dc_coef * r + self.dc;
            *o = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowpass_rejects_highs() {
        let sr = 44100.0;
        let run = |freq: f32| {
            let mut f = Ms20::new(sr);
            f.initialize(0.5, 0.0, 0.0);
            let x: Vec<f32> = (0..8192).map(|i| (i as f32 * freq / sr * std::f32::consts::TAU).sin() * 0.1).collect();
            let mut y = vec![0.0; 8192];
            for (c, o) in x.chunks(64).zip(y.chunks_mut(64)) {
                f.process(c, o, 1.0 / 64.0);
            }
            (y[4096..].iter().map(|v| v * v).sum::<f32>() / 4096.0).sqrt()
        };
        assert!(run(8000.0) < run(100.0) * 0.2, "{} {}", run(8000.0), run(100.0));
    }
}
