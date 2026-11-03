//! [re:c7fe15] (Classic Guru Filter, kind 37; processing
//! [re:c686b0]) around [re:9be7ff] (parameter set [re:c65d82], stereo processing
//! [re:541ab7]): a double-precision 12 dB state-variable filter run twice per
//! sample, its band-pass soft-limited (`x − k₃x³`, clamped at the turning
//! point), output a Mode-morphed mix (low → band → high) of both passes.
//!
//! The parameter set compares against cached parameters that it never stores, so it
//! recomputes (and dithers the states by ±1e-20) on every block unless all
//! three parameters are exactly zero.

use crate::fx::{Ctx, Node, Port, ports, ports_from};

#[derive(Default)]
struct Svf {
    g: f64,
    damp: f64,
    k3: f64,
    lim: f64,
    lp: [f64; 2],
    bp: [f64; 2],
    hp: [f64; 2],
    w_lp: f64,
    w_hp: f64,
    w_bp: f64,
}

impl Svf {
    fn set(&mut self, freq: f64, res: f64, mode: f64, ctx: &Ctx) {
        if freq == 0.0 && res == 0.0 && mode == 0.0 {
            return;
        }
        let f = (freq * 10.0 + -4.0).exp2();
        let d = (1.0 / ctx.sample_rate) as f64 * ctx.tune as f64 * f * 0.5;
        let w = ((d.abs() + 0.25) - (d + -0.25).abs()) * 0.5 * std::f64::consts::PI;
        let w = ((w * w * 0.00761 + -0.16605) * w * w + 1.0) * w;
        let g = w + w;
        self.g = g;
        let r = ((res * 1.01).abs() + res * 1.01) * 0.5;
        let lim = g * g * -0.75 + 2.0;
        let dmp = r.sqrt().sqrt();
        let dmp = 2.0 - (dmp + dmp);
        self.damp = ((dmp + lim) - (dmp - lim).abs()) * 0.5;
        self.k3 = (r + 0.001) * (g * 0.5 * g + g * 0.125);
        let l = (self.k3 * 3.0).sqrt() as f32;
        let la = l.abs();
        let mut y = f32::from_bits(0xbe6a3000u32.wrapping_sub(la.to_bits()) >> 1);
        y = (1.5 - y * y * la * 0.5) * y;
        y = (1.5 - y * y * la * 0.5) * y;
        let sign = if l.is_sign_negative() { -1.0f32 } else { 1.0 };
        self.lim = (sign * y * y) as f64;
        let dm = ((mode.abs() + 1.0) - (mode + -1.0).abs()) * 0.5;
        let dm = dm + dm;
        self.w_bp = (1.0 - (1.0 - dm).abs()).sqrt();
        self.w_lp = (1.0 - ((dm + 1.0) - (dm + -1.0).abs()) * 0.5) * 0.5;
        self.w_hp = (1.0 - ((dm + -1.0).abs() + dm + 1.0) * 0.5) * 0.5;
        for v in self.lp.iter_mut().chain(&mut self.bp) {
            *v = *v + 1e-20 + -1e-20;
        }
    }

    #[inline]
    fn sat(&self, v: f64) -> f64 {
        let l = self.lim;
        let c = ((v + l).abs() - (v - l).abs()) * 0.5;
        c - self.k3 * c * c * c
    }

    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let (g, dmp) = (self.g, self.damp);
        let [ol, or] = out;
        for i in 0..inp[0].len() {
            let mut y = [0.0f64; 2];
            for c in 0..2 {
                let x = inp[c][i] as f64;
                let lp1 = self.bp[c] * g + self.lp[c];
                let hp1 = (x - dmp * self.bp[c]) - lp1;
                let bp1 = self.sat(g * hp1 + self.bp[c]);
                let lp2 = g * bp1 + lp1;
                let hp2 = (x - dmp * bp1) - lp2;
                let l = self.lim;
                let b2 = g * hp2 + bp1;
                let b2 = ((l + b2).abs() - (b2 - l).abs()) * 0.5;
                let bp2 = b2 - self.k3 * b2 * b2 * b2;
                y[c] = self.w_hp * hp2 + self.w_bp * bp2 + self.w_lp * lp2 + hp1 * self.w_hp + self.w_bp * bp1 + lp1 * self.w_lp;
                self.bp[c] = bp2;
                self.lp[c] = lp2;
                self.hp[c] = hp2;
            }
            ol[i] = y[0] as f32;
            or[i] = y[1] as f32;
        }
    }
}

pub struct Guru {
    ports: Vec<Port>,
    svf: Svf,
}

impl Guru {
    pub fn new() -> Guru {
        Guru { ports: ports_from(ports::GURU_FILTER), svf: Svf::default() }
    }
}

impl Default for Guru {
    fn default() -> Self {
        Guru::new()
    }
}

impl Node for Guru {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value as f64;
        let (m, f, r) = (p(0), p(1), p(2));
        self.svf.set(f, r, m, ctx);
        self.svf.process(inp, out);
    }

    fn reset(&mut self) {
        self.svf.lp = [0.0; 2];
        self.svf.bp = [0.0; 2];
        self.svf.hp = [0.0; 2];
    }
}
