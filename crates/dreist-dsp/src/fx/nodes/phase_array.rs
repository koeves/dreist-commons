//! [re:5789e6] (PhaseMod Array, kind 27; `Reset` [re:8108e3],
//! `ProcessBuffer` [re:3f15df]): up to four sine-like oscillators (folded
//! triangle through `(x − x³)·2.6`) phase-modulated by the input (left
//! drives 1 and 3, right 2 and 4), detuned by Spread in one of three
//! patterns, weighted by Stack (1–4 voices, `1/√stack` with a fractional
//! last voice), panned by Width and scaled by an input envelope follower
//! ([re:fd576d]; the Attack/Release ports
//! are not used).

use crate::fx::{Ctx, Node, Port, ports, ports_from};

#[inline]
fn osc(phase: f32, pm: f32) -> f32 {
    let s = phase + pm;
    let mut v = s - s.floor();
    if 0.5 < v {
        v -= 1.0;
    }
    if 0.25 < v {
        v -= (v + -0.25) + (v + -0.25);
    }
    if v < -0.25 {
        v -= (v + 0.25) + (v + 0.25);
    }
    let v = v * 2.32;
    (v - v * v * v) * f32::from_bits(0x40266666)
}

pub struct PhaseArray {
    ports: Vec<Port>,
    env: f32,
    phase: [f32; 4],
    weight: [f32; 4],
}

impl PhaseArray {
    pub fn new() -> PhaseArray {
        let mut ports = ports_from(ports::PHASE_ARRAY);
        // Int-form ctor: Mode 0..2.
        ports[2] = Port::new(0.0, 0.0, 2.0);
        PhaseArray { ports, env: 0.0, phase: [0.0; 4], weight: [0.0; 4] }
    }
}

impl Default for PhaseArray {
    fn default() -> Self {
        PhaseArray::new()
    }
}

impl Node for PhaseArray {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let sr = ctx.sample_rate;
        let isr = 1.0 / sr;
        let mode = (((p(2).abs() + 2.0) - (p(2) + -2.0).abs()) * 0.5).round() as i32;
        let f1 = p(1);
        let s3 = p(5) * p(5) * p(5);
        // [re:8872a7](0.05, 0.25, sr).
        let ta = (0.05f32 * 12.0 + -8.0 + -1.0).exp2();
        let tr = (0.25f32 * 12.0 + -8.0 + -1.0).exp2();
        let c_att = 0.01f32.powf(1.0 / (ta * sr));
        let c_rel = 0.01f32.powf(1.0 / (tr * sr));
        let (p2, p3, p4) = match mode {
            2 => {
                let t = ((s3 + -0.083333336).abs() + 1.0) - (s3 + -0.083333336 + -1.0).abs();
                (t * 0.5 + f1, f1 - s3, (s3 * 0.083333336 + f1) - s3 * 0.5)
            }
            1 => {
                let t = (((0.083333336 - s3) + 1.0).abs() + -1.0) - (0.083333336 - s3).abs();
                (t * 0.5 + f1, f1 + s3, s3 * 0.5 + s3 * 0.083333336 + f1)
            }
            _ => (f1 - s3, f1 + s3, s3 * -0.5 + f1),
        };
        let stack = p(3);
        let norm = 1.0 / stack.sqrt();
        let i = stack.floor() as i32;
        let frac = (stack - i as f32) * norm;
        let pick = |k: i32| if i == k { frac } else { norm };
        self.weight = [pick(0), if i > 0 { pick(1) } else { 0.0 }, if i > 1 { pick(2) } else { 0.0 }, if i > 2 { pick(3) } else { 0.0 }];
        let inc = [f1, p2, p3, p4].map(|pt| pt.exp2() * ctx.tune * isr);
        let width = p(4);
        let pan = [width * 0.0 + 0.5, width * 0.4 + 0.5, width * -0.5 + 0.5, width * 0.15 + 0.5];
        let wet = ((p(0).abs() + 1.0) - (p(0) + -1.0).abs()) * 0.5;
        let dry = 1.0 - wet;
        let depth = p(8) * p(8) * p(8) * 8.0;
        let g = self.weight;
        let [ol, or] = out;
        for n in 0..inp[0].len() {
            let (xl, xr) = (inp[0][n], inp[1][n]);
            let mut ph = [0.0f32; 4];
            for k in 0..4 {
                ph[k] = self.phase[k] + inc[k];
            }
            let e = xr.abs() + xl.abs();
            let k = if e < self.env {
                c_rel
            } else if self.env < e {
                c_att
            } else {
                0.0
            };
            self.env = (self.env - e) * k + e;
            for k in 0..4 {
                if 1.0 < ph[k] {
                    ph[k] -= 1.0;
                }
            }
            self.phase = ph;
            let dl = depth * xl;
            let dr = depth * xr;
            let s1 = osc(ph[0], dl);
            let s2 = osc(ph[1], dr);
            let s3 = osc(ph[2], dl);
            let s4 = osc(ph[3], dr);
            let a = self.env * wet;
            let a = a + a;
            ol[n] = xl * dry + ((1.0 - pan[3]) * g[3] * s4 + (1.0 - pan[2]) * g[2] * s3 + (1.0 - pan[1]) * g[1] * s2 + (1.0 - pan[0]) * g[0] * s1) * a;
            or[n] = xr * dry + a * (s4 * (pan[3] + 0.0) * g[3] + s3 * (pan[2] + 0.0) * g[2] + s2 * (pan[1] + 0.0) * g[1] + s1 * (pan[0] + 0.0) * g[0]);
        }
    }

    fn reset(&mut self) {
        self.env = 0.0;
        self.phase = [0.0; 4];
        self.weight = [0.0; 4];
    }
}
