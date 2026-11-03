//! [re:0585f8] (Channel Compressor; processing
//! [re:ab0032]) driving [re:f585ec] ([re:8d51bb]..[re:364f59]): a 1176-style
//! FET compressor solved as a circuit — the FET gain stage solve (Newton
//! on a piecewise square-law FET, ≤10 iterations) per channel, then the
//! shared detector solve (diode rectifier into the attack/release RC,
//! ≤10 iterations) whose control voltage feeds the next sample's FET.

use crate::fx::{Ctx, Node, Port, ports, ports_from};

const VCRIT: f32 = 0.7871292;

/// FET gain stage solve for `(v1, v2, x, vctl)`.
fn solve_1176a(v1: &mut f32, v2: &mut f32, x: f32, vctl: f32) {
    let mut f4 = *v2;
    let mut f5 = *v1;
    let mut it = 0u32;
    loop {
        let f3 = f4 - (f4 - f5);
        let (mut f6, mut f7, mut f8) = (0.0f32, 0.0f32, 0.0f32);
        if f3 <= 0.0 {
            let f2 = (f4 - f5) + 1.7372;
            if 0.0 <= f2 {
                f6 = f3 * -0.0177211 + 1.0;
                if -f3 <= f2 {
                    f6 *= 0.00116621;
                    let f9 = f2 + f2 + f3;
                    f8 = f9 * f6 * f3;
                    f7 = f3 * (f6 + f6);
                    f6 = (f6 + f6) * f2 + f3 * -2.0666525e-05 * f9;
                } else {
                    let g = f6 * -0.00116621;
                    f8 = f2 * g * f2;
                    f7 = (g + g) * f2;
                    f6 = (f6 * 0.00116621 + f6 * 0.00116621) * f2 + f2 * 2.0666525e-05 * f2;
                }
            }
        } else {
            let f2 = f4 + 1.7372;
            if 0.0 <= f2 {
                f6 = (f3 * 0.0177211 + 1.0) * 0.00116621;
                if f3 <= f2 {
                    let f9 = (f2 + f2) - f3;
                    f8 = f9 * f6 * f3;
                    f7 = f3 * (f6 + f6);
                    f6 = f3 * 2.0666525e-05 * f9 + (f2 - f3) * (f6 + f6);
                } else {
                    f8 = f2 * f6 * f2;
                    f7 = (f6 + f6) * f2;
                    f6 = f2 * 2.0666525e-05 * f2;
                }
            }
        }
        let f2 = ((f8 - f4 * f7) - f3 * f6) * -10000.0;
        let f3b = f6 * 10000.0 + 1.0;
        let inv = 1.0 / ((f7 * 0.001 * 10000.0 * 1250.0 + f3b) * 2.2e6 + f6 * 10000.0 * 2.20125e6 + 2.20125e6);
        let n1 = ((x - f7 * 10000.0 * vctl * 1.75) * 2.20125e6 + f2 * 4.40125e6 + x * 2.2e6) * inv;
        let n2 = (f3b * 2.20125e6 * vctl * 1.75 + (f2 + x) * 2.75e6) * inv;
        *v1 = n1;
        *v2 = n2;
        let more = (n1 * 0.001).abs() < (n1 - f5).abs() || (n2 * 0.001).abs() < (n2 - f4).abs();
        it += 1;
        if !more || it >= 10 {
            return;
        }
        f4 = n2;
        f5 = n1;
    }
}

