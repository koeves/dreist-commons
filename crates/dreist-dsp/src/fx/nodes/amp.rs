//! [re:828d0f] ([re:83a34d]), the Gain effect
//! (`InitializePorts` [re:c2ae81], `Voice::ProcessBuffer` [re:cc6036]).

use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct Amp {
    ports: Vec<Port>,
    gain: f32,
}

impl Amp {
    pub fn new() -> Amp {
        Amp { ports: ports_from(ports::GAIN), gain: 0.0 }
    }
}

impl Default for Amp {
    fn default() -> Self {
        Amp::new()
    }
}

impl Node for Amp {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// `Voice::Reset` [re:cff9b2].
    fn reset(&mut self) {
        self.gain = 0.0;
    }

    fn process(&mut self, _ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        // AmpLog domain: amplitude = 2^x − 0.016367754, floored at 0.
        let x = self.ports[0].value.clamp(-10.0, 10.0);
        let a = x.exp2() - 0.016367754;
        let target = ((a.abs() + a) * 0.5 - self.gain) * (1.0 / n as f32);
        let [ol, or] = out;
        for i in 0..n {
            self.gain += target;
            ol[i] = self.gain * inp[0][i];
            or[i] = self.gain * inp[1][i];
        }
    }
}
