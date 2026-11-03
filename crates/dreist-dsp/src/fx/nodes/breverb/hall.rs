//! [re:c3ddf6] (construction [re:af1687], rate change [re:f2831f], parameter set
//! [re:887a80], size change [re:378aed], feedback change [re:39a517],
//! response change [re:33b0c4], processing [re:8a43cd]): per-channel predelay
//! combs and diffusers feeding a four-stage serial loop (each stage a
//! pre-delay with feedforward tap, allpass and delay; stages 2 and 4 add
//! the shelved loop signal, damping and an LFO-swept allpass), read by
//! pan-weighted multi-taps and high-passed at the output.

use super::blocks::*;
use super::{Algo, Param};

/// Hall parameter table [re:3c3501].
pub const PARAMS: &[Param] = &[
    Param(1, 0.6, 20.0),       // 0 Time
    Param(0, 0.0, 100.0),      // 1 Size
    Param(0, 0.0, 100.0),      // 2 Diffusion
    Param(0, 0.0, 100.0),      // 3 Shape
    Param(1, 0.25, 4.0),       // 4 High
    Param(0, 0.0, 100.0),      // 5 Wet
    Param(0, 0.0, 100.0),      // 6 Spread
    Param(0, 0.0, 0.5),        // 7 Predelay
    Param(0, 0.0, 100.0),      // 8 FB Left
    Param(0, 0.0, 100.0),      // 9 FB Right
    Param(1, 800.0, 18000.0),  // 10 Damping
    Param(0, 0.0, 100.0),      // 11 Spin
    Param(0, 0.0, 100.0),      // 12 Wander
    Param(1, 0.25, 4.0),       // 13 Low
    Param(1, 30.0, 960.0),     // 14 Low Freq
    Param(1, 1100.0, 18000.0), // 15 High Freq
    Param(1, 20.0, 1000.0),    // 16 Low Cut
    Param(0, 0.0, 100.0),      // 17 Dry
    Param(2, 0.0, 1.0),        // 18 Predelay Sync
    Param(2, 0.0, 8.0),        // 19 Predelay BPM
];

include!("hall_pan.rs");

/// One loop stage's taps into the outputs: (delay index, length, gain, to right).
type Tap = (usize, i64, f32, bool);

pub struct Hall {
    sr: f32,
    n_in: i64,
    pre: [DelayItem; 2],
    /// Input diffusers (two left, two right).
    ap_in: [DelayItem; 4],
    /// Stage pre-delays.
    sd: [DelayItem; 4],
    /// LFO allpasses (stage 2, stage 4).
    lfo: [DelayItemLfo; 2],
    /// Allpasses (stage 1, 3, 2b, 4b).
    ap: [DelayItem; 4],
    /// Loop delays.
    dl: [DelayItem; 6],
    damp: [Bilin; 2],
    low_cut: [Bilin; 2],
    shelf: [Biquad; 2],
    /// Feedforward taps / gains of the stage pre-delays.
    sd_tap: [i64; 4],
    sd_ff: [f32; 4],
    sd_in: [f32; 4],
    taps: [Tap; 25],
    time: f32,
    size: f32,
    diffusion: f32,
    predelay: f32,
    spread: f32,
    shape: f32,
    spin: f32,
    wander: f32,
    damping: f32,
    low_cut_freq: f32,
    low: f32,
    low_freq: f32,
    high: f32,
    high_freq: f32,
    sync: i64,
    sync_mult: f32,
    bpm: f32,
    fade_state: i32,
    fade: f32,
    fade_step: f32,
    time_mult: f32,
    shelf_norm: f32,
    fb: f32,
    wet: f32,
    wet_t: f32,
    dry: f32,
    dry_t: f32,
    a: Block,
    b: Block,
    d: Block,
    f: Block,
    /// Loop signal carried between stages and blocks.
    e: Block,
    noise: Block,
}

