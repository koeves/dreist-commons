//! [re:de3bd1] (ctor [re:f49597], ChangeWaveInfo [re:944e2a],
//! SetParameterVal [re:0999a3], ChangeSizes [re:f3c19a], Process [re:8d0bb5]): the
//! early multi-tap network followed by a band-limited second tap stage
//! whose gains rise towards the end (reverse envelope).

use super::blocks::*;
use super::early::{Core, Taps};
use super::{Algo, Param};

/// `_InverseReverbParam` [re:c1122a].
pub const PARAMS: &[Param] = &[
    Param(1, 0.1, 1.5),       // 0 Time (s); used as a size position
    Param(0, 0.0, 100.0),     // 1 Diffusion
    Param(1, 720.0, 18000.0), // 2 High
    Param(0, 0.0, 100.0),     // 3 Wet
    Param(0, 0.0, 0.5),       // 4 Predelay
    Param(1, 30.0, 720.0),    // 5 Low
    Param(0, 0.0, 100.0),     // 6 Spin
    Param(0, 0.0, 100.0),     // 7 Wander
    Param(0, 0.0, 100.0),     // 8 Dry
    Param(2, 0.0, 1.0),       // 9 Predelay Sync
    Param(2, 0.0, 8.0),       // 10 Predelay BPM
    Param(0, -1.0, 1.0),      // 11 Mix Offset
];

// Tap tables ([re:41ab1b]..[re:314a39]).
const MAIN_LL_POS: [i32; 10] = [601, 709, 901, 1193, 1783, 2113, 4024, 4399, 5329, 5449];
const MAIN_LL_AMP: [f32; 10] = [0.239044189, -0.239257812, 0.318725586, -0.239257812, 0.318725586, 0.397125244, -0.292480469, -0.432525635, 0.611450195, 0.589172363];
const MAIN_LR_POS: [i32; 10] = [2053, 2339, 2873, 3187, 3289, 3433, 3609, 4489, 4904, 5054];
const MAIN_LR_AMP: [f32; 10] = [-0.14932251, 0.297576904, 0.297576904, 0.397125244, -0.297790527, 0.397125244, 0.400360107, -0.361297607, -0.471954346, 0.47088623];
const MAIN_RL_POS: [i32; 10] = [121, 1991, 2257, 2693, 2971, 3067, 3355, 4215, 4710, 4740];
const MAIN_RL_AMP: [f32; 10] = [0.149108887, 0.149108887, -0.297790527, -0.223236084, 0.397125244, 0.297576904, -0.39364624, 0.277099609, 0.399353027, -0.45526123];
const MAIN_RR_POS: [i32; 9] = [811, 1003, 1219, 1321, 1903, 2245, 4565, 5460, 5510];
const MAIN_RR_AMP: [f32; 9] = [-0.239257812, -0.239257812, 0.239044189, -0.318939209, 0.318725586, -0.394775391, 0.332794189, -0.637756348, 0.647155762];
const SEC_LL_POS: [i32; 9] = [1100, 2674, 4108, 6238, 8078, 9928, 11384, 13137, 15382];
const SEC_LL_AMP: [f32; 9] = [0.109999999, 0.109999999, 0.150000006, 0.180000007, 0.219999999, 0.25999999, 0.333000004, 0.444400012, 1.0];
const SEC_RL_POS: [i32; 8] = [2612, 3438, 5258, 6908, 8990, 10298, 12379, 15382];
const SEC_RL_AMP: [f32; 8] = [0.119999997, 0.150000006, 0.180000007, 0.219999999, 0.25999999, 0.333000004, 0.444400012, 1.0];
const SEC_LR_POS: [i32; 9] = [548, 2704, 4636, 6124, 7654, 9358, 11260, 13137, 15382];
const SEC_LR_AMP: [f32; 9] = SEC_LL_AMP;
const SEC_RR_POS: [i32; 8] = [2540, 4174, 6034, 7700, 9566, 11428, 13137, 15382];
const SEC_RR_AMP: [f32; 8] = SEC_RL_AMP;

pub struct Inverse {
    c: Core,
    second: [DelayItem; 2],
    sec: [Taps; 4],
    band: [Biquad; 2],
    low: f32,
    high: f32,
}

