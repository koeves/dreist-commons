//! [re:637b7b] (Japan Filter, kind 36; `ProcessBuffer` [re:73307f]) and
//! [re:cb76f5] (Little Fatty Filter, kind 34; `ProcessBuffer` [re:3f1bf2]),
//! both dual-mono.
//!
//! SH101: OTA ladder whose stages are the integrators themselves
//! (`ms_otalim_s_tab`, already ×32), diode-limited resonance feedback
//! (`ms_diodelim_s_tab`), `SetModeSh101` [re:cbe6ef] output matrix (22
//! modes), FM from `ModIn`, wet/dry. The analogue-noise inputs
//! ([re:ae6557]/+0xe0) are never written, so the noise terms are zero.
//!
//! MS20: two tanh-limited integrators with a diode feedback path and four
//! routings (Mode 0–3), otalim output and 10 Hz DC blocker.

use super::ladder::MOOG_MODES as SH101_MODES;
use crate::analog::{analog, lut_pos};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

use super::blocks::ALT;

#[inline]
fn lut(t: &[f32], v: f32) -> f32 {
    let (i, f) = lut_pos(v);
    (t[i + 1] - t[i]) * f + t[i]
}

#[derive(Clone)]
struct Sh101Voice {
    alt: f32,
    m: [f32; 5],
    res: f32,
    cut: f32,
    hp: f32,
    dc: f32,
    u_prev: f32,
    s: [f32; 4],
}

impl Sh101Voice {
    fn new() -> Sh101Voice {
        Sh101Voice { alt: f32::from_bits(ALT), m: SH101_MODES[0], res: 0.0, cut: 0.0, hp: 0.0, dc: 0.0, u_prev: 0.0, s: [0.0; 4] }
    }

    fn reset(&mut self) {
        self.res = 0.0;
        self.hp = 0.0;
        self.dc = 0.0;
        self.cut = 0.0;
        self.u_prev = 0.0;
        self.s = [0.0; 4];
    }

    fn process(&mut self, ctx: &Ctx, p: &[f32], x: &[f32], m_in: &[f32], out: &mut [f32]) {
        let a = analog();
        let isr = 1.0 / ctx.sample_rate;
        let k10 = 1.0 - (isr * -62.831856).exp();
        let c10 = |v: f32| if v < -10.0 { -10.0 } else { v.min(10.0) };
        let dlin = c10(p[0]).exp2();
        let drive = ((dlin + -0.016367754).abs() + dlin + -0.016367754) * 0.5 * 32.0;
        let mode = (((p[1].abs() + 21.0) - (p[1] + -21.0).abs()) * 0.5).round() as usize;
        let fmd = (p[4].max(0.0) * 8.0 + -8.0).exp2();
        if mode < 22 {
            self.m = SH101_MODES[mode];
        }
        let pitch = if p[2] < -12.0 { -12.0 } else { p[2].min(12.0) };
        let alt = self.alt;
        self.alt = -alt;
        for v in self.s.iter_mut() {
            *v -= alt;
        }
        self.hp -= alt;
        self.dc -= alt;
        let n = x.len();
        if n == 0 {
            return;
        }
        let inv_n = 1.0 / n as f32;
        let wet = ((p[5].abs() + 1.0) - (p[5] + -1.0).abs()) * 0.5;
        let f = pitch.exp2() * ctx.tune * isr;
        let ft = if f < 0.0 { 0.0 } else { f.min(1.0) };
        let r = p[3] * 4.03;
        let rt = if r < 0.0 { 0.0 } else { r.min(32.0) };
        let d_res = rt - self.res;
        let d_cut = ft - self.cut;
        for i in 0..n {
            self.res += d_res * inv_n;
            self.cut += d_cut * inv_n;
            let c = self.cut + m_in[i] * (fmd + -0.00390625) * 48000.0 * isr;
            let w = 1.0 - (((c.abs() + 0.25) - (c + -0.25).abs()) * 0.5 * 6.2831855 * -1.442695).exp2();
            let g = (((w * 0.393442 + -0.131474) * w + 0.111105) * w + 0.991549) * w;
            let u0 = (x[i] * drive - self.hp) + 0.0;
            self.hp = k10 * u0 + self.hp;
            let s4 = self.s[3];
            let fbv = self.res * (((w * -0.22 + -0.0408209) * w + -0.0635353) * w + 1.00175) * s4;
            let u = u0 - lut(&a.diodelim_s, fbv);
            let prev = self.u_prev;
            self.u_prev = u;
            let s1o = self.s[0];
            let s1 = lut(&a.otalim_s, ((u * 0.74 + prev * 0.26) - s1o) + 0.0) * g + s1o;
            self.s[0] = s1;
            let s2o = self.s[1];
            let s2 = lut(&a.otalim_s, ((s1o * 0.26 + s1 * 0.74) - s2o) + 0.0) * g + s2o;
            self.s[1] = s2;
            let s3o = self.s[2];
            let s3 = lut(&a.otalim_s, ((s2o * 0.26 + s2 * 0.74) - s3o) + 0.0) * g + s3o;
            self.s[2] = s3;
            let s4n = lut(&a.otalim_s, ((s3o * 0.26 + s3 * 0.74) - s4) + 0.0) * g + s4;
            self.s[3] = s4n;
            let m = &self.m;
            let y = (s4n * m[4] + s3 * m[3] + s2 * m[2] + s1 * m[1] + u * m[0]) - self.dc;
            self.dc = k10 * y + self.dc;
            out[i] = y * 0.03125 * wet + x[i] * (1.0 - wet);
        }
    }
}

