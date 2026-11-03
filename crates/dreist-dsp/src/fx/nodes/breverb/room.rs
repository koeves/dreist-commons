//! [re:9f5917] (ctor [re:b6e43d], ChangeWaveInfo [re:ed44da], SetParameterVal
//! [re:28c7be], ChangeSizes [re:c8ed8e], Process [re:8f5d48]): the early tap network
//! (band-limited inputs) feeding two cross-coupled damped feedback delays
//! with a multi-tap late section.

use super::blocks::*;
use super::early::{Core, Taps};
use super::{Algo, Param};

/// `_RoomReverbParam` [re:51ffca].
pub const PARAMS: &[Param] = &[
    Param(1, 0.2, 4.5),        // 0 Time
    Param(0, 0.0, 100.0),      // 1 Size
    Param(0, 0.0, 100.0),      // 2 Diffusion
    Param(0, 0.0, 100.0),      // 3 Decay
    Param(1, 720.0, 18000.0),  // 4 High
    Param(0, 0.0, 100.0),      // 5 Wet
    Param(0, 0.0, 0.5),        // 6 Predelay
    Param(1, 1500.0, 18000.0), // 7 Damping
    Param(1, 30.0, 720.0),     // 8 Low
    Param(0, 0.0, 100.0),      // 9 Spin
    Param(0, 0.0, 100.0),      // 10 Wander
    Param(0, 0.0, 100.0),      // 11 Dry
    Param(2, 0.0, 1.0),        // 12 Predelay Sync
    Param(2, 0.0, 8.0),        // 13 Predelay BPM
];

// Tap tables ([re:54edee]..[re:4a52e2]).
const MAIN_LL_POS: [i32; 10] = [62, 182, 1112, 1487, 3398, 3728, 4318, 4610, 4802, 4910];
const MAIN_LL_AMP: [f32; 10] = [0.589172363, 0.611450195, -0.432525635, -0.292480469, 0.397125244, 0.318725586, -0.239257812, 0.318725586, -0.239257812, 0.239044189];
const MAIN_LR_POS: [i32; 10] = [457, 607, 1022, 1902, 2078, 2222, 2324, 2638, 3172, 3458];
const MAIN_LR_AMP: [f32; 10] = [0.47088623, -0.471954346, -0.361297607, 0.400360107, 0.397125244, -0.297790527, 0.397125244, 0.297576904, 0.297576904, -0.14932251];
const MAIN_RL_POS: [i32; 10] = [771, 801, 1296, 2156, 2444, 2540, 2818, 3254, 3520, 5390];
const MAIN_RL_AMP: [f32; 10] = [-0.45526123, 0.399353027, 0.277099609, -0.39364624, 0.297576904, 0.397125244, -0.223236084, -0.297790527, 0.149108887, 0.149108887];
const MAIN_RR_POS: [i32; 9] = [1, 51, 946, 3266, 3608, 4190, 4292, 4508, 4700];
const MAIN_RR_AMP: [f32; 9] = [0.647155762, -0.637756348, 0.332794189, -0.394775391, 0.318725586, -0.318939209, 0.239044189, -0.239257812, -0.239257812];
const SEC_LL_POS: [i32; 8] = [3863, 5616, 7072, 8922, 10762, 12892, 14326, 15900];
const SEC_LL_AMP: [f32; 8] = [0.444400012, 0.333000004, 0.25999999, 0.219999999, 0.180000007, 0.150000006, 0.109999999, 0.109999999];
const SEC_RL_POS: [i32; 7] = [4621, 6702, 8010, 10092, 11742, 13562, 14388];
const SEC_RL_AMP: [f32; 7] = [0.444400012, 0.36500001, 0.284000009, 0.201000005, 0.159999996, 0.120999999, 0.120999999];
const SEC_LR_POS: [i32; 8] = [3863, 5740, 7642, 9346, 10876, 12364, 14296, 16452];
const SEC_LR_AMP: [f32; 8] = [0.444000006, 0.333000004, 0.25999999, 0.219999999, 0.150000006, 0.109999999, 0.109999999, 0.109999999];
const SEC_RR_POS: [i32; 7] = [3863, 5572, 7434, 9300, 10966, 12826, 14460];
const SEC_RR_AMP: [f32; 7] = [0.444000006, 0.36500001, 0.284000009, 0.201000005, 0.159999996, 0.120999999, 0.120999999];

