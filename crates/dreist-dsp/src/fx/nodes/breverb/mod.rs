//! Breverb (kinds 39–42): [re:10c03d] (ctor [re:9bdf23],
//! stream setup [re:994866], processing [re:9703a2]) wrapping
//! the embedded engine API ([re:f1a802]..): a [re:ac5b7b]
//! ([re:2f8320]) feeding 32-sample [re:4614f9]s to one of the [re:5a6b3d]
//! algorithms Hall / Plate / Room / Inverse.
//!
//! Node ports (all raw 0..1 positions): 0 Mix, 1 Time, 2 Predelay,
//! 3 Diffusion, 4 Low, 5 High, 6 Size, 7 Spread, 8 Shape. The engine's parameter set
//! maps them onto algorithm parameters through the model's parameter map; only the
//! first `count` ports are polled, and the last polled port always lands on
//! parameter 0 (Time). See the dreist-re effects notes.

mod blocks;

pub(crate) use blocks::reset_randnext;
mod early;
mod hall;
mod inverse;
mod plate;
mod room;

use blocks::{B, Block, anti_underflow_noise, mix};

use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// [re:2178e3] {type, min, max} (one entry of a model's parameter table).
#[derive(Clone, Copy)]
pub struct Param(pub i32, pub f32, pub f32);

impl Param {
    /// [re:f6ca87] (position → value).
    pub fn value(&self, p: f32) -> f32 {
        let Param(t, lo, hi) = *self;
        let exp = |e: f32| ((hi / lo) as f64).powf(e as f64);
        match t {
            1 => (exp(p) * lo as f64) as f32,
            2 => {
                let v = (hi - lo) * p + lo;
                (if v < 0.0 { v - 0.5 } else { v + 0.5 }) as i32 as f32
            }
            3 if p == 0.0 => -1000.0,
            4 => {
                if p == 0.0 {
                    0.0
                } else if p == 1.0 {
                    1000.0
                } else {
                    (exp(p) * lo as f64) as f32
                }
            }
            5 => {
                if p == 0.5 {
                    0.0
                } else if p <= 0.5 {
                    ((-lo) as f64 * exp((0.5 - p) + (0.5 - p))) as f32
                } else {
                    (exp((p - 0.5) + (p - 0.5)) * lo as f64) as f32
                }
            }
            6 => ((hi + lo) as f64 - ((hi / lo) as f64).powf((1.0 - p) as f64) * lo as f64) as f32,
            _ => (hi - lo) * p + lo,
        }
    }

    /// [re:1be54e]: clamps `v` in place and
    /// returns its position.
    pub fn position(&self, v: &mut f32) -> f32 {
        let Param(t, lo, hi) = *self;
        match t {
            3 => {
                if hi < *v {
                    *v = hi;
                }
            }
            5 => {
                if !(*v <= hi && -hi <= *v) {
                    *v = if hi < *v { hi } else { -hi };
                }
            }
            _ => {
                if *v < lo {
                    *v = lo;
                } else if hi < *v {
                    *v = hi;
                }
            }
        }
        let x = *v;
        let ln = |a: f32| (a as f64).ln();
        match t {
            1 | 4 => (ln(x / lo) / ln(hi / lo)) as f32,
            3 => {
                if lo > x {
                    0.0
                } else {
                    (x - lo) / (hi - lo)
                }
            }
            5 => {
                if lo > x && x > -lo {
                    0.5
                } else if x > 0.0 {
                    (ln(x / lo) / ln(hi / lo) * 0.5 + 0.5) as f32
                } else {
                    (-ln(-x / lo) / ln(hi / lo) * 0.5 + 0.5) as f32
                }
            }
            6 => (1.0 - ln((hi + lo - x) / lo) / ln(hi / lo)) as f32,
            _ => (x - lo) / (hi - lo),
        }
    }
}

/// A [re:5a6b3d] algorithm.
pub trait Algo: Send {
    fn params(&self) -> &'static [Param];
    /// Parameter-value set: `pos` from the position lookup, `val` clamped.
    fn set_val(&mut self, idx: usize, pos: f32, val: f32);
    fn change_wave_info(&mut self, sr: f32, n_in: i64, n_out: i64);
    fn reset(&mut self);
    fn process(&mut self, out: &mut [Block; 2], inp: &[Block; 2]);

    /// [re:59336b] (e.g. [re:bbc207]): position → value → parameter-value set.
    fn set_parameter(&mut self, idx: usize, p: f32) {
        let v = self.params()[idx].value(p);
        self.set_parameter_val(idx, v);
    }

    fn set_parameter_val(&mut self, idx: usize, v: f32) {
        let mut v = v;
        let pos = self.params()[idx].position(&mut v);
        self.set_val(idx, pos, v);
    }
}

/// [re:ac5b7b] (setup [re:5c6631], processing [re:2f8320]).
struct Public {
    algo: Box<dyn Algo>,
    fifo: [Vec<f32>; 2],
    fill: usize,
    pend: usize,
    fast: bool,
    blk_in: [Block; 2],
    blk_out: [Block; 2],
    noise: Block,
}

