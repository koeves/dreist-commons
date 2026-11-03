//! [re:d40288] (Driver; `Voice::ProcessBuffer` [re:24da21]) and
//! [re:0ca5a3] (Shaped Distortion; [re:5dade8], with
//! [re:8f2521]..[re:856f3d] and [re:31ef56]
//! [re:d39957]..[re:837667]) built on [re:fdc386].

use super::blocks::{DrumDynamics, DualPole, Gain, StereoLimiter, WetDry};
use super::distortion::MultiModeDistortion;
use super::math::sat;
use crate::fx::{Ctx, Node, Port, ports, ports_from, scratch};

fn saturate(l: &mut [f32], r: &mut [f32]) {
    for v in l.iter_mut().chain(r.iter_mut()) {
        *v = sat(*v);
    }
}

/// Driver: input gain → distortion → tone-drive gain → saturate → limit.
pub struct Driver {
    ports: Vec<Port>,
    sr: f32,
    dist: MultiModeDistortion,
    gain_in: Gain,
    gain_out: Gain,
    limiter: StereoLimiter,
}

impl Driver {
    pub fn new() -> Driver {
        let mut ports = ports_from(ports::MAULT);
        ports[2] = Port::new(0.0, 0.0, 32.0);
        Driver { ports, sr: 0.0, dist: MultiModeDistortion::default(), gain_in: Gain::default(), gain_out: Gain::default(), limiter: StereoLimiter::default() }
    }
}

impl Default for Driver {
    fn default() -> Self {
        Driver::new()
    }
}

impl Node for Driver {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            self.dist.set_sample_rate(self.sr);
            self.limiter.set_sample_rate(self.sr);
        }
        let p = |i: usize| self.ports[i].value;
        self.dist.init(p(0), p(2) / 32.0);
        self.gain_in.init(p(1) * 1.5835);
        self.gain_out.init(p(3) * 1.5835);
        let [ol, or] = out;
        ol.copy_from_slice(inp[0]);
        or.copy_from_slice(inp[1]);
        self.gain_in.process(ol, or);
        self.dist.process(ol, or);
        self.gain_out.process(ol, or);
        if p(4) >= 0.5 {
            saturate(ol, or);
        }
        if p(5) >= 0.5 {
            self.limiter.process(ol, or);
        }
    }
}

/// [re:31ef56]: gain → drum dynamics (side-chained by the
/// gained left channel) → distortion → gain.
struct Companding {
    dist: MultiModeDistortion,
    gain_in: Gain,
    gain_out: Gain,
    dyn_: DrumDynamics,
    dyn_on: bool,
    release: f32,
    /// Side-chain copy, reused between blocks.
    sc: Vec<f32>,
}

impl Companding {
    fn new() -> Companding {
        Companding { dist: MultiModeDistortion::default(), gain_in: Gain::default(), gain_out: Gain::default(), dyn_: DrumDynamics::new(), dyn_on: false, release: 0.5, sc: scratch() }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.dist.set_sample_rate(sr);
        self.dyn_.set_sample_rate(sr);
        // SetMode(1).
        self.release = 0.5;
    }

    fn init(&mut self, dynamics: f32, drive: f32, gain_in: f32, gain_out: f32, type01: f32) {
        self.dyn_on = 1e-6 <= (dynamics + -0.5).abs();
        self.dyn_.init(1.0 - dynamics, dynamics, self.release);
        self.dist.init(drive, type01);
        self.gain_in.init(gain_in);
        self.gain_out.init(gain_out);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.gain_in.process(l, r);
        if self.dyn_on {
            self.sc.clear();
            self.sc.extend_from_slice(l);
            self.dyn_.process(l, r, &self.sc);
        }
        self.dist.process(l, r);
        self.gain_out.process(l, r);
    }
}

/// Shaped Distortion.
pub struct ShapedDistortion {
    ports: Vec<Port>,
    sr: f32,
    comp: Companding,
    filter: DualPole,
    filter_on: bool,
    chan_mix: WetDry,
    level: Gain,
    mix: WetDry,
    limiter: StereoLimiter,
    /// Dry copy for the channel mixer, reused between blocks.
    dry: [Vec<f32>; 2],
}

impl ShapedDistortion {
    pub fn new() -> ShapedDistortion {
        let mut ports = ports_from(ports::SINGLE_MAULT);
        ports[3] = Port::new(0.0, 0.0, 32.0);
        let mut filter = DualPole::default();
        filter.dual = false;
        ShapedDistortion {
            ports,
            sr: 0.0,
            comp: Companding::new(),
            filter,
            filter_on: false,
            chan_mix: WetDry::default(),
            level: Gain::default(),
            mix: WetDry::default(),
            limiter: StereoLimiter::default(),
            dry: [scratch(), scratch()],
        }
    }
}

impl Default for ShapedDistortion {
    fn default() -> Self {
        ShapedDistortion::new()
    }
}

impl Node for ShapedDistortion {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            self.comp.set_sample_rate(self.sr);
            self.filter.set_sample_rate(self.sr);
            self.filter.set_mode(0, 1);
            self.limiter.set_sample_rate(self.sr);
        }
        let p = |i: usize| self.ports[i].value;
        // [re:d7464c].
        self.comp.init(p(2), p(0), p(1) * 1.5835, p(4) * 1.5835, p(3) / 32.0);
        self.filter.init(p(6), p(7), 0.5, false);
        self.chan_mix.init(1.0, 0.5012318 * 1.5835);
        let m = p(5) * 3.0;
        let mode = (((m.abs() + 3.0) - (m - 3.0).abs()) * 0.5).round() as u32;
        match mode {
            3 => self.filter.set_mode(1, 1),
            2 => self.filter.set_mode(0, 1),
            1 => self.filter.set_mode(0, 0),
            _ => {}
        }
        self.filter.dual = mode == 2;
        self.filter_on = mode != 0;
        self.level.init(p(10) * 1.5835);
        self.mix.init(p(11), 0.7937005);
        let [ol, or] = out;
        ol.copy_from_slice(inp[0]);
        or.copy_from_slice(inp[1]);
        // [re:fc550e] (the dry copy and the output share the
        // buffer, so its mixer only applies the gain).
        self.comp.process(ol, or);
        saturate(ol, or);
        if self.filter_on {
            self.filter.process(ol, or);
        }
        let [dl, dr] = &mut self.dry;
        dl.clear();
        dl.extend_from_slice(ol);
        dr.clear();
        dr.extend_from_slice(or);
        self.chan_mix.process([&dl[..], &dr[..]], [&mut *ol, &mut *or]);
        self.level.process(ol, or);
        if p(8) >= 0.5 {
            saturate(ol, or);
        }
        self.mix.process(inp, [&mut *ol, &mut *or]);
        if p(9) >= 0.5 {
            self.limiter.process(ol, or);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_and_shaped_are_finite_and_nonzero() {
        let ctx = Ctx::new(48000.0);
        let x: Vec<f32> = (0..512).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
        let mut d = Driver::new();
        let mut s = ShapedDistortion::new();
        for (k, node) in [&mut d as &mut dyn Node, &mut s as &mut dyn Node].into_iter().enumerate() {
            let mut e = 0.0;
            for _ in 0..8 {
                let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
                node.process(&ctx, [&x, &x], [&mut l, &mut r]);
                assert!(l.iter().all(|v| v.is_finite()), "node {k}");
                e = l.iter().map(|v| v * v).sum::<f32>();
            }
            assert!(e > 1e-3, "node {k}: {e}");
        }
    }
}