pub struct Room {
    c: Core,
    second: [DelayItem; 2],
    sec: [Taps; 4],
    band: [Biquad; 2],
    damp: [Bilin; 2],
    loop_len: i64,
    fb: f32,
    time: f32,
    time_mult: f32,
    decay_t: f32,
    decay: f32,
    damping: f32,
    low: f32,
    high: f32,
    blk_c: Block,
    blk_d: Block,
}

impl Room {
    pub fn new() -> Room {
        let taps = [Taps::new(&MAIN_LL_POS, &MAIN_LL_AMP), Taps::new(&MAIN_LR_POS, &MAIN_LR_AMP), Taps::new(&MAIN_RL_POS, &MAIN_RL_AMP), Taps::new(&MAIN_RR_POS, &MAIN_RR_AMP)];
        let mut r = Room {
            c: Core::new(taps, 0x10000),
            second: [DelayItem::new(0x8000), DelayItem::new(0x8000)],
            sec: [Taps::new(&SEC_LL_POS, &SEC_LL_AMP), Taps::new(&SEC_RL_POS, &SEC_RL_AMP), Taps::new(&SEC_LR_POS, &SEC_LR_AMP), Taps::new(&SEC_RR_POS, &SEC_RR_AMP)],
            band: [Biquad::default(); 2],
            damp: [Bilin::default(); 2],
            loop_len: 0,
            fb: 0.0,
            time: 1.0,
            time_mult: 1.0,
            decay_t: 1.0,
            decay: 1.0,
            damping: 4200.0,
            low: 50.0,
            high: 8000.0,
            blk_c: [0.0; B],
            blk_d: [0.0; B],
        };
        r.c.init_lfos();
        r.change_wave_info(44100.0, 2, 2);
        r.c.fade = 1.0;
        r.c.fade_state = 0;
        r
    }

    /// `ChangeRoomResponse` [re:33e494].
    fn change_response(&mut self) {
        self.band[0].sr = self.c.sr;
        self.band[0].set_band(self.low, self.high);
        self.band[1].sr = self.band[0].sr;
        self.band[1].c = self.band[0].c;
        let w = self.damping * 6.283;
        self.damp[0].sr = self.c.sr;
        self.damp[0].prewarp(0.0, w, 1.0, w);
        self.damp[1].sr = self.damp[0].sr;
        self.damp[1].c = self.damp[0].c;
    }

    /// `ChangeRevTime` [re:aab46b]: loop gain for a 60 dB (×0.01) decay over
    /// `min(time, 5)` seconds.
    fn change_rev_time(&mut self) {
        let t = self.time * self.time_mult;
        if t <= 0.0 {
            self.fb = 0.0;
            return;
        }
        let e = ((self.loop_len as f32) / self.c.sr) / if t <= 5.0 { t } else { 5.0 };
        self.fb = (0.01f32 as f64).powf(e as f64) as f32;
    }

    fn change_sizes(&mut self) {
        let f = self.c.size_factor();
        self.c.change_sizes(f, 0x41);
        for t in &mut self.sec {
            t.scale(f, 0);
        }
        for d in &mut self.second {
            d.set_length((f * 16383.0) as i32 as i64);
        }
        self.loop_len = (f * 12000.0) as i32 as i64;
        self.change_rev_time();
    }
}