impl Public {
    fn new(algo: Box<dyn Algo>) -> Public {
        let mut p = Public {
            algo,
            // Pending input stays below two blocks plus a node block.
            fifo: [Vec::with_capacity(4 * B), Vec::with_capacity(4 * B)],
            fill: 32,
            pend: 0,
            fast: true,
            blk_in: [[0.0; B]; 2],
            blk_out: [[0.0; B]; 2],
            noise: anti_underflow_noise(),
        };
        p.reset();
        p
    }

    /// [re:ad9611].
    fn reset(&mut self) {
        self.algo.reset();
        for f in &mut self.fifo {
            f.clear();
            f.resize(B, 0.0);
        }
        self.fill = B;
        self.pend = 0;
        self.fast = true;
        self.blk_in = [[0.0; B]; 2];
        self.blk_out = [[0.0; B]; 2];
    }

    fn run_block(&mut self) {
        for b in &mut self.blk_in {
            mix(b, &self.noise);
        }
        self.algo.process(&mut self.blk_out, &self.blk_in);
    }

    fn process(&mut self, out: [&mut [f32]; 2], inp: [&[f32]; 2]) {
        let n = inp[0].len();
        let [ol, or] = out;
        if self.pend == 0 && n.is_multiple_of(B) && self.fast {
            for k in (0..n).step_by(B) {
                for ch in 0..2 {
                    self.blk_in[ch].copy_from_slice(&inp[ch][k..k + B]);
                }
                self.run_block();
                ol[k..k + B].copy_from_slice(&self.blk_out[0]);
                or[k..k + B].copy_from_slice(&self.blk_out[1]);
            }
            return;
        }
        self.fast = false;
        for ch in 0..2 {
            self.fifo[ch].truncate(self.fill);
            self.fifo[ch].extend_from_slice(inp[ch]);
        }
        self.fill += n;
        let mut produced = 0;
        let mut consumed = 0;
        if self.pend != 0 {
            let c = self.pend.min(n);
            ol[..c].copy_from_slice(&self.blk_out[0][B - self.pend..B - self.pend + c]);
            or[..c].copy_from_slice(&self.blk_out[1][B - self.pend..B - self.pend + c]);
            produced = self.pend;
        }
        while produced < n {
            let cnt = (n - produced).min(B);
            for ch in 0..2 {
                self.blk_in[ch].copy_from_slice(&self.fifo[ch][consumed..consumed + B]);
            }
            self.run_block();
            consumed += B;
            ol[produced..produced + cnt].copy_from_slice(&self.blk_out[0][..cnt]);
            or[produced..produced + cnt].copy_from_slice(&self.blk_out[1][..cnt]);
            produced += B;
        }
        self.pend = produced - n;
        self.fill -= consumed;
        for f in &mut self.fifo {
            f.drain(..consumed);
        }
    }
}

/// The engine's reverb model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    Hall = 0,
    Plate = 1,
    Room = 2,
    Inverse = 3,
}

impl Model {
    /// Engine creation [re:f1a802]: polled port count and the model's parameter map.
    fn map(self) -> &'static [usize] {
        match self {
            Model::Inverse => &[8, 3, 0, 4, 1, 5, 2, 0],
            Model::Room => &[11, 5, 0, 6, 2, 8, 4, 1, 3, 0],
            Model::Plate => &[11, 5, 0, 12, 2, 6, 10, 1, 3, 0],
            Model::Hall => &[17, 5, 0, 7, 2, 13, 10, 1, 6, 3, 0],
        }
    }

    fn count(self) -> usize {
        self.map().len() - 1
    }
}

/// Engine handle.
struct Bfd {
    model: Model,
    eff: Public,
}

impl Bfd {
    fn new(model: Model) -> Bfd {
        let algo: Box<dyn Algo> = match model {
            Model::Inverse => Box::new(inverse::Inverse::new()),
            Model::Room => Box::new(room::Room::new()),
            Model::Plate => Box::new(plate::Plate::new()),
            Model::Hall => Box::new(hall::Hall::new()),
        };
        Bfd { model, eff: Public::new(algo) }
    }

    /// Engine parameter set [re:2cf862].
    fn set_parameter(&mut self, i: usize, v: f32) {
        let map = self.model.map();
        if i == 0 {
            self.eff.algo.set_parameter(map[0], 1.0 - v);
            self.eff.algo.set_parameter(map[1], v);
        } else {
            self.eff.algo.set_parameter(map[i + 1], v);
        }
    }

