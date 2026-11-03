//! [re:97750e] (Tin Can Verb; `InitializePorts` [re:1c6c9c],
//! `Voice::PrepareStream` [re:cec0ec], `Voice::ProcessBuffer` [re:3f85e2]):
//! Freeverb-style reverb — 8 damped combs and 4 allpasses per channel,
//! crossfed 80/20 input, Pinch/Squeeze reshape the delay lengths.

use super::math::{clip01, pow2_at_least};
use crate::fx::{Ctx, Node, Port, ports, ports_from};

const COMB_L: [f32; 8] = [0.0253061, 0.0269388, 0.0289569, 0.0307483, 0.0322449, 0.0338095, 0.0338095, 0.0353061];
const COMB_R: [f32; 8] = [0.0258277, 0.0274603, 0.0294785, 0.0312698, 0.0327664, 0.0343311, 0.0343311, 0.0358277];
const AP_L: [f32; 4] = [0.0126077, 0.01, 0.00773243, 0.00510204];
const AP_R: [f32; 4] = [0.0131293, 0.0105215, 0.00825397, 0.00562358];

pub struct TinCan {
    ports: Vec<Port>,
    sr: f32,
    combs: Vec<Vec<f32>>,
    aps: Vec<Vec<f32>>,
    comb_mask: usize,
    ap_mask: usize,
    comb_len: [u32; 16],
    ap_len: [u32; 8],
    filt: [f32; 16],
    idx: usize,
    key: [f32; 3],
    alt: f32,
}

impl TinCan {
    pub fn new() -> TinCan {
        TinCan {
            ports: ports_from(ports::TIN_CAN_VERB),
            sr: 0.0,
            combs: Vec::new(),
            aps: Vec::new(),
            comb_mask: 0,
            ap_mask: 0,
            comb_len: [5; 16],
            ap_len: [5; 8],
            filt: [0.0; 16],
            idx: 0,
            key: [f32::NAN; 3],
            alt: super::ssl_filter::DENORMAL,
        }
    }

    /// `PrepareStream`: power-of-two buffers. The original allocates 5 s
    /// (combs) and 2.5 s (allpasses); the lengths reach at most
    /// Size³·3 × the longest tuning (0.0358 s combs, 0.0131 s allpasses) ≈
    /// 0.108 s / 0.040 s, and the reads are relative to the shared write
    /// index, so 0.125 s / 0.0625 s buffers play the same.
    fn prepare(&mut self, sr: f32) {
        if self.sr == sr {
            return;
        }
        self.sr = sr;
        let cs = pow2_at_least((sr * 0.125).ceil() as i64);
        let aps = pow2_at_least((sr * 0.0625).ceil() as i64);
        self.combs = vec![vec![0.0; cs]; 16];
        self.aps = vec![vec![0.0; aps]; 8];
        self.comb_mask = cs - 1;
        self.ap_mask = aps - 1;
        self.key = [f32::NAN; 3];
    }
}

impl Default for TinCan {
    fn default() -> Self {
        TinCan::new()
    }
}

impl Node for TinCan {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn prepare_rate(&mut self, sr: f32) {
        self.prepare(sr);
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.prepare(ctx.sample_rate);
        let p = |i: usize| self.ports[i].value;
        let s = clip01(p(0));
        let size = s * s * s * 3.0;
        let pinch = clip01(p(3)) * 0.98;
        let squeeze = clip01(p(4)) * 0.98;
        if self.key != [size, pinch, squeeze] {
            self.key = [size, pinch, squeeze];
            let t = size * self.sr;
            let o = 1.0 - squeeze;
            let s1 = clip01(1.0 - pinch) * t;
            let cq = squeeze * 0.03175;
            let aq = squeeze * 0.0091;
            for k in 0..8 {
                self.comb_len[k] = ((o * COMB_L[k] + cq) * s1 + 5.0).round() as u32;
                self.comb_len[8 + k] = ((o * COMB_R[k] + cq) * s1 + 5.0).round() as u32;
            }
            for k in 0..4 {
                self.ap_len[k] = ((o * AP_L[k] + aq) * t + 5.0).round() as u32;
                self.ap_len[4 + k] = ((o * AP_R[k] + aq) * t + 5.0).round() as u32;
            }
        }
        let (fb, gain_in, damp) = if p(5) >= 0.5 { (1.0, 0.0, 1.0) } else { (clip01(p(1)) * 0.99, 1.0, 1.0 - clip01(p(2))) };
        let w = p(6) + p(6);
        let wet = clip01(w) * 0.125;
        let dry = clip01(2.0 - w);
        let (cm, am) = (self.comb_mask, self.ap_mask);
        let [ol, or] = out;
        for i in 0..inp[0].len() {
            let a = self.alt;
            self.alt = -a;
            let (xl, xr) = (inp[0][i], inp[1][i]);
            self.idx = (self.idx + 1) & cm;
            let idx = self.idx;
            let mut ap_out = [0.0f32; 2];
            let mixes = [xr * 0.2 + xl * 0.8, xl * 0.2 + xr * 0.8];
            for ch in 0..2 {
                let input = gain_in * mixes[ch] - a;
                let mut sum = 0.0f32;
                for k in ch * 8..ch * 8 + 8 {
                    let len = self.comb_len[k] as usize;
                    let o = self.combs[k][idx.wrapping_sub(len) & cm];
                    let f = (o - self.filt[k]) * damp + self.filt[k];
                    self.filt[k] = f;
                    self.combs[k][idx & cm] = f * fb + input;
                    sum += o;
                }
                let mut v = sum;
                for k in ch * 4..ch * 4 + 4 {
                    let len = self.ap_len[k] as usize;
                    let bo = self.aps[k][idx.wrapping_sub(len) & am] - v * 0.5;
                    self.aps[k][idx & am] = 0.5 * bo + v;
                    v = bo;
                }
                ap_out[ch] = v;
            }
            ol[i] = (ap_out[1] * 0.2 + ap_out[0] * 0.8) * wet + mixes[0] * dry;
            or[i] = (ap_out[0] * 0.2 + ap_out[1] * 0.8) * wet + mixes[1] * dry;
        }
    }

    fn reset(&mut self) {
        for b in self.combs.iter_mut().chain(self.aps.iter_mut()) {
            b.fill(0.0);
        }
        self.filt = [0.0; 16];
        self.idx = 0;
    }
}
