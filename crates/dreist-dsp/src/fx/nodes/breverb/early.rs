//! State shared by [re:de3bd1] and [re:9f5917]: two input chains
//! (predelay → two allpass diffusers → main delay), four multi-tap
//! readers with two LFO-swept taps each, the size-change crossfade and the
//! wet/dry smoothers. Fields follow [re:9a68cc] / [re:6507ff].

use super::blocks::*;

/// Tap set for one input→output path: positions (scaled by size) and
/// gains.
pub struct Taps {
    pub base: &'static [i32],
    pub amp: &'static [f32],
    pub pos: Vec<i64>,
}

impl Taps {
    pub fn new(base: &'static [i32], amp: &'static [f32]) -> Taps {
        Taps { base, amp, pos: vec![0; base.len()] }
    }

    pub fn scale(&mut self, f: f32, offset: i64) {
        for (p, &b) in self.pos.iter_mut().zip(self.base) {
            *p = ((b as f32 * f) as i32) as i64 + offset;
        }
    }
}

pub struct Core {
    pub sr: f32,
    pub n_in: i64,
    /// Allpass diffusers: L1, L2, R1, R2 (`DelayItem(0x4000)`).
    pub ap: [DelayItem; 4],
    /// Main delays ([re:a6ebf7]).
    pub main: [DelayItem; 2],
    /// Predelays ([re:dc3b7d], gain 0).
    pub pre: [DelayItem; 2],
    pub lfo: [Lfo; 2],
    pub depth: i64,
    /// Variable tap positions (16.16).
    pub pav: [i64; 6],
    /// Main taps LL, LR, RL, RR.
    pub taps: [Taps; 4],
    pub size: f32,
    pub diffusion: f32,
    pub predelay: f32,
    pub spin: f32,
    pub wander: f32,
    pub sync: i64,
    pub sync_mult: f32,
    pub bpm: f32,
    pub fade_state: i32,
    pub fade: f32,
    pub fade_step: f32,
    pub wet_t: f32,
    pub wet: f32,
    pub dry_t: f32,
    pub dry: f32,
    pub a: Block,
    pub b: Block,
}

impl Core {
    pub fn new(taps: [Taps; 4], pre_size: i64) -> Core {
        let mut pre = [DelayItem::new(pre_size), DelayItem::new(pre_size)];
        for p in &mut pre {
            p.gain = 0.0;
        }
        Core {
            sr: 0.0,
            n_in: 2,
            ap: std::array::from_fn(|_| DelayItem::new(0x4000)),
            main: [DelayItem::new(0x8000), DelayItem::new(0x8000)],
            pre,
            lfo: [Lfo::new(), Lfo::new()],
            depth: 0,
            pav: [0; 6],
            taps,
            size: 0.5,
            diffusion: 0.7,
            predelay: 0.05,
            spin: 0.2,
            wander: 0.2,
            sync: 0,
            sync_mult: 1.0,
            bpm: 120.0,
            fade_state: 0,
            fade: 1.0,
            fade_step: 0.0,
            wet_t: 1.0,
            wet: 1.0,
            dry_t: 1.0,
            dry: 1.0,
            a: [0.0; B],
            b: [0.0; B],
        }
    }

    /// The two LFO setups (sine, 0.21 / 0.18 Hz, 0.5) at construction.
    pub fn init_lfos(&mut self) {
        self.lfo[0].set(3, f32::from_bits(0x3e570a3d), 0.5);
        self.lfo[1].set(3, f32::from_bits(0x3e3851ec), 0.5);
    }

    /// Size factor of the size change.
    pub fn size_factor(&self) -> f32 {
        ((self.size * 36.0 + 4.0) * (self.sr / 44100.0)) / 20.0
    }

    /// Allpass and main-delay lengths plus main taps (size change).
    pub fn change_sizes(&mut self, f: f32, tap_offset: i64) {
        for (d, n) in self.ap.iter_mut().zip([224.0, 369.0, 194.0, 304.0]) {
            d.set_length((f * n) as i32 as i64);
        }
        for t in &mut self.taps {
            t.scale(f, tap_offset);
        }
        for d in &mut self.main {
            d.set_length((f * 8000.0) as i32 as i64);
        }
    }

    /// Diffusion change.
    pub fn change_diffusion(&mut self) {
        let g2 = self.diffusion * -0.64;
        let g1 = self.diffusion * -0.725;
        self.ap[1].gain = g2;
        self.ap[3].gain = g2;
        self.ap[0].gain = g1;
        self.ap[2].gain = g1;
    }

    /// Spin change: rate `20^spin · 0.1` Hz (×0.9 / ×1.1), sweep depth
    /// `min(16·wander / rate, 64)` samples.
    pub fn change_spin(&mut self) {
        let f = (20f64.powf(self.spin as f64) * 0.1f32 as f64) as f32;
        let d = ((self.wander * 16.0) / f) as i32 as i64;
        self.depth = if d < 0x41 { d } else { 0x40 };
        self.lfo[0].set_freq(f * 0.9);
        self.lfo[1].set_freq(f * 1.1);
    }

