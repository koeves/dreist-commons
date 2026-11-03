//! The ladder-filter effects (all dual-mono, `ModIn` = own channel):
//! [re:408b3e] (Three Oh! Three, kind 31; processing [re:9f5684],
//! TB303 processing [re:1ceba5]), [re:b3c4dd] (Hoover, kind 32;
//! [re:393f55], processing [re:492081]) and [re:5c35cf] (Grandad, kind 33;
//! [re:4817d5]) and [re:96b293] (Child Genius, kind 35; [re:2b6fa1],
//! asymmetric Prodigy processing [re:da1055]). Four one-pole stages through [re:554bd9] lookup tables
//! (mooglim/transisterlim or otalim), resonance feedback through otalim1,
//! a mode matrix mixing the input and stage outputs (mode setup [re:9409c2],
//! Juno mode setup [re:f4c7e8], Moog mode setup [re:69cf3b]), an otalim output
//! stage and a 10 Hz DC blocker.

use super::math::sat;
use crate::analog::{analog, lut_pos};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

use super::blocks::ALT;

#[inline]
fn lut(t: &[f32], v: f32) -> f32 {
    let (i, f) = lut_pos(v);
    (t[i + 1] - t[i]) * f + t[i]
}

/// One-pole coefficient warp `w = 1 − e^{−2π·min(f, 0.25)}` and its
/// polynomial correction.
#[inline]
fn warp(f: f32) -> f32 {
    let c = ((f.abs() + 0.25) - (f + -0.25).abs()) * 0.5;
    1.0 - (c * 6.2831855 * -1.442695).exp2()
}

#[inline]
fn gcoef(w: f32) -> f32 {
    (((w * 0.393442 + -0.131474) * w + 0.111105) * w + 0.991549) * w
}

/// Mode table of the standard and Juno mode setups (24 modes; Juno mode 0
/// falls through into mode 1 in the original).
const MODES: [[f32; 5]; 24] = [
    [0.0, -1.0, 0.0, -0.0, 0.0],
    [0.0, -0.0, 1.0, -0.0, 0.0],
    [0.0, -0.0, 0.0, -1.0, 0.0],
    [0.0, -0.0, 0.0, -0.0, 1.0],
    [0.0, -1.0, 1.0, -0.0, 0.0],
    [0.0, -0.0, 1.0, -2.0, 1.0],
    [1.0, -1.0, 0.0, -0.0, 0.0],
    [1.0, -2.0, 1.0, -0.0, 0.0],
    [1.0, -3.0, 3.0, -1.0, 0.0],
    [1.0, -4.0, 6.0, -4.0, 1.0],
    [0.0, -1.0, 2.0, -1.0, 0.0],
    [0.0, -1.0, 3.0, -3.0, 1.0],
    [1.0, -2.0, 2.0, -0.0, 0.0],
    [0.0, -1.0, 2.0, -2.0, 0.0],
    [0.0, -0.0, 1.0, -2.0, 2.0],
    [1.0, -3.0, 4.0, -2.0, 0.0],
    [0.0, -1.0, 3.0, -4.0, 2.0],
    [1.0, -3.0, 6.0, -4.0, 0.0],
    [0.0, -1.0, 3.0, -6.0, 4.0],
    [0.0, -4.0, 8.0, -5.0, 0.0],
    [0.0, -0.0, 4.0, -8.0, 5.0],
    [1.0, -3.0, 7.0, -5.0, 0.0],
    [0.0, -1.0, 3.0, -7.0, 5.0],
    [1.0, -2.0, 4.0, -4.0, 0.0],
];

