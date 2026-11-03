//! [re:3d22ad] (Stereo Width; port setup [re:675068],
//! processing [re:ef9b38], reset [re:e883e3]).

use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct StereoMixer {
    ports: Vec<Port>,
    gain: f32,
    width: f32,
}

impl StereoMixer {
    pub fn new() -> StereoMixer {
        StereoMixer { ports: ports_from(ports::STEREO_WIDTH), gain: 0.0, width: 1.0 }
    }
}

impl Default for StereoMixer {
    fn default() -> Self {
        StereoMixer::new()
    }
}

impl Node for StereoMixer {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// Reset [re:e883e3].
    fn reset(&mut self) {
        self.gain = 0.0;
        self.width = 1.0;
    }

    fn process(&mut self, _ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let inv = 1.0 / n as f32;
        let a = crate::fx::exp2(self.ports[0].value.clamp(-10.0, 10.0)) - 0.016367754;
        let dg = (a.abs() + a) * 0.5 - self.gain;
        let w = self.ports[1].value;
        let dw = (w.abs() + w) * 0.5 - self.width;
        let [ol, or] = out;
        for i in 0..n {
            self.width += dw * inv;
            self.gain += dg * inv;
            let k = self.width * 0.5 + 0.5;
            let (l, r) = (inp[0][i], inp[1][i]);
            ol[i] = (r * (1.0 - k) + l * k) * self.gain;
            or[i] = (k * r + (1.0 - k) * l) * self.gain;
        }
    }
}
