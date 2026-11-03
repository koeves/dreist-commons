//! [re:8fc789] (Noise Gate, kind 11; reset [re:fa4788],
//! processing [re:bacb48]): key = band-passed (LP/HP one-poles)
//! input, peak envelope with release, hold counter with hysteresis, gain
//! attack/release; Preview outputs the key.

use super::blocks::ALT;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct NoiseGate {
    ports: Vec<Port>,
    alt: f32,
    hold: i32,
    env: f32,
    gain: f32,
    lp: [f32; 2],
    hp: [f32; 2],
    /// Offset by the alternator, never otherwise used.
    spare: [f32; 4],
}

impl NoiseGate {
    pub fn new() -> NoiseGate {
        NoiseGate::with_ports(ports_from(ports::NOISE_GATE))
    }

    /// As built around `ports` (no allocation).
    fn with_ports(ports: Vec<Port>) -> NoiseGate {
        NoiseGate { ports, alt: f32::from_bits(ALT), hold: 0, env: 0.0, gain: 0.0, lp: [0.0; 2], hp: [0.0; 2], spare: [0.0; 4] }
    }
}

impl Default for NoiseGate {
    fn default() -> Self {
        NoiseGate::new()
    }
}

impl Node for NoiseGate {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let sr = ctx.sample_rate;
        let isr = 1.0 / sr;
        let att = (p(0) + -1.0).exp2();
        let hold_n = ((p(1) + -1.0).exp2() * sr).round() as i32;
        let thr = 10f32.powf((p(3).log10() * 20.0) / 20.0);
        let hyst = 10f32.powf((p(6).log10() * 20.0) / 20.0);
        let ka = (-6.2831855 / (att * sr)).exp();
        let f_lp = (p(7) + 1.0).exp2();
        let f_hp = (p(8) + 1.0).exp2();
        let k_hp = 1.0 - ((f_hp + f_hp) * 0.5 * -6.2831855 * isr).exp();
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let preview = p(9) >= 0.5;
        let mix = ((p(5).abs() + 1.0) - (p(5) - 1.0).abs()) * 0.5;
        let e_lp = (isr * (f_lp + f_lp) * -0.5 * 6.2831855).exp();
        let rel = (p(2) + -1.0).exp2();
        let kr = 1.0 - (1.0 - (-6.2831855 / (sr * rel)).exp());
        let [ol, or] = out;
        for i in 0..n {
            let a = self.alt;
            self.alt = -a;
            for v in self.lp.iter_mut().chain(&mut self.hp).chain(&mut self.spare) {
                *v -= a;
            }
            let key = [inp[0][i], inp[1][i]];
            let mut b = [0.0f32; 2];
            for c in 0..2 {
                self.lp[c] = (key[c] - self.lp[c]) * (1.0 - e_lp) + self.lp[c];
                let d = self.lp[c] - self.hp[c];
                self.hp[c] = k_hp * d + self.hp[c];
                b[c] = d * (1.0 - k_hp);
            }
            let (al, ar) = (b[0].abs(), b[1].abs());
            let peak = if ar <= al { al } else { ar };
            let decayed = self.env * kr;
            self.env = if peak <= decayed { decayed } else { peak };
            let mut next = hold_n;
            if peak <= thr {
                // Saturating: hours of silence would wrap the counter and
                // open the gate.
                let down = self.hold.saturating_sub(2);
                let reset = if self.env <= hyst * thr { down } else { hold_n };
                next = if 1 < self.hold { reset } else { down };
            }
            self.hold = next;
            if self.hold < 0 {
                self.gain *= kr;
            } else {
                self.gain = (1.0 - self.gain) * (1.0 - ka) + self.gain;
            }
            if preview {
                ol[i] = b[0];
                or[i] = b[1];
            } else {
                let (x, y) = (inp[0][i], inp[1][i]);
                ol[i] = x * mix * self.gain + (1.0 - mix) * x;
                or[i] = y * mix * self.gain + (1.0 - mix) * y;
            }
        }
    }

    fn reset(&mut self) {
        let alt = self.alt;
        *self = NoiseGate::with_ports(std::mem::take(&mut self.ports));
        self.alt = alt;
    }
}
