//! [re:deea98] (Parametric EQ, DJ EQ; port setup [re:6fd56c],
//! coefficient update [re:445da1], processing [re:36b9e6]) with
//! [re:86c5ff] ([re:026f43]..[re:41d336]): four bands in series — LF shelf/bell,
//! two peaking mids (Orfanidis-style prescribed-Nyquist-gain design,
//! `peqx`), HF shelf/bell — each switchable, plus a wet/dry mix.

use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub(crate) type Coeffs = ([f64; 3], [f64; 3]);

const PASS: Coeffs = ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);

/// [re:074878] (arithmetic order checked against the original).
fn peqx(g0: f64, g: f64, gb: f64, w0: f64, dw: f64, fs: f64) -> Coeffs {
    let g2 = g * g;
    let d = g2 - gb * gb;
    if d.abs() < 1e-8 {
        return PASS;
    }
    let g02 = g0 * g0;
    let k = ((gb * gb - g02) / d).sqrt() * dw;
    let w99 = w0 * 0.99 * w0 * 0.99;
    let mut t = w99 - w0 * w0;
    t *= t;
    let g1 = ((g02 * t + w99 * g * k * g * k) / (t + w99 * k * k)).sqrt();
    let g12 = g1 * g1;
    let d11 = (g2 - g12).abs();
    if d11 < 1e-8 {
        return PASS;
    }
    let w = w0 / fs;
    let dwn = (w + w) * 0.010000000000000009;
    let d8 = (g12 - g02).abs();
    let mut q = w * w + -9.869604401089358;
    q *= q;
    let pi2 = 9.869604401089358;
    let gn = (((g2 * d8 * pi2 * dwn * dwn) / d11 + g02 * q) / ((d8 * pi2 * dwn * dwn) / d11 + q)).sqrt();
    let d3 = (g2 - gn * gn).abs();
    let d12 = (g12 - gn * gn).abs();
    let tw = (w * 0.5).tan();
    let w2 = tw * tw * (d3 / (g2 - g02).abs()).sqrt();
    let dw2 = ((d8 / d12).sqrt() * w2 + 1.0) * (dwn * 0.5).tan();
    let c = dw2 * dw2 * d12 - ((g12 - gn * g0).abs() - (d12 * d8).sqrt()) * (w2 + w2);
    let dd = ((g2 - gn * g0).abs() - (d3 * (g2 - g02).abs()).sqrt()) * (w2 + w2);
    let a = ((dd + c) / d11).abs().sqrt();
    let b = ((c * g2 + g12 * dd) / d11).sqrt();
    let n = 1.0 / (w2 + 1.0 + a);
    let gw = g0 * w2;
    ([(gn + gw + b) * n, (gn - gw) * -2.0 * n, ((gw + gn) - b) * n], [1.0, n * (1.0 - w2) * -2.0, n * ((w2 + 1.0) - a)])
}

fn db(g: f64) -> f64 {
    10f64.powf(g / 20.0)
}

/// `SSLpeak(f, 1/Q, gain dB, fs)`: bandwidth in octaves from Q.
fn ssl_peak(f: f64, inv_q: f64, gain: f64, fs: f64) -> Coeffs {
    let h = 1.0 / (inv_q + inv_q);
    let l = ((h * h + 1.0).sqrt() + h).ln();
    let bw = (l + l) / std::f64::consts::LN_2;
    let w0 = f * 6.2831854820251465;
    peqx(1.0, db(gain), db(gain * 0.5), w0, bw * w0, fs)
}

/// Low/high bell: bandwidth 0.57·w0.
fn ssl_bell(f: f64, gain: f64, fs: f64) -> Coeffs {
    let w0 = f * 6.2831854820251465;
    peqx(1.0, db(gain), db(gain * 0.5), w0, w0 * 0.57, fs)
}

/// Low/high shelf: RBJ shelf with slope 0.5.
pub(crate) fn ssl_shelf(f: f64, gain: f64, fs: f64, high: bool) -> Coeffs {
    let a = db(gain).sqrt();
    let lim = if high { 0.45 } else { 0.44999998807907104 };
    let x = f / fs;
    let w = (((x + -0.001).abs() + 0.001 + lim) - (x - lim).abs()) * 0.5 * 6.2831854820251465;
    let (sn, cs) = (w.sin(), w.cos());
    let (ap, am) = (a + 1.0, a - 1.0);
    let s = (1.0 / a + a + 2.0).sqrt() * sn * 0.5 * (a.sqrt() + a.sqrt());
    if !high {
        let d8 = ap - am * cs;
        let d7 = am * cs + ap;
        let a0 = d7 + s;
        ([a * (d8 + s) / a0, (a + a) * (am - cs * ap) / a0, ((d8 - s) * a) / a0], [1.0, (cs * ap + am) * -2.0 / a0, (d7 - s) / a0])
    } else {
        let d6 = ap - am * cs;
        let d7 = am * cs + ap;
        let a0 = d6 + s;
        let d10 = am - cs * ap;
        ([a * (d7 + s) / a0, a * -2.0 * (am + cs * ap) / a0, ((d7 - s) * a) / a0], [1.0, (d10 + d10) / a0, (d6 - s) / a0])
    }
}

