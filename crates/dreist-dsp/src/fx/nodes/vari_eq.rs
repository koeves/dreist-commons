//! [re:95343c] (Vari-EQ; `InitializePorts` [re:c30681],
//! `Voice::ProcessBuffer` [re:affd00]): one RBJ biquad per channel.

use super::biquad::{Biquad, EQ_MODES};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct VariEq {
    ports: Vec<Port>,
    f: [Biquad; 2],
}

impl VariEq {
    pub fn new() -> VariEq {
        let mut ports = ports_from(ports::VARI_EQ);
        ports[0].max = (EQ_MODES - 1) as f64;
        VariEq { ports, f: [Biquad::default(); 2] }
    }
}

impl Default for VariEq {
    fn default() -> Self {
        VariEq::new()
    }
}

impl Node for VariEq {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// `Voice::Reset` [re:193fb7]: both filters' coefficients and history.
    fn reset(&mut self) {
        self.f = [Biquad::default(); 2];
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let m = self.ports[0].value;
        let mode = (((m.abs() + 8.0) - (m - 8.0).abs()) * 0.5).round() as i32;
        let f = (self.ports[1].value + 1.0).exp2();
        let (q, g) = (self.ports[2].value, self.ports[3].value);
        let fs = (1.0 / ctx.sample_rate) * f * 400.0;
        for b in &mut self.f {
            b.set(mode, fs, q, g);
        }
        let [ol, or] = out;
        for i in 0..inp[0].len() {
            ol[i] = self.f[0].tick(inp[0][i]);
            or[i] = self.f[1].tick(inp[1][i]);
        }
    }
}