/// Moog mode setup (22 modes).
pub const MOOG_MODES: [[f32; 5]; 22] = [
    [0.0, 0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 1.0],
    [0.0, -1.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 2.0, -4.0, 2.0],
    [1.0, -2.0, 1.0, 0.0, 0.0],
    [1.0, -4.0, 6.0, -4.0, 1.0],
    [1.0, 0.0, 0.0, 0.0, 0.0],
    [1.0, -2.0, 3.0, -2.0, 1.0],
    [1.0, -2.0, 1.25, 0.0, 0.0],
    [0.0, 0.0, 4.0, -8.0, 5.0],
    [0.0, -2.0, 6.0, -6.5, 2.5],
    [0.25, -0.5, 1.25, 0.0, 0.0],
    [1.0, -4.0, 10.0, -12.0, 5.0],
    [0.0, -0.5, 1.5, -3.5, 2.5],
    [1.0, -4.0, 10.25, -12.5, 6.25],
    [1.0, -2.0, 2.0, 0.0, 0.0],
    [1.0, -4.0, 8.0, -8.0, 4.0],
    [0.0, 0.0, 1.0, -2.0, 2.0],
    [1.0, -4.0, 7.0, -6.0, 2.0],
    [0.0, -2.0, 6.0, -8.0, 4.0],
    [0.25, -1.0, 2.75, -3.5, 2.5],
    [1.0, -4.0, 7.25, -6.5, 2.5],
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tb303,
    Juno,
    Moog,
    Prodigy,
}

