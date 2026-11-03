//! [re:2428e6] (Envelope Shaper, kind 12;
//! `PrepareStream` [re:ef9fb6], `Reset` [re:da41b9], `ProcessBuffer` [re:14163c])
//! around [re:1800b5] (`CreateNewTransientDesigner` [re:32ebc6], channel
//! `Process` [re:f30c18]): a log-domain transient designer. Detector levels
//! (softplus-shaped) drive attack and sustain envelopes whose differences
//! become a gain in dB; parameters glide over 2048 samples.
//!
//! Parameters (0..1 → value): 0 Mix, 1 Attack ±15, 2 Sustain ±24, 3 Gain
//! ±12 dB, 4 Sensitivity, 5 Linked (> 0.5: one detector on the summed input
//! for both channels).

use crate::fx::{Ctx, Node, Port, ports, ports_from};

fn to_value(i: usize, p: f32) -> f32 {
    match i {
        1 => (p + -0.5) * 30.0,
        2 => (p + -0.5) * 48.0,
        3 => (p + -0.5) * 24.0,
        _ => p,
    }
}

#[inline]
fn softplus(x: f64) -> f64 {
    ((x * 10.0).exp() + 1.0).ln() * 0.1
}

#[derive(Clone, Default)]
struct Channel {
    s20: f64,
    s28: f64,
    s30: f64,
    s60: f64,
    s68: f64,
    s70: f64,
    s78: f64,
    sc8: f64,
    c: [f64; 14],
    target: [f32; 6],
    cur: [f32; 6],
    step: [f32; 6],
    count: [i32; 6],
    meter: f64,
}

impl Channel {
    /// [re:bcc4a0].
    fn build(&mut self, sr: f64) {
        let k = 1.0 / sr;
        let e = |a: f64| (a * k).exp();
        self.c = [
            e(-40.80895976715265),   // 0x38
            e(-1.5318088844263265),  // 0x40
            e(-1.5915494309189535),  // 0x48
            e(-159.15494309189532),  // 0x50
            e(-14.468631190172303),  // 0x58
            e(-0.03617157797543075), // 0x80
            e(-33.862753849339434),  // 0x88
            e(-10.826866876999683),  // 0x90
            e(-18.293671619758086),  // 0x98
            e(-18.08578898771538),   // 0xa0
            e(-5.167368282204394),   // 0xa8
            e(-36.17157797543076),   // 0xb0 (and 0xd0)
            e(-4.521447246928845),   // 0xb8
            e(-12.057192658476918),  // 0xc0
        ];
    }

    /// [re:fb274e]: clear, build, take the parameter values.
    fn configure(&mut self, sr: f64, values: &[f32; 6]) {
        *self = Channel::default();
        self.build(sr);
        self.target = *values;
        self.cur = *values;
    }

