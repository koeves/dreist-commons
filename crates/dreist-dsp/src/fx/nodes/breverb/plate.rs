//! [re:252d9d] (ctor [re:3fd3ae], ChangeWaveInfo [re:32a8a5], SetParameterVal
//! [re:485cda], ChangeDelays [re:00fbb4], ChangeFilter [re:4c8b34], Process
//! [re:7e05cb]): a figure-of-eight plate tank (mono input → predelay comb →
//! high-pass → four input diffusers → two cross-fed halves, each an
//! LFO-swept allpass, delay, damping, allpass, delay and shelving EQ),
//! read by seven output taps per side.

use super::blocks::*;
use super::{Algo, Param};

/// `_PlateReverbParam` [re:587f7d].
pub const PARAMS: &[Param] = &[
    Param(1, 0.5, 20.0),       // 0 Time
    Param(0, 0.0, 100.0),      // 1 Size
    Param(0, 0.0, 100.0),      // 2 Diffusion
    Param(0, 0.0, 100.0),      // 3 Shape
    Param(1, 0.25, 4.0),       // 4 High (decay multiplier)
    Param(0, 0.0, 100.0),      // 5 Wet
    Param(1, 0.25, 4.0),       // 6 Low (decay multiplier)
    Param(1, 30.0, 960.0),     // 7 Low Freq
    Param(1, 1100.0, 18000.0), // 8 High Freq
    Param(0, 0.0, 100.0),      // 9 Motion
    Param(1, 5000.0, 18000.0), // 10 RtHC
    Param(0, 0.0, 100.0),      // 11 Dry
    Param(0, 0.0, 0.5),        // 12 Predelay
    Param(0, 0.0, 100.0),      // 13 Regen
    Param(2, 0.0, 1.0),        // 14 Predelay Sync
    Param(2, 0.0, 8.0),        // 15 Predelay BPM
];

/// Output taps (`ChangeDelays`, scaled by size·sr/29800): left 0x398..0x3c8,
/// right 0x3d0..0x400.
const TAPS: [f32; 14] = [266.0, 2974.0, 1913.0, 1996.0, 1990.0, 187.0, 1066.0, 353.0, 3627.0, 1228.0, 2673.0, 2111.0, 335.0, 121.0];

pub struct Plate {
    sr: f32,
    n_in: i64,
    n_out: i64,
    pre: DelayItem,
    /// Input diffusers 0x430/0x478/0x4c0/0x508.
    ap_in: [DelayItem; 4],
    /// Tank allpasses 0x550 (left) / 0x598 (right).
    ap_tank: [DelayItem; 2],
    /// LFO allpasses 0x5e0 (left) / 0x680 (right).
    ap_lfo: [DelayItemLfo; 2],
    /// Tank delays 0x720, 0x768 (left), 0x7b0, 0x7f8 (right).
    dl: [DelayItem; 4],
    hpf: Bilin,
    shelf: [Biquad; 2],
    rthc: [Bilin; 2],
    taps: [i64; 14],
    hpf_freq: f32,
    size: f32,
    density: f32,
    motion: f32,
    predelay: f32,
    time: f32,
    low_freq: f32,
    low_mult: f32,
    high_freq: f32,
    high_mult: f32,
    rthc_freq: f32,
    sync: i64,
    sync_mult: f32,
    bpm: f32,
    fb: f32,
    wet: f32,
    wet_t: f32,
    dry: f32,
    dry_t: f32,
    shape: f32,
    shape_t: f32,
    fade_state: i32,
    fade: f32,
    fade_step: f32,
    time_mult: f32,
    a: Block,
    b: Block,
    d: Block,
    /// Shelved tank outputs fed back one block later (0x280 left, 0x318 right).
    fl: Block,
    fr: Block,
}

impl Plate {
    pub fn new() -> Plate {
        let mut pre = DelayItem::new(0x10000);
        pre.gain = 0.0;
        let mut p = Plate {
            sr: 0.0,
            n_in: 2,
            n_out: 2,
            pre,
            ap_in: std::array::from_fn(|_| DelayItem::new(0x4000)),
            ap_tank: [DelayItem::new(0x4000), DelayItem::new(0x4000)],
            ap_lfo: [DelayItemLfo::new(0x4000), DelayItemLfo::new(0x4000)],
            dl: std::array::from_fn(|_| DelayItem::new(0x4000)),
            hpf: Bilin::default(),
            shelf: [Biquad::default(); 2],
            rthc: [Bilin::default(); 2],
            taps: [0; 14],
            hpf_freq: 80.0,
            size: 0.8,
            density: 0.5,
            motion: 0.5,
            predelay: 0.0,
            time: 1.5,
            low_freq: 200.0,
            low_mult: 0.9,
            high_freq: 5000.0,
            high_mult: 0.4,
            rthc_freq: 18000.0,
            sync: 0,
            sync_mult: 1.0,
            bpm: 120.0,
            fb: 0.0,
            wet: 1.0,
            wet_t: 1.0,
            dry: 1.0,
            dry_t: 1.0,
            shape: 0.5,
            shape_t: 0.5,
            fade_state: 0,
            fade: 1.0,
            fade_step: 0.0,
            time_mult: 1.0,
            a: [0.0; B],
            b: [0.0; B],
            d: [0.0; B],
            fl: [0.0; B],
            fr: [0.0; B],
        };
        p.change_wave_info(44100.0, 2, 2);
        p.fade = 1.0;
        p.fade_state = 0;
        p
    }

