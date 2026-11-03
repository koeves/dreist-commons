//! [re:2be1ec]: the envelope hold that makes a one-shot
//! layer's envelope last about as long as its sample region.
//!
//! - OneShotLengthBound layers: both holds = 1.
//! - Else d = (region end − start) / sr, or the whole file when the region
//!   is empty; with pitch compensation (load-time fix-up only) and
//!   TimestretchActive < 0.5, d /= 2^((coarse semitones + fine) / 12).
//! - x = d − (release⁴·15 + (sustain ≠ 1 ? decay⁴·10 : 0) + attack⁴·10)
//!   using the amp envelope's values, clamped to 0..60 s; hold =
//!   (x / 60)^0.25 clamped to 0..1, written to AmpEnvHold and FreeEnvHold.
//!
//! Callers: [re:24af02] (every sample
//! load, no pitch compensation), [re:87acba] (load: when either hold
//! is negative — the attribute's default is −1 — or the layer version is
//! below 3000, unless OneShotLengthBound; with pitch compensation), the
//! 0x87 `e_resetLayerHoldTime` message and `ResetEnvelopeState` (0x86).

use dreist_formats::params::LayerParam;

/// The hold for a layer whose parameters `p` gives, with a region of
/// `region` frames in a file of `total` frames at `sr`.
pub fn reset_hold_time(p: &dyn Fn(LayerParam) -> f32, length_bound: bool, region: f64, total: f64, sr: f32, pitch: bool) -> f32 {
    if length_bound {
        return 1.0;
    }
    let mut d = (region * (1.0 / sr as f64)).max(0.0);
    if d.abs() <= 1e-6 {
        d = 0.0;
        if sr > 0.0 {
            d = (total as f32 / sr) as f64;
        }
    }
    if pitch {
        let semis = dreist_dsp::convert::semitones(p(LayerParam::CoarseTune), false) + dreist_dsp::convert::fine_semitones(p(LayerParam::FineTune));
        let r = ((semis) * (1.0 / 12.0)).exp2();
        if (r as f32) >= 1e-6 && p(LayerParam::TimestretchActive) < 0.5 {
            d *= 1.0 / r;
        }
    }
    let a = p(LayerParam::AmpEnvAttack).powi(2);
    let decay = if (p(LayerParam::AmpEnvSustain) - 1.0).abs() > 1e-6 {
        let x = p(LayerParam::AmpEnvDecay).powi(2);
        x * x * 10.0
    } else {
        0.0
    };
    let r = p(LayerParam::AmpEnvRelease).powi(2);
    let x = ((d - (r * r * 15.0 + decay + a * a * 10.0) as f64) as f32).clamp(0.0, 60.0);
    (x / 60.0).powf(0.25).clamp(0.0, 1.0)
}

/// Whether [re:87acba] recomputes the holds of `layer` on load.
pub fn needs_reset_on_load(layer: &dreist_formats::Node) -> bool {
    let neg = |k: &str| layer.f32(k).unwrap_or(-1.0) < 0.0;
    let old = layer.version.is_some_and(|v| v < 3000);
    let length_bound = layer.attr("PlaybackMode").map(dreist_dsp::voice::PlaybackMode::canonical) == Some("OneShotLengthBound");
    (old || neg("AmpEnvHold") || neg("FreeEnvHold")) && !length_bound
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_from_region_length() {
        let p = |q: LayerParam| q.default_value();
        // 1 s region: (1 − envelope times) / 60 s, fourth root.
        let h = reset_hold_time(&p, false, 44100.0, 44100.0, 44100.0, false);
        let a = LayerParam::AmpEnvAttack.default_value().powi(2);
        let r = LayerParam::AmpEnvRelease.default_value().powi(2);
        let x = (1.0 - (r * r * 15.0 + a * a * 10.0)).clamp(0.0, 60.0);
        assert!((h - (x / 60.0).powf(0.25)).abs() < 1e-6);
        assert_eq!(reset_hold_time(&p, true, 0.0, 0.0, 44100.0, false), 1.0);
        // Empty region: the whole file.
        assert_eq!(reset_hold_time(&p, false, 0.0, 44100.0, 44100.0, false), h);
    }
}