pub struct Sh101Filter {
    ports: Vec<Port>,
    voices: [Sh101Voice; 2],
}

impl Sh101Filter {
    pub fn new() -> Sh101Filter {
        let mut ports = ports_from(ports::JAPAN_FILTER);
        // Float-form enum: max = item count (22).
        ports[1] = Port::new(1.0, 0.0, 22.0);
        Sh101Filter { ports, voices: [Sh101Voice::new(), Sh101Voice::new()] }
    }
}

impl Default for Sh101Filter {
    fn default() -> Self {
        Sh101Filter::new()
    }
}

impl Node for Sh101Filter {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = crate::fx::port_values(&self.ports);
        for (k, o) in out.into_iter().enumerate() {
            self.voices[k].process(ctx, &p, inp[k], inp[k], o);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
    }
}

#[derive(Clone)]
struct Ms20Voice {
    alt: f32,
    s1: f32,
    s2: f32,
    fb_prev: f32,
    dc: f32,
    cut: f32,
    drive: f32,
    out: f32,
}

impl Ms20Voice {
    fn new() -> Ms20Voice {
        Ms20Voice { alt: f32::from_bits(ALT), s1: 0.0, s2: 0.0, fb_prev: 0.0, dc: 0.0, cut: 0.0, drive: 0.0, out: 0.0 }
    }

    fn reset(&mut self) {
        let alt = self.alt;
        *self = Ms20Voice::new();
        self.alt = alt;
    }

    fn process(&mut self, ctx: &Ctx, p: &[f32], x: &[f32], out: &mut [f32]) {
        let a = analog();
        let isr = 1.0 / ctx.sample_rate;
        let kd = 1.0 - (isr * -62.831856).exp();
        let k_dc = ((kd + 1.0) - (kd + -1.0).abs()) * 0.5;
        let mode = (((p[1].abs() + 3.0) - (p[1] + -3.0).abs()) * 0.5).round() as u32;
        let f = p[2].exp2() * (isr * ctx.tune);
        let e = ((((f.abs() + 1.0) - (f + -1.0).abs()) * 0.5) * -6.2831855).exp();
        let rr = p[3].sqrt() + p[3].sqrt();
        let r = ((rr.abs() + 16.0) - (rr + -16.0).abs()) * 0.5;
        let ca = ((r * 0.25 + 1.0) - (r * 0.25 + -1.0).abs()) * 0.5;
        let cb = 1.0 - ca;
        let alt = self.alt;
        self.alt = -alt;
        self.s1 -= alt;
        self.s2 -= alt;
        self.dc -= alt;
        if mode >= 4 {
            return;
        }
        let n = x.len();
        let inv_n = 1.0 / n.max(1) as f32;
        let d = (p[0].abs() + p[0]) * 0.5;
        let o = (p[4].abs() + p[4]) * 0.5;
        let o3 = o * o * o;
        let dd = (d * d * d * 0.5 - self.drive) * inv_n;
        let dout = ((o3 + o3) - self.out) * inv_n;
        let w = 1.0 - e;
        let w = ((w * 0.25 + 0.33333334) * w + 0.5) * w * w + w;
        let dcut = (((w + 1.0) - (w + -1.0).abs()) * 0.5 - self.cut) * inv_n;
        for i in 0..n {
            self.drive += dd;
            self.out += dout;
            self.cut += dcut;
            let g = self.cut;
            let fb = lut(&a.diodelim, self.s2 * 32.0);
            let xd = self.drive * x[i];
            let (in1, in2) = match mode {
                0 => (xd, 0.0),
                1 => (0.0, xd),
                _ => (xd, xd),
            };
            let v = (((in1 - ca * fb * r) - cb * self.fb_prev * r) - self.s1) * 32.0;
            self.fb_prev = fb;
            self.s1 = lut(&a.tanhlim, v) * g + self.s1;
            let v2 = if mode == 0 { ((self.s1 + 0.0 + fb * r) - self.s2) * 32.0 } else { (((self.s1 - in2) + fb * r) - self.s2) * 32.0 };
            self.s2 = lut(&a.tanhlim, v2) * g + self.s2;
            let tap = if mode == 0 || mode == 2 { fb * self.out } else { (fb + xd) * self.out };
            let y = lut(&a.otalim, tap * 32.0) - self.dc;
            self.dc = k_dc * y + self.dc;
            out[i] = y;
        }
    }
}

pub struct Ms20Filter {
    ports: Vec<Port>,
    voices: [Ms20Voice; 2],
}

impl Ms20Filter {
    pub fn new() -> Ms20Filter {
        let mut ports = ports_from(ports::LITTLE_FATTY_FILTER);
        // Float-form enum: max = item count (4).
        ports[1] = Port::new(0.0, 0.0, 4.0);
        Ms20Filter { ports, voices: [Ms20Voice::new(), Ms20Voice::new()] }
    }
}

impl Default for Ms20Filter {
    fn default() -> Self {
        Ms20Filter::new()
    }
}

impl Node for Ms20Filter {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = crate::fx::port_values(&self.ports);
        for (k, o) in out.into_iter().enumerate() {
            self.voices[k].process(ctx, &p, inp[k], o);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
    }
}
