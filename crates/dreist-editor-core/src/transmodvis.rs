//! Transmod modulator graphs ([re:1962d1]
//! [re:f491ae], the `DisplayPanel` of each modulator skin): `width` values in
//! 0..1 drawn as a 2 px line at `y = 3 + (h − 6)·(1 − v)`. The shapes come
//! from each modulator's visualisation (two cycles across the width
//! for the LFOs). Maths and Sample & Hold draw nothing.

use dreist_dsp::modulators::lfo_table;
use dreist_formats::ModulatorType;

/// [re:58b842] / `Envelope` curve exponent (`6c − 2` above 0.5, else
/// `1.5c + 0.25`).
fn curve(c: f32) -> f32 {
    if c > 0.5 { c * 6.0 - 2.0 } else { c * 1.5 + 0.25 }
}

/// [re:6c5778]: from `phase`, step 2/n.
pub fn lfo(n: usize, phase: f32, shape: u32, seed: u32) -> Vec<f32> {
    let inc = 2.0 / n.max(1) as f32;
    let mut p = phase.rem_euclid(1.0);
    let mut r = seed | 1;
    (0..n)
        .map(|_| {
            let v = match shape {
                0 | 1 | 6 | 7 => lfo_table(shape, p as f64) + 0.5,
                2 => {
                    (if p < 0.25 {
                        p + p
                    } else if p < 0.75 {
                        1.0 - (p + p)
                    } else {
                        p + p - 2.0
                    }) + 0.5
                }
                3 => p,
                4 => 1.0 - p,
                5 => (p < 0.5) as i32 as f32,
                _ => {
                    r ^= r << 13;
                    r ^= r >> 17;
                    r ^= r << 5;
                    (r >> 8) as f32 / (1u32 << 24) as f32
                }
            };
            let q = p + inc;
            p = if q < 1.0 { q } else { q - 1.0 };
            v
        })
        .collect()
}

/// [re:f2e051]: saw → square → sine →
/// triangle → arcsine weights from `asym·4`; swing changes the rate of
/// the two half cycles.
pub fn morph(n: usize, phase: f32, asym: f32, swing: f32) -> Vec<f32> {
    let f = asym * 4.0;
    let (mut saw, mut sq, mut sin, mut tri, mut asin) = (0.0, 0.0, 0.0, 0.0, 0.0);
    if f < 1.0 {
        saw = 1.0 - f;
        sq = f;
    } else if f < 2.0 {
        sq = 2.0 - f;
        sin = f - 1.0;
    } else if f < 3.0 {
        sin = 3.0 - f;
        tri = f - 2.0;
    } else {
        tri = 4.0 - f;
        asin = f - 3.0;
    }
    let s15 = swing * 1.5;
    let inc = 2.0 / n.max(1) as f32;
    let mut p = phase.rem_euclid(1.0);
    (0..n)
        .map(|_| {
            let step = if (s15 - 0.75).abs() >= 1e-6 { inc / if p < 0.5 { s15 + 0.25 } else { 1.75 - s15 } } else { inc };
            let q = p + step;
            p = if q > 1.0 { q - 1.0 } else { q };
            let t = if p < 0.25 {
                p + p
            } else if p < 0.75 {
                1.0 - (p + p)
            } else {
                p + p - 2.0
            };
            (0.5 - p) * saw + 0.5 + if p < 0.5 { 0.5 } else { -0.5 } * sq + lfo_table(0, p as f64) * sin + t * tri + lfo_table(6, p as f64) * asin
        })
        .collect()
}

