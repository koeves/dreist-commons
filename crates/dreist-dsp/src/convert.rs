//! Conversion helpers used by [re:9d5283].

/// [re:a51561]: normalized gain to linear amplitude, `2g³`.
pub fn gain_to_amp(g: f32) -> f32 {
    (g + g) * g * g
}

/// [re:4d2569]: `8x - 4`, floored when `quantize`.
pub fn octave(x: f32, quantize: bool) -> f64 {
    let v = x as f64 * 8.0 - 4.0;
    if quantize { v.floor() } else { v }
}

/// [re:dba367]: semitones `24x - 12`,
/// rounded half away from zero when `quantize`.
pub fn semitones(x: f32, quantize: bool) -> f64 {
    let v = x as f64 * 24.0;
    let v = if quantize { (v + if v < 0.0 { -0.5 } else { 0.5 }) as i32 as f64 } else { v };
    v - 12.0
}

/// [re:62fd01]: fine tune in
/// semitones, `2x - 1` (±100 cents).
pub fn fine_semitones(x: f32) -> f64 {
    (x + x - 1.0) as f64
}

/// [re:770509]: bend `v` towards 1 (`m > 0.5`)
/// or 0 (`m < 0.5`) by `|2m - 1|`, clamped to `0..=1`.
pub fn modulate_pos_neg(v: f32, m: f32) -> f32 {
    let d = m + m - 1.0;
    let v = if m > 0.5 {
        v + d * (1.0 - v)
    } else if m < 0.5 {
        v - d.abs() * v
    } else {
        v
    };
    v.min(1.0).max(0.0)
}

/// [re:4675a9]: `amount` (normalized,
/// 0.5 = none) scales the cutoff towards 0 or 1 by velocity `vel` (0..=1).
pub fn cutoff_with_velocity(cutoff: f32, amount: f32, vel: f32) -> f32 {
    let mut c = cutoff;
    if (amount - 0.5).abs() >= 1e-6 {
        let d = amount + amount - 1.0;
        if amount > 0.5 {
            c += (1.0 - c) * vel * d;
        } else if amount < 0.5 {
            c += vel * c * d;
        }
    }
    c.max(0.0).min(1.0)
}

/// Number of filter types minus one for each `FilterAlgorithm` (`[re:3141bd]`).
pub const FILTER_TYPE_COUNTS: [f32; 4] = [21.0, 9.0, 3.0, 2.0];

/// [re:70cc81]: a step's filter-type
/// lane overrides the layer's type unless it is in the lowest bucket.
pub fn filter_type_from_step(step: f32, algorithm: f32, layer_type: f32) -> f32 {
    let n = match (algorithm * 3.0).round() as i64 {
        i @ 0..=3 => FILTER_TYPE_COUNTS[i as usize],
        _ => 0.0,
    };
    let i = ((n + 1.0) * step) as i32;
    if i != 0 { (i - 1) as f32 / n } else { layer_type }
}

/// Sample-point offset curve from [re:f524f7]
/// [re:559ba2]. `x` is the normalized parameter (0.5 = no offset); `o = 2x - 1`
/// moves `base` towards `hi` by `o³` of the gap, or towards `lo` by `1 - (1-|o|)³`.
/// Returns `None` when the parameter is centred.
fn cubic_offset(x: f32, lo: f64, base: f64, hi: f64) -> Option<f64> {
    let o = x + x - 1.0;
    if o.abs() < 1e-6 {
        return None;
    }
    Some(if o >= 0.0 {
        (hi - base) * (o * o * o) as f64 + base
    } else {
        let k = 1.0 - o.abs();
        (k * k * k) as f64 * (base - lo) + lo
    })
}

/// Apply `SampleStartPoint` / `SampleEndPoint` to the layer's start/end markers
/// ([re:f79f64], frames). `frames` is the audio file length.
pub fn sample_points(start: u32, end: u32, frames: u32, start_param: f32, end_param: f32) -> (u32, u32) {
    let mut s = start as f64;
    if let Some(v) = cubic_offset(start_param, 0.0, s, end as f64) {
        s = v.floor() as i32 as u32 as f64;
    }
    let mut e = end as f64;
    if let Some(v) = cubic_offset(end_param, s, e, (frames as i32 - 1) as f64) {
        e = v.floor() as i32 as u32 as f64;
    }
    (s.floor() as u32, e.floor() as u32)
}

/// Playback increment in source frames per output frame
/// ([re:2865ad]): `2^(semitones/12)·base`,
/// where `base` is the file-to-host sample-rate ratio.
pub fn pitch_step(semitones: f64, base: f64) -> f64 {
    (semitones * (1.0 / 12.0)).exp2() * base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_values_are_neutral() {
        assert_eq!(semitones(0.5, true), 0.0);
        assert_eq!(octave(0.5, true), 0.0);
        assert_eq!(fine_semitones(0.5), 0.0);
        assert_eq!(modulate_pos_neg(0.3, 0.5), 0.3);
        assert_eq!(sample_points(100, 900, 1000, 0.5, 0.5), (100, 900));
        assert!((gain_to_amp(0.5f32.cbrt()) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sample_point_offsets() {
        // Start fully up reaches the end marker; fully down reaches 0.
        assert_eq!(sample_points(100, 900, 1000, 1.0, 0.5).0, 900);
        assert_eq!(sample_points(100, 900, 1000, 0.0, 0.5).0, 0);
        // End fully up reaches the last frame; fully down collapses to start.
        assert_eq!(sample_points(100, 900, 1000, 0.5, 1.0).1, 999);
        assert_eq!(sample_points(100, 900, 1000, 0.5, 0.0).1, 100);
    }

    #[test]
    fn quantized_pitch_rounds_half_away_from_zero() {
        assert_eq!(semitones(13.0 / 24.0, true), 1.0);
        assert_eq!(semitones(0.0, true), -12.0);
        assert_eq!(octave(1.0, true), 4.0);
    }
}
