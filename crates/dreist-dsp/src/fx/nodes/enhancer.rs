//! [re:66e923] (Enhancer, kind 7; `Voice::Reset` [re:68dc02],
//! `Voice::ProcessBuffer` [re:84f941]): high-pass at `Frequency`, fold the
//! part above an 80 Hz-smoothed envelope (`k = (1 − Mode)·16 − 4`), high-pass
//! again and add `2^(Amount − 0.016367754)` of it to the dry signal.

use super::blocks::{PolySect, highpass_poly, lowpass_poly};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct Enhancer {
    ports: Vec<Port>,
    env: PolySect,
    hp1: PolySect,
    hp2: PolySect,
}

impl Enhancer {
    pub fn new() -> Enhancer {
        let mut ports = ports_from(ports::ENHANCER);
        // Int-form ctor: Mode 0..3 ([re:922f8d] I–IV).
        ports[1] = Port::new(0.0, 0.0, 3.0);
        Enhancer { ports, env: PolySect::new(), hp1: PolySect::new(), hp2: PolySect::new() }
    }
}

impl Default for Enhancer {
    fn default() -> Self {
        Enhancer::new()
    }
}

impl Node for Enhancer {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let isr = 1.0 / ctx.sample_rate;
        let amount = (p(2) + -0.016367754).exp2();
        let c_env = lowpass_poly(isr * 80.0);
        let c_hp = highpass_poly(isr * p(0));
        let mix = ((p(3).abs() + 1.0) - (p(3) - 1.0).abs()) * 0.5;
        let k = (1.0 - p(1)) * 16.0 + -4.0;
        let [ol, or] = out;
        for i in 0..inp[0].len() {
            let x = [inp[0][i], inp[1][i]];
            let h = self.hp1.run(&c_hp, x);
            let e = self.env.run(&c_env, [h[0].abs(), h[1].abs()]);
            let mut f = [0.0f32; 2];
            for c in 0..2 {
                let d = h[c] - e[c] * k;
                let d = if 0.0 <= d { d } else { 0.0 };
                f[c] = h[c] - (d + d);
            }
            let y = self.hp2.run(&c_hp, f);
            ol[i] = (y[0] * amount + x[0]) * mix + x[0] * (1.0 - mix);
            or[i] = (y[1] * amount + x[1]) * mix + x[1] * (1.0 - mix);
        }
    }

    fn reset(&mut self) {
        for s in [&mut self.env, &mut self.hp1, &mut self.hp2] {
            s.clear();
        }
    }
}