/// [re:6f6db6].
pub fn ad(n: usize, attack: f32, decay: f32, attack_curve: f32, decay_curve: f32) -> Vec<f32> {
    let step = |t: f32| if t.abs() >= 1e-6 { 2.0 / (n as f32 * t) } else { 0.0 };
    let (ia, id) = (step(attack), step(decay));
    let (ea, ed) = (curve(attack_curve), curve(decay_curve));
    let mut level = 0.0f32;
    let mut rising = true;
    (0..n)
        .map(|_| {
            if rising {
                level += if ia.abs() <= 1e-6 { 1.0 } else { ia };
                if level < 1.0 {
                    return level.powf(ea);
                }
                rising = false;
                level = 1.0;
                1.0
            } else {
                level -= if id.abs() <= 1e-6 { 1.0 } else { id };
                if level <= 0.0 {
                    level = 0.0;
                    0.0
                } else {
                    level.powf(ed)
                }
            }
        })
        .collect()
}

/// [re:863913]: `steps` equal
/// segments of the step values, smoothed by `exp(−1/(segment·slew))`.
pub fn sequencer(n: usize, steps: usize, values: &[f32], slew: f32) -> Vec<f32> {
    if steps == 0 {
        return vec![0.0; n];
    }
    let seg = (n / steps).max(1);
    let mut out: Vec<f32> = (0..n).map(|i| values.get((i / seg).min(values.len().saturating_sub(1))).copied().unwrap_or(0.0)).collect();
    if slew.abs() >= 1e-6 {
        let k = (-1.0 / (seg as f32 * slew)).exp();
        let mut prev = 0.0;
        for v in out.iter_mut() {
            let x = v.abs();
            prev = (prev - x) * k + x;
            *v = prev;
        }
    }
    out
}

/// [re:c98305].
pub fn ball(n: usize, gravity: f32, speed: f32) -> Vec<f32> {
    let mut out = vec![0.0; n];
    let base = 1.0 / ((speed * 5.4 + 0.1) * n as f32 * 0.5);
    let (mut v, mut height, mut step) = (0.0f32, 1.0f32, base);
    let mut i = 0;
    while i < n {
        v -= step;
        out[i] = v;
        if v <= 0.0 || v.abs() <= 1e-6 {
            out[i] = 0.0;
            v = height - gravity * 0.49 - 0.01;
            if v < 0.01 {
                break;
            }
            step = v * base;
            height = v;
        }
        i += 1;
    }
    out
}

/// The graph of a modulator with parameters `p` (index order of
/// `ModulatorType::params`), or None when it draws nothing.
pub fn modulator(kind: ModulatorType, p: &[f32], n: usize) -> Option<Vec<f32>> {
    let g = |i: usize| p.get(i).copied().unwrap_or(0.0);
    Some(match kind {
        ModulatorType::LFO => lfo(n, g(3), (g(2) * 8.0).round() as u32, 0x1234),
        ModulatorType::MorphingLFO => morph(n, g(3), g(2), g(4)),
        ModulatorType::ADEnvelope => ad(n, g(0), g(1), g(2), g(3)),
        ModulatorType::MiniSequencer => sequencer(n, (g(1) * 15.0).floor() as usize, &p[6.min(p.len())..p.len().min(22)], g(2)),
        ModulatorType::Constant => vec![g(0); n],
        ModulatorType::BouncingBall => ball(n, g(0), g(1)),
        ModulatorType::SampleAndHold | ModulatorType::Maths => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        let s = lfo(100, 0.0, 3, 1);
        assert!(s[0] == 0.0 && (s[49] - 0.98).abs() < 1e-4 && s[52] < 0.05);
        let a = ad(100, 0.5, 0.5, 0.5, 0.5);
        assert!(a[0] > 0.0 && a.iter().cloned().fold(0.0, f32::max) == 1.0 && a[99] == 0.0);
        let b = ball(200, 0.5, 0.5);
        assert!(b.iter().all(|v| (0.0..=1.0).contains(v)));
        let q = sequencer(16, 4, &[0.0, 1.0, 0.5, 0.25], 0.0);
        assert_eq!(q[5], 1.0);
        let m = morph(64, 0.0, 0.0, 0.5);
        assert!(m.iter().all(|v| (-0.01..=1.01).contains(v)));
    }
}