    /// Predelay change.
    pub fn change_predelay(&mut self) {
        let s = if self.sync == 0 { self.predelay } else { (self.sync_mult * 60.0) / self.bpm };
        let n = predelay_len(s, self.sr);
        for d in &mut self.pre {
            d.set_length(n);
        }
    }

    /// Note-value multiplier of the Predelay BPM parameter.
    pub fn set_sync_mult(&mut self, v: f32) {
        let m = [0.125, 0.1875, 0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0];
        if let Some(&x) = m.get(v as i32 as usize) {
            self.sync_mult = x;
        }
    }

    /// Start of the rate change once the rate changed by more than 1 Hz.
    pub fn set_rate(&mut self, sr: f32) {
        self.sr = sr;
        for l in &mut self.lfo {
            l.set_rate(sr);
        }
    }

    /// Size-change crossfade at the top of processing; returns true when
    /// the fade reached zero and the caller must apply sizes and reset.
    pub fn fade_tick(&mut self) -> bool {
        match self.fade_state {
            1 => {
                self.fade -= self.fade_step;
                if self.fade < 0.0 {
                    self.fade = 0.0;
                    self.fade_state = 2;
                    return true;
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
        false
    }

    pub fn reset(&mut self) {
        for d in self.ap.iter_mut().chain(&mut self.main).chain(&mut self.pre) {
            d.reset();
        }
        self.pav = [0; 6];
    }

    /// One input chain: `a = in·fade` (optionally band-filtered), predelay,
    /// two diffusers, into the main delay.
    pub fn input_chain(&mut self, ch: usize, x: &Block, band: Option<&mut Biquad>) {
        let (a, b) = (&mut self.a, &mut self.b);
        mul(a, x, self.fade);
        if let Some(f) = band {
            f.run(a);
        }
        self.pre[ch].basic(b, a);
        self.ap[2 * ch].allpass(a, b);
        self.ap[2 * ch + 1].allpass(b, a);
        self.main[ch].basic(a, b);
    }

    /// Left-input taps: LL into `ol` (overwritten), LR into `or`.
    pub fn taps_left(&mut self, ol: &mut Block, or: &mut Block, l1: f32, l2: f32) {
        let d = self.depth as f32;
        let m = &self.main[0];
        let t = &self.taps[0];
        m.pick_gain(ol, t.pos[9], t.amp[9]);
        self.pav[0] = m.pick_add_variable(ol, self.pav[0], d * l1 + t.pos[0] as f32, t.amp[0]);
        self.pav[1] = m.pick_add_variable(ol, self.pav[1], d * l2 + t.pos[1] as f32, t.amp[1]);
        for k in 2..=8 {
            m.pick_add(ol, t.pos[k], t.amp[k]);
        }
        let t = &self.taps[1];
        m.pick_gain(or, t.pos[9], t.amp[9]);
        self.pav[2] = m.pick_add_variable(or, self.pav[2], d * -l1 + t.pos[0] as f32, t.amp[0]);
        for k in 1..=8 {
            m.pick_add(or, t.pos[k], t.amp[k]);
        }
    }

    /// Right-input taps: RL added to `ol`, RR added to `or`.
    pub fn taps_right(&mut self, ol: &mut Block, or: &mut Block, l1: f32, l2: f32) {
        let d = self.depth as f32;
        let m = &self.main[1];
        let t = &self.taps[2];
        self.pav[3] = m.pick_add_variable(ol, self.pav[3], d * l1 + t.pos[0] as f32, t.amp[0]);
        self.pav[4] = m.pick_add_variable(ol, self.pav[4], d * l2 + t.pos[1] as f32, t.amp[1]);
        for k in 2..=9 {
            m.pick_add(ol, t.pos[k], t.amp[k]);
        }
        let t = &self.taps[3];
        self.pav[5] = m.pick_add_variable(or, self.pav[5], t.pos[0] as f32 + d * -l1, t.amp[0]);
        for k in 1..=8 {
            m.pick_add(or, t.pos[k], t.amp[k]);
        }
    }

    /// Output stage: wet scaled by `wet·k·fade`, plus the dry input.
    pub fn mix_out(&mut self, out: &mut [Block; 2], inp: &[Block; 2], k: f32) {
        let w = smooth(&mut self.wet, self.wet_t) * k * self.fade;
        let d = smooth(&mut self.dry, self.dry_t);
        gain(&mut out[0], w);
        mix_mul(&mut out[0], &inp[0], d);
        gain(&mut out[1], w);
        mix_mul(&mut out[1], &inp[if self.n_in > 1 { 1 } else { 0 }], d);
    }
}
