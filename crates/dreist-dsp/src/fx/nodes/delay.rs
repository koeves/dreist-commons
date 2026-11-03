//! [re:c23a25] (Delay; `InitializePorts` [re:ae61ec], `SetMaxDelay`
//! [re:71bddc], `Reset` [re:0322d7], `PrepareStream` [re:b8c3e6], `ProcessBuffer`
//! [re:0fa774]): tempo-syncable stereo (or summed-mono) feedback delay. The
//! delay time glides tape-style toward its target, the feedback path is
//! soft-saturated and band-limited (one-pole low-pass then high-pass), and
//! Swing shortens one side's output tap.

use super::math::{cubic, pow2_at_least, sat};
use crate::fx::convert::{bpm_from_text, time_beat_to_units};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// [re:ea9df4] (max sample rate, [re:3e3b67]) × 8.
const MAX_DELAY: f32 = 384000.0 * 8.0;

fn clip(x: f32, lo: f32, hi: f32) -> f32 {
    ((x - lo).abs() + lo + hi - (x - hi).abs()) * 0.5
}

pub struct Delay {
    ports: Vec<Port>,
    buf: [Vec<f32>; 2],
    mask: u32,
    idx: u32,
    /// 0x138 current delay (samples), 0x13c target, 0x140 glide rate.
    cur: f32,
    target: f32,
    rate: f32,
    /// 0x144/0x148 low-pass, 0x14c/0x150 high-pass states.
    lp: [f32; 2],
    hs: [f32; 2],
    /// 0x12c/0x130 swing tap multipliers, 0x134 their smoothing.
    swing: [f32; 2],
    /// 0x154 wet, 0x158 dry, 0x15c feedback ramps.
    wet: f32,
    dry: f32,
    fb: f32,
    /// 0x168: previous block was summed-mono (copy L → R when leaving it).
    was_mono: u32,
    alt: f32,
}

impl Delay {
    pub fn new() -> Delay {
        let mut ports = ports_from(ports::DELAY);
        let (def, lo, hi) = (bpm_from_text("1/64 note"), bpm_from_text("1/64 note"), bpm_from_text("2 note"));
        ports[0] = Port::new(def, lo, hi);
        Delay {
            ports,
            buf: [Vec::new(), Vec::new()],
            mask: 0,
            idx: 0,
            cur: 5.0,
            target: 5.0,
            rate: 0.0,
            lp: [0.0; 2],
            hs: [0.0; 2],
            swing: [1.0; 2],
            wet: 1.0,
            dry: 1.0,
            fb: 0.0,
            was_mono: 2,
            alt: super::ssl_filter::DENORMAL,
        }
    }

    /// `SetMaxDelay`: power-of-two buffers covering `MAX_DELAY` samples
    /// (allocated on first use).
    fn alloc(&mut self) {
        if !self.buf[0].is_empty() {
            return;
        }
        let size = pow2_at_least(MAX_DELAY.round() as i64);
        self.buf = [vec![0.0; size], vec![0.0; size]];
        self.mask = size as u32 - 1;
    }
}

impl Default for Delay {
    fn default() -> Self {
        Delay::new()
    }
}

impl Node for Delay {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn port_value(&self, index: usize) -> Option<f32> {
        self.ports.get(index).map(|p| p.value)
    }