#[derive(Clone, Copy, Default)]
struct Bq {
    c: Coeffs,
    x1: [f64; 2],
    x2: [f64; 2],
    y1: [f64; 2],
    y2: [f64; 2],
}

impl Bq {
    #[inline]
    fn tick(&mut self, ch: usize, x: f32) -> f32 {
        let ([b0, b1, b2], [_, a1, a2]) = self.c;
        let xd = x as f64;
        let y = ((self.x2[ch] * b2 + self.x1[ch] * b1 + b0 * xd) - self.y1[ch] * a1) - self.y2[ch] * a2;
        self.x2[ch] = self.x1[ch];
        self.x1[ch] = xd;
        self.y2[ch] = self.y1[ch];
        self.y1[ch] = y;
        y as f32
    }
}

pub struct SslEq {
    ports: Vec<Port>,
    first: bool,
    keys: [[f32; 3]; 4],
    on: [bool; 4],
    bands: [Bq; 4],
}

impl SslEq {
    pub fn new() -> SslEq {
        SslEq { ports: ports_from(ports::PARAMETRIC_EQ), first: true, keys: [[f32::NAN; 3]; 4], on: [false; 4], bands: [Bq::default(); 4] }
    }

    /// Coefficient update (round-to-nearest; each band recomputed when its ports change).
    fn update(&mut self, sr: f32) {
        let p = |i: usize| self.ports[i].value;
        let fs = sr as f64;
        for b in 0..4 {
            let base = b * 4;
            self.on[b] = p(base + 3) >= 0.5;
            let key = [p(base), p(base + 1), p(base + 2)];
            if !self.first && key == self.keys[b] {
                continue;
            }
            self.keys[b] = key;
            self.bands[b].c = band_coeffs(b, key, fs);
        }
        self.first = false;
    }
}

/// Band `b`'s coefficients from its first three ports (shape or Q,
/// frequency, gain) at `fs`: LF / HF shelf or bell, the mids peaking.
pub(crate) fn band_coeffs(b: usize, key: [f32; 3], fs: f64) -> Coeffs {
    let gain = |x: f32| ((x as f64 * 1.19029).tan() as f32 * 8.0) as f64;
    let f = ((key[1] + 1.0).exp2()) as f64;
    match b {
        0 => {
            if key[0] < 0.5 {
                ssl_shelf(f, gain(key[2]), fs, false)
            } else {
                ssl_bell(f, gain(key[2]), fs)
            }
        }
        1 | 2 => ssl_peak(f, (-key[0]).exp() as f64, gain(key[2]), fs),
        _ => {
            if key[0] < 0.5 {
                ssl_shelf(f, gain(key[2]), fs, true)
            } else {
                ssl_bell(f, gain(key[2]), fs)
            }
        }
    }
}

impl Default for SslEq {
    fn default() -> Self {
        SslEq::new()
    }
}

impl Node for SslEq {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// Reset [re:4f13fd]: every band cleared, recomputed next block.
    fn reset(&mut self) {
        self.first = true;
        self.keys = [[f32::NAN; 3]; 4];
        self.on = [false; 4];
        self.bands = [Bq::default(); 4];
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.update(ctx.sample_rate);
        let [ol, or] = out;
        if !self.on.iter().any(|b| *b) {
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
            return;
        }
        let w = self.ports[17].value;
        let wet = ((w.abs() + 1.0) - (w - 1.0).abs()) * 0.5;
        for i in 0..inp[0].len() {
            let mut y = [inp[0][i], inp[1][i]];
            for b in 0..4 {
                if self.on[b] {
                    y[0] = self.bands[b].tick(0, y[0]);
                    y[1] = self.bands[b].tick(1, y[1]);
                }
            }
            ol[i] = inp[0][i] * (1.0 - wet) + wet * y[0];
            or[i] = inp[1][i] * (1.0 - wet) + y[1] * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_has_gain_at_center() {
        let fs = 48000.0;
        let (b, a) = ssl_peak(1000.0, 1.0, 12.0, fs);
        let w = std::f64::consts::TAU * 1000.0 / fs;
        let z = |c: &[f64; 3]| {
            let (re, im) = (c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos(), -(c[1] * w.sin() + c[2] * (2.0 * w).sin()));
            (re * re + im * im).sqrt()
        };
        let g = 20.0 * (z(&b) / z(&a)).log10();
        assert!((g - 12.0).abs() < 0.5, "{g}");
        let (b, a) = ssl_shelf(100.0, 6.0, fs, false);
        let dc = (b[0] + b[1] + b[2]) / (a[0] + a[1] + a[2]);
        assert!((20.0 * dc.log10() - 6.0).abs() < 0.1);
    }
}
