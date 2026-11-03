//! [re:55b4db] (Modulated Filter; `InitializePorts` [re:4ea5f6],
//! `Voice::Reset` [re:c59236], `PrepareStream` [re:8abef9], `ProcessBuffer`
//! [re:4f3743]): Chamberlin state-variable filter with a polynomial soft
//! clipper and diode feedback, cutoff modulated by a diode/RC envelope
//! follower (Newton-solved per sample) and optional FM from the sidechain.
//! Modes: Low, Band, High, Peak, Notch.

use crate::analog::{analog, lerp, lut_pos, sin_tab};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// The node's saturator: `((|c|·0.0016875 − 0.0225)·c² + 1)·c`, c clamped to ±6.6667.
#[inline]
fn sat(x: f32) -> f32 {
    let c = ((x + 6.6666665).abs() - (x - 6.6666665).abs()) * 0.5;
    ((c.abs() * 0.0016875 + -0.0225) * c * c + 1.0) * c
}

pub struct AutoFilter {
    ports: Vec<Port>,
    sr: f32,
    env: Vec<f32>,
    c_dc: f32,
    c_hp: f32,
    lp: [f32; 2],
    bp: [f32; 2],
    dc: [f32; 2],
    hp_in: [f32; 2],
    diode_i: [f32; 2],
    cap: f32,
    smooth: f32,
    drive: f32,
    out_drive: f32,
    pitch: f32,
    wet: f32,
    alt: f32,
    /// Envelope meter (control-out).
    pub meter: f32,
}

impl AutoFilter {
    pub fn new() -> AutoFilter {
        let mut ports = ports_from(ports::MODULATED_FILTER);
        ports[1].max = 4.0;
        AutoFilter { ports, env: Vec::with_capacity(256), ..AutoFilter::state() }
    }

    /// The running state as built (no ports or buffer).
    fn state() -> AutoFilter {
        AutoFilter {
            ports: Vec::new(),
            sr: 0.0,
            env: Vec::new(),
            c_dc: 0.0,
            c_hp: 0.0,
            lp: [1e-8, 1e-8],
            bp: [0.0, 0.0],
            dc: [0.0; 2],
            hp_in: [0.0; 2],
            diode_i: [1e-12, 1e-12],
            cap: 0.0,
            smooth: 0.0,
            drive: 0.0,
            out_drive: 0.0,
            // Voice::Reset leaves the pitch state at 1.0.
            pitch: 1.0,
            wet: 1.0,
            alt: super::ssl_filter::DENORMAL,
            meter: 0.0,
        }
    }
}

impl Default for AutoFilter {
    fn default() -> Self {
        AutoFilter::new()
    }
}

