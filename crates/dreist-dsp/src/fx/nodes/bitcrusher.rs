//! [re:93069c] (Bit Crusher, kind 23; `Voice::Reset` [re:66d1b7],
//! `Voice::ProcessBuffer` [re:40b373]): band-pass (LP then HP one-poles),
//! driven soft clip, sample-and-hold at `Freq` with `2^Bits` levels,
//! tone low-pass, dirty/clean mix, 2 Hz DC blocker and wet/dry.

use super::math::clip01;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct BitCrusher {
    ports: Vec<Port>,
    phase: f32,
    held: [f32; 2],
    dc_k: f32,
    lp: [f32; 2],
    hp: [f32; 2],
    dc: [f32; 2],
    tone: [f32; 2],
    /// Ramped drive, dirty, clean, mix (+0x154..+0x160).
    drive: f32,
    dirty: f32,
    clean: f32,
    mix: f32,
}

impl BitCrusher {
    pub fn new() -> BitCrusher {
        BitCrusher::with_ports(ports_from(ports::BIT_CRUSHER))
    }

    /// As built around `ports` (no allocation).
    fn with_ports(ports: Vec<Port>) -> BitCrusher {
        BitCrusher { ports, phase: 1.0, held: [0.0; 2], dc_k: 0.0, lp: [0.0; 2], hp: [0.0; 2], dc: [0.0; 2], tone: [0.0; 2], drive: 0.0, dirty: 1.0, clean: 1.0, mix: 1.0 }
    }
}

impl Default for BitCrusher {
    fn default() -> Self {
        BitCrusher::new()
    }
}

#[inline]
fn pos_cube(x: f32) -> f32 {
    let c = x * x * x;
    (c.abs() + c) * 0.5
}

impl Node for BitCrusher {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        let isr = 1.0 / ctx.sample_rate;
        self.dc_k = 1.0 - (isr * -12.566371).exp();
        if n == 0 {
            return;
        }
        let p = |i: usize| self.ports[i].value;
        let levels = crate::fx::exp2(p(2));
        let f = crate::fx::exp2(p(1) + 1.0);
        let onepole = |v: f32| {
            let h = crate::fx::exp2(v + 1.0);
            (-((h + h) * 0.5 * 6.2831855 * isr)).exp()
        };
        let k_lp = 1.0 - onepole(p(3));
        let k_hp = 1.0 - onepole(p(4));
        let e_tone = onepole(p(7));
        let rate = clip01(isr * (f + f));
        let inv = 1.0 / n as f32;
        let dd = (pos_cube(p(0)) - self.drive) * inv;
        let ddi = (pos_cube(p(5)) - self.dirty) * inv;
        let dcl = (pos_cube(p(6)) - self.clean) * inv;
        let dm = (clip01(p(8)) - self.mix) * inv;
        let inv_levels = 1.0 / levels;
        let [ol, or] = out;
        for i in 0..n {
            self.drive += dd;
            self.dirty += ddi;
            self.clean += dcl;
            self.mix += dm;
            let x = [inp[0][i], inp[1][i]];
            let mut d = [0.0f32; 2];
            let mut band = [0.0f32; 2];
            for c in 0..2 {
                d[c] = x[c] - self.lp[c];
                self.lp[c] = k_lp * d[c] + self.lp[c];
                band[c] = self.lp[c] - self.hp[c];
            }
            self.hp[0] = k_hp * band[0] + self.hp[0];
            self.hp[1] = self.hp[1] + band[1] * k_hp;
            self.phase += rate;
            if 1.0 <= self.phase {
                self.phase -= self.phase.floor();
                for c in 0..2 {
                    let a = self.drive * band[c] * (1.0 - k_hp);
                    let s = ((a + 2.0).abs() - (a - 2.0).abs()) * 0.5;
                    let y = ((s.abs() * 0.0625 + -0.25) * s * s + 1.0) * s;
                    self.held[c] = (y * levels + 0.5).floor() * inv_levels;
                }
            }
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                self.tone[c] = (self.held[c] - self.tone[c]) * (1.0 - e_tone) + self.tone[c];
            }
            y[0] = (((1.0 - k_lp) * d[0] + self.hp[0]) * self.clean + self.tone[0] * self.dirty) - self.dc[0];
            y[1] = (self.clean * (d[1] * (1.0 - k_lp) + self.hp[1]) + self.tone[1] * self.dirty) - self.dc[1];
            self.dc[0] = y[0] * self.dc_k + self.dc[0];
            self.dc[1] = self.dc_k * y[1] + self.dc[1];
            ol[i] = (1.0 - self.mix) * x[0] + y[0] * self.mix;
            or[i] = x[1] * (1.0 - self.mix) + y[1] * self.mix;
        }
    }

    fn reset(&mut self) {
        *self = BitCrusher::with_ports(std::mem::take(&mut self.ports));
    }
}
