//! [re:6846b4] gain/pan (`ProcessLinearGain` [re:c4a00e] / [re:2e5cc9]).

use crate::convert::gain_to_amp;

/// Balance law on normalized pan `p` (0.5 = centre): `(left, right)` gains,
/// each `min(…, 1)`, so the centre is unity on both sides.
pub fn pan_gains(p: f32) -> (f32, f32) {
    let r = p + p;
    ((2.0 - r).min(1.0), r.min(1.0))
}

/// Normalized gain/pan with per-block linear ramps from the previous values.
#[derive(Debug, Clone, Copy)]
pub struct Mixable {
    pub gain: f32,
    pub pan: f32,
    last_gain: f32,
    last_pan: f32,
}

impl Default for Mixable {
    /// [re:2a2917]: unity
    /// ([re:db2d5b], ∛0.5), centred.
    fn default() -> Self {
        Mixable::new(0.5f32.cbrt(), 0.5)
    }
}

impl Mixable {
    /// Starts at `gain`/`pan` with no ramp.
    pub fn new(gain: f32, pan: f32) -> Mixable {
        Mixable { gain, pan, last_gain: gain, last_pan: pan }
    }

    /// Continue `old`'s ramps (the values its last block ended on).
    pub fn continue_from(&mut self, old: &Mixable) {
        self.last_gain = old.last_gain;
        self.last_pan = old.last_pan;
    }

    /// [re:61f794] / `SetPan` clamp to `0..=1`.
    pub fn set(&mut self, gain: f32, pan: f32) {
        self.gain = gain.clamp(0.0, 1.0);
        self.pan = pan.clamp(0.0, 1.0);
    }

    /// Stereo in place: ramps `last → target` across the block, then
    /// `ResetLastGain`/`ResetLastPan` (the voice calls both after processing).
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let (mut g, mut p) = (self.last_gain, self.last_pan);
        let (dg, dp) = if n == 0 {
            (0.0, 0.5)
        } else {
            let inv = 1.0 / n as f32;
            (inv * (self.gain - g), inv * (self.pan - p))
        };
        let mut amp = gain_to_amp(g);
        if dg.abs() <= 1e-6 && dp.abs() <= 1e-6 {
            let (l, r) = pan_gains(p);
            if (l * amp - 1.0).abs() >= 1e-6 || (r * amp - 1.0).abs() >= 1e-6 {
                left.iter_mut().for_each(|s| *s *= l * amp);
                right.iter_mut().for_each(|s| *s *= r * amp);
            }
        } else {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                let (gl, gr) = pan_gains(p);
                *l *= gl * amp;
                *r *= gr * amp;
                g += dg;
                amp = gain_to_amp(g);
                p += dp;
            }
        }
        self.last_gain = self.gain;
        self.last_pan = self.pan;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new mixable is at unity: its first block is not ramped from +6 dB.
    #[test]
    fn default_is_unity() {
        let mut m = Mixable::default();
        let (mut l, mut r) = ([1.0f32; 32], [1.0f32; 32]);
        m.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| (v - 1.0).abs() < 1e-5), "{l:?}");
    }

    #[test]
    fn centre_is_unity_both_sides() {
        assert_eq!(pan_gains(0.5), (1.0, 1.0));
        assert_eq!(pan_gains(0.0), (1.0, 0.0));
        assert_eq!(pan_gains(1.0), (0.0, 1.0));
    }
}
