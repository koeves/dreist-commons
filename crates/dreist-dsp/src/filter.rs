//! `VoiceFilter` ([re:92b893]): [re:2b7ce5] followed by [re:e0adbe], which
//! picks one of four cores by `FilterAlgorithm`.

use crate::comb::Comb;
use crate::ms20::Ms20;
use crate::sh101::Sh101;
use crate::svf::Svf;

/// [re:7ca28c] resonance curve.
pub fn shape_resonance(r: f32) -> f32 {
    let d = r + -0.5;
    let p = 4.0 * d;
    let p = p * p;
    (r * 0.1 + 0.45 + p * p * (d * 0.057142857)).min(1.0).max(0.0)
}

#[derive(Debug, Clone)]
pub struct VoiceFilter {
    algorithm: u32,
    sh101: Sh101,
    svf: Svf,
    ms20: [Ms20; 2],
    comb: [Comb; 2],
    drive_target: f32,
    drive_last: f32,
    ramp: Vec<f32>,
    /// Input copies for the in-place cores (reused: no allocation per block).
    input: [Vec<f32>; 2],
    sr: f32,
}

/// Scratch capacity reserved up front (a voice sub-block is at most 32).
const SCRATCH: usize = 256;

impl VoiceFilter {
    pub fn new(sample_rate: f32) -> VoiceFilter {
        VoiceFilter {
            algorithm: 0,
            sh101: Sh101::new(sample_rate),
            svf: Svf::new(sample_rate),
            ms20: [Ms20::new(sample_rate), Ms20::new(sample_rate)],
            comb: [Comb::new(sample_rate), Comb::new(sample_rate)],
            drive_target: 0.0,
            drive_last: 0.0,
            ramp: Vec::with_capacity(SCRATCH),
            input: [Vec::with_capacity(SCRATCH), Vec::with_capacity(SCRATCH)],
            sr: sample_rate,
        }
    }

    /// Back to the state after `new`, keeping the comb buffers and ramp
    /// (no allocation at note start).
    pub fn reset(&mut self) {
        let sr = self.sr;
        self.algorithm = 0;
        self.sh101 = Sh101::new(sr);
        self.svf = Svf::new(sr);
        self.ms20 = [Ms20::new(sr), Ms20::new(sr)];
        for c in &mut self.comb {
            c.reset();
        }
        self.drive_target = 0.0;
        self.drive_last = 0.0;
    }

    /// [re:a34549]: normalized cutoff, resonance, filter
    /// type, drive and algorithm (all 0..=1, as passed by
    /// [re:6db2e6]).
    pub fn initialize(&mut self, cutoff: f32, res: f32, ftype: f32, drive: f32, algorithm: f32) {
        let a = (algorithm * 3.0).round() as i64;
        self.algorithm = a.clamp(0, u32::MAX as i64) as u32;
        if a < 4 {
            let r = shape_resonance(res);
            match a {
                0 => self.sh101.initialize(cutoff, r, ftype),
                1 => self.svf.initialize(cutoff, r, ftype),
                2 => self.ms20.iter_mut().for_each(|f| f.initialize(cutoff, r, ftype)),
                _ => self.comb.iter_mut().for_each(|f| f.initialize(cutoff, r, ftype)),
            }
        }
        // [re:bfde32].
        let g = drive * 1.5 + 0.5;
        self.drive_target = g * g * g;
    }

    /// [re:42b8bb], in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], inv_n: f32) {
        let n = left.len();
        // [re:69c1c0] via [re:ca55f9].
        let inc = (self.drive_target - self.drive_last) * inv_n;
        self.ramp.clear();
        self.ramp.extend((0..n).map(|i| (i + 1) as f32 * inc + self.drive_last));
        if let Some(l) = self.ramp.last() {
            self.drive_last = *l;
        }
        for ((l, r), g) in left.iter_mut().zip(right.iter_mut()).zip(&self.ramp) {
            *l *= g;
            *r *= g;
        }
        let [li, ri] = &mut self.input;
        li.clear();
        li.extend_from_slice(left);
        ri.clear();
        ri.extend_from_slice(right);
        let (li, ri) = (&li[..], &ri[..]);
        match self.algorithm {
            0 => self.sh101.process([li, ri], [left, right], inv_n),
            1 => self.svf.process([li, ri], [left, right], inv_n),
            2 => {
                self.ms20[0].process(li, left, inv_n);
                self.ms20[1].process(ri, right, inv_n);
            }
            3 => {
                self.comb[0].process(li, left, inv_n);
                self.comb[1].process(ri, right, inv_n);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resonance_curve_endpoints() {
        assert!((shape_resonance(0.5) - 0.5).abs() < 1e-6);
        assert!(shape_resonance(0.0) < shape_resonance(1.0));
    }

    #[test]
    fn every_algorithm_runs() {
        for a in 0..4 {
            let mut f = VoiceFilter::new(48000.0);
            f.initialize(0.6, 0.4, 0.0, 0.3, a as f32 / 3.0);
            let mut l: Vec<f32> = (0..256).map(|i| (i as f32 * 0.05).sin()).collect();
            let mut r = l.clone();
            f.process(&mut l, &mut r, 1.0 / 256.0);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()), "algorithm {a}");
        }
    }
}
