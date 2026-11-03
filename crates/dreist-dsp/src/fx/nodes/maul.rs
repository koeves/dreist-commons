//! [re:e1d705] (Maul; `Voice::PrepareStream` [re:06ee28], `ProcessBuffer`
//! [re:5d97fa]): two parallel bands — low-pass and high-pass
//! ([re:e2af9b], single stage) each into its own
//! [re:fdc386] and gain — averaged and soft-saturated.

use super::blocks::{Gain, StereoIir};
use super::distortion::MultiModeDistortion;
use super::math::sat;
use crate::fx::{Ctx, Node, Port, ports, ports_from, scratch};

struct Band {
    filter: StereoIir,
    dist: MultiModeDistortion,
    gain: Gain,
}

impl Band {
    fn new(mode: u32) -> Band {
        let mut filter = StereoIir::default();
        filter.set_mode(mode);
        Band { filter, dist: MultiModeDistortion::default(), gain: Gain::default() }
    }

    /// `PrepareStream` for the band.
    fn prepare(&mut self, sr: f32, mode: u32) {
        self.dist.set_sample_rate(sr);
        self.filter.set_sample_rate(sr);
        self.filter.set_mode(mode);
    }
}

pub struct Maul {
    ports: Vec<Port>,
    sr: f32,
    bands: [Band; 2],
    tmp: [Vec<f32>; 2],
}

impl Maul {
    pub fn new() -> Maul {
        let mut ports = ports_from(ports::MAUL);
        // Type: 0..([re:d02059] items − 1).
        ports[0] = Port::new(0.0, 0.0, 32.0);
        Maul { ports, sr: 0.0, bands: [Band::new(0), Band::new(1)], tmp: [scratch(), scratch()] }
    }
}

impl Default for Maul {
    fn default() -> Self {
        Maul::new()
    }
}

impl Node for Maul {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            for (b, m) in self.bands.iter_mut().zip([0, 1]) {
                b.prepare(self.sr, m);
            }
        }
        let p = |i: usize| self.ports[i].value;
        let t = p(0) / 32.0;
        let (lo, hi) = ((p(1), p(2), p(3), p(4)), (p(5), p(6), p(7), p(8)));
        for (b, (f, q, d, g)) in self.bands.iter_mut().zip([lo, hi]) {
            b.dist.init(d, t);
            b.filter.set_01(f, q);
            b.gain.init(g * 1.5835);
        }
        let [ol, or] = out;
        let [tl, tr] = &mut self.tmp;
        for (k, b) in self.bands.iter_mut().enumerate() {
            tl.clear();
            tl.extend_from_slice(inp[0]);
            tr.clear();
            tr.extend_from_slice(inp[1]);
            b.filter.process(tl, tr);
            b.dist.process(tl, tr);
            b.gain.process(tl, tr);
            if k == 0 {
                ol.copy_from_slice(tl);
                or.copy_from_slice(tr);
            } else {
                for i in 0..ol.len() {
                    ol[i] = tl[i] * 0.5 + ol[i] * 0.5;
                    or[i] = tr[i] * 0.5 + or[i] * 0.5;
                }
            }
        }
        for v in ol.iter_mut().chain(or.iter_mut()) {
            *v = sat(*v);
        }
    }
}