    /// `ChangeDensity` [re:256fd6].
    fn change_density(&mut self) {
        self.ap_lfo[0].d.gain = 0.7;
        self.ap_lfo[1].d.gain = 0.7;
        self.ap_tank[0].gain = -0.5;
        self.ap_tank[1].gain = -0.5;
        let d = self.density * 0.8 + 0.2;
        self.ap_in[3].gain = d * -0.625;
        self.ap_in[2].gain = d * -0.625;
        self.ap_in[0].gain = d * -0.75;
        self.ap_in[1].gain = d * -0.75;
    }

    /// `ChangeSpin` [re:f1421f].
    fn change_spin(&mut self) {
        let m = self.motion * 0.7 + 0.3;
        self.ap_lfo[0].set_lfo((m * 16.0) as i64, m * 1.2);
        self.ap_lfo[1].set_lfo((m * 20.0) as i64, m);
    }

    fn change_predelay(&mut self) {
        let s = if self.sync == 0 { self.predelay } else { (self.sync_mult * 60.0) / self.bpm };
        self.pre.set_length(predelay_len(s, self.sr));
    }

    /// `ChangeDelays` [re:00fbb4].
    fn change_delays(&mut self) {
        let f = self.size * self.sr;
        let len = |n: f32| ((f * n) / 29800.0) as i64;
        for (d, n) in self.ap_in.iter_mut().zip([142.0, 107.0, 379.0, 277.0]) {
            d.set_length(len(n));
        }
        self.ap_lfo[0].d.set_length(len(908.0));
        self.ap_lfo[1].d.set_length(len(670.0));
        self.change_spin();
        for (d, n) in self.dl.iter_mut().zip([4217.0, 3163.0, 4453.0, 3720.0]) {
            d.set_length(len(n));
        }
        self.ap_tank[0].set_length(len(2656.0));
        self.ap_tank[1].set_length(len(1800.0));
        self.change_filter();
        for (t, &n) in self.taps.iter_mut().zip(&TAPS) {
            *t = len(n);
        }
    }

    /// `ChangeFilter` [re:4c8b34]: loop gain for −60 dB over the (size
    /// normalised) decay time, low/high shelves for the frequency-dependent
    /// decay multipliers, input high-pass and the RtHC damping.
    fn change_filter(&mut self) {
        let t = self.time_mult * self.time;
        let t = (if t <= 20.0 { t * 0.8 } else { 16.0 }) / self.size;
        let k = 0.001f32 as f64;
        let fb = k.powf((0.3 / t) as f64) as f32;
        self.fb = fb;
        let fb = fb as f64;
        let sr = self.sr;
        let sr_pi = sr / 3.1415927;
        let g_hi = (k.powf((0.3 / (t * self.high_mult)) as f64) / fb) as f32;
        let f_hi = (self.high_freq * 3.1415927 / sr).tan() * sr_pi;
        let g_lo = (k.powf((0.3 / (t * self.low_mult)) as f64) / fb) as f32;
        let f_lo = (3.1415927 * self.low_freq / sr).tan() * sr_pi;
        self.shelf[0].sr = sr;
        self.shelf[0].set_shelving(f_lo, g_lo, f_hi, g_hi);
        self.shelf[1].sr = self.shelf[0].sr;
        self.shelf[1].c = self.shelf[0].c;
        self.hpf.sr = sr;
        self.hpf.set_hpf(self.hpf_freq);
        self.rthc[0].sr = sr;
        if self.rthc_freq >= 17999.0 {
            self.rthc[0].c = [1.0, 0.0, 0.0];
        } else {
            let w = self.rthc_freq * 6.283;
            self.rthc[0].prewarp(0.0, w, 1.0, w);
        }
        self.rthc[1].sr = self.rthc[0].sr;
        self.rthc[1].c = self.rthc[0].c;
    }

    fn set_sync_mult(&mut self, v: f32) {
        let m = [0.125, 0.1875, 0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0];
        if let Some(&x) = m.get(v as i32 as usize) {
            self.sync_mult = x;
        }
    }
}