    fn process(&mut self, values: &[f32; 6], inp: &[&[f32]], out: &mut [&mut [f32]]) {
        for i in 0..6 {
            if values[i] != self.target[i] {
                self.target[i] = values[i];
                self.step[i] = (values[i] - self.cur[i]) * 0.00048828125;
                self.count[i] = 0x800;
            }
        }
        for s in [&mut self.s20, &mut self.s28, &mut self.s30, &mut self.s60, &mut self.s68, &mut self.s70, &mut self.s78, &mut self.sc8] {
            if (s.to_bits() >> 52) & 0x7fe == 0 {
                *s = 0.0;
            }
        }
        let c = self.c;
        let mix0 = self.cur[0];
        self.meter = 0.0;
        let nch = inp.len();
        for n in 0..inp[0].len() {
            for i in 0..6 {
                if self.count[i] != 0 {
                    self.count[i] -= 1;
                    if self.count[i] == 0 {
                        self.cur[i] = self.target[i];
                    } else {
                        self.cur[i] += self.step[i];
                    }
                }
            }
            let mut sum = 0.0f32;
            for ch in inp {
                sum += ch[n];
            }
            let x = (sum * (1.0 / nch as f32)).abs().max(1e-05);
            let l = (x.log10() * -0.1 + -0.48).min(0.0);
            let a = l as f64 * -2.78 + -0.3;
            let t = softplus(self.s20 * -0.318 + a);
            if t <= self.s20 {
                self.s20 *= c[1];
            } else {
                self.s20 = (t - self.s20) * (1.0 - c[0]) + self.s20;
            }
            let hold = if t <= self.s28 { self.s28 * c[2] } else { t };
            self.s28 = hold;
            let e = ((hold - self.s20) * 1.78 + -0.05).max(-0.05);
            let u = softplus(e + -0.3).max(0.0);
            if self.s20 < u {
                self.s20 = u;
                let v = softplus(a + u * -0.318);
                if hold < v {
                    self.s28 = v;
                }
            }
            let mut w = ((0.1 - e) + (1.0 - self.cur[4]) as f64 * 0.4).min(0.0);
            self.s30 = if self.s30 <= w { self.s30 * c[4] } else { (w - self.s30) * c[3] + self.s30 };
            if self.s30 <= w {
                w = self.s30;
            }
            let attack = self.cur[1];
            let p = {
                let p = softplus((l * -2.0) as f64 + -0.3);
                if p <= self.s60 { self.s60 * c[5] } else { p }
            };
            self.s60 = p;
            let q0 = softplus((l * -2.0) as f64 + -0.4);
            let s68 = if q0 <= self.s68 { self.s68 * c[7] } else { (q0 - self.s68) * c[6] + self.s68 };
            self.s68 = s68;
            let q = if q0 <= s68 { s68 } else { q0 };
            let mut s70 = self.s70;
            let mut d = q - s70;
            let r = (d * 4.7).max(0.0).min(13.0);
            let rise = s70 < r;
            if rise {
                self.s70 = r;
                self.s78 = (1.0 - c[11]) * (r - self.s78) + self.s78;
                d = q - r;
                s70 = r;
            }
            if 0.0 < d {
                self.s70 = (1.0 - c[9]) * d + s70;
                self.s78 = (1.0 - c[13]) * d + self.s78;
            } else {
                if s68 < s70 {
                    self.s68 = s68 + (c[8] - 1.0) * d;
                }
                if r < s70 {
                    self.s70 = s70 * c[10];
                    self.s78 *= c[12];
                }
            }
            self.sc8 = (1.0 - c[11]) * ((p - r) - self.sc8) + self.sc8;
            let g2 = (0.3 - self.sc8).min(0.94);
            let db = (w * -0.2733333333333333 * attack as f64) as f32 + (self.cur[2] as f64 * -0.05024509803921568 * g2) as f32;
            if self.meter < db as f64 {
                self.meter = db as f64;
            }
            let g = 10f64.powf((db * 20.0 + self.cur[3]) as f64 * 0.05) as f32 * self.cur[0] + (1.0 - mix0);
            for (o, i) in out.iter_mut().zip(inp) {
                o[n] = i[n] * g;
            }
        }
    }
}

pub struct EnvelopeShaper {
    ports: Vec<Port>,
    sr: f32,
    p01: [f32; 6],
    values: [f32; 6],
    stored: [f32; 6],
    ch: [Channel; 2],
}

impl EnvelopeShaper {
    pub fn new() -> EnvelopeShaper {
        let values = [1.0, 0.0, 0.0, 0.0, 0.725, 0.0];
        EnvelopeShaper { ports: ports_from(ports::ENVELOPE_SHAPER), sr: 0.0, p01: [0.0; 6], values, stored: [0.0; 6], ch: [Channel::default(), Channel::default()] }
    }

    fn set(&mut self, i: usize, v: f32) {
        self.p01[i] = v;
        self.values[i] = to_value(i, v);
    }
}

impl Default for EnvelopeShaper {
    fn default() -> Self {
        EnvelopeShaper::new()
    }
}

impl Node for EnvelopeShaper {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// `Voice::Reset` [re:da41b9]: every parameter back to its default
    /// (1, then 0.5); the detectors keep running.
    fn reset(&mut self) {
        for i in 0..6 {
            let v = if i == 0 { 1.0 } else { 0.5 };
            self.set(i, v);
            self.stored[i] = v;
        }
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            // PrepareStream: Configure(sr, 2), then the voice Reset.
            self.sr = ctx.sample_rate;
            for c in &mut self.ch {
                c.configure(self.sr as f64, &self.values);
            }
            self.reset();
        }
        for i in 0..6 {
            let v = self.ports[i].value;
            if 1e-06 <= (self.stored[i] - v).abs() {
                self.stored[i] = v;
                self.set(i, v);
            }
        }
        let values = self.values;
        let [ol, or] = out;
        if values[5] <= 0.5 {
            self.ch[0].process(&values, &[inp[0]], &mut [ol]);
            self.ch[1].process(&values, &[inp[1]], &mut [or]);
        } else {
            self.ch[0].process(&values, &inp, &mut [ol, or]);
        }
    }
}
