//! [re:711690] ([re:a8dea9] .. [re:c336cf]), the "Japan" voice filter algorithm:
//! a 4-pole OTA ladder with a mode-dependent mix of the input node and the four
//! stage outputs (22 modes). The original runs 4 SIMD lanes; L/R kept here.

use crate::analog::{analog, lut_pos};

/// [re:892a33] names (mode name map [re:c0edaa]).
pub const SH101_MODES: [&str; 22] =
    ["L2", "L4", "B2", "B4", "H2", "H4", "P2", "NP2", "N2P2T", "N2L2T", "N2H1L1T", "P2N2T", "H2N2T", "H1L1N2T", "N2P2N2T", "N2", "N4", "N2L2", "H2N2", "H1N2L1", "N2N2AT", "N2N2BT"];

/// [re:7e7636]: weights of `[input, y1, y2, y3, y4]`.
const MODE_MIX: [[f32; 5]; 22] = [
    [0.0, 0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 1.0],
    [0.0, -1.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 2.0, -4.0, 2.0],
    [1.0, -2.0, 1.0, 0.0, 0.0],
    [1.0, -4.0, 6.0, -4.0, 1.0],
    [1.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, -2.0, 3.0, -2.0, 1.0],
    [1.0, -2.0, 1.25, 0.0, 0.0],
    [0.0, 0.0, 4.0, -8.0, 5.0],
    [0.0, -2.0, 6.0, -6.5, 2.5],
    [0.25, -0.5, 1.25, 0.0, 0.0],
    [1.0, -4.0, 10.0, -12.0, 5.0],
    [0.0, -0.5, 1.5, -3.5, 2.5],
    [1.0, -4.0, 10.25, -12.5, 6.25],
    [1.0, -2.0, 2.0, 0.0, 0.0],
    [1.0, -4.0, 8.0, -8.0, 4.0],
    [0.0, 0.0, 1.0, -2.0, 2.0],
    [1.0, -4.0, 7.0, -6.0, 2.0],
    [0.0, -2.0, 6.0, -8.0, 4.0],
    [0.25, -1.0, 2.75, -3.5, 2.5],
    [1.0, -4.0, 7.25, -6.5, 2.5],
];

/// Cutoff warp: `1 − 2^(−9.06472·min(g, k))` mapped through a quartic.
fn warp(e: f32) -> f32 {
    (((e * 0.393442 + -0.131474) * e + 0.111105) * e + 0.991549) * e
}

/// Resonance compensation for the same warp variable.
fn res_comp(e: f32) -> f32 {
    ((-0.0408209 - e * 0.22) * e + -0.0635353) * e + 1.00175
}

#[derive(Debug, Clone)]
pub struct Sh101 {
    inv_sr: f32,
    hp_coef: f32,
    /// Warp limit; 1.0 at the voice's oversampling factor of 1 ([re:3eafba]).
    limit: f32,
    octave: f32,
    res: f32,
    mode: usize,
    mix: [f32; 5],
    g_prev: f32,
    r_prev: f32,
    /// `[x0, y1, y2, y3, y4]` per lane.
    st: [[f32; 2]; 5],
    hp: [f32; 2],
    denormal: f32,
}

impl Sh101 {
    pub fn new(sample_rate: f32) -> Sh101 {
        let mut f = Sh101 { inv_sr: 0.0, hp_coef: 0.0, limit: 0.25, octave: 440.0, res: 0.0, mode: 0, mix: MODE_MIX[0], g_prev: 0.0, r_prev: 0.0, st: [[0.0; 2]; 5], hp: [0.0; 2], denormal: 1e-8 };
        f.set_sample_rate(sample_rate, 1);
        f
    }

    /// Sample-rate setup [re:9044ed]: 10 Hz high-pass on the input.
    pub fn set_sample_rate(&mut self, sr: f32, oversample: i32) {
        self.inv_sr = 1.0 / sr;
        let e = (self.inv_sr * -62.831856).exp();
        self.hp_coef = ((2.0 - e) - e.abs()) * 0.5;
        self.limit = if oversample == 1 { 1.0 } else { 0.25 };
    }

    /// Setup [re:fbaa62]. Changing mode clears the state.
    pub fn initialize(&mut self, cutoff: f32, res: f32, mode: f32) {
        self.octave = cutoff * 12.0 - 6.0;
        self.res = res * 1.01;
        let m = (mode * 21.0).round() as i64;
        if m as usize != self.mode || m < 0 {
            self.st = [[0.0; 2]; 5];
            self.hp = [0.0; 2];
            self.g_prev = 0.0;
            self.r_prev = 0.0;
            self.mix = [0.0, 0.0, 1.0, 0.0, 0.0];
            self.mode = m.max(0) as usize;
        }
    }