impl Algo for Plate {
    fn params(&self) -> &'static [Param] {
        PARAMS
    }

    fn set_val(&mut self, idx: usize, pos: f32, val: f32) {
        match idx {
            0 => {
                self.time = val;
                self.change_filter();
            }
            1 => {
                let s = pos * 0.77 + 0.36;
                if s != self.size {
                    self.size = s;
                    self.fade_state = 1;
                }
            }
            2 => {
                self.density = pos;
                self.change_density();
            }
            3 => self.shape_t = pos,
            4 => {
                self.high_mult = val;
                self.change_filter();
            }
            5 => self.wet_t = pos,
            6 => {
                self.low_mult = val;
                self.change_filter();
            }
            7 => {
                self.low_freq = val;
                self.change_filter();
            }
            8 => {
                self.high_freq = val;
                self.change_filter();
            }
            9 => {
                self.motion = pos;
                self.change_spin();
            }
            10 => {
                self.rthc_freq = val;
                self.change_filter();
            }
            11 => self.dry_t = pos,
            12 => {
                self.predelay = val;
                self.change_predelay();
            }
            13 => self.pre.gain = pos * 0.85,
            14 => {
                self.sync = val as i32 as i64;
                self.change_predelay();
            }
            15 => {
                self.set_sync_mult(val);
                self.change_predelay();
            }
            _ => {}
        }
    }

    fn change_wave_info(&mut self, sr: f32, n_in: i64, n_out: i64) {
        if (self.sr - sr).abs() > 1.0 {
            self.sr = sr;
            for l in &mut self.ap_lfo {
                l.change_freq(sr);
            }
            self.change_delays();
            self.change_filter();
            self.change_density();
            self.change_spin();
            self.change_predelay();
            self.fade_step = 32.0 / (self.sr * 0.05);
        }
        self.n_in = n_in;
        self.n_out = n_out;
    }

    /// `Reset` [re:67bb1b].
    fn reset(&mut self) {
        self.fl = [0.0; B];
        self.fr = [0.0; B];
        for d in self.ap_in.iter_mut().chain(&mut self.dl).chain(&mut self.ap_tank) {
            d.reset();
        }
        for s in &mut self.shelf {
            s.clear();
        }
        for b in &mut self.rthc {
            b.clear();
        }
        self.pre.reset();
        self.hpf.clear();
        self.ap_lfo[0].reset(0.0);
        self.ap_lfo[1].reset(0.25);
    }

    fn process(&mut self, out: &mut [Block; 2], inp: &[Block; 2]) {
        match self.fade_state {
            1 => {
                self.fade -= self.fade_step;
                if self.fade < 0.0 {
                    self.fade = 0.0;
                    self.change_delays();
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
        let (a, b, d) = (&mut self.a, &mut self.b, &mut self.d);
        if self.n_in < 2 {
            mul(b, &inp[0], self.fade);
        } else {
            add(b, &inp[0], &inp[1]);
            gain(b, self.fade * 0.5);
        }
        self.pre.comb(a, b);
        self.hpf.run(a);
        self.ap_in[0].allpass(b, a);
        self.ap_in[1].allpass(a, b);
        self.ap_in[2].allpass(b, a);
        self.ap_in[3].allpass(a, b);
        // Left half.
        mul_add(b, a, &self.fr, self.fb);
        self.ap_lfo[0].allpass(d, b);
        self.dl[0].basic(b, d);
        self.rthc[0].run(b);
        self.ap_tank[0].allpass(d, b);
        self.dl[1].basic(b, d);
        // Right half, fed by the previous block's left output.
        mul_add(d, a, &self.fl, self.fb);
        self.shelf[0].run2(&mut self.fl, b);
        self.ap_lfo[1].allpass(b, d);
        self.dl[2].basic(d, b);
        self.rthc[1].run(d);
        self.ap_tank[1].allpass(b, d);
        self.dl[3].basic(d, b);
        self.shelf[1].run2(&mut self.fr, d);

        let s = smooth(&mut self.shape, self.shape_t);
        let [ol, or] = out;
        let t = &self.taps;
        self.dl[0].pick(ol, t[0]);
        self.dl[0].pick(a, t[1]);
        mix(ol, a);
        self.ap_tank[0].pick(a, t[2]);
        sub(ol, a);
        self.dl[1].pick(a, t[3]);
        gain(a, s);
        mix(ol, a);
        self.dl[2].pick(a, t[4]);
        sub(ol, a);
        self.ap_tank[1].pick(a, t[5]);
        gain(a, s);
        sub(ol, a);
        self.dl[3].pick(a, t[6]);
        gain(a, s);
        sub(ol, a);
        let w = smooth(&mut self.wet, self.wet_t) * 0.5 * self.fade;
        let dry = smooth(&mut self.dry, self.dry_t);
        gain(ol, w);
        mix_mul(ol, &inp[0], dry);
        if self.n_out > 1 {
            self.dl[2].pick(or, t[7]);
            self.dl[2].pick(a, t[8]);
            mix(or, a);
            self.ap_tank[1].pick(a, t[9]);
            gain(a, s);
            sub(or, a);
            self.dl[3].pick(a, t[10]);
            gain(a, s);
            mix(or, a);
            self.dl[0].pick(a, t[11]);
            sub(or, a);
            self.ap_tank[0].pick(a, t[12]);
            gain(a, s);
            sub(or, a);
            self.dl[1].pick(a, t[13]);
            sub(or, a);
            gain(or, w);
            mix_mul(or, &inp[if self.n_in > 1 { 1 } else { 0 }], dry);
        }
    }
}