/// Detector solve for `(v1, v2, v3, in_l, in_r, attack, release, c, scale, sr)`.
#[allow(clippy::too_many_arguments)]
fn solve_drcr2(v1: &mut f32, v2: &mut f32, v3: &mut f32, p4: f32, p5: f32, p6: f32, p7: f32, p8: f32, p9: f32, p10: f32) {
    let f9 = p8 * p10;
    let f12 = -(f9 * *v3);
    let f13 = f9 * p7 + 1.0;
    let f10 = f9 * p6 * p7;
    let f14 = p7 + p7 + p6 + f10;
    let mut f11 = *v1;
    let mut f2 = *v3;
    let mut it = 0u32;
    loop {
        let f1 = *v2;
        let a = p4 - f11;
        let b = p5 - f1;
        let f7 = ((a + VCRIT) - (a - VCRIT).abs()) * 0.5;
        let f8 = ((b + VCRIT) - (b - VCRIT).abs()) * 0.5;
        let e1 = (f7 * 38.791595).exp();
        let e2 = (f8 * 38.791595).exp();
        let f15 = e1 * 1e-15 * 38.791595;
        let f16 = e2 * 1e-15 * 38.791595;
        let f19 = (e1 * 1e-15 + -1e-15) - f7 * f15;
        let f8 = (e2 * 1e-15 + -1e-15) - f8 * f16;
        let f6 = f16 * p6;
        let f17 = f6 + 1.0;
        let f18 = f15 * p6 + 1.0;
        let inv = 1.0 / ((f9 * f18 * f17 + (f15 + f15) * f16 * p6 + f15 + f16) * p7 + f18 * f17);
        let f18 = f18 * f12;
        let f7 = (f14 * f16 + f13) * p6 + p7;
        let n1 = (f6 * p9 + f16 * p7 * p5 + f7 * f15 * p4 + f19 * f7 + (f8 * p7 - f17 * f12 * p7) + p9) * inv;
        let n2 = (((f6 * p5 + p9) * p6 + ((f9 * p6 + 2.0) * f6 * p5 + p4) * p7) * f15 + (p6 + p7 + f10) * f16 * p5 + ((f14 * f15 + f13) * p6 + p7) * f8 + (f19 * p7 - f18 * p7) + p9) * inv;
        let n3 = ((f15 * f16 * p6 + f15 + f16) * p6 * p9 + (p5 * f16 + ((f16 * f19 * p6 + f19 + f8) - f18 * f17) + (((p4 + p5) * f16 + f8) * p6 + p4) * f15) * p7 + p9) * inv;
        *v1 = n1;
        *v2 = n2;
        *v3 = n3;
        if (n1 - f11).abs() <= (n1 * 0.001).abs() && (n2 - f1).abs() <= (n2 * 0.001).abs() && (n3 - f2).abs() <= (n3 * 0.001).abs() {
            return;
        }
        it += 1;
        f11 = n1;
        f2 = n3;
        if 9 < it {
            return;
        }
    }
}

/// [re:c17f34] state.
struct Comp {
    inv_sr: f32,
    alt: f32,
    ratio: u32,
    attack: f32,
    release: f32,
    q: f32,
    meter: [f32; 3],
    in_cur: f32,
    in_db: f32,
    out_db: f32,
    mix: f32,
    hp: [f32; 2],
    fet: [[f32; 2]; 2],
    det: [f32; 3],
}

impl Comp {
    fn new() -> Comp {
        Comp {
            inv_sr: f32::from_bits(0x37be37c6),
            alt: 1e-10,
            ratio: 0,
            attack: 8200.0,
            release: 560000.0,
            q: 0.5,
            meter: [1.0; 3],
            in_cur: 0.0,
            in_db: 0.0,
            out_db: 1.0,
            mix: 1.0,
            hp: [0.0; 2],
            fet: [[0.0; 2]; 2],
            det: [-1.6452; 3],
        }
    }