    /// `Process` [re:c336cf] (`inv_n` = 1/len).
    pub fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], inv_n: f32) {
        let t = &analog().otalim_s;
        let n = out[0].len();
        let f = crate::fx::exp2(self.octave) * self.inv_sr * 440.0;
        let r = (self.res * 4.03).min(32.0).max(0.0);
        if let Some(m) = MODE_MIX.get(self.mode) {
            self.mix = *m;
        }
        // Alternating ±1e-8 keeps the state out of denormals.
        self.denormal = -self.denormal;
        for s in self.st.iter_mut().flatten() {
            *s += self.denormal;
        }
        let k = self.limit;
        let lim = |g: f32| ((g.abs() + k) - (g - k).abs()) * -4.53236;
        let e_old = crate::fx::exp2(lim(self.g_prev));
        let frac = inv_n * n as f32;
        let g_new = (((f.abs() + 1.0) - (f - 1.0).abs()) * 0.5 - self.g_prev) * frac + self.g_prev;
        let r_old = self.r_prev;
        let r_new = frac * (r - r_old) + r_old;
        self.g_prev = g_new;
        self.r_prev = r_new;
        let e_new = crate::fx::exp2(lim(g_new));
        if n == 0 {
            return;
        }
        let (e0, e1) = (1.0 - e_old, 1.0 - e_new);
        let mut g = warp(e0);
        let mut rc = res_comp(e0) * r_old;
        let inv = 1.0 / n as f32;
        let dg = (warp(e1) - g) * inv;
        let dr = (res_comp(e1) * r_new - rc) * inv;
        let [c0, c1, c2, c3, c4] = self.mix;
        let [ol, or] = out;
        for i in 0..n {
            for (l, o) in [&mut *ol, &mut *or].into_iter().enumerate() {
                let [x0o, y1o, y2o, y3o, y4o] = [0, 1, 2, 3, 4].map(|j| self.st[j][l]);
                let lerp_at = |v: f32| {
                    let (j, fr) = lut_pos(v);
                    (t[j + 1] - t[j]) * fr + t[j]
                };
                let u = y4o * rc;
                let (j, _) = lut_pos(u);
                let tr = if u.is_nan() || u.abs() >= 2147483648.0 { i32::MIN as f32 } else { (u as i32) as f32 };
                let h = inp[l][i] * 48.0 - self.hp[l];
                self.hp[l] = h * self.hp_coef + self.hp[l];
                let x0 = (h - t[j]) + (t[j + 1] - t[j]) * (tr - u);
                let y1 = lerp_at((x0o * 0.26 - y1o) + x0 * 0.74) * g + y1o;
                let y2 = lerp_at((y1o * 0.26 - y2o) + y1 * 0.74) * g + y2o;
                let y3 = lerp_at((y2o * 0.26 - y3o) + y2 * 0.74) * g + y3o;
                let y4 = lerp_at((y3o * 0.26 - y4o) + y3 * 0.74) * g + y4o;
                o[i] = ((((y4 * c4 + y3 * c3) + y2 * c2) + y1 * c1) + x0 * c0) * 0.03125;
                for (j, v) in [x0, y1, y2, y3, y4].into_iter().enumerate() {
                    self.st[j][l] = v;
                }
            }
            rc += dr;
            g += dg;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_through(mode: usize, freq: f32) -> f32 {
        let sr = 44100.0;
        let mut f = Sh101::new(sr);
        f.initialize(0.5, 0.0, mode as f32 / 21.0);
        let x: Vec<f32> = (0..8192).map(|i| (i as f32 * freq / sr * std::f32::consts::TAU).sin() * 0.1).collect();
        let (mut y, mut y2) = (vec![0.0; 8192], vec![0.0; 8192]);
        for (c, (o, o2)) in x.chunks(64).zip(y.chunks_mut(64).zip(y2.chunks_mut(64))) {
            f.process([c, c], [o, o2], 1.0 / 64.0);
        }
        (y[4096..].iter().map(|v| v * v).sum::<f32>() / 4096.0).sqrt()
    }

    #[test]
    fn ladder_modes_filter() {
        // L4 passes lows, rejects highs; H4 the reverse.
        assert!(rms_through(1, 100.0) > 0.03);
        assert!(rms_through(1, 8000.0) < 0.005);
        assert!(rms_through(5, 100.0) < 0.01);
        assert!(rms_through(5, 8000.0) > 0.03);
    }
}