#[derive(Clone)]
struct Voice {
    alt: f32,
    m: [f32; 5],
    /// Integrators (TB303, Juno, Moog).
    st: [f32; 4],
    /// Stage outputs y0..y4.
    y: [f32; 5],
    u_prev: f32,
    hp: f32,
    dc: f32,
    spare: f32,
    cut: f32,
    pm_prev: f32,
    drive: f32,
    out: f32,
    res: f32,
    /// Prodigy: feedback low-pass and noise seed.
    fb_lp: f32,
    seed: u32,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            alt: f32::from_bits(ALT),
            m: [0.0; 5],
            st: [0.0; 4],
            y: [0.0; 5],
            u_prev: 0.0,
            hp: 0.0,
            dc: 0.0,
            spare: 0.0,
            cut: 0.0,
            pm_prev: 0.0,
            drive: 0.0,
            out: 0.0,
            res: 0.0,
            fb_lp: 0.0,
            seed: 0,
        }
    }

    fn reset(&mut self) {
        let alt = self.alt;
        let m = self.m;
        let seed = self.seed;
        *self = Voice::new();
        self.alt = alt;
        self.m = m;
        self.seed = seed;
    }

    fn flip(&mut self) {
        let a = self.alt;
        self.alt = -a;
        for s in &mut self.st {
            *s -= a;
        }
        self.hp -= a;
        self.spare -= a;
        self.dc -= a;
    }

    /// TB303 processing (sat = mooglim / transisterlim) and Juno processing
    /// (same stages; y0 gets a slight square term).
    fn ladder(&mut self, kind: Kind, x: f32, g: f32, k: f32, k_hp: f32) {
        let a = analog();
        let y1_old = self.y[1];
        let d = x - self.hp;
        self.hp = k_hp * d + self.hp;
        let u = d - lut(&a.otalim1, k * self.y[4] * 32.0);
        let prev = self.u_prev;
        self.u_prev = u;
        self.y[0] = if kind == Kind::Juno { u * 0.01 * u + u } else { lut(&a.otalim1, u * 32.0) };
        let mut inp = (u * 0.74 + prev * 0.26) - y1_old;
        let mut old = y1_old;
        for s in 0..4 {
            self.st[s] = lut(&a.mooglim, inp * 32.0) * g + self.st[s];
            let y = lut(&a.transisterlim, self.st[s] * 32.0);
            self.y[s + 1] = y;
            if s < 3 {
                let next_old = self.y[s + 2];
                inp = (old * 0.26 + y * 0.74) - next_old;
                old = next_old;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process(&mut self, kind: Kind, ctx: &Ctx, p: &[f32], x: &[f32], out: &mut [f32]) {
        let isr = 1.0 / ctx.sample_rate;
        let n = x.len();
        let inv_n = 1.0 / n.max(1) as f32;
        let a = analog();
        let k10 = 1.0 - (isr * -62.831856).exp();
        match kind {
            Kind::Tb303 | Kind::Juno => {
                let k_hp = if kind == Kind::Tb303 { ((k10 + 1.0) - (k10 + -1.0).abs()) * 0.5 } else { k10 };
                let mode = (((p[1].abs() + 23.0) - (p[1] + -23.0).abs()) * 0.5).round() as usize;
                if mode < 24 {
                    // Juno: case 0 falls through into case 1.
                    self.m = MODES[if kind == Kind::Juno && mode == 0 { 1 } else { mode }];
                }
                let pm = {
                    let v = (p[5].abs() + p[5]) * 0.5;
                    v * v * 6.2831855
                };
                let fm = {
                    let v = (p[6].abs() + p[6]) * 0.5;
                    v * v * 6.2831855 * 6.2831855
                };
                let f = p[2].exp2() * (isr + isr) * ctx.tune;
                let d_cut = (((f.abs() + 1.0) - (f + -1.0).abs()) * 0.5 * 0.5 - self.cut) * inv_n;
                let r4 = p[3] * 4.02;
                let (drive_t, out_t, k) = if kind == Kind::Tb303 {
                    let d = (p[0].abs() + p[0]) * 0.5;
                    let o = (p[4].abs() + p[4]) * 0.5;
                    (d * d * d + d * d * d, o * o * o * 0.5, ((r4.abs() + 32.0) - (r4 + -32.0).abs()) * 0.5)
                } else {
                    let o = (p[4].abs() + p[4]) * 0.5;
                    (0.0, o * o * o, ((r4 + 32.0).abs() - (r4 + -32.0).abs()) * 0.5)
                };
                let juno_drive = {
                    let d = (p[0].abs() + p[0]) * 0.5;
                    d * d * d
                };
                self.res = k;
                self.flip();
                let dd = (drive_t - self.drive) * inv_n;
                let dout = (out_t - self.out) * inv_n;
                let modulated = 1e-06 <= fm.abs() + pm.abs();
                for i in 0..n {
                    if kind == Kind::Tb303 {
                        self.drive += dd;
                        self.out += dout;
                    } else {
                        self.out = out_t;
                    }
                    self.cut += d_cut;
                    let v = if modulated {
                        let prev = self.pm_prev;
                        let pmv = x[i] * pm;
                        self.pm_prev = pmv;
                        self.cut * fm * x[i] + ((pmv + self.cut) - prev)
                    } else {
                        self.cut
                    };
                    let g = gcoef(warp(v));
                    let xin = if kind == Kind::Tb303 { sat(self.drive * x[i]) } else { x[i] * juno_drive };
                    self.ladder(kind, xin, g, k, k_hp);
                    let y = &self.y;
                    let m = &self.m;
                    let sum = (((y[4] * m[4] + y[2] * m[2]) + m[0] * y[0]) + y[1] * m[1]) + y[3] * m[3];
                    let v = if kind == Kind::Tb303 { sum * self.out * 32.0 } else { (k + 1.0) * self.out * sum * 32.0 };
                    let s = lut(&a.otalim, v) - self.dc;
                    self.dc = k10.min(1.0) * s + self.dc;
                    out[i] = s;
                }
            }
            Kind::Prodigy => {
                let clamp1 = |v: f32| ((v + 1.0) - (v + -1.0).abs()) * 0.5;
                let k_dc = clamp1(1.0 - (isr * -62.831856).exp());
                let k_lp = clamp1(1.0 - (isr * -408.40704).exp());
                let mode = (((p[1].abs() + 23.0) - (p[1] + -23.0).abs()) * 0.5).round() as usize;
                if mode < 24 {
                    self.m = MODES[mode];
                }
                let pm = {
                    let v = (p[5].abs() + p[5]) * 0.5;
                    v * v * 6.2831855
                };
                let fm = {
                    let v = (p[6].abs() + p[6]) * 0.5;
                    v * v * 6.2831855 * 6.2831855
                };
                let f = p[2].exp2() * (isr + isr) * ctx.tune;
                let r4 = p[3] * 4.08;
                self.res = ((r4.abs() + 32.0) - (r4 + -32.0).abs()) * 0.5;
                let d = (p[0].abs() + p[0]) * 0.5;
                let d3 = d * d * d;
                // Seven LCG draws of ±1e-8 noise into the states.
                let mut rnd = || {
                    self.seed = self.seed.wrapping_mul(0xbb38433).wrapping_add(0x3619636b);
                    (f32::from_bits(self.seed >> 9 | 0x4000_0000) + -3.0) * 1e-08
                };
                let r: [f32; 7] = std::array::from_fn(|_| rnd());
                for k in 0..4 {
                    self.st[k] += r[k];
                }
                self.hp = r[4] + self.hp;
                self.fb_lp = r[5] + self.fb_lp;
                self.dc = r[6] + self.dc;
                let dd = ((d3 + d3) - self.drive) * inv_n;
                let o = (p[4].abs() + p[4]) * 0.5;
                let dout = (o * o * o * 0.5 - self.out) * inv_n;
                let d_cut = (((f.abs() + 1.0) - (f + -1.0).abs()) * 0.5 * 0.5 - self.cut) * inv_n;
                let modulated = 1e-06 <= fm.abs() + pm.abs();
                for i in 0..n {
                    self.drive += dd;
                    self.out += dout;
                    self.cut += d_cut;
                    let v = if modulated {
                        let prev = self.pm_prev;
                        let pmv = x[i] * pm;
                        self.pm_prev = pmv;
                        self.cut * fm * x[i] + ((pmv + self.cut) - prev)
                    } else {
                        self.cut
                    };
                    let w = warp(v);
                    let g = gcoef(w);
                    let k = (((w * -0.28608 + -0.0808209) * w + -0.0635353) * w + 1.00175) * self.res;
                    // Asymmetric Prodigy processing.
                    let (ga, gb) = (g * 0.74, g * 0.26);
                    let dx = self.drive * x[i] - self.hp;
                    self.hp = k_dc * dx + self.hp;
                    let s4 = self.st[3];
                    let e = s4 - self.fb_lp;
                    self.fb_lp = e * k_lp + self.fb_lp;
                    let u = k * -0.5 * (s4 + e) + dx;
                    self.spare = u;
                    let y0_old = self.y[0];
                    let y0 = lut(&a.otalim, (u + 0.02) * 32.0);
                    self.y[0] = y0;
                    let y1_old = self.y[1];
                    self.st[0] = ((y0 * ga + y0_old * gb) - y1_old * g) + self.st[0];
                    let y1 = lut(&a.mooglim, (self.st[0] + 0.02) * 32.0);
                    self.y[1] = y1;
                    let y2_old = self.y[2];
                    self.st[1] = ((y1_old * gb + y1 * ga) - y2_old * g) + self.st[1];
                    let y2 = lut(&a.mooglim, (self.st[1] + 0.02) * 32.0);
                    self.y[2] = y2;
                    let y3_old = self.y[3];
                    self.st[2] = ((y2_old * gb + y2 * ga) - y3_old * g) + self.st[2];
                    let y3 = lut(&a.mooglim, (self.st[2] + 0.02) * 32.0);
                    self.y[3] = y3;
                    self.st[3] = ((y3 * ga + gb * y3_old) - g * self.y[4]) + s4;
                    self.y[4] = lut(&a.otalim, (self.st[3] + 0.02) * 32.0);
                    let y = &self.y;
                    let m = &self.m;
                    let sum = (((y[4] * m[4] + y[2] * m[2]) + m[0] * y[0]) + y[1] * m[1]) + y[3] * m[3];
                    let s = lut(&a.otalim, (sum * self.out + 0.02) * 32.0) - self.dc;
                    self.dc = k_dc * s + self.dc;
                    out[i] = s;
                }
            }
            Kind::Moog => {
                let clamp10 = |v: f32| {
                    if v < -10.0 {
                        -10.0
                    } else if v <= 10.0 {
                        v
                    } else {
                        10.0
                    }
                };
                let da = clamp10(p[0]).exp2();
                let oa = clamp10(p[4]).exp2();
                let mode = (((p[1].abs() + 21.0) - (p[1] + -21.0).abs()) * 0.5).round() as usize;
                if mode < 22 {
                    self.m = MOOG_MODES[mode];
                }
                let fc = (p[2] + 1.0).exp2() * isr;
                let fc = ((fc.abs() + 1.0) - (fc + -1.0).abs()) * 0.5;
                self.flip();
                let gt = warp(fc);
                let k_t = {
                    let r = p[3] * 4.02;
                    ((r.abs() + 32.0) - (r + -32.0).abs()) * 0.5
                };
                let dk = k_t - self.res;
                let dd = (k_t + 1.0) * ((da + -0.016367754).abs() + da + -0.016367754) * 0.5 - self.drive;
                let dout = ((oa + -0.016367754).abs() + oa + -0.016367754) * 0.5 - self.out;
                let dg = gt - self.cut;
                for i in 0..n {
                    self.res += dk * inv_n;
                    self.cut += dg * inv_n;
                    self.out += dout * inv_n;
                    self.drive += dd * inv_n;
                    let gc = self.cut;
                    let w = gcoef(gc);
                    let (aw, bw) = (w * 0.74, w * 0.26);
                    let u0 = self.drive * x[i] - self.hp;
                    self.hp = k10 * u0 + self.hp;
                    let u = u0 - (((gc * -0.22 + -0.0408209) * gc + -0.0635353) * gc + 1.00175) * self.res * self.y[4];
                    self.spare = u;
                    let s0_old = self.y[0];
                    let s0 = lut(&a.otalim, u * 32.0);
                    self.y[0] = s0;
                    let mut prev_in = s0_old;
                    let mut cur_in = s0;
                    for s in 0..4 {
                        let y_old = self.y[s + 1];
                        self.st[s] = ((aw * cur_in + prev_in * bw) - w * y_old) + self.st[s];
                        let y = lut(&a.otalim, self.st[s] * 32.0);
                        self.y[s + 1] = y;
                        prev_in = y_old;
                        cur_in = y;
                    }
                    let y = &self.y;
                    let m = &self.m;
                    let sum = (((y[4] * m[4] + y[2] * m[2]) + y[0] * m[0]) + y[1] * m[1]) + y[3] * m[3];
                    let s = lut(&a.otalim, sum * self.out * 32.0) - self.dc;
                    self.dc = k10 * s + self.dc;
                    out[i] = s;
                }
            }
        }
    }
}

pub struct Ladder {
    ports: Vec<Port>,
    kind: Kind,
    voices: [Voice; 2],
}

impl Ladder {
    pub fn new(kind: Kind) -> Ladder {
        let (defs, modes) = match kind {
            Kind::Tb303 => (ports::THREE_OH_THREE_FILTER, 24.0),
            Kind::Juno => (ports::HOOVER_FILTER, 24.0),
            Kind::Moog => (ports::GRANDAD_FILTER, 22.0),
            Kind::Prodigy => (ports::CHILD_GENIUS_FILTER, 24.0),
        };
        let mut ports = ports_from(defs);
        // Float-form enum: max = item count.
        let def = ports[1].value;
        ports[1] = Port::new(def, 0.0, modes);
        Ladder { ports, kind, voices: [Voice::new(), Voice::new()] }
    }
}

impl Node for Ladder {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = crate::fx::port_values(&self.ports);
        let kind = self.kind;
        for (k, o) in out.into_iter().enumerate() {
            self.voices[k].process(kind, ctx, &p, inp[k], o);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
    }
}