    fn prepare_rate(&mut self, _sr: f32) {
        self.alloc();
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.alloc();
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let p = |i: usize| self.ports[i].value;
        let sr = ctx.sample_rate;
        let inv_sr = 1.0 / sr;
        let inv_n = 1.0 / n as f32;
        let fb_step = (p(2) - self.fb) * inv_n;
        let t = time_beat_to_units(p(0), p(1), 60.0 / ctx.bpm.max(1e-8));
        let lo_f = (p(5) + 1.0).exp2();
        let hi_f = (p(6) + 1.0).exp2();
        let k_lp = 1.0 - (-((hi_f + hi_f) * 0.5 * 6.2831855 * inv_sr)).exp();
        let e_hp = (-((lo_f + lo_f) * 0.5 * 6.2831855 * inv_sr)).exp();
        let k_hp = 1.0 - e_hp;
        let w = p(7) + p(7);
        let sw = p(3);
        let s = sw * 0.5;
        let target = ((MAX_DELAY + 2.0 + (t * sr - 2.0).abs()) - (t * sr - MAX_DELAY).abs()) * 0.5;
        if self.target != target {
            self.rate = 1.0 - self.cur / target;
            self.target = target;
        }
        let mono = p(4) >= 0.5;
        // Mono → stereo: the right line starts as a copy of the summed one.
        // After a reset (`was_mono` 2) both lines are silent already, so the
        // 16 MiB copy is skipped.
        if !mono && self.was_mono == 1 {
            let (l, r) = self.buf.split_at_mut(1);
            r[0].copy_from_slice(&l[0]);
        }
        let dry_step = (clip(2.0 - w, 0.0, 1.0) - self.dry) * inv_n;
        let wet_step = (clip(w, 0.0, 1.0) - self.wet) * inv_n;
        let sw_t = [clip(1.0 + s, 0.5, 1.0), clip(1.0 - s, 0.5, 1.0)];
        self.was_mono = mono as u32;
        let a = self.alt;
        self.alt = -a;
        let mask = self.mask;
        let [ol, or] = out;
        let gliding = self.rate != 0.0 || (self.swing[0] - sw_t[0]).abs() > 0.01 || (self.swing[1] - sw_t[1]).abs() > 0.01;
        let chans = if mono { 1 } else { 2 };
        for c in 0..chans {
            self.lp[c] -= a;
            self.hs[c] -= a;
        }
        if gliding {
            self.swing[0] -= a;
            self.swing[1] -= a;
            for i in 0..n {
                self.dry += dry_step;
                self.wet += wet_step;
                self.fb += fb_step;
                let fb = self.fb;
                if (self.cur - target).abs() <= self.rate.abs() {
                    self.cur = target;
                    self.rate = 0.0;
                } else {
                    self.cur += self.rate;
                }
                let d = self.cur;
                for c in 0..2 {
                    self.swing[c] += (sw_t[c] - self.swing[c]) * 0.03125;
                }
                self.idx = self.idx.wrapping_add(1) & mask;
                let idx = self.idx;
                let pos = idx as i32 as f32;
                let x = [inp[0][i], inp[1][i]];
                if mono {
                    let tap = clip(cubic(&self.buf[0], mask, pos - d), -2.0, 2.0);
                    let lp = ((fb * sat(tap) + (x[0] + x[1]) * 0.5) - self.lp[0]) * k_lp + self.lp[0];
                    self.lp[0] = lp;
                    let hp = lp - self.hs[0];
                    self.hs[0] += k_hp * hp;
                    self.buf[0][(idx & mask) as usize] = hp * e_hp;
                    ol[i] = cubic(&self.buf[0], mask, pos - self.swing[0] * d) * self.wet + x[0] * self.dry;
                    or[i] = cubic(&self.buf[0], mask, pos - self.swing[1] * d) * self.wet + x[1] * self.dry;
                } else {
                    let taps = [cubic(&self.buf[0], mask, pos - d), cubic(&self.buf[1], mask, pos - d)];
                    for c in 0..2 {
                        let tap = clip(taps[c], -2.0, 2.0);
                        let lp = ((sat(tap) * fb + x[c]) - self.lp[c]) * k_lp + self.lp[c];
                        self.lp[c] = lp;
                        let hp = lp - self.hs[c];
                        self.hs[c] += k_hp * hp;
                        self.buf[c][(idx & mask) as usize] = hp * e_hp;
                    }
                    ol[i] = cubic(&self.buf[0], mask, pos - self.swing[0] * d) * self.wet + x[0] * self.dry;
                    or[i] = cubic(&self.buf[1], mask, pos - self.swing[1] * d) * self.wet + x[1] * self.dry;
                }
            }
        } else {
            let d = self.cur;
            let dl = (self.swing[0] * d).round() as i32 as u32;
            let dr = (self.swing[1] * d).round() as i32 as u32;
            let di = d.round() as i32 as u32;
            for i in 0..n {
                self.dry += dry_step;
                self.wet += wet_step;
                self.fb += fb_step;
                let fb = self.fb;
                self.idx = self.idx.wrapping_add(1) & mask;
                let idx = self.idx;
                let x = [inp[0][i], inp[1][i]];
                let rd = |b: &Vec<f32>, off: u32| b[(idx.wrapping_sub(off) & mask) as usize];
                if mono {
                    let tap = clip(rd(&self.buf[0], di), -2.0, 2.0);
                    let lp = ((sat(tap) * fb + (x[0] + x[1]) * 0.5) - self.lp[0]) * k_lp + self.lp[0];
                    self.lp[0] = lp;
                    let hp = lp - self.hs[0];
                    self.hs[0] += k_hp * hp;
                    self.buf[0][(idx & mask) as usize] = hp * e_hp;
                    ol[i] = self.wet * rd(&self.buf[0], dl) + x[0] * self.dry;
                    or[i] = self.wet * rd(&self.buf[0], dr) + x[1] * self.dry;
                } else {
                    let taps = [rd(&self.buf[0], di), rd(&self.buf[1], di)];
                    for c in 0..2 {
                        let tap = clip(taps[c], -2.0, 2.0);
                        let lp = ((sat(tap) * fb + x[c]) - self.lp[c]) * k_lp + self.lp[c];
                        self.lp[c] = lp;
                        let hp = lp - self.hs[c];
                        self.hs[c] += k_hp * hp;
                        self.buf[c][(idx & mask) as usize] = hp * e_hp;
                    }
                    ol[i] = self.wet * rd(&self.buf[0], dl) + x[0] * self.dry;
                    or[i] = self.wet * rd(&self.buf[1], dr) + x[1] * self.dry;
                }
            }
        }
        if sw.abs() < 1e-4 && ((self.swing[0] - sw_t[0]).abs() < 1e-4 || (self.swing[1] - sw_t[1]).abs() < 1e-4) {
            self.swing = sw_t;
        }
    }