impl Node for AutoFilter {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// `Voice::Reset` [re:c59236] (`state` is its field values); the rate
    /// coefficients and the voice's denormal offset are kept.
    fn reset(&mut self) {
        let mut env = std::mem::take(&mut self.env);
        env.fill(0.0);
        let fresh = AutoFilter { ports: std::mem::take(&mut self.ports), env, ..AutoFilter::state() };
        let (sr, c_dc, c_hp, alt) = (self.sr, self.c_dc, self.c_hp, self.alt);
        *self = AutoFilter { sr, c_dc, c_hp, alt, ..fresh };
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if self.sr != ctx.sample_rate {
            // PrepareStream: 10 Hz DC blocker and 2.4 Hz envelope high-pass.
            self.sr = ctx.sample_rate;
            let inv = 1.0 / self.sr;
            self.c_dc = 1.0 - (inv * -62.831856).exp();
            self.c_hp = 1.0 - (inv * -15.079646).exp();
        }
        let p = |i: usize| self.ports[i].value;
        let drive_in = (p(0).abs() + p(0)) * 0.5;
        let drive_out = (p(4).abs() + p(4)) * 0.5;
        let mode = (((p(1).abs() + 4.0) - (p(1) - 4.0).abs()) * 0.5).round() as u32;
        let (pitch_t, res, fm, depth) = (p(2), p(3), p(11), p(10));
        let attack = crate::fx::exp2(p(8) - 1.0);
        let release = crate::fx::exp2(p(9) - 1.0);
        let wet_t = p(5);
        let inv_n = 1.0 / n as f32;
        let inv_sr = 1.0 / self.sr;
        let [ol, or] = out;
        // Anti-denormal offset flips every block.
        let a = self.alt;
        self.alt = -a;
        for c in 0..2 {
            self.lp[c] -= a;
            self.bp[c] -= a;
            self.dc[c] -= a;
        }
        self.smooth -= a;
        self.cap -= a;
        let k_att = ((-1.0 / ((attack / 0.001) * 8200.0 * 4.7e-07)) * inv_sr).exp();
        if p(6) < 0.5 {
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
            self.meter = 0.0;
            return;
        }
        let w0 = ctx.tune * inv_sr;
        self.env.clear();
        self.env.resize(n, 0.0);
        let depth10 = depth * 10.0;
        let mut meter = 0.0f32;
        let mut v = self.cap;
        for i in 0..n {
            let mut drv = [0.0f32; 2];
            for c in 0..2 {
                let e = inp[c][i] - self.hp_in[c];
                self.hp_in[c] += e * self.c_hp;
                let x = (e * depth10 + e * depth10).abs();
                drv[c] = sat(x.min(6.6666665)) - v;
            }
            let mut cur = [0.0f32; 2];
            if drv[0] > 0.0 || drv[1] > 0.0 {
                cur = [self.diode_i[0].max(0.0), self.diode_i[1].max(0.0)];
                for _ in 0..3 {
                    let s = (cur[0] + cur[1]) * 820.0;
                    let next: [f32; 2] = std::array::from_fn(|c| {
                        let f = ((cur[c] + 5.9e-9).ln() * 0.0475 + s + 0.900044858) - drv[c];
                        let d = 0.0475 / (cur[c] + 5.9e-9) + 820.0;
                        (cur[c] - f / d).max(0.0)
                    });
                    let moved = (next[0] * 0.001 <= (next[0] - cur[0]).abs()) || (next[1] * 0.001 <= (next[1] - cur[1]).abs());
                    cur = next;
                    if !moved {
                        break;
                    }
                }
            }
            self.diode_i = cur;
            let leak = v / ((release / 0.1) * 180000.0) + v / 3_300_000.0;
            v += ((cur[1] + cur[0] - leak) / 4.7e-07) * inv_sr;
            self.smooth = (v - self.smooth) * (1.0 - k_att) + self.smooth;
            let env = ((self.smooth + 1.0).log2() - 1.0) * depth10;
            if meter.abs() < env.abs() {
                meter = env;
            }
            self.env[i] = env;
        }
        self.cap = v;
        self.meter = meter;
        if mode > 4 {
            return;
        }
        let fm5 = {
            let f = (fm.abs() + fm) * 0.5;
            f * f * 5.0
        };
        let r = ((res.abs() + res) * 0.5).sqrt();
        let k = 2.0 - (r + r);
        let d_drive = ((drive_in * drive_in * drive_in) / 0.3 - self.drive) * inv_n;
        let d_out = (drive_out * drive_out * drive_out * 0.3 - self.out_drive) * inv_n;
        let d_wet = (((wet_t.abs() + 1.0) - (wet_t - 1.0).abs()) * 0.5 - self.wet) * inv_n;
        let d_pitch = (pitch_t - self.pitch) * inv_n;
        let st = sin_tab();
        let dl = &analog().diodelim;
        for i in 0..n {
            self.wet += d_wet;
            self.drive += d_drive;
            self.out_drive += d_out;
            self.pitch += d_pitch;
            let pc = self.pitch + self.env[i];
            let f = ((pc + 10.0).abs() - (pc - 10.0).abs()) * 0.5;
            let f = crate::fx::exp2(f);
            // One coefficient for both channels, FM from the left input
            // (one exp2 call and one 0.24 clamp per sample in the original).
            let w = (fm5 * inp[0][i] * f * w0 + f * w0) * 0.5;
            let wc = ((w.abs() + 0.24) - (w - 0.24).abs()) * 0.5;
            // `ms_sin_tab + 0x1000` indexed by `floor(wc·2048)` directly
            // (no table-centre offset; wc is clamped to 0..0.24).
            let v = wc * 2048.0;
            let iv = v.floor();
            let j = 1024 + iv as usize;
            let g = ((st[j + 1] - st[j]) * (v - iv) + st[j]) * 4.0;
            let t = (k * g * g) / 6.0;
            let (kd, gg) = (k - t, g - t);
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                let x = inp[c][i];
                let bp = self.bp[c];
                self.lp[c] += sat(bp) * gg;
                let lp = self.lp[c];
                let hp = lerp(dl, lut_pos(bp * 32.0)) + (((x * self.drive - lp) - bp * kd) - bp);
                let hs = sat(hp);
                self.bp[c] = hs * gg + bp;
                let o = match mode {
                    0 => sat(lp),
                    1 => sat(self.bp[c]),
                    2 => hs,
                    3 => sat(lp - hp),
                    _ => sat(hp + lp),
                } * self.out_drive
                    - self.dc[c];
                self.dc[c] += self.c_dc * o;
                y[c] = o;
            }
            ol[i] = (1.0 - self.wet) * inp[0][i] + y[0] * self.wet;
            or[i] = (1.0 - self.wet) * inp[1][i] + y[1] * self.wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Hollow "Vocals" settings (high-pass, resonance 0.73): the
    /// coefficient comes from the first quarter of the sine table, so the
    /// filter stays stable and passes signal for seconds (it used to read
    /// half a cycle further on, go negative and run away within a second).
    #[test]
    fn stays_stable_and_audible() {
        let ctx = Ctx::new(44100.0);
        for mode in 0..5 {
            let mut f = AutoFilter::new();
            let v = [1.0, mode as f32, 0.876, 0.726, 1.0, 1.0, 1.0, 0.0, -12.29, -4.08, 0.004, 0.0];
            for (p, v) in f.ports.iter_mut().zip(v) {
                p.value = v;
            }
            let mut seed = 1u32;
            let mut last = 0.0;
            for _ in 0..400 {
                let x: Vec<f32> = (0..256)
                    .map(|_| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed >> 9) as f32 / 8388608.0 - 1.0
                    })
                    .map(|v| v * 0.3)
                    .collect();
                let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
                f.process(&ctx, [&x, &x], [&mut l, &mut r]);
                assert!(f.lp.iter().chain(&f.bp).all(|s| s.abs() < 100.0), "mode {mode}: {:?} {:?}", f.lp, f.bp);
                last = l.iter().map(|v| v * v).sum::<f32>() / 256.0;
            }
            assert!(last > 1e-4, "mode {mode}: {last}");
        }
    }
}