impl Algo for Room {
    fn params(&self) -> &'static [Param] {
        PARAMS
    }

    fn set_val(&mut self, idx: usize, pos: f32, val: f32) {
        match idx {
            0 => {
                self.time = val;
                self.change_sizes();
            }
            1 => {
                if pos != self.c.size {
                    self.c.size = pos;
                    self.c.fade_state = 1;
                }
            }
            2 => {
                self.c.diffusion = pos;
                self.c.change_diffusion();
            }
            3 => self.decay_t = pos,
            4 => {
                self.high = val;
                self.change_response();
            }
            5 => self.c.wet_t = pos,
            6 => {
                self.c.predelay = val;
                self.c.change_predelay();
            }
            7 => {
                self.damping = val;
                self.change_response();
            }
            8 => {
                self.low = val;
                self.change_response();
            }
            9 => {
                self.c.spin = pos;
                self.c.change_spin();
            }
            10 => {
                self.c.wander = pos;
                self.c.change_spin();
            }
            11 => self.c.dry_t = pos,
            12 => {
                self.c.sync = val as i32 as i64;
                self.c.change_predelay();
            }
            13 => {
                self.c.set_sync_mult(val);
                self.c.change_predelay();
            }
            _ => {}
        }
    }

    fn change_wave_info(&mut self, sr: f32, n_in: i64, _n_out: i64) {
        self.c.n_in = n_in;
        if (self.c.sr - sr).abs() > 1.0 {
            self.c.set_rate(sr);
            self.change_response();
            self.c.change_diffusion();
            self.change_sizes();
            self.c.change_spin();
            self.c.change_predelay();
            self.c.fade_step = 32.0 / (sr * 0.1);
        }
    }

    /// `Reset` [re:37567b].
    fn reset(&mut self) {
        self.blk_d = [0.0; B];
        self.blk_c = [0.0; B];
        self.c.reset();
        for d in &mut self.second {
            d.reset();
        }
        for b in &mut self.band {
            b.clear();
        }
        for b in &mut self.damp {
            b.clear();
        }
    }

    fn process(&mut self, out: &mut [Block; 2], inp: &[Block; 2]) {
        if self.c.fade_tick() {
            self.change_sizes();
            self.reset();
        }
        let [ol, or] = out;
        self.c.input_chain(0, &inp[0], Some(&mut self.band[0]));
        let l1 = self.c.lfo[0].value();
        let l2 = self.c.lfo[1].value();
        self.c.taps_left(ol, or, l1, l2);
        let xr = if self.c.n_in < 2 { &inp[0] } else { &inp[1] };
        self.c.input_chain(1, xr, Some(&mut self.band[1]));
        self.c.taps_right(ol, or, l1, l2);

        // Cross-coupled feedback delays: L's fresh output feeds R in the
        // same block, R's (from the previous block) feeds L.
        let a = &mut self.c.a;
        add(a, ol, &self.blk_c);
        self.damp[0].run(a);
        self.second[0].basic(&mut self.blk_d, a);
        self.second[0].pick_gain(&mut self.blk_d, self.loop_len, self.fb);
        add(a, or, &self.blk_d);
        self.damp[1].run(a);
        self.second[1].basic(&mut self.blk_c, a);
        self.second[1].pick_gain(&mut self.blk_c, self.loop_len, self.fb);

        let dec = smooth(&mut self.decay, self.decay_t);
        let [ll, rl, lr, rr] = &self.sec;
        for k in 0..ll.pos.len() {
            self.second[0].pick_add(ol, ll.pos[k], dec * ll.amp[k]);
        }
        for k in 0..rl.pos.len() {
            self.second[1].pick_add(ol, rl.pos[k], dec * rl.amp[k]);
        }
        for k in 0..lr.pos.len() {
            self.second[0].pick_add(or, lr.pos[k], dec * lr.amp[k]);
        }
        for k in 0..rr.pos.len() {
            self.second[1].pick_add(or, rr.pos[k], dec * rr.amp[k]);
        }
        self.c.mix_out(out, inp, 0.5);
    }
}
