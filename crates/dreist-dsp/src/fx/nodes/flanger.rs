//! [re:6695a0] (Flanger, kind 16; reset [re:519138], stream setup
//! [re:743a2b], processing [re:0b99bd]): two cubic-interpolated delay lines
//! swept by a raised-cosine LFO (right channel phase-offset), saturated and
//! DC-blocked feedback, cross-mixed by Spread, wet/dry.

use super::math::{clip01, cubic, pow2_at_least, sat};
use crate::analog::sin_tab;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

use super::blocks::ALT;

/// Raised cosine `0.5·cos(2πp) + 0.5` from [re:19db55] (+1536).
#[inline]
fn rcos(t: &[f32], p: f32) -> f32 {
    let x = p * 2048.0;
    let k = x.floor() as i32;
    let j = (1536 + k) as usize;
    (t[j + 1] - t[j]) * (x - k as f32) + t[j] + 0.5
}

pub struct Flanger {
    ports: Vec<Port>,
    alt: f32,
    idx: u32,
    phase: f32,
    delay: [f32; 2],
    dc: [f32; 2],
    max_delay: f32,
    k_dc: f32,
    buf: [Vec<f32>; 2],
    mask: u32,
    sr: f32,
}

impl Flanger {
    pub fn new() -> Flanger {
        let mut ports = ports_from(ports::FLANGER);
        // Float-form enum: max = item count (2).
        ports[6] = Port::new(0.0, 0.0, 2.0);
        Flanger { ports, alt: f32::from_bits(ALT), idx: 0, phase: 0.0, delay: [0.0; 2], dc: [0.0; 2], max_delay: 0.0, k_dc: 0.0, buf: [Vec::new(), Vec::new()], mask: 0, sr: 0.0 }
    }

    /// Stream setup: both lines hold `2·max(0.01·384000, 2) + 4` samples
    /// rounded up to a power of two.
    fn prepare(&mut self, sr: f32) {
        let m = 384000.0f32 * 0.01;
        let m = ((m + -2.0).abs() + m + 2.0) * 0.5;
        self.max_delay = m;
        let n = (m + m + 4.0).round() as i32;
        let size = pow2_at_least(n as i64);
        if self.buf[0].len() != size {
            self.buf = [vec![0.0; size], vec![0.0; size]];
        }
        self.mask = size as u32 - 1;
        let k = (1.0 / sr) * 251.32742;
        self.k_dc = ((k + 1.0) - (k + -1.0).abs()) * 0.5;
    }
}

impl Default for Flanger {
    fn default() -> Self {
        Flanger::new()
    }
}

impl Node for Flanger {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn prepare_rate(&mut self, sr: f32) {
        if self.sr != sr {
            self.sr = sr;
            self.prepare(sr);
        }
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            self.prepare(self.sr);
        }
        let p = |i: usize| self.ports[i].value;
        let sign = if clip01(p(6)).round() as i32 != 0 { -1.0 } else { 1.0 };
        let a = self.alt;
        self.alt = -a;
        self.delay[0] -= a;
        self.delay[1] -= a;
        self.dc[0] -= a;
        self.dc[1] -= a;
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let isr = 1.0 / ctx.sample_rate;
        let cross = clip01(0.5 - p(4) * 0.5);
        let wet = clip01(p(7));
        let pos = ctx.sample_rate * (p(2).abs() + p(2)) * 0.5;
        let md = self.max_delay;
        let base = ((md + 0.0 + pos.abs()) - (pos - md).abs()) * 0.5;
        let depth = clip01(p(1)) * md;
        let dry = 1.0 - wet;
        let fb = clip01(p(3)).sqrt();
        let g_in = (fb * -0.95 + 1.0).sqrt();
        let f = ((p(0).abs() + 5.0) - (p(0) + -5.0).abs()) * 0.5 * isr;
        let inc = clip01(f);
        let k = ((inc * 8.0 + 1.0) - (inc * 8.0 + -1.0).abs()) * 0.5;
        let off = clip01(p(5));
        let wet = wet * 0.5;
        let t = sin_tab();
        let m = self.mask;
        let [ol, or] = out;
        for i in 0..n {
            self.idx = (self.idx + 1) & m;
            let w = self.idx;
            self.phase = (self.phase + inc) % 1.0;
            let ph2 = (off + self.phase) % 1.0;
            self.delay[0] = ((rcos(t, self.phase) * depth + base) - self.delay[0]) * k + self.delay[0];
            self.delay[1] = ((rcos(t, ph2) * depth + base) - self.delay[1]) * k + self.delay[1];
            let (xl, xr) = (inp[0][i], inp[1][i]);
            let yl = cubic(&self.buf[0], m, (w as i32 as f32 - self.delay[0]) + -2.0) * sign;
            let yr = cubic(&self.buf[1], m, (w as i32 as f32 - self.delay[1]) + -2.0) * sign;
            let fl = fb * yl + g_in * xl;
            let fr = fb * yr + g_in * xr;
            let back = (w.wrapping_sub(2) & m) as usize;
            ol[i] = xl * dry + (cross * yr + (1.0 - cross) * yl + self.buf[0][back]) * wet;
            or[i] = xr * dry + (yr * (1.0 - cross) + yl * cross + self.buf[1][back]) * wet;
            let sl = sat(fl) - self.dc[0];
            self.dc[0] = self.k_dc * sl + self.dc[0];
            let sr = sat(fr) - self.dc[1];
            self.dc[1] = self.k_dc * sr + self.dc[1];
            self.buf[0][w as usize] = sl;
            self.buf[1][w as usize] = sr;
        }
    }

    fn reset(&mut self) {
        self.idx = 0;
        self.phase = 0.0;
        self.delay = [0.0; 2];
        self.dc = [0.0; 2];
        for b in &mut self.buf {
            b.fill(0.0);
        }
    }
}
