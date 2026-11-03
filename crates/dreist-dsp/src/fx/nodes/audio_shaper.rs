//! [re:741970] (Distortion, kind 19; `Reset` [re:e4a051],
//! `ProcessBuffer` [re:29ca21]): band-limit (LP then HP one-poles), drive into
//! one of four shapers (0 diode pair `ms_diodelim_tab`, 1 OTA
//! `ms_otalim1_tab`, 2/3 soft hard-limit at ±19 or +19/−1 in 1/20.1 units),
//! tone low-pass, dirty/clean mix, 2 Hz DC blocker and wet/dry.

use super::math::sat;
use crate::analog::{analog, lut_pos};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

use super::blocks::ALT;

#[inline]
fn lut(t: &[f32], v: f32) -> f32 {
    let (i, f) = lut_pos(v);
    (t[i + 1] - t[i]) * f + t[i]
}

/// The limiter of modes 2/3: unchanged inside `[-lo, 19]/20.1`.
#[inline]
fn limit(x: f32, lo: f32) -> f32 {
    let v = x * 20.1;
    if v <= 19.0 {
        if v < -lo {
            return (sat(v + lo) + -lo) / 20.1;
        }
        return x;
    }
    (sat(v + -19.0) + 19.0) / 20.1
}

#[inline]
fn pos_cube(x: f32) -> f32 {
    let c = x * x * x;
    (c.abs() + c) * 0.5
}

pub struct AudioShaper {
    ports: Vec<Port>,
    alt: f32,
    lp: [f32; 2],
    hp: [f32; 2],
    dc: [f32; 2],
    tone: [f32; 2],
    drive: f32,
    dirty: f32,
    clean: f32,
    mix: f32,
}

impl AudioShaper {
    pub fn new() -> AudioShaper {
        let mut ports = ports_from(ports::DISTORTION);
        // Int-form ctor: Mode 0..3.
        ports[0] = Port::new(0.0, 0.0, 3.0);
        AudioShaper { ports, alt: f32::from_bits(ALT), lp: [0.0; 2], hp: [0.0; 2], dc: [0.0; 2], tone: [0.0; 2], drive: 0.0, dirty: 1.0, clean: 1.0, mix: 1.0 }
    }
}

impl Default for AudioShaper {
    fn default() -> Self {
        AudioShaper::new()
    }
}

impl Node for AudioShaper {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let isr = 1.0 / ctx.sample_rate;
        let k_dc = 1.0 - (isr * -12.566371).exp();
        let p = |i: usize| self.ports[i].value;
        let mode = (((p(0).abs() + 3.0) - (p(0) + -3.0).abs()) * 0.5).round() as u32;
        let onepole = |v: f32| {
            let h = (v + 1.0).exp2();
            (-((h + h) * 0.5 * 6.2831855 * isr)).exp()
        };
        let e_lp = onepole(p(2));
        let e_hp = onepole(p(3));
        let e_tone = onepole(p(6));
        let n = inp[0].len();
        if mode >= 4 || n == 0 {
            return;
        }
        let k_lp = 1.0 - e_lp;
        let k_hp = 1.0 - e_hp;
        let k_tone = 1.0 - e_tone;
        let g_lp = 1.0 - k_lp;
        let g_hp = 1.0 - k_hp;
        let inv = 1.0 / n as f32;
        let dd = inv * (pos_cube(p(1)) - self.drive);
        let ddi = inv * (pos_cube(p(4)) - self.dirty);
        let dcl = inv * (pos_cube(p(5)) - self.clean);
        let dm = inv * (((p(7).abs() + 1.0) - (p(7) + -1.0).abs()) * 0.5 - self.mix);
        let a = analog();
        let [ol, or] = out;
        for i in 0..n {
            let alt = self.alt;
            self.alt = -alt;
            for v in self.lp.iter_mut().chain(&mut self.hp).chain(&mut self.dc).chain(&mut self.tone) {
                *v -= alt;
            }
            self.drive += dd;
            self.dirty += ddi;
            self.clean += dcl;
            self.mix += dm;
            let x = [inp[0][i], inp[1][i]];
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                let d = x[c] - self.lp[c];
                self.lp[c] = k_lp * d + self.lp[c];
                let band = self.lp[c] - self.hp[c];
                self.hp[c] = k_hp * band + self.hp[c];
                let s = match mode {
                    0 => lut(&a.diodelim, band * g_hp * self.drive * 32.0),
                    1 => lut(&a.otalim1, band * g_hp * self.drive * 32.0),
                    2 => limit(band * g_hp * self.drive, 19.0),
                    _ => limit(band * g_hp * self.drive, 1.0),
                };
                self.tone[c] = (s - self.tone[c]) * k_tone + self.tone[c];
                y[c] = ((d * g_lp + self.hp[c]) * self.clean + self.tone[c] * self.dirty) - self.dc[c];
                self.dc[c] = k_dc * y[c] + self.dc[c];
            }
            ol[i] = (1.0 - self.mix) * x[0] + y[0] * self.mix;
            or[i] = x[1] * (1.0 - self.mix) + y[1] * self.mix;
        }
    }

    fn reset(&mut self) {
        self.lp = [0.0; 2];
        self.hp = [0.0; 2];
        self.dc = [0.0; 2];
        self.tone = [0.0; 2];
        self.drive = 0.0;
        self.dirty = 1.0;
        self.clean = 1.0;
        self.mix = 1.0;
    }
}
