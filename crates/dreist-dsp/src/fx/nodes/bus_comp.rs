//! [re:68112c] (Bus Compressor, kind 9; `Voice::Reset`
//! [re:256986], `Voice::ProcessBuffer` [re:5658cd]) around [re:8a3887]
//! ([re:5ab80f]..[re:07c121]): a feedback bus compressor modelled as a circuit.
//! The high-passed side-chain, scaled by the current gain, goes through a
//! ratio transfer table (ideal or "analog limit" clip curves); two diodes
//! (Newton solve, up to 12 iterations from 1e-12 A) charge the timing
//! capacitor through the attack resistor; release is a resistor (or the
//! two-capacitor "Auto" network); the capacitor voltage sets the gain.

use super::mixbus_tables::{RATIO_C, RATIO_K, RATIO_TABLES};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// [re:fd1190] values (attack resistor, Ω).
const ATTACK: [f32; 6] = [820.0, 2700.0, 8200.0, 27000.0, 82000.0, 270000.0];
/// [re:6254bc] values (release resistor, Ω; 0 = Auto).
const RELEASE: [f32; 5] = [180000.0, 270000.0, 560000.0, 1.2e6, 0.0];
/// [re:b70f73] values.
const RATIO: [f32; 3] = [2.0, 4.0, 10.0];

struct Mixbus {
    sr: f32,
    dt: f32,
    alt: f32,
    attack: f32,
    release: f32,
    v1: f32,
    v2: f32,
    makeup_lp: f32,
    spare: f32,
    k_makeup: f32,
    gain_db: f32,
    threshold: f32,
    table: usize,
    meter: [f32; 2],
    hp_freq: f32,
    hp: [f32; 2],
    k_hp: f32,
    auto: bool,
    mix: f32,
    makeup: f32,
}

impl Mixbus {
    /// [re:a59913].
    fn new() -> Mixbus {
        Mixbus {
            sr: 44100.0,
            dt: f32::from_bits(0x37be37c6),
            alt: f32::from_bits(0x322bcc77),
            attack: 8200.0,
            release: 560000.0,
            v1: 0.0,
            v2: 0.0,
            makeup_lp: 0.0,
            spare: 0.0,
            k_makeup: 0.0,
            gain_db: 0.0,
            threshold: 0.5,
            table: 4,
            meter: [1.0; 2],
            hp_freq: 0.0,
            hp: [0.0; 2],
            k_hp: 0.0,
            auto: false,
            mix: 1.0,
            makeup: 1.0,
        }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.dt = 1.0 / sr;
        self.k_makeup = 1.0 - ((1.0 / sr) * -20734.512).exp();
    }

    /// `setRatio(ratio, clip)` [re:5ab80f].
    fn set_ratio(&mut self, r: i32, clip: bool) {
        let i = match r {
            10 => 2,
            2 => 0,
            _ => 1,
        };
        self.table = i + if clip { 3 } else { 0 };
    }

    fn set_sidechain_hp(&mut self, f: f32) {
        if self.hp_freq != f {
            self.hp_freq = f;
            self.k_hp = 1.0 - (f * -6.2831855 * self.dt).exp();
        }
    }

    /// `setRelease` [re:173900].
    fn set_release(&mut self, r: f32) {
        let mut auto = true;
        if 1e-08 <= r.abs() {
            if self.auto {
                self.v1 = self.v2 + self.v1;
                self.v2 = 0.0;
            }
            auto = false;
        }
        self.auto = auto;
        self.release = r;
    }

    /// Two-diode Newton solve; returns the (negated) diode currents.
    fn diodes(&self, d: [f32; 2], v: f32) -> [f32; 2] {
        let e = [d[0] - v, d[1] - v];
        if !(e[0] < 0.0 || e[1] < 0.0) {
            return [0.0; 2];
        }
        let r = self.attack;
        let mut c = [9.999999960041972e-13f32; 2];
        for _ in 0..12 {
            let s = (c[0] + c[1]) * r;
            let next: [f32; 2] = std::array::from_fn(|k| {
                let x = c[k] + 5.9e-09;
                let f = ((s + x.ln() * 0.0475) + 0.900044858) + e[k];
                let dv = 0.0475 / x + r;
                (c[k] - f / dv).max(0.0)
            });
            let done = (next[0] - c[0]).abs() < next[0] * 0.001 && (next[1] - c[1]).abs() < next[1] * 0.001;
            c = next;
            if done {
                break;
            }
        }
        [-c[0], -c[1]]
    }

    fn lookup(&self, v: f32) -> f32 {
        if v > 6.0 {
            return f32::from_bits(RATIO_C[self.table]) - f32::from_bits(RATIO_K[self.table]) * v;
        }
        let t = v * -100.0 + 600.0;
        let fi = t.floor() as i32;
        let frac = t - fi as f32;
        let i = fi.clamp(0, 1200) as usize;
        let tab = &RATIO_TABLES[self.table];
        frac * f32::from_bits(tab[i + 1]) + (1.0 - frac) * f32::from_bits(tab[i])
    }

