//! [re:d30192] ([re:06fa4d] .. [re:db9460]), the "SVF" voice filter
//! algorithm: a Chamberlin SVF with table saturation (`tanhlim`) on every
//! integrator input and a diode term in the damping path. 4-pole modes run the
//! selected tap through a second stage. The original processes four SIMD lanes
//! (L, R and two unused); this port keeps the two live lanes.

use crate::analog::{analog, lerp, lut_pos, sin_tab};

/// [re:d068ec] (`CreateFilterModeMap` [re:a10a12]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvfMode {
    L2,
    L4,
    H2,
    H4,
    B2,
    B4,
    N2,
    N4,
    P2,
    P4,
}

impl SvfMode {
    pub fn from_index(i: i64) -> Option<SvfMode> {
        use SvfMode::*;
        [L2, L4, H2, H4, B2, B4, N2, N4, P2, P4].get(usize::try_from(i).ok()?).copied()
    }

    fn four_pole(self) -> bool {
        matches!(self, SvfMode::L4 | SvfMode::H4 | SvfMode::B4 | SvfMode::N4 | SvfMode::P4)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Stage {
    a: [f32; 2],
    b: [f32; 2],
    hp: [f32; 2],
    prev_tap: [f32; 2],
}

/// Signals are scaled by 32 before every table read (`0xe0` slot).
const IN_SCALE: f32 = 32.0;

#[derive(Debug, Clone)]
pub struct Svf {
    octave: f32,
    freq: f32,
    res: f32,
    mode: Option<SvfMode>,
    mode_index: i64,
    inv_sr: f32,
    dc_coef: f32,
    g1: f32,
    g2: f32,
    k1: f32,
    k2: f32,
    s1: Stage,
    s2: Stage,
    dc: [f32; 2],
}

impl Svf {
    pub fn new(sample_rate: f32) -> Svf {
        let mut f = Svf {
            octave: 440.0,
            freq: 0.0,
            res: 0.0,
            mode: Some(SvfMode::L2),
            mode_index: 0,
            inv_sr: 0.0,
            dc_coef: 0.00142,
            g1: 0.0,
            g2: 0.0,
            k1: 0.0,
            k2: 0.0,
            s1: Stage::default(),
            s2: Stage::default(),
            dc: [0.0; 2],
        };
        f.set_sample_rate(sample_rate);
        f
    }

    /// `SetSampleRate` [re:691eb1]: output DC blocker at 10 Hz.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.inv_sr = 1.0 / sr;
        let e = (self.inv_sr * -62.831856).exp();
        self.dc_coef = ((2.0 - e) - e.abs()) * 0.5;
    }

    /// `Initialize` [re:002722]. `cutoff` normalized (`440·2^(12c-6)` Hz),
    /// `res` already through [re:e0adbe]'s resonance curve, `mode` normalized
    /// over the 10 modes. Changing mode clears all state.
    pub fn initialize(&mut self, cutoff: f32, res: f32, mode: f32) {
        self.octave = cutoff * 12.0 - 6.0;
        self.res = res * 0.909 + 0.101;
        self.freq = crate::fx::exp2(self.octave.min(12.0).max(-12.0)) * 440.0;
        let m = (mode * 9.0).round() as i64;
        if m != self.mode_index {
            self.g1 = 0.0;
            self.g2 = 0.0;
            self.k1 = 0.0;
            self.k2 = 0.0;
            self.s1 = Stage::default();
            self.s2 = Stage::default();
            self.dc = [0.0; 2];
            self.mode_index = m;
            self.mode = SvfMode::from_index(m);
        }
    }

    /// One SVF stage for lane `l` on input already scaled by 32.
    #[inline]
    fn stage(st: &mut Stage, l: usize, x32: f32, g: f32, k: f32) {
        let t = analog();
        let pb = lut_pos(IN_SCALE * st.b[l]);
        st.a[l] = lerp(&t.tanhlim, pb) * g + st.a[l];
        let px = lut_pos(x32);
        let (ib, fb) = pb;
        let (ix, fx) = px;
        let d0 = t.diodesvflim[ib];
        st.hp[l] = ((t.tanhlim[ix] - d0) - st.a[l]) + ((d0 - t.diodesvflim[ib + 1]) * fb - st.b[l] * k) + (t.tanhlim[ix + 1] - t.tanhlim[ix]) * fx;
        st.b[l] = lerp(&t.tanhlim, lut_pos(IN_SCALE * st.hp[l])) * g + st.b[l];
    }

    fn tap(mode: SvfMode, st: &Stage, l: usize) -> f32 {
        use SvfMode::*;
        match mode {
            L2 | L4 => st.a[l],
            H2 | H4 => st.hp[l],
            B2 | B4 => st.b[l],
            N2 | N4 => st.hp[l] + st.a[l],
            P2 | P4 => st.a[l] - st.hp[l],
        }
    }

    /// Smoothed tap: `¼·g·(previous − current) + current`.
    fn smooth(st: &mut Stage, l: usize, v: f32, g: f32) -> f32 {
        let prev = st.prev_tap[l];
        st.prev_tap[l] = v;
        g * 0.25 * (prev - v) + v
    }

    /// `Process` [re:db9460], `inv_n` = 1/len (coefficients ramp across the block).
    pub fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], inv_n: f32) {
        let res = (self.res.abs() + self.res) * 0.5;
        let sq = if res == 0.0 { 0.0 } else { res.sqrt() };
        let w = self.inv_sr * self.freq;
        let v = ((w + 0.25) - (w - 0.25).abs()) * 512.0;
        let iv = v.floor() as i32;
        let st = &sin_tab()[0x400..];
        let g = (st[iv as usize + 1] - st[iv as usize]) * (v - iv as f32) + st[iv as usize];
        let g4 = g * 4.0;
        let k = 2.0 - (sq + sq);
        let k2 = (0.5 - sq * 0.5) * k + 1.0;
        let gg = g * 0.5 * g4;
        let a = gg * k;
        let b = gg * k2;
        self.k1 = k - a;
        self.k2 = k2 - b;
        let Some(mode) = self.mode else { return };
        let dg1 = ((g4 - self.g1) - a) * inv_n;
        let dg2 = ((g4 - self.g2) - b) * inv_n;
        let t = analog();
        let [ol, or] = out;
        for n in 0..ol.len() {
            self.g1 += dg1;
            self.g2 += dg2;
            for (l, o) in [&mut *ol, &mut *or].into_iter().enumerate() {
                Self::stage(&mut self.s1, l, inp[l][n] * IN_SCALE, self.g1, self.k1);
                let v1 = Self::tap(mode, &self.s1, l);
                let mut y = Self::smooth(&mut self.s1, l, v1, self.g1);
                if mode.four_pole() {
                    Self::stage(&mut self.s2, l, y * IN_SCALE, self.g2, self.k2);
                    let v2 = Self::tap(mode, &self.s2, l);
                    y = Self::smooth(&mut self.s2, l, v2, self.g2);
                }
                let (i, f) = lut_pos(y * IN_SCALE);
                let r = (t.tanhlim[i] - self.dc[l]) + (t.tanhlim[i + 1] - t.tanhlim[i]) * f;
                self.dc[l] = self.dc_coef * r + self.dc[l];
                o[n] = r;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * freq / sr * std::f32::consts::TAU).sin() * amp).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Run blocks of 64 and return the RMS of the second half.
    fn run(mode: f32, cutoff: f32, freq: f32) -> f32 {
        let sr = 44100.0;
        let mut f = Svf::new(sr);
        f.initialize(cutoff, 0.0, mode);
        let x = sine(freq, sr, 8192, 0.1);
        let mut y = vec![0.0; x.len()];
        let mut y2 = vec![0.0; x.len()];
        for (c, (o, o2)) in x.chunks(64).zip(y.chunks_mut(64).zip(y2.chunks_mut(64))) {
            f.process([c, c], [o, o2], 1.0 / 64.0);
        }
        rms(&y[4096..])
    }

    #[test]
    fn lowpass_and_highpass_respond() {
        // 440 Hz cutoff (c = 0.5): lowpass keeps 100 Hz, rejects 5 kHz.
        assert!(run(0.0, 0.5, 100.0) > 0.05);
        assert!(run(0.0, 0.5, 5000.0) < 0.01);
        // 4-pole rejects more.
        assert!(run(1.0 / 9.0, 0.5, 5000.0) < run(0.0, 0.5, 5000.0));
        // Highpass the other way round.
        assert!(run(2.0 / 9.0, 0.5, 100.0) < 0.03);
        assert!(run(2.0 / 9.0, 0.5, 5000.0) > 0.05);
    }

    #[test]
    fn stays_finite_at_full_resonance() {
        let mut f = Svf::new(48000.0);
        f.initialize(0.6, 1.0, 0.0);
        let x = sine(300.0, 48000.0, 4096, 1.0);
        let mut y = vec![0.0; 4096];
        let mut y2 = vec![0.0; 4096];
        for (c, (o, o2)) in x.chunks(128).zip(y.chunks_mut(128).zip(y2.chunks_mut(128))) {
            f.process([c, c], [o, o2], 1.0 / 128.0);
        }
        assert!(y.iter().all(|v| v.is_finite() && v.abs() < 4.0));
    }
}