    /// `process` [re:364f59].
    fn process(&mut self, sr: f32, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.inv_sr = 1.0 / sr;
        let n = inp[0].len();
        let mut out_g = 10f32.powf((self.out_db - 5.0) / 20.0);
        let (mut off, mut scale, mut gain) = (0.0f32, 0.0f32, 0.0f32);
        if self.ratio < 5 {
            let r1 = (1.0 - self.q) * 2000.0;
            let r2 = self.q * 2000.0;
            let a = r2 * 13900.0;
            let f8 = (a + 3.9e7) * 150.0;
            let f3 = (a + 585000.0 + 4.05e7) * r1;
            let s = r2 + r1;
            let t = s + 10000.0;
            let (den, num_s, num_o);
            match self.ratio {
                0 => {
                    den = (t * 47000.0 * 3900.0 + s * 47000.0 * 10000.0 + f8 + f3) * 4730.0 + (f8 + f3) * 47000.0;
                    num_s = ((a + 5.8032e7) * 47000.0 + 2.76705e9) * r1 * -10.0;
                    num_o = ((a + 5.7447e7 + 2.085e6) * r1 + f8) * 47000.0 * -10.0;
                    gain = 0.16607773;
                }
                1 => {
                    den = (t * 47000.0 * 3900.0 + s * 47000.0 * 10000.0 + f8 + f3) * 4260.0 + (f8 + f3) * 47000.0 + (t * 3900.0 + s * 10000.0) * 2.40922e7;
                    num_s = ((a + 5.8032e7) * 47000.0 + 1.030068e10) * r1 * -10.0;
                    num_o = ((a + 5.5614e7 + 2.085e6) * r1 + t * 470.0 * 3900.0 + s * 470.0 * 10000.0 + f8) * 47000.0 * -10.0;
                    gain = 0.36395758;
                }
                2 => {
                    den = (t * 47000.0 * 3900.0 + s * 47000.0 * 10000.0 + f8 + f3) * 3700.0 + (f8 + f3) * 47000.0 + (t * 3900.0 + s * 10000.0) * 5.2221e7;
                    num_s = ((a + 5.8032e7) * 47000.0 + 1.70274e10) * r1 * -10.0;
                    num_o = ((a + 5.343e7 + 2.085e6) * r1 + t * 1030.0 * 3900.0 + s * 1030.0 * 10000.0 + f8) * 47000.0 * -10.0;
                    gain = 0.56183743;
                }
                3 => {
                    den = (t * 47000.0 * 3900.0 + s * 47000.0 * 10000.0 + f8 + f3) * 2200.0 + (f8 + f3) * 47000.0 + (t * 3900.0 + s * 10000.0) * 1.24476e8;
                    num_s = ((a + 5.8032e7) * 47000.0 + 2.29944e10) * r1 * -10.0;
                    num_o = ((a + 4.758e7 + 2.085e6) * r1 + t * 2530.0 * 3900.0 + s * 2530.0 * 10000.0 + f8) * 47000.0 * -10.0;
                    gain = 0.80212015;
                }
                _ => {
                    den = (t * 47000.0 * 3900.0 + s * 47000.0 * 10000.0 + f8 + f3) * 2200.0 + (f8 + f3) * 47000.0;
                    num_s = ((a + 4.8165e7) * 47000.0 + 1.287e9) * r1 * -10.0;
                    num_o = ((a + 4.758e7 + 2.085e6) * r1 + f8) * 47000.0 * -10.0;
                    gain = 0.4563107;
                }
            }
            scale = num_s / den;
            off = num_o / den;
        }
        let k_hp = (self.inv_sr * -62.831856).exp();
        let d = self.alt;
        self.alt = -d;
        self.det[2] -= d;
        self.hp[0] -= d;
        self.hp[1] -= d;
        self.meter[0] -= d;
        self.meter[1] -= d;
        let mut peak = -1e10f32;
        let [ol, or] = out;
        if n > 0 {
            let rel = self.release * 0.5;
            out_g *= self.mix;
            let dry = 1.0 - self.mix;
            let step = self.in_db - self.in_cur;
            for i in 0..n {
                let vctl = self.det[2];
                self.in_cur += (1.0 / n as f32) * step;
                let g = 10f32.powf((self.in_cur - 10.0) / 20.0);
                let (xl, xr) = (inp[0][i], inp[1][i]);
                let [fl, fr] = &mut self.fet;
                let [a, b] = fl;
                solve_1176a(a, b, xl * g, vctl);
                let [a, b] = fr;
                solve_1176a(a, b, g * xr, vctl);
                let hl = fl[0] - self.hp[0];
                let hr = fr[0] - self.hp[1];
                self.hp[0] += (1.0 - k_hp) * hl;
                self.hp[1] += (1.0 - k_hp) * hr;
                let sl = hl * 21.0;
                let sr_ = hr * 21.0;
                let al = (gain * sl * 8.912509).abs();
                let ar = (gain * sr_ * 8.912509).abs();
                let [d1, d2, d3] = &mut self.det;
                solve_drcr2(d1, d2, d3, ((al + 10.0) - (al - 10.0).abs()) * 0.5 + off, ((ar + 10.0) - (ar - 10.0).abs()) * 0.5 + off, self.attack, rel, 4.4e-7, scale, sr);
                ol[i] = xl * dry + sl * out_g;
                or[i] = xr * dry + sr_ * out_g;
                peak = if self.det[2] <= peak { peak } else { self.det[2] };
            }
        }
        let k = (self.inv_sr * -31.415928 * n as f32).exp();
        let m1 = ((1.0 - (scale - peak) / scale) - self.meter[0]) * (1.0 - k) + self.meter[0];
        self.meter[0] = m1;
        self.meter[1] += (m1 - self.meter[1]) * (1.0 - k);
        let m = self.meter[1];
        self.meter[2] = ((m.abs() + 1.0) - (m - 1.0).abs()) * 0.5;
    }
}