impl Hall {
    pub fn new() -> Hall {
        let mut pre = [DelayItem::new(0x10000), DelayItem::new(0x10000)];
        for p in &mut pre {
            p.gain = 0.0;
        }
        let mut h = Hall {
            sr: 0.0,
            n_in: 2,
            pre,
            ap_in: std::array::from_fn(|_| DelayItem::new(0x4000)),
            sd: std::array::from_fn(|_| DelayItem::new(0x8000)),
            lfo: [DelayItemLfo::new(0x8000), DelayItemLfo::new(0x8000)],
            ap: std::array::from_fn(|_| DelayItem::new(0x8000)),
            dl: std::array::from_fn(|_| DelayItem::new(0x8000)),
            damp: [Bilin::default(); 2],
            low_cut: [Bilin::default(); 2],
            shelf: [Biquad::default(); 2],
            sd_tap: [0; 4],
            sd_ff: [0.0; 4],
            sd_in: [0.0; 4],
            taps: [(0, 0, 0.0, false); 25],
            time: 1.0,
            size: 0.5,
            diffusion: 0.7,
            predelay: f32::from_bits(0x3b03126f),
            spread: 0.5,
            shape: 0.5,
            spin: 0.2,
            wander: 0.2,
            damping: 18000.0,
            low_cut_freq: 20.0,
            low: 0.5,
            low_freq: 200.0,
            high: 0.5,
            high_freq: 5000.0,
            sync: 0,
            sync_mult: 1.0,
            bpm: 120.0,
            fade_state: 0,
            fade: 1.0,
            fade_step: 0.0,
            time_mult: 1.0,
            shelf_norm: 0.0,
            fb: 0.0,
            wet: 1.0,
            wet_t: 1.0,
            dry: 1.0,
            dry_t: 1.0,
            a: [0.0; B],
            b: [0.0; B],
            d: [0.0; B],
            f: [0.0; B],
            e: [0.0; B],
            noise: anti_underflow_noise(),
        };
        h.change_wave_info(44100.0, 2, 2);
        h.fade = 1.0;
        h.fade_state = 0;
        h
    }

    fn change_diffusion(&mut self) {
        let g = self.diffusion * -0.8;
        for d in &mut self.ap_in {
            d.gain = g;
        }
    }

    fn change_predelay(&mut self) {
        let s = if self.sync == 0 { self.predelay } else { (self.sync_mult * 60.0) / self.bpm };
        let n = predelay_len(s, self.sr);
        for p in &mut self.pre {
            p.set_length(n);
        }
    }

    /// Spin change [re:064afb].
    fn change_spin(&mut self) {
        let f = (20f64.powf(self.spin as f64) * 0.1f32 as f64) as f32;
        let d = ((self.wander * 16.0) / f) as i32 as i64;
        let d = if d > 0x40 { 0x40 } else { d };
        self.lfo[0].set_lfo(d, f * 0.95);
        self.lfo[1].set_lfo(d, f * 1.05);
    }

    fn change_low_cut(&mut self) {
        self.low_cut[0].sr = self.sr;
        self.low_cut[0].set_hpf(self.low_cut_freq);
        self.low_cut[1].sr = self.low_cut[0].sr;
        self.low_cut[1].c = self.low_cut[0].c;
    }

