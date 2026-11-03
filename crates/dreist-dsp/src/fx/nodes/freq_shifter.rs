//! [re:61a767] (Frequency Shifter; processing [re:e4377c],
//! reset [re:8c22fd]): the input is modulated by (−1)ⁿ (a shift of fs/2),
//! run at 2× through polyphase half-band allpass pairs
//! ([re:8ef854]), mixed with a quadrature oscillator at
//! `fs/2 + (2^pitch − 1)·tune` and decimated back ([re:613387]).

use super::math::clip01;
use crate::analog::sin_tab;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// Allpass coefficients; branch 1 uses the even entries, branch 2 the odd.
const C: [f32; 7] = [0.0457281, 0.168088, 0.332501, 0.504486, 0.663202, 0.803781, 0.933856];

/// One two-branch half-band section (states in [re:613387] order).
#[derive(Debug, Clone, Copy, Default)]
struct HalfBand {
    x1: f32,
    y1: [f32; 4],
    x2: f32,
    y2: [f32; 3],
}

impl HalfBand {
    /// Feed `a` to branch 1 and `b` to branch 2; returns both branch outputs.
    #[inline]
    fn step(&mut self, a: f32, b: f32) -> (f32, f32) {
        let mut inp = a;
        let mut prev = self.x1;
        self.x1 = a;
        for k in 0..4 {
            let old = self.y1[k];
            self.y1[k] = (inp - old) * C[2 * k] + prev;
            inp = self.y1[k];
            prev = old;
        }
        let mut inp = b;
        let mut prev = self.x2;
        self.x2 = b;
        for k in 0..3 {
            let old = self.y2[k];
            self.y2[k] = (inp - old) * C[2 * k + 1] + prev;
            inp = self.y2[k];
            prev = old;
        }
        (self.y1[3], self.y2[2])
    }
}

/// [re:8ef854]: the two 2×-rate phases of one input.
#[derive(Debug, Clone, Copy, Default)]
struct Upper {
    a: HalfBand,
    b: HalfBand,
    pending: f32,
}

impl Upper {
    fn step(&mut self, x: f32) -> (f32, f32) {
        let (a1, a2) = self.a.step(0.0, self.pending);
        let (b1, b2) = self.b.step(x, 0.0);
        self.pending = x;
        (a1 + a2, b1 + b2)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Chan {
    up_cos: Upper,
    up_sin: Upper,
    down_cos: HalfBand,
    down_sin: HalfBand,
}

/// `0.5·sin(2π·x/2048)` with linear interpolation (the sine table from its midpoint).
#[inline]
fn sin_at(t: &[f32], x: f32) -> f32 {
    let i = x.floor() as i32;
    let k = (1024 + i) as usize;
    (t[k + 1] - t[k]) * (x - i as f32) + t[k]
}

pub struct FreqShifter {
    ports: Vec<Port>,
    sign: f32,
    phase: f32,
    amp: f32,
    ch: [Chan; 2],
}

impl FreqShifter {
    pub fn new() -> FreqShifter {
        FreqShifter { ports: ports_from(ports::FREQUENCY_SHIFTER), sign: 1.0, phase: 0.0, amp: 0.0, ch: [Chan::default(); 2] }
    }
}

impl Default for FreqShifter {
    fn default() -> Self {
        FreqShifter::new()
    }
}

impl Node for FreqShifter {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let t = sin_tab();
        let a = self.ports[1].value;
        let a = a * a * a;
        let ratio = crate::fx::exp2(self.ports[0].value);
        let inc = clip01((ratio - 1.0) * (ctx.tune * (1.0 / ctx.sample_rate)) + 0.5);
        let half = inc * 0.5;
        let wet = clip01(self.ports[2].value);
        let start = self.amp;
        let inv_n = 1.0 / n as f32;
        let [ol, or] = out;
        for i in 0..n {
            self.sign = -self.sign;
            self.amp += ((a.abs() + a) * 0.5 - start) * inv_n;
            let p = self.phase + inc;
            let ph = if p < 1.0 { p } else { p - 1.0 };
            self.phase = ph;
            let c = {
                let v = half + ph + 0.25;
                if v < 1.0 { v } else { v - 1.0 }
            };
            let osc = [sin_at(t, (ph + 0.25) * 2048.0), sin_at(t, c * 2048.0), sin_at(t, ph * 2048.0), sin_at(t, (ph + half) * 2048.0)];
            for (k, (x, o)) in [(inp[0][i], &mut ol[i]), (inp[1][i], &mut or[i])].into_iter().enumerate() {
                let ch = &mut self.ch[k];
                let xm = self.sign * x;
                let (ca, cb) = ch.up_cos.step(xm);
                let (sa, sb) = ch.up_sin.step(xm);
                let (e1, e2) = ch.down_cos.step(osc[1] * cb, osc[0] * ca);
                let (f1, f2) = ch.down_sin.step(osc[3] * sb, osc[2] * sa);
                *o = x * (1.0 - wet) + (f1 + f2 + e1 + e2) * self.amp * wet;
            }
        }
    }

    fn reset(&mut self) {
        self.sign = 1.0;
        self.phase = 0.0;
        self.amp = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 kHz sine shifted by +1 octave of the 440 Hz reference (+440 Hz)
    /// should come out near 1440 Hz.
    #[test]
    fn shifts_frequency() {
        let sr = 48000.0;
        let ctx = Ctx::new(sr);
        let mut f = FreqShifter::new();
        f.ports[0].value = 1.0;
        f.ports[1].value = 1.0;
        f.ports[2].value = 1.0;
        let n = 4800;
        let x: Vec<f32> = (0..n).map(|i| (i as f32 * std::f32::consts::TAU * 1000.0 / sr).sin() * 0.5).collect();
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        f.process(&ctx, [&x, &x], [&mut l, &mut r]);
        let mag = |hz: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in l.iter().enumerate().skip(2400) {
                let w = i as f32 * std::f32::consts::TAU * hz / sr;
                re += v * w.cos();
                im += v * w.sin();
            }
            (re * re + im * im).sqrt()
        };
        let (shifted, orig) = (mag(1440.0), mag(1000.0));
        assert!(shifted > orig * 4.0, "shifted {shifted} orig {orig}");
    }
}
