//! The response curves the EQ and filter effect panels plot
//! ([re:56f45c]): one value (0..1, y up) per point of the plot's width.
//!
//! - Parametric EQ: its two mid bands; DJ EQ: bands 0, 2 and 3 (the
//!   biquad response of the band when its In port is on, else 0 dB;
//!   [re:c549d1]);
//! - Vari-EQ: its filter ([re:beeefa]);
//! - Filter: the low-pass, then the high-pass (each 0 dB while switched
//!   off; the high-pass is shown as an RBJ high-pass, Q 1/√2, at its
//!   frequency, not the filter the audio runs through, [re:b3ae9c]);
//! - Hi-Lo Balance: the low and the high part, levels with a half-cosine
//!   crossfade 0.2 wide round the crossover ([re:671626]).
//!
//! The EQ / filter x axis is 20·10^(3x) Hz (20 Hz..20 kHz), the y axis
//! (clamp(dB, −20, 20) + 20)/40.

use super::nodes::{biquad::Biquad, ssl_eq};

/// One biquad section: `[b0, b1, b2, a1, a2]` (a0 = 1).
pub type Section = [f64; 5];

/// x (0..1) → Hz (`ConvertToUnitsFromVal01X`).
pub fn hz(x: f32) -> f32 {
    (10f64.powf(x as f64 * 3.0) * 20.0) as f32
}

/// dB → y (0..1) (`ConvertToVal01FromUnitsY`).
pub fn y_of_db(db: f32) -> f32 {
    let d = db.clamp(-20.0, 20.0);
    ((d + 20.0) / 40.0).clamp(0.0, 1.0)
}

/// The response of `sections` in series, `n` points at `sample_rate`.
pub fn curve(sections: &[Section], n: usize, sample_rate: f32) -> Vec<f32> {
    let inv_sr = 1.0 / sample_rate;
    let step = 1.0 / n as f32;
    (0..n)
        .map(|i| {
            let f = hz(i as f32 * step);
            let th = (f * -6.2831855 * inv_sr) as f64;
            // e^{jθ} inverted.
            let (c, s) = (th.cos(), th.sin());
            let m = s * s + c * c;
            let (wr, wi) = (c / m, -s / m);
            let (mut re, mut im) = (1.0f64, 0.0f64);
            for k in sections {
                let w2r = wr * wr - wi * wi;
                let w2i = wr * wi + wr * wi;
                let nr = k[1] * wr + k[0] + w2r * k[2];
                let ni = k[1] * wi + w2i * k[2];
                let dr = w2r * k[4] + k[3] * wr + 1.0;
                let di = w2i * k[4] + wi * k[3];
                let dm = di * di + dr * dr;
                let (hr, hi) = ((dr * nr + ni * di) / dm, (dr * ni - nr * di) / dm);
                let t = re * hi;
                re = re * hr - im * hi;
                im = t + im * hr;
            }
            y_of_db(((im * im + re * re).sqrt().log10() * 20.0) as f32)
        })
        .collect()
}

fn section(b: &Biquad) -> Section {
    [b.b0, b.b1, b.b2, b.a1, b.a2]
}

/// Parametric / DJ EQ band `band` for the node's port values.
fn eq_band(ports: &[f32], band: usize, n: usize, sr: f32) -> Vec<f32> {
    let p = |i: usize| ports.get(i).copied().unwrap_or(0.0);
    let base = band * 4;
    if p(base + 3) < 0.5 {
        return curve(&[], n, sr);
    }
    let ([b0, b1, b2], [_, a1, a2]) = ssl_eq::band_coeffs(band, [p(base), p(base + 1), p(base + 2)], sr as f64);
    curve(&[[b0, b1, b2, a1, a2]], n, sr)
}

/// Filter part 0 (low-pass) or 1 (high-pass).
fn filter_part(ports: &[f32], part: usize, n: usize, sr: f32) -> Vec<f32> {
    let p = |i: usize| ports.get(i).copied().unwrap_or(0.0);
    let (on, freq, mode) = if part == 0 { (2, 3, 3) } else { (0, 1, 6) };
    if p(on) < 0.5 {
        return curve(&[], n, sr);
    }
    let mut b = Biquad::default();
    b.set(mode, (p(freq) + 1.0).exp2() * (1.0 / sr), std::f32::consts::FRAC_1_SQRT_2, 0.0);
    curve(&[section(&b)], n, sr)
}

fn vari_eq(ports: &[f32], n: usize, sr: f32) -> Vec<f32> {
    let p = |i: usize| ports.get(i).copied().unwrap_or(0.0);
    let m = p(0);
    let mode = (((m.abs() + 8.0) - (m - 8.0).abs()) * 0.5).round() as i32;
    let mut b = Biquad::default();
    b.set(mode, (p(1) + 1.0).exp2() * 400.0 * (1.0 / sr), p(2), p(3));
    // The x axis is clamped to 0..1 here (the same points).
    curve(&[section(&b)], n, sr)
}