    /// Engine sample-rate / block-size change [re:7a3028].
    fn change_wave_info(&mut self, sr: f32) {
        let a = &mut self.eff.algo;
        a.change_wave_info(sr, 2, 2);
        match self.model {
            Model::Plate => {
                a.set_parameter_val(4, 1.0);
                a.set_parameter_val(7, 300.0);
                a.set_parameter_val(8, 5000.0);
                a.set_parameter(9, 0.25);
                a.set_parameter(13, 0.0);
            }
            Model::Hall => {
                a.set_parameter_val(4, 1.0);
                a.set_parameter_val(8, 0.0);
                a.set_parameter_val(9, 0.0);
                a.set_parameter(11, 0.2);
                a.set_parameter(12, 0.2);
                a.set_parameter_val(14, 300.0);
                a.set_parameter_val(15, 5000.0);
                a.set_parameter_val(16, 80.0);
            }
            Model::Room => {
                a.set_parameter_val(7, 3500.0);
                a.set_parameter(9, 0.2);
                a.set_parameter(10, 0.2);
            }
            Model::Inverse => {
                a.set_parameter(6, 0.2);
                a.set_parameter(7, 0.2);
            }
        }
    }
}

pub struct Breverb {
    ports: Vec<Port>,
    model: Model,
    sr: f32,
    bfd: Bfd,
    stored: [f32; 9],
}

impl Breverb {
    pub fn new(model: Model) -> Breverb {
        Breverb { ports: ports_from(ports::BREVERB), model, sr: 0.0, bfd: Bfd::new(model), stored: [0.0; 9] }
    }

    /// Stream setup [re:994866]. Hall's port 9 is past the 9-entry
    /// control array (an uninitialised read in the original); it is sent
    /// 0.5 here and never polled again.
    fn prepare(&mut self, sr: f32) {
        let count = self.model.count();
        for i in 0..count {
            let v = if i == 2 { 0.0 } else { 0.5 };
            if i < 9 {
                self.stored[i] = v;
            }
            self.bfd.set_parameter(i, v);
        }
        self.bfd.change_wave_info(sr);
    }
}

impl Node for Breverb {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        if self.sr != ctx.sample_rate {
            self.sr = ctx.sample_rate;
            self.prepare(self.sr);
        }
        // Processing [re:9703a2].
        for i in 0..self.model.count().min(9) {
            let v = self.ports[i].value;
            if (self.stored[i] - v).abs() >= 1e-6 {
                self.stored[i] = v;
                self.bfd.set_parameter(i, v);
            }
        }
        if inp[0].is_empty() {
            return;
        }
        self.bfd.eff.process(out, inp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_round_trip() {
        let p = Param(1, 0.1, 1.5);
        let mut v = p.value(0.25);
        assert!((p.position(&mut v) - 0.25).abs() < 1e-6);
        let mut big = 9.0;
        assert_eq!(p.position(&mut big), 1.0);
        assert_eq!(big, 1.5);
    }

    fn run_signal(model: Model, block: usize, x: &[f32]) -> Vec<f32> {
        let ctx = Ctx::new(48000.0);
        let mut r = Breverb::new(model);
        let mut y = Vec::new();
        for c in x.chunks_exact(block) {
            let (mut l, mut rr) = (vec![0.0; block], vec![0.0; block]);
            r.process(&ctx, [c, c], [&mut l, &mut rr]);
            assert!(l.iter().chain(&rr).all(|v| v.is_finite()));
            y.extend_from_slice(&l);
        }
        y
    }

    fn impulse(n: usize, at: usize) -> Vec<f32> {
        let mut x = vec![0.0f32; n];
        x[at] = 1.0;
        x
    }

    #[test]
    fn reverbs_ring() {
        for m in [Model::Inverse, Model::Room, Model::Plate, Model::Hall] {
            // After the start-up size crossfade (Plate resets its tank
            // 50 ms in, swallowing anything fed before).
            let y = run_signal(m, 256, &impulse(48128, 12000));
            let tail: f32 = y[24000..40000].iter().map(|v| v * v).sum();
            assert!(tail > 1e-6, "{m:?} tail {tail}");
        }
    }

    #[test]
    fn fifo_path_is_fast_path_after_a_zero_block() {
        // Odd host blocks take the FIFO path, which runs one extra leading
        // block of silence (32 samples latency).
        let x = impulse(48000, 600);
        let mut pre = vec![0.0f32; 32];
        pre.extend_from_slice(&x);
        let a = run_signal(Model::Room, 32, &pre);
        let b = run_signal(Model::Room, 250, &x);
        let err = (0..b.len()).map(|i| (a[i] - b[i]).abs()).fold(0.0f32, f32::max);
        assert_eq!(err, 0.0);
    }
}

#[cfg(test)]
mod diag {
    use super::*;
    #[test]
    #[ignore]
    fn envelope() {
        for m in [Model::Inverse, Model::Room, Model::Plate, Model::Hall] {
            let ctx = Ctx::new(48000.0);
            let mut r = Breverb::new(m);
            let mut x = vec![0.0f32; 96000];
            x[4800] = 1.0;
            let mut y = vec![];
            for c in x.chunks(256) {
                let (mut l, mut rr) = (vec![0.0; 256], vec![0.0; 256]);
                r.process(&ctx, [c, c], [&mut l, &mut rr]);
                y.extend(l);
            }
            let e: Vec<String> = y.chunks(4800).map(|c| format!("{:.1}", 10.0 * (c.iter().map(|v| v * v).sum::<f32>() + 1e-20).log10())).collect();
            println!("{m:?}: {}", e.join(" "));
        }
    }
}
