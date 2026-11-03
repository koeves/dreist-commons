//! [re:d1f784] (VCA Compressor, kind 10; reset
//! [re:610c55], processing [re:a3c763]): peak detector floored at the
//! threshold, one-pole attack/release (`exp(−ln2/(t·sr))`), static curve
//! `gr = dB − ((dB − thr)/ratio + thr)` with ratio = Ratio port + 1 (the
//! port holds `log2` of the displayed ratio, so "4" gives 3:1), makeup and
//! wet/dry. The gain-reduction meter smoothing is reproduced for the
//! control output only.

use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct VcaComp {
    ports: Vec<Port>,
    env: f32,
    meter: [f32; 3],
}

impl VcaComp {
    pub fn new() -> VcaComp {
        VcaComp { ports: ports_from(ports::VCA_COMPRESSOR), env: 0.25, meter: [0.0; 3] }
    }
}

impl Default for VcaComp {
    fn default() -> Self {
        VcaComp::new()
    }
}

impl Node for VcaComp {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let in_gain = 10f32.powf(p(3) / 20.0);
        let makeup = 10f32.powf(p(4) / 20.0);
        let mix = p(5);
        let thr_db = p(2);
        let thr = 10f32.powf(thr_db / 20.0);
        let ratio = p(6) + 1.0;
        let sr = ctx.sample_rate as f64;
        let att = (p(0) + -1.0).exp2();
        let rel = (p(1) + -1.0).exp2();
        let ka = (((-std::f64::consts::LN_2) / att as f64) / sr).exp() as f32;
        let kr = (((-std::f64::consts::LN_2) / rel as f64) / sr).exp() as f32;
        // External side-chain inputs are unconnected in effect chains.
        let n = inp[0].len();
        let mut max_gr = 0.0f32;
        let mut peak_gr = 0.0f32;
        let [ol, or] = out;
        for i in 0..n {
            let a = (inp[0][i] * in_gain).abs();
            let b = (inp[1][i] * in_gain).abs();
            let pk = if b <= a { a } else { b };
            let level = if thr <= pk { pk } else { thr };
            let k = if level <= self.env { kr } else { ka };
            self.env += (level - self.env) * (1.0 - k);
            let g = if thr <= self.env {
                let db = self.env.log10() * 20.0;
                let gr = db - ((db - thr_db) * (1.0 / ratio) + thr_db);
                if max_gr <= gr {
                    peak_gr = gr;
                    max_gr = gr;
                } else {
                    max_gr = peak_gr;
                }
                10f32.powf(gr * -0.05)
            } else {
                1.0
            };
            let (x, y) = (inp[0][i], inp[1][i]);
            let dry = 1.0 - mix;
            ol[i] = makeup * x * g * mix + x * dry;
            or[i] = makeup * y * g * mix + dry * y;
        }
        let m = (max_gr / -60.0 + 1.0).max(0.0);
        let k = (n as f32 * (1.0 / ctx.sample_rate) * -31.415928).exp();
        self.meter[0] = (m - self.meter[0]) * (1.0 - k) + self.meter[0];
        self.meter[1] = (self.meter[0] - self.meter[1]) * (1.0 - k) + self.meter[1];
        self.meter[2] = ((self.meter[1].abs() + 1.0) - (self.meter[1] - 1.0).abs()) * 0.5;
    }

    fn reset(&mut self) {
        self.env = 0.25;
    }
}