pub struct Urei1176 {
    ports: Vec<Port>,
    comp: Comp,
}

impl Urei1176 {
    pub fn new() -> Urei1176 {
        let mut ports = ports_from(ports::CHANNEL_COMPRESSOR);
        // Ratio: int ctor over [re:b70f73] (5 items).
        ports[5] = Port::new(0.0, 0.0, 4.0);
        Urei1176 { ports, comp: Comp::new() }
    }

    /// Gain-reduction meter (control output).
    pub fn meter(&self) -> f32 {
        self.comp.meter[2]
    }
}

impl Default for Urei1176 {
    fn default() -> Self {
        Urei1176::new()
    }
}

impl Node for Urei1176 {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let c = &mut self.comp;
        c.in_db = p(0);
        c.out_db = p(6);
        c.attack = ((p(1) - 1.0).exp2() * 5.2) / 2.2e-7;
        c.release = (p(2) - 1.0).exp2() / 2.2e-7;
        let q = p(3);
        c.q = if q < 0.0001 { 0.0001 } else { q.min(0.9999) };
        let r = p(5);
        c.ratio = (((r.abs() + 4.0) - (r - 4.0).abs()) * 0.5).round() as u32;
        let m = p(7);
        c.mix = ((m.abs() + 1.0) - (m - 1.0).abs()) * 0.5;
        if p(8) >= 0.5 {
            c.process(ctx.sample_rate, inp, out);
        } else {
            let [ol, or] = out;
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_loud_signal() {
        let sr = 48000.0;
        let ctx = Ctx::new(sr);
        let run = |amp: f32| {
            let mut u = Urei1176::new();
            u.ports[0].value = 20.0;
            let n = 256;
            let mut last = 0.0f32;
            for b in 0..40 {
                let x: Vec<f32> = (0..n).map(|i| ((b * n + i) as f32 * std::f32::consts::TAU * 200.0 / sr).sin() * amp).collect();
                let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
                u.process(&ctx, [&x, &x], [&mut l, &mut r]);
                assert!(l.iter().all(|v| v.is_finite()));
                last = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            }
            last
        };
        let (quiet, loud) = (run(0.01), run(0.5));
        // 34 dB more input should give much less than 34 dB more output.
        assert!(loud / quiet < 20.0, "quiet {quiet} loud {loud}");
        assert!(loud > quiet);
    }
}