    fn reset(&mut self) {
        self.cur = 5.0;
        self.target = 5.0;
        self.rate = 0.0;
        self.lp = [0.0; 2];
        self.hs = [0.0; 2];
        self.wet = 1.0;
        self.dry = 1.0;
        self.fb = 0.0;
        self.swing = [1.0; 2];
        self.was_mono = 2;
        for b in &mut self.buf {
            b.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_arrives_after_delay_time() {
        let mut d = Delay::new();
        let ctx = Ctx::new(48000.0);
        // Free-time mode, 0.25 s (port value log2(t) + 1), full wet, no swing.
        d.ports[1].value = 0.0;
        d.ports[0].value = 0.25f32.log2() + 1.0;
        d.ports[3].value = 0.0;
        d.ports[7].value = 1.0;
        d.ports[2].value = 0.0;
        d.ports[5].value = 20f32.log2() - 1.0;
        d.ports[6].value = 18000f32.log2() - 1.0;
        let n = 512;
        let mut peak = (0usize, 0.0f32);
        let mut t = 0usize;
        for blk in 0..80 {
            let mut x = vec![0.0f32; n];
            if blk == 40 {
                x[0] = 1.0;
            }
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            d.process(&ctx, [&x, &x], [&mut l, &mut r]);
            for (i, v) in l.iter().enumerate() {
                if blk > 40 && v.abs() > peak.1 {
                    peak = (t + i, v.abs());
                }
            }
            t += n;
        }
        assert_eq!(peak.0 - 40 * n, 12000, "{peak:?}");
    }

    /// `GetLengthOfTail`: echo time × (1 + repeats until the feedback falls
    /// to the voice noise floor), so a pad's voice outlives its echoes.
    #[test]
    fn tail_covers_the_echoes() {
        let mut d = Delay::new();
        let ctx = Ctx { bpm: 120.0, ..Ctx::new(44100.0) };
        // Beat sync, half a beat (0.25 s), feedback 0.514.
        let v = [-0.069996834, 1.0, 0.5140119, -1.0, 0.0, 7.247441, 13.13571, 1.0];
        for (p, v) in d.ports.iter_mut().zip(v) {
            p.value = v;
        }
        let t = crate::fx::nodes::tail(15, &[0.0; 12], &d, &ctx);
        // 22050 · (1 + |−260.3 / −11.9|) ≈ 504 500 samples.
        assert!((500_000..510_000).contains(&t), "{t}");
    }
}
