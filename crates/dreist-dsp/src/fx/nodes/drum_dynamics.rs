//! [re:339ec8] (Drum Dynamics, kind 13; `Voice::Reset`
//! [re:6ed5a6], `PrepareStream` [re:5704f4], `ProcessBuffer` [re:f8ab3a]): the
//! `DrumDynamics` transient/sustain shaper side-chained by the left input
//! (the side-chain audio port is unconnected in effect chains), crossfaded
//! in from dry over the first block after a reset
//! (`vfcSIMD_VectorWetDryMixS_io` [re:e67f1c]).

use super::blocks::DrumDynamics as Shaper;
use crate::fx::{Ctx, Node, Port, ports, ports_from, scratch};

pub struct DrumDynamics {
    ports: Vec<Port>,
    sr: f32,
    dd: Shaper,
    last: [f32; 3],
    mix: f32,
    wet: [Vec<f32>; 2],
}

impl DrumDynamics {
    pub fn new() -> DrumDynamics {
        DrumDynamics { ports: ports_from(ports::DRUM_DYNAMICS), sr: 0.0, dd: Shaper::new(), last: [f32::NAN; 3], mix: 0.0, wet: [scratch(), scratch()] }
    }
}

impl Default for DrumDynamics {
    fn default() -> Self {
        DrumDynamics::new()
    }
}

/// `vfcSIMD_VectorWetDryMixS_io`: wet gain ramps `w0 → w1` (dry the
/// complement) in four-lane steps, the remainder continuing from lane 0.
pub fn wet_dry_mix(out: &mut [f32], wet: &[f32], dry: &[f32], w0: f32, w1: f32) {
    let n = out.len();
    if n == 0 {
        return;
    }
    let dw = (w1 - w0) / n as f32;
    let dd = ((1.0 - w1) - (1.0 - w0)) / n as f32;
    let n4 = n & !3;
    let mut w = [dw * 0.0 + w0, dw + w0, dw + dw + w0, dw * 3.0 + w0];
    let mut d = [dd * 0.0 + (1.0 - w0), (1.0 - w0) + dd, dd + dd + (1.0 - w0), dd * 3.0 + (1.0 - w0)];
    for k in (0..n4).step_by(4) {
        for j in 0..4 {
            out[k + j] = dry[k + j] * d[j] + wet[k + j] * w[j];
            w[j] += dw * 4.0;
            d[j] += dd * 4.0;
        }
    }
    let mut d = (1.0 - w0) + n4 as f32 * dd;
    let mut w = w0 + n4 as f32 * dw;
    for i in n4..n {
        out[i] = dry[i] * d + wet[i] * w;
        d += dd;
        w += dw;
    }
}

impl Node for DrumDynamics {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            self.dd.set_sample_rate(self.sr);
        }
        let p = [self.ports[0].value, self.ports[1].value, self.ports[2].value];
        if p != self.last {
            self.dd.init(p[0], p[1], p[2]);
            self.last = p;
        }
        let n = inp[0].len();
        for (w, x) in self.wet.iter_mut().zip(inp) {
            w.clear();
            w.extend_from_slice(x);
        }
        let [wl, wr] = &mut self.wet;
        self.dd.process(wl, wr, inp[0]);
        let [ol, or] = out;
        wet_dry_mix(&mut ol[..n], wl, inp[0], self.mix, 1.0);
        wet_dry_mix(&mut or[..n], wr, inp[1], self.mix, 1.0);
        self.mix = 1.0;
    }

    fn reset(&mut self) {
        self.mix = 0.0;
    }
}