    /// Size change [re:378aed].
    fn change_sizes(&mut self) {
        let r = self.sr / 44100.0;
        let f6 = (self.size * 9.0 + 1.0) * r;
        let f5 = r * 16.0 * self.spread;
        let pi = (self.shape * 127.0) as i32 as usize;
        let p1 = f32::from_bits(PAN[pi][0]);
        let p2 = f32::from_bits(PAN[pi][1]);
        let n = |k: f32| (f6 * k) as i32 as i64;
        for (d, k) in self.ap_in.iter_mut().zip([38.0, 89.0, 45.0, 74.0]) {
            d.set_length(n(k));
        }
        let m = |k: f32| (f5 * k) as i32;
        let clamp = |v: i32| if v > 0x7fff { 0x7fff } else { v as i64 };
        self.sd_tap = [0, m(150.0) as i64, m(55.0) as i64, m(281.0) as i64];
        self.sd[0].set_length(clamp((f5 * 4077.0 * 0.125) as i32));
        self.sd[1].set_length(clamp((f5 * 7027.0 * 0.125) as i32 + m(150.0)));
        self.sd[2].set_length(clamp((f5 * 6305.0 * 0.125) as i32 + m(55.0)));
        self.sd[3].set_length(clamp((f5 * 8120.0 * 0.125) as i32 + m(281.0)));
        let g = f32::from_bits(0x3f0f5c29);
        self.ap[0].set_length(n(428.0));
        self.ap[0].gain = -g;
        self.lfo[0].d.set_length(n(129.0));
        self.lfo[0].d.gain = g;
        self.ap[1].set_length(n(285.0));
        self.ap[1].gain = g;
        self.lfo[1].d.set_length(n(269.0));
        self.lfo[1].d.gain = -g;
        self.ap[2].set_length(n(228.0));
        self.ap[2].gain = -g;
        self.ap[3].set_length(n(226.0));
        self.ap[3].gain = g;
        self.dl[0].set_length(n(423.0));
        self.dl[1].set_length(n(761.0));
        self.dl[4].set_length(n(581.0) + 2);
        self.dl[2].set_length(n(443.0) - 1);
        self.dl[3].set_length(n(837.0));
        self.dl[5].set_length(n(547.0) - 32);
        // Output taps; delay indices 0..5 = dl, 6 = lfo[0], 7 = ap[1].
        let x = |v: u32| f32::from_bits(v);
        self.taps = [
            // Stage 1 (dl0): left overwrites, right overwrites then adds.
            (0, n(15.0), p1 * -0.172, false),
            (0, n(4.0), p1 * -0.222, true),
            (0, n(279.0) + n(4.0), p1 * 0.19092001, true),
            // Stage 2 (lfo0, dl1).
            (6, 0, p2 * 0.06, false),
            (1, n(160.0), p2 * 0.0182, false),
            (1, n(433.0) + n(160.0), p2 * 0.016, false),
            (1, n(42.0), p2 * 0.0444, true),
            (1, n(432.0) + n(42.0), p2 * -0.017094, true),
            // Stage 2b (dl4).
            (4, 2, x(0x3ea66667), false),
            (4, (n(431.0) + 2), x(0xbe051eb8), false),
            (4, (n(91.0) + 2), x(0xbecb2ca6), true),
            (4, (n(192.0) + 2), x(0xbe73cf2d), true),
            // Stage 3 (ap1, dl2).
            (7, n(25.0), p1 * 0.67, false),
            (2, n(183.0) + n(25.0), p1 * -0.195, false),
            (2, n(175.0) + (n(183.0) + n(25.0)), p1 * -0.17, false),
            (2, n(61.0), p1 * 0.50616, true),
            (2, n(237.0) + n(61.0), p1 * 0.18981001, true),
            // Stage 4 (dl3).
            (3, n(356.0), p2 * -0.0167, false),
            (3, n(398.0) + n(356.0), p2 * 0.0144, false),
            (3, n(166.0), p2 * -0.018537, true),
            (3, n(374.0) + n(166.0), p2 * 0.015984, true),
            // Stage 4b (dl5).
            (5, 0, x(0xbea66667), false),
            (5, n(431.0), x(0x3e47ae14), false),
            (5, n(126.0), x(0xbe13c361), true),
            (5, n(265.0), x(0x3e13c361), true),
        ];
    }

    /// Feedback change [re:39a517].
    fn change_feedback(&mut self) {
        let t = ((self.time_mult as f64).log10() * 0.699999988079071 + self.time as f64) as f32;
        let t = if t >= 0.0 { if t > 1.0 { 1.0 } else { t as f64 } } else { 0.0 };
        self.shelf_norm = (t.powf(0.4f32 as f64) * 7.9f32 as f64 + 1.8f32 as f64) as f32;
        let p = (self.time as f64).powf(0.2f32 as f64);
        self.fb = (p * 0.099f32 as f64 + 0.002f32 as f64) as f32;
        let g = self.fb * 38.0 + 1.0;
        let k = f32::from_bits(0x3df1a9fc);
        self.sd_ff = [k, g * f32::from_bits(0x404ccccd), k, g * f32::from_bits(0x404ccccd)];
        self.sd_in = [1.0, g, 1.0, g];
        self.change_response();
    }