    /// [re:9fc815] (side-chain = the main input here).
    fn process(&mut self, sc: [&[f32]; 2], x: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let a = self.alt;
        self.alt = -a;
        self.meter[0] -= a;
        self.meter[1] -= a;
        self.v1 -= a;
        self.v2 -= a;
        self.makeup_lp -= a;
        self.spare -= a;
        self.gain_db -= a;
        self.hp[0] -= a;
        self.hp[1] -= a;
        let inv_rel = 1.0 / self.release;
        let mk = (self.makeup * 40.0 + -10.0) / 164.0;
        let th = (self.threshold * -50.0 + 30.0) / 7.8;
        let dry = 1.0 - self.mix;
        let [ol, or] = out;
        for i in 0..x[0].len() {
            let g = 10f32.powf((self.gain_db * -7.8) / 20.0) * 2.4 * 0.36;
            let hl = sc[0][i] - self.hp[0];
            let hr = sc[1][i] - self.hp[1];
            self.hp[0] = hl * self.k_hp + self.hp[0];
            self.hp[1] = self.k_hp * hr + self.hp[1];
            let dl = self.lookup((hl * g).abs());
            let dr = self.lookup((hr * g).abs());
            let v = if self.auto {
                let vt = self.v1 + self.v2;
                let c = self.diodes([dl, dr], vt);
                self.v1 = (((self.v1 / -91000.0 + c[1] + c[0]) - vt / 3.3e6) / 4.7e-07) * self.dt + self.v1;
                self.v2 = (((c[1] + c[0] + self.v2 / -750000.0) - vt / 3.3e6) / 6.8e-06) * self.dt + self.v2;
                self.v2 + self.v1
            } else {
                let v = self.v1;
                let c = self.diodes([dl, dr], v);
                self.v1 = ((((c[1] + c[0]) - inv_rel * v) + v / -3.3e6) / 4.7e-07) * self.dt + v;
                self.v1
            };
            self.gain_db = -v - th;
            self.makeup_lp = ((v * -0.0455166 - mk) - self.makeup_lp) * self.k_makeup + self.makeup_lp;
            let gain = 10f32.powf((self.makeup_lp * -164.0) / 20.0) * self.mix + dry;
            ol[i] = x[0][i] * gain;
            or[i] = gain * x[1][i];
        }
    }
}

pub struct BusComp {
    ports: Vec<Port>,
    comp: Mixbus,
}

impl BusComp {
    pub fn new() -> BusComp {
        let mut ports = ports_from(ports::BUS_COMPRESSOR);
        // [re:7d9636]: pot = (dB − min)/(max − min).
        ports[0] = Port::new(0.6, 0.0, 1.0);
        ports[4] = Port::new(0.25, 0.0, 1.0);
        // Float-form enum ports: max = item count (as for the Comb mode).
        ports[1] = Port::new(1.0, 0.0, 6.0);
        ports[2] = Port::new(1.0, 0.0, 3.0);
        ports[3] = Port::new(2.0, 0.0, 5.0);
        // Side-chain HP "Off": below 20.01 Hz, which selects the 2.34 Hz
        // fallback (log2(2000) − 1 at the top).
        ports[8] = Port::new(0.0, 0.0, 2000f32.log2() - 1.0);
        BusComp { ports, comp: Mixbus::new() }
    }
}

impl Default for BusComp {
    fn default() -> Self {
        BusComp::new()
    }
}

impl Node for BusComp {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    // `Voice::Reset` [re:256986] only zeroes the gain-reduction control-out,
    // which is not modelled.

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = |i: usize| self.ports[i].value;
        let c = &mut self.comp;
        c.set_sample_rate(ctx.sample_rate);
        let clamp = |x: f32, m: f32| (((x.abs() + m) - (x - m).abs()) * 0.5).round() as usize;
        let ratio = RATIO[clamp(p(2), 2.0).min(2)].floor() as i32;
        c.threshold = p(0);
        c.attack = ATTACK[clamp(p(1), 5.0).min(5)];
        c.set_release(RELEASE[clamp(p(3), 4.0).min(4)]);
        c.makeup = p(4);
        c.set_ratio(ratio, 0.5 <= p(9));
        let f = (p(8) + 1.0).exp2();
        c.set_sidechain_hp(if f < 20.01 { f32::from_bits(0x4015c28f) } else { f });
        c.mix = ((p(5).abs() + 1.0) - (p(5) - 1.0).abs()) * 0.5;
        if 0.5 <= p(6) {
            c.process(inp, inp, out);
        } else {
            let [ol, or] = out;
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_out(thr: f32) -> f32 {
        let ctx = Ctx::new(48000.0);
        let mut c = BusComp::new();
        c.ports[0].value = thr;
        c.ports[2].value = 2.0;
        let n = 480;
        let mut acc = 0.0;
        for b in 0..200 {
            let x: Vec<f32> = (0..n).map(|i| ((b * n + i) as f32 * 0.05).sin() * 0.5).collect();
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            c.process(&ctx, [&x, &x], [&mut l, &mut r]);
            assert!(l.iter().all(|v| v.is_finite()));
            if b >= 100 {
                acc += l.iter().map(|v| v * v).sum::<f32>();
            }
        }
        (acc / (100.0 * n as f32)).sqrt()
    }

    #[test]
    fn lower_threshold_compresses_more() {
        let hi = rms_out(1.0);
        let lo = rms_out(0.0);
        assert!(lo < hi * 0.8, "lo {lo} hi {hi}");
    }
}
