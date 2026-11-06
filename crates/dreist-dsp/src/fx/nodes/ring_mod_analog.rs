//! [re:a863c4] (Analog Ring Mod, kind 47; reset [re:8c376b],
//! processing [re:054f4a]): a four-diode ring modulator. The carrier is a
//! band-limited square from a triangle at `2^Pitch · tune` (modes: sine-ish
//! shaped, square, double-shaped, cubed), the input splits into positive
//! and negative halves biasing the diode pair, the carrier-only response is
//! subtracted, then a high shelf ([re:335573] type 2,
//! 3× the carrier, Q 0.75, Tone·24 dB) with its gain compensation, wet/dry.

use super::blocks::PolySect;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// Coefficients for type 2 with `(f, q, gain)` [re:e24f60] and the compensation gain.
fn high_shelf(f: f32, q: f32, gain: f32) -> ([f32; 5], f32) {
    let g2 = ((gain / 1.5) / 6.0).exp2();
    let y = f32::from_bits(0xbe6a3000u32.wrapping_sub(g2.to_bits()) >> 1);
    let y = y * 1.5 - y * y * y * g2 * 0.5;
    let inv = ((g2.to_bits() as i32 >> 30) | 1) as f32 * y * y;
    let comp = if 1.0 < g2 { inv } else { inv.sqrt() };
    let q = ((q + -0.5).abs() + 1000.5 - (q + -1000.0).abs()) * 0.5;
    let p = 0.5 - (((f + -0.001).abs() + 0.45099998) - (f + -0.45).abs()) * 0.5;
    let a = p.abs();
    let s = p * p;
    let w = ((((a * 55.079624 + -110.15925) * s + a * 11.302022 + 79.43631) * s + a * 0.21989952 + -41.350807) * s + 6.2831855) * p;
    let c = -1.0 - (((a * 71.34317 + -124.85512) * s + a * 12.23143 + 62.747192) * s + a * 0.21205398 + -19.747784) * s;
    let k = 10f32.powf(gain / 40.0);
    let ap = k + 1.0;
    let am = k + -1.0;
    let d = ap - am * c;
    let alpha = (k.sqrt() / q) * w;
    let n = 1.0 / (d + alpha);
    let e = am * c + ap;
    ([n * (e + alpha) * k, n * (am + ap * c) * k * -2.0, n * (e - alpha) * k, n * (am - ap * c) * 2.0, (d - alpha) * n], comp)
}

pub struct RingModAnalog {
    ports: Vec<Port>,
    first: bool,
    phase: f32,
    inc: f32,
    eq: PolySect,
}

impl RingModAnalog {
    pub fn new() -> RingModAnalog {
        let mut ports = ports_from(ports::ANALOG_RING_MOD);
        // Int-form ctor: Mode 0..3.
        ports[1] = Port::new(0.0, 0.0, 3.0);
        RingModAnalog { ports, first: true, phase: 0.0, inc: 0.0, eq: PolySect::new() }
    }
}

impl Default for RingModAnalog {
    fn default() -> Self {
        RingModAnalog::new()
    }
}

#[inline]
fn halves(x: f32, c: f32) -> (f32, f32) {
    let xp = if x <= 0.0 { 0.0 } else { x };
    let a = xp * 0.5 + c + 1.0;
    let a = if a < 0.0 {
        0.0
    } else if 1.0 < a {
        1.0
    } else {
        a
    };
    let xn = if 0.0 <= x { 0.0 } else { x };
    let b = xn * 0.5 + c + -1.0;
    let b = if 0.0 < b {
        0.0
    } else if b < -1.0 {
        -1.0
    } else {
        b
    };
    let fa = a * 0.5;
    let fb = b * 0.5;
    (fa - fa * fa, fb * fb + fb)
}

impl Node for RingModAnalog {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let isr = 1.0 / ctx.sample_rate;
        let e = p(2).exp2();
        let inc0 = self.inc;
        let m = (((p(1).abs() + 3.0) - (p(1) + -3.0).abs()) * 0.5).round() as i32;
        let w = [m == 0, m == 1, m == 2, m == 3].map(|b| if b { 1.0f32 } else { 0.0 });
        if self.first {
            self.phase = 0.0;
        }
        self.first = false;
        let mut inc = if 1e-08 <= inc0 { inc0 } else { 1e-08 };
        let (c, comp) = high_shelf(ctx.tune * isr * 3.0 * e, 0.75, p(3) * 24.0);
        let n = inp[0].len();
        if n == 0 {
            self.inc = inc;
            return;
        }
        let f = ctx.tune * isr * e;
        let wet = ((p(4).abs() + 1.0) - (p(4) + -1.0).abs()) * 0.5;
        let step = (1.0 / n as f32) * (((f.abs() + 0.5) - (f + -0.5).abs()) * 0.5 - inc0);
        let k = inc + inc + inc + inc;
        let rk = 1.0 / k;
        let off = (k + -1.0) * rk;
        let amp = (p(0) * p(0) * p(0) + 0.025) * 0.5;
        let gain = (1.0 / (amp * 0.25)) * 4.0;
        let [ol, or] = out;
        for i in 0..n {
            let t = if 0.5 < self.phase { (1.0 - (self.phase + self.phase)) + self.phase } else { self.phase };
            let t = (t + -0.25) * 4.0;
            let u = rk * t;
            let a = (off + u).max(0.0);
            let a = if a <= 0.0 { 0.0 } else { a };
            let b = u - off;
            let b = if 0.0 <= b { 0.0 } else { b };
            let sq = b * b * k + (t - a * a * k);
            let s1 = sq * 1.16 * 0.5;
            let s1 = (s1 - s1 * s1 * s1) * f32::from_bits(0x40266666);
            let s2 = s1 - s1 * s1 * s1 * 0.5;
            let s2 = s2 + s2;
            let car = (s2 * w[2] + sq * w[1] + s1 * w[0] + s2 * s2 * s2 * w[3]) * amp;
            let (al, bl) = halves(inp[0][i] * 0.25, car);
            let (ar, br) = halves(inp[1][i] * 0.25, car);
            let r1 = {
                let v = car + 1.0;
                let v = if 0.0 <= v { v } else { 0.0 };
                let v = if 1.0 < v { 1.0 } else { v } * 0.5;
                v - v * v
            };
            let r2 = {
                let v = car + -1.0;
                let v = if v <= 0.0 { v } else { 0.0 };
                let v = if -1.0 <= v { v } else { -1.0 } * 0.5;
                v * v + v
            };
            let reference = r2 + r2 + r1 + r1;
            let yl = ((bl + bl + al + al) - reference) * gain;
            let yr = ((br + br + ar + ar) - reference) * gain;
            self.phase += inc;
            if 1.0 < self.phase {
                self.phase += -1.0;
            }
            let y = self.eq.run(&c, [yl, yr]);
            ol[i] = y[0] * comp * wet + inp[0][i] * (1.0 - wet);
            inc += step;
            or[i] = y[1] * comp * wet + inp[1][i] * (1.0 - wet);
        }
        self.inc = inc;
    }

    fn reset(&mut self) {
        self.inc = 0.0;
        self.phase = 0.0;
        self.first = true;
        self.eq.clear();
    }
}