    /// Response change [re:33b0c4].
    fn change_response(&mut self) {
        self.damp[0].sr = self.sr;
        if self.damping >= 17999.0 {
            self.damp[0].c = [1.0, 0.0, 0.0];
        } else {
            let w = self.damping * 6.283;
            self.damp[0].prewarp(0.0, w, 1.0, w);
        }
        self.damp[1].sr = self.damp[0].sr;
        self.damp[1].c = self.damp[0].c;
        let t = ((self.time_mult as f64).log10() * 0.699999988079071 + self.time as f64) as f32;
        let tc = if t < 0.0 {
            0.0
        } else if t <= 1.0 {
            t
        } else {
            1.0
        };
        let curve = |x: f32| {
            let x = if x < 0.0 {
                0.0
            } else if x > 1.0 {
                1.0
            } else {
                x as f64
            };
            x.powf(0.4f32 as f64)
        };
        let dlo = curve((self.low - 0.5) + tc);
        let dhi = curve((self.high - 0.5) + tc);
        let sr = self.sr;
        let f_hi = (self.high_freq * 3.1415927 / sr).tan();
        let f_lo = (self.low_freq * 3.1415927 / sr).tan();
        let sr_pi = sr / 3.1415927;
        let n = self.shelf_norm;
        self.shelf[0].sr = sr;
        self.shelf[0].set_shelving(f_lo * sr_pi, (dlo * 7.9f32 as f64 + 1.8f32 as f64) as f32 / n, f_hi * sr_pi, (dhi * 7.9f32 as f64 + 1.8f32 as f64) as f32 / n);
        self.shelf[1].sr = self.shelf[0].sr;
        self.shelf[1].c = self.shelf[0].c;
    }

    fn set_sync_mult(&mut self, v: f32) {
        let m = [0.125, 0.1875, 0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0];
        if let Some(&x) = m.get(v as i32 as usize) {
            self.sync_mult = x;
        }
    }

    fn tap_line(&self, i: usize) -> &DelayItem {
        match i {
            6 => &self.lfo[0].d,
            7 => &self.ap[1],
            _ => &self.dl[i],
        }
    }

    fn taps_into(&self, out: &mut [Block; 2], range: std::ops::Range<usize>) {
        for &(i, len, g, right) in &self.taps[range] {
            self.tap_line(i).pick_add(&mut out[right as usize], len, g);
        }
    }

    /// Stage head: `a = sd(x)·in_gain + sd[tap]·ff`.
    fn stage_head(&mut self, k: usize, from_f: bool) {
        let x = if from_f { &self.f } else { &self.d };
        self.sd[k].basic(&mut self.a, x);
        gain(&mut self.a, self.sd_in[k]);
        self.sd[k].pick_add(&mut self.a, self.sd_tap[k], self.sd_ff[k]);
    }
}