impl Inverse {
    pub fn new() -> Inverse {
        let taps = [Taps::new(&MAIN_LL_POS, &MAIN_LL_AMP), Taps::new(&MAIN_LR_POS, &MAIN_LR_AMP), Taps::new(&MAIN_RL_POS, &MAIN_RL_AMP), Taps::new(&MAIN_RR_POS, &MAIN_RR_AMP)];
        let mut r = Inverse {
            c: Core::new(taps, 0x4000),
            second: [DelayItem::new(0x8000), DelayItem::new(0x8000)],
            sec: [Taps::new(&SEC_LL_POS, &SEC_LL_AMP), Taps::new(&SEC_RL_POS, &SEC_RL_AMP), Taps::new(&SEC_LR_POS, &SEC_LR_AMP), Taps::new(&SEC_RR_POS, &SEC_RR_AMP)],
            band: [Biquad::default(); 2],
            low: 50.0,
            high: 8000.0,
        };
        r.c.init_lfos();
        r.change_wave_info(44100.0, 2, 2);
        r.c.fade_state = 0;
        r.c.fade = 1.0;
        r
    }

    /// `ChangeInverseResponse` [re:560f1b].
    fn change_response(&mut self) {
        self.band[0].sr = self.c.sr;
        self.band[0].set_band(self.low, self.high);
        self.band[1].sr = self.band[0].sr;
        self.band[1].c = self.band[0].c;
    }

    fn change_sizes(&mut self) {
        let f = self.c.size_factor();
        self.c.change_sizes(f, 0);
        for t in &mut self.sec {
            t.scale(f, 0);
        }
        for d in &mut self.second {
            d.set_length((f * 16000.0) as i32 as i64);
        }
    }
}

impl Algo for Inverse {
    fn params(&self) -> &'static [Param] {
        PARAMS
    }

    fn set_val(&mut self, idx: usize, pos: f32, val: f32) {
        let c = &mut self.c;
        match idx {
            0 => {
                if pos != c.size {
                    c.size = pos;
                    c.fade_state = 1;
                }
            }
            1 => {
                c.diffusion = pos;
                c.change_diffusion();
            }
            2 => {
                self.high = val;
                self.change_response();
            }
            3 => c.wet_t = pos,
            4 => {
                c.predelay = val;
                c.change_predelay();
            }
            5 => {
                self.low = val;
                self.change_response();
            }
            6 => {
                c.spin = pos;
                c.change_spin();
            }
            7 => {
                c.wander = pos * 0.85 + 0.15;
                c.change_spin();
            }
            8 => c.dry_t = pos,
            9 => {
                c.sync = val as i32 as i64;
                c.change_predelay();
            }
            10 => {
                c.set_sync_mult(val);
                c.change_predelay();
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

    /// `Reset` [re:3707a3].
    fn reset(&mut self) {
        self.c.reset();
        for d in &mut self.second {
            d.reset();
        }
        for b in &mut self.band {
            b.clear();
        }
    }

    fn process(&mut self, out: &mut [Block; 2], inp: &[Block; 2]) {
        if self.c.fade_tick() {
            self.change_sizes();
            self.reset();
        }
        let [ol, or] = out;
        self.c.input_chain(0, &inp[0], None);
        let l1 = self.c.lfo[0].value();
        let l2 = self.c.lfo[1].value();
        self.c.taps_left(ol, or, l1, l2);
        let xr = if self.c.n_in < 2 { &inp[0] } else { &inp[1] };
        self.c.input_chain(1, xr, None);
        self.c.taps_right(ol, or, l1, l2);

        let (a, b) = (&mut self.c.a, &mut self.c.b);
        self.band[0].run2(a, ol);
        self.second[0].basic(b, a);
        self.band[1].run2(a, or);
        self.second[1].basic(b, a);
        let [ll, rl, lr, rr] = &self.sec;
        // Pick at tap 8 (gain 1) and the loop over taps 1..=8 both read tap
        // 8, so it sounds at gain 2; tap 0 is unused.
        self.second[0].pick_gain(ol, ll.pos[8], 1.0);
        for k in 1..=8 {
            self.second[0].pick_add(ol, ll.pos[k], ll.amp[k]);
        }
        for k in 0..8 {
            self.second[1].pick_add(ol, rl.pos[k], rl.amp[k]);
        }
        self.second[0].pick_gain(or, lr.pos[8], 1.0);
        for k in 1..=8 {
            self.second[0].pick_add(or, lr.pos[k], lr.amp[k]);
        }
        for k in 0..8 {
            self.second[1].pick_add(or, rr.pos[k], rr.amp[k]);
        }
        self.c.mix_out(out, inp, 0.4);
    }
}
