//! [re:691de1] (Ring Mod, kind 24; reset [re:4bb22f],
//! processing [re:66c1f6]): soft-clipped input times a carrier at
//! `2^Pitch · tune` Hz (max Nyquist/… clamp 0.5 cycles/sample): sine
//! ([re:19db55]), triangle, saw, pulse, parabola, white or pink
//! sample-and-hold, wet/dry.
//!
//! Triangle and saw reuse the distortion core's naive [re:2e09a0] ticks
//! (the 3-argument variants); pulse and parabola follow the 2-argument
//! pulse [re:d08eff] / parabola [re:fae61a] ticks.

use super::blocks::Rng;
use super::distortion::Osc;
use super::math::sat;
use crate::analog::sin_tab;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct RingMod {
    ports: Vec<Port>,
    osc: Osc,
    rng: Rng,
    held: f32,
    mode: i32,
    fresh: bool,
    f: f32,
    drive: f32,
}

impl RingMod {
    pub fn new() -> RingMod {
        let mut ports = ports_from(ports::RING_MOD);
        // Int-form ctor: Mode 0..6 (Sin, Tri, Saw, Sqr, Prb, White, Pink).
        ports[1] = Port::new(0.0, 0.0, 6.0);
        RingMod { ports, osc: Osc::default(), rng: Rng::with_seed(0), held: 0.0, mode: -1, fresh: true, f: 0.0, drive: 1.0 }
    }
}

impl Default for RingMod {
    fn default() -> Self {
        RingMod::new()
    }
}

impl Node for RingMod {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let f = p(2).exp2() * ctx.tune * (1.0 / ctx.sample_rate);
        let ft = ((f.abs() + 0.5) - (f + -0.5).abs()) * 0.5;
        let m = (((p(1).abs() + 6.0) - (p(1) + -6.0).abs()) * 0.5).round() as i32;
        let prev = self.mode;
        self.mode = m;
        if prev != m || self.fresh {
            match m {
                0 => self.osc.init_exact(4, ft),
                1 => self.osc.init_exact(3, ft),
                2 => self.osc.init_exact(0, ft),
                3 => self.osc.init_exact(1, ft),
                4 => self.osc.init_exact(2, ft),
                5 | 6 => {
                    self.osc.init_exact(10, ft);
                    self.rng.reseed();
                }
                _ => {}
            }
        }
        self.fresh = false;
        let n = inp[0].len();
        if !(0..7).contains(&m) || n == 0 {
            return;
        }
        let d = p(0) * p(0) * p(0);
        let inv_n = 1.0 / n as f32;
        let wet = ((p(3).abs() + 1.0) - (p(3) + -1.0).abs()) * 0.5;
        let dry = 1.0 - wet;
        let df = (ft - self.f) * inv_n;
        let dd = ((d.abs() + d) * 0.5 - self.drive) * inv_n;
        let tab = sin_tab();
        let [ol, or] = out;
        for i in 0..n {
            self.f += df;
            self.drive += dd;
            let l = sat(inp[0][i] * self.drive);
            let r = sat(self.drive * inp[1][i]);
            let f = self.f;
            let s = match m {
                0 => {
                    let ph = self.osc.phase_saw_mut();
                    *ph += f;
                    if 1.0 <= *ph {
                        *ph += -1.0;
                    }
                    let x = *ph * 2048.0;
                    let k = x.floor() as i32;
                    let j = (1024 + k) as usize;
                    (tab[j + 1] - tab[j]) * (x - k as f32) + tab[j]
                }
                1 => self.osc.tick_tri(f),
                2 => self.osc.tick_saw(f),
                3 => self.osc.tick_sqr_pwm(f, 0.5),
                4 => self.osc.tick_prb(f),
                _ => {
                    let ph = self.osc.phase_saw_mut();
                    let v = f + f + *ph;
                    *ph = v;
                    if 1.0 <= v {
                        *ph = v - v.floor();
                        self.held = if m == 5 { self.rng.white_raw() } else { self.rng.pink_raw() };
                    }
                    self.held
                }
            };
            ol[i] = l * wet * s + dry * l;
            or[i] = r * wet * s + dry * r;
        }
    }

    fn reset(&mut self) {
        self.f = 0.0;
        self.drive = 1.0;
        self.mode = -1;
        self.fresh = true;
    }
}