impl Algo for Hall {
    fn params(&self) -> &'static [Param] {
        PARAMS
    }

    fn set_val(&mut self, idx: usize, pos: f32, val: f32) {
        match idx {
            0 => {
                self.time = pos;
                self.change_feedback();
            }
            1 => {
                if pos != self.size {
                    self.size = pos;
                    self.fade_state = 1;
                }
            }
            2 => {
                self.diffusion = pos;
                self.change_diffusion();
            }
            3 => {
                self.shape = pos;
                self.change_sizes();
            }
            4 => {
                self.high = pos;
                self.change_response();
            }
            5 => self.wet_t = pos,
            6 => {
                self.spread = pos;
                self.change_sizes();
            }
            7 => {
                self.predelay = val;
                self.change_predelay();
            }
            8 => self.pre[0].gain = pos * 0.85,
            9 => self.pre[1].gain = pos * 0.85,
            10 => {
                self.damping = val;
                self.change_response();
            }
            11 => {
                self.spin = pos;
                self.change_spin();
            }
            12 => {
                self.wander = pos * 0.85 + 0.15;
                self.change_spin();
            }
            13 => {
                self.low = pos;
                self.change_response();
            }
            14 => {
                self.low_freq = val;
                self.change_response();
            }
            15 => {
                self.high_freq = val;
                self.change_response();
            }
            16 => {
                self.low_cut_freq = val;
                self.change_low_cut();
            }
            17 => self.dry_t = pos,
            18 => {
                self.sync = val as i32 as i64;
                self.change_predelay();
            }
            19 => {
                self.set_sync_mult(val);
                self.change_predelay();
            }
            _ => {}
        }
    }

    fn change_wave_info(&mut self, sr: f32, n_in: i64, _n_out: i64) {
        self.n_in = n_in;
        if (self.sr - sr).abs() > 1.0 {
            self.sr = sr;
            self.lfo[0].change_freq(sr);
            self.lfo[1].change_freq(sr);
            self.change_diffusion();
            self.change_sizes();
            self.change_response();
            self.change_predelay();
            self.change_spin();
            self.change_low_cut();
            self.fade_step = 32.0 / (sr * 0.1);
        }
    }

    /// Reset [re:1e88b5].
    fn reset(&mut self) {
        self.e = [0.0; B];
        for d in self.pre.iter_mut().chain(&mut self.ap_in).chain(&mut self.sd).chain(&mut self.ap).chain(&mut self.dl) {
            d.reset();
        }
        self.lfo[0].reset(0.0);
        self.lfo[1].reset(0.0);
        for b in self.damp.iter_mut().chain(&mut self.low_cut) {
            b.clear();
        }
        for s in &mut self.shelf {
            s.clear();
        }
    }

    fn process(&mut self, out: &mut [Block; 2], inp: &[Block; 2]) {
        match self.fade_state {
            1 => {
                self.fade -= self.fade_step;
                if self.fade < 0.0 {
                    self.fade = 0.0;
                    self.change_sizes();
                    self.reset();
                    self.fade_state = 2;
                }
            }
            2 => {
                self.fade += self.fade_step;
                if self.fade > 1.0 {
                    self.fade = 1.0;
                    self.fade_state = 0;
                }
            }
            _ => {}
        }
        // Input diffusion per channel.
        mul(&mut self.a, &inp[0], self.fade);
        self.pre[0].comb(&mut self.d, &self.a);
        self.ap_in[0].allpass(&mut self.a, &self.d);
        self.ap_in[1].allpass(&mut self.d, &self.a);
        let xr = if self.n_in < 2 { &inp[0] } else { &inp[1] };
        mul(&mut self.a, xr, self.fade);
        self.pre[1].comb(&mut self.f, &self.a);
        self.ap_in[2].allpass(&mut self.a, &self.f);
        self.ap_in[3].allpass(&mut self.f, &self.a);

        let [ol, or] = &mut *out;
        // Stage 1.
        self.stage_head(0, false);
        sub(&mut self.a, &self.e);
        self.ap[0].allpass(&mut self.b, &self.a);
        self.dl[0].basic(&mut self.e, &self.b);
        let t = &self.taps;
        self.dl[0].pick_gain(ol, t[0].1, t[0].2);
        self.dl[0].pick_gain(or, t[1].1, t[1].2);
        self.dl[0].pick_add(or, t[2].1, t[2].2);
        // Stage 2.
        self.stage_head(1, false);
        self.shelf[0].run(&mut self.e);
        mix_mul(&mut self.a, &self.e, self.shelf_norm);
        self.damp[0].run(&mut self.a);
        self.lfo[0].allpass(&mut self.b, &self.a);
        self.dl[1].basic(&mut self.e, &self.b);
        self.taps_into(out, 3..8);
        gain(&mut self.e, self.fb);
        mix(&mut self.e, &self.noise);
        self.ap[2].allpass(&mut self.b, &self.e);
        self.dl[4].basic(&mut self.e, &self.b);
        self.taps_into(out, 8..12);
        // Stage 3.
        self.stage_head(2, true);
        sub(&mut self.a, &self.e);
        self.ap[1].allpass(&mut self.b, &self.a);
        self.dl[2].basic(&mut self.e, &self.b);
        self.taps_into(out, 12..17);
        // Stage 4.
        self.stage_head(3, true);
        self.shelf[1].run(&mut self.e);
        mix_mul(&mut self.a, &self.e, self.shelf_norm);
        self.damp[1].run(&mut self.a);
        self.lfo[1].allpass(&mut self.b, &self.a);
        self.dl[3].basic(&mut self.e, &self.b);
        self.taps_into(out, 17..21);
        gain(&mut self.e, self.fb);
        mix(&mut self.e, &self.noise);
        self.ap[3].allpass(&mut self.b, &self.e);
        self.dl[5].basic(&mut self.e, &self.b);
        self.taps_into(out, 21..25);

        let w = smooth(&mut self.wet, self.wet_t) * 0.5 * self.fade;
        let dry = smooth(&mut self.dry, self.dry_t);
        let [ol, or] = out;
        self.low_cut[0].run(ol);
        gain(ol, w);
        mix_mul(ol, &inp[0], dry);
        self.low_cut[1].run(or);
        gain(or, w);
        mix_mul(or, if self.n_in < 2 { &inp[0] } else { &inp[1] }, dry);
    }
}