/// Hi-Lo Balance part 0 (low) or 1 (high).
fn hilo(ports: &[f32], part: usize, n: usize) -> Vec<f32> {
    let p = |i: usize| ports.get(i).copied().unwrap_or(0.0);
    let f = (p(1) + 1.0).exp2();
    let mut at = 0.0f32;
    if 1.0 <= (f + f) * 0.5 {
        at = ((f + f) * 0.5).ln() / 9.903487;
        if 1.0 < at {
            at = 1.0;
        }
    }
    let b = p(0);
    let a = ((b.abs() + 1.0) - (b - 1.0).abs()) * 0.5 * std::f32::consts::PI * 0.5;
    let lo = a.cos() * std::f32::consts::SQRT_2 * 0.7;
    let hi = a.sin() * std::f32::consts::SQRT_2 * 0.7;
    let step = 1.0 / n as f32;
    (0..n)
        .map(|i| {
            let x = i as f32 * step;
            let t = ((((x - at) + 0.1) as f64 * std::f64::consts::PI) / 0.4) as f32;
            if x <= at - 0.1 {
                if part == 0 { lo } else { 0.0 }
            } else if x >= at + 0.1 {
                if part == 0 { 0.0 } else { hi }
            } else if part == 0 {
                t.cos() * lo
            } else {
                t.sin() * hi
            }
        })
        .collect()
}

/// The curves of effect `kind` for its node's port values (`None` for
/// effects without a plot).
pub fn curves(kind: usize, ports: &[f32], n: usize, sample_rate: f32) -> Option<Vec<Vec<f32>>> {
    let n = n.max(1);
    Some(match kind {
        3 => vec![eq_band(ports, 1, n, sample_rate), eq_band(ports, 2, n, sample_rate)],
        4 => vec![eq_band(ports, 0, n, sample_rate), eq_band(ports, 2, n, sample_rate), eq_band(ports, 3, n, sample_rate)],
        5 => vec![vari_eq(ports, n, sample_rate)],
        6 => vec![hilo(ports, 0, n), hilo(ports, 1, n)],
        28 => vec![filter_part(ports, 0, n, sample_rate), filter_part(ports, 1, n, sample_rate)],
        _ => return None,
    })
}

/// The node's port values for effect `kind` with front-panel values
/// `params` (0..1): the hidden ports set up, then the parameters pushed.
pub fn node_ports(kind: usize, params: &[f32]) -> Option<Vec<f32>> {
    let mut node = super::nodes::create(kind)?;
    super::nodes::setup_internal(kind, node.as_mut());
    let ports = node.ports();
    for (k, v) in params.iter().enumerate() {
        if let Some(p) = ports.get_mut(super::nodes::node_port(kind, k)) {
            p.set01(*v);
        }
    }
    Some(ports.iter().map(|p| p.value).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_hz(c: &[f32], f: f32) -> f32 {
        c[((f / 20.0).log10() / 3.0 * c.len() as f32).round() as usize]
    }

    #[test]
    fn axes_span_20hz_to_20khz_and_40db() {
        assert!((hz(0.0) - 20.0).abs() < 1e-4 && (hz(1.0) - 20000.0).abs() < 0.1);
        assert_eq!(y_of_db(0.0), 0.5);
        assert_eq!(y_of_db(-30.0), 0.0);
        assert!((y_of_db(10.0) - 0.75).abs() < 1e-6);
        assert!(curve(&[], 205, 44100.0).iter().all(|y| *y == 0.5));
    }

    #[test]
    fn plotted_effects() {
        assert!(curves(1, &[], 100, 44100.0).is_none());
        for (k, n) in [(3, 2), (4, 3), (5, 1), (6, 2), (28, 2)] {
            let p = node_ports(k, &[0.5; 8]).unwrap();
            let c = curves(k, &p, 205, 44100.0).unwrap();
            assert_eq!(c.len(), n, "{k}");
            assert!(c.iter().all(|v| v.len() == 205 && v.iter().all(|y| (0.0..=1.0).contains(y))));
        }
    }

    #[test]
    fn parametric_eq_band_peaks_at_its_frequency() {
        // L Freq at the top of its range, L Gain up.
        let p = node_ports(3, &[0.5, 0.5, 0.9, 0.5, 0.5, 0.5]).unwrap();
        let c = curves(3, &p, 400, 48000.0).unwrap();
        let peak = c[0].iter().cloned().fold(0.0, f32::max);
        assert!(peak > 0.6, "{peak}");
        // The second band at 0 dB gain: flat.
        assert!(c[1].iter().all(|y| (*y - 0.5).abs() < 1e-3));
    }

    #[test]
    fn filter_parts_and_their_switches() {
        // HP on at 200 Hz, LP on at 2 kHz (node ports).
        let mut p = node_ports(28, &[1.0, 0.5, 1.0, 0.5]).unwrap();
        p[1] = 200f32.log2() - 1.0;
        p[3] = 2000f32.log2() - 1.0;
        let c = curves(28, &p, 600, 48000.0).unwrap();
        assert!((at_hz(&c[0], 2000.0) - y_of_db(-3.0)).abs() < 0.01);
        assert!((at_hz(&c[1], 200.0) - y_of_db(-3.0)).abs() < 0.01);
        p[2] = 0.0;
        assert!(curves(28, &p, 600, 48000.0).unwrap()[0].iter().all(|y| *y == 0.5));
    }

    #[test]
    fn hilo_parts_cross_at_the_crossover() {
        let p = node_ports(6, &[0.5, 0.5]).unwrap();
        let c = curves(6, &p, 200, 44100.0).unwrap();
        // Low part on the left, high part on the right, both 0.7 at balance 0.5.
        assert!((c[0][0] - 0.7).abs() < 1e-3 && c[1][0] == 0.0);
        assert!((c[1][199] - 0.7).abs() < 1e-3 && c[0][199] == 0.0);
    }
}
