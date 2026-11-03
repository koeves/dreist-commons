//! [re:c38c8c] (Hi-Lo Balance, kind 6; reset [re:8ba832],
//! processing [re:44061e]): one-pole crossover; the low part is
//! weighted `cos(b·π/2)·√2`, the high part `sin(b·π/2)·√2`, then wet/dry.

use super::math::clip01;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct HiLo {
    ports: Vec<Port>,
    /// Ramped coefficient, low gain, high gain, mix.
    k: f32,
    lo: f32,
    hi: f32,
    mix: f32,
    lp: [f32; 2],
}

impl HiLo {
    pub fn new() -> HiLo {
        HiLo::with_ports(ports_from(ports::HI_LO))
    }

    /// As built around `ports` (no allocation).
    fn with_ports(ports: Vec<Port>) -> HiLo {
        HiLo { ports, k: 0.1, lo: 0.0, hi: 0.0, mix: 0.0, lp: [0.0; 2] }
    }
}

impl Default for HiLo {
    fn default() -> Self {
        HiLo::new()
    }
}

impl Node for HiLo {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let f = (self.ports[1].value + 1.0).exp2();
        let e = (((f + f) * 0.5) * -12.566371 * (1.0 / ctx.sample_rate)).exp();
        let a = (clip01(self.ports[0].value) as f64 * std::f64::consts::PI * 0.5) as f32;
        let (s, c) = (a.sin(), a.cos());
        let inv = 1.0 / n as f32;
        let dk = (1.0 - e) - self.k;
        let dlo = c * 1.4142135 - self.lo;
        let dhi = s * 1.4142135 - self.hi;
        let dm = clip01(self.ports[2].value) - self.mix;
        let [ol, or] = out;
        for i in 0..n {
            self.k += inv * dk;
            self.lo += inv * dlo;
            self.hi += inv * dhi;
            self.mix += inv * dm;
            let (x, y) = (inp[0][i], inp[1][i]);
            let hl = x - self.lp[0];
            let hr = y - self.lp[1];
            self.lp[0] = self.k * hl + self.lp[0];
            self.lp[1] = self.k * hr + self.lp[1];
            ol[i] = x * (1.0 - self.mix) + (hl * self.hi + self.lp[0] * self.lo) * self.mix;
            or[i] = (1.0 - self.mix) * y + (self.hi * hr + self.lp[1] * self.lo) * self.mix;
        }
    }

    fn reset(&mut self) {
        *self = HiLo::with_ports(std::mem::take(&mut self.ports));
    }
}
