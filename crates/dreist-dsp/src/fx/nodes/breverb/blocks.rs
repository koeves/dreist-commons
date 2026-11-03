//! Breverb engine primitives ([re:1c27f4]–[re:4b821e]): 32-sample block helpers,
//! `DelayItem` and its [re:6e1fad] / `Comb` / [re:dc3b7d] views, biquad and
//! one-pole filters and [re:fb93db]. Every operation works on one
//! [re:4614f9] (32 samples) at a time, like the original.

use std::cell::Cell;

pub const B: usize = 32;
pub type Block = [f32; B];

thread_local! {
    /// `_Randnext` [re:362ab5] (initial value 1): the engine-wide LCG shared by
    /// every instance (`× 0x41c64e6d + 0x3039`). Per thread, reset when an
    /// engine is built ([`crate::fx::nodes::reset_shared_rng`]), so renders
    /// are reproducible.
    static RANDNEXT: Cell<u32> = const { Cell::new(1) };
}

/// Advances `_Randnext` and returns the new state.
pub fn randnext() -> u32 {
    RANDNEXT.with(|c| {
        let r = c.get().wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        c.set(r);
        r
    })
}

pub(crate) fn reset_randnext() {
    RANDNEXT.with(|c| c.set(1));
}

/// `(r & 0x3fffffff) · 2^-29 − 1`.
#[inline]
pub fn rand_bipolar(r: u32) -> f32 {
    (r & 0x3fffffff) as f32 * 1.8626451e-09 - 1.0
}

/// `_AntiUnderflowNoise` (static init [re:9fbb60]): the first 32 LCG values
/// from seed 1 scaled by 1e-10, added to every input block.
pub fn anti_underflow_noise() -> Block {
    let mut r = 1u32;
    let mut b = [0.0; B];
    for v in b.iter_mut() {
        r = r.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
        *v = rand_bipolar(r) * 1e-10;
    }
    b
}

/// `DelayItem` (ctor [re:f4e117]): power-of-two ring with write index `w`
/// (+0x30), read index `r` (+0x38), length (+0x28) and gain (+0x40).
pub struct DelayItem {
    pub armed: bool, // +0x08, set by Reset; SetLength only clears memory once armed
    pub buf: Vec<f32>,
    pub size: i64,
    pub mask: i64,
    pub len: i64,
    pub w: i64,
    pub r: i64,
    pub gain: f32,
}

impl DelayItem {
    pub fn new(n: i64) -> DelayItem {
        let mut size = 1i64;
        let mut mask = 0;
        if n > 1 {
            while size < n {
                size *= 2;
            }
            mask = size - 1;
        }
        let half = if size <= size / 2 { size - 1 } else { size / 2 };
        DelayItem { armed: false, buf: vec![0.0; size as usize], size, mask, len: half, w: half, r: 0, gain: 0.5 }
    }

    /// `SetLength` [re:bf3894].
    pub fn set_length(&mut self, n: i64) {
        let len = if n < self.size { n as i32 } else { self.size as i32 - 1 } as i64;
        if self.armed {
            if len < self.len {
                self.len = len;
                self.r = (self.size + self.w - len) & self.mask;
                return;
            }
            let u = (self.size + self.w - len) & self.mask;
            if self.r < u {
                self.buf[u as usize..].fill(0.0);
                self.buf[..self.r as usize].fill(0.0);
            } else {
                self.buf[u as usize..self.r as usize].fill(0.0);
            }
            self.r = u;
        }
        self.len = len;
    }

    /// `Reset` [re:0a459a].
    pub fn reset(&mut self) {
        self.armed = true;
        self.buf.fill(0.0);
        self.r = 0;
        self.w = self.len;
    }

    #[inline]
    fn tap_start(&self, d: i64) -> i64 {
        ((self.size + self.w) - d - 32) & self.mask
    }

    /// `Pick(float*, long)` [re:d92373]: the 32 samples ending `d` behind `w`.
    pub fn pick(&self, out: &mut Block, d: i64) {
        let u = self.tap_start(d);
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.buf[((u + i as i64) & self.mask) as usize];
        }
    }

    /// `Pick(float*, long, float)` [re:17cace].
    pub fn pick_gain(&self, out: &mut Block, d: i64, g: f32) {
        self.pick(out, d);
        gain(out, g);
    }

    /// `PickAdd` [re:ceb829].
    pub fn pick_add(&self, out: &mut Block, d: i64, g: f32) {
        let u = self.tap_start(d);
        for (i, o) in out.iter_mut().enumerate() {
            *o = g * self.buf[((u + i as i64) & self.mask) as usize] + *o;
        }
    }

    /// `PickAddVariable` [re:f88cd5]: 16.16 read position gliding linearly to
    /// `target` over the block, 4-point interpolation; returns the new position.
    pub fn pick_add_variable(&self, out: &mut Block, mut pos: i64, target: f32, g: f32) -> i64 {
        let step = ((target * 65536.0) as i64 - pos) / 32;
        let m = self.mask;
        for (base, o) in (self.w - 32..).zip(out.iter_mut()) {
            let idx = (base - (pos >> 16)) & m;
            let y0 = self.buf[idx as usize];
            let ym2 = self.buf[((idx - 2) & m) as usize];
            let ym1 = self.buf[((idx - 1) & m) as usize];
            let yp1 = self.buf[((idx + 1) & m) as usize];
            let t = (pos & 0xffff) as f32 * 1.5258789e-05;
            let v = y0 + ((ym1 - yp1) * 0.5 + (y0 * -2.5 + yp1 + ym1 + ym1 + ym2 * -0.5 + ((ym2 - yp1) * 0.5 + (y0 - ym1) * 1.5) * t) * t) * t;
            *o = v * g + *o;
            pos += step;
        }
        pos
    }

    /// [re:24934a]: `y = buf[r] − g·x; buf[w] = y·g + x`.
    pub fn allpass(&mut self, out: &mut Block, inp: &Block) {
        let g = self.gain;
        for i in 0..B {
            let y = self.buf[self.r as usize] - g * inp[i];
            out[i] = y;
            self.buf[self.w as usize] = y * g + inp[i];
            self.r = (self.r + 1) & self.mask;
            self.w = (self.w + 1) & self.mask;
        }
    }

    /// `Comb::operator()` [re:f04500]: `y = buf[r]; buf[w] = y·g + x`.
    pub fn comb(&mut self, out: &mut Block, inp: &Block) {
        let g = self.gain;
        for i in 0..B {
            let y = self.buf[self.r as usize];
            out[i] = y;
            self.buf[self.w as usize] = y * g + inp[i];
            self.r = (self.r + 1) & self.mask;
            self.w = (self.w + 1) & self.mask;
        }
    }

    /// [re:2e64e5]: read a whole block at `r`,
    /// then write the input block at `w`.
    pub fn basic(&mut self, out: &mut Block, inp: &Block) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.buf[((self.r + i as i64) & self.mask) as usize];
        }
        self.r = (self.r + 32) & self.mask;
        self.write(inp);
    }

    fn write(&mut self, inp: &Block) {
        for (i, &x) in inp.iter().enumerate() {
            self.buf[((self.w + i as i64) & self.mask) as usize] = x;
        }
        self.w = (self.w + 32) & self.mask;
    }
}

/// [re:459093]: x1 +0, x2 +4, y1 +8, y2 +0xc, b0 b1 b2 a1 a2 +0x10..,
/// sample rate +0x24 (denominator signs folded into a1/a2).
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    pub x1: f32,
    pub x2: f32,
    pub y1: f32,
    pub y2: f32,
    pub c: [f32; 5],
    pub sr: f32,
}

impl Biquad {
    /// `SetSPoly` [re:33fffa]: bilinear transform (k = 2·sr) of
    /// `(p1 s² + p2 s + p3) / (p4 s² + p5 s + p6)`.
    pub fn set_spoly(&mut self, p1: f32, p2: f32, p3: f32, p4: f32, p5: f32, p6: f32) {
        let k = self.sr + self.sr;
        let f4 = p4 * k * k;
        let f5 = p1 * k * k;
        let f3 = f4 + p6;
        let f6 = f5 + p3;
        let f5b = p3 - f5;
        let n = 1.0 / (f3 + p5 * k);
        self.c[1] = (f5b + f5b) * n;
        self.c[2] = (f6 - p2 * k) * n;
        self.c[0] = (f6 + p2 * k) * n;
        self.c[3] = (p6 - f4) * -2.0 * n;
        self.c[4] = n * (p5 * k - f3);
    }

    /// Band-limit used by Inverse/Room (`Change*Response`): high-pass at
    /// `lo` Hz times a prewarped low-pass at `hi` Hz.
    pub fn set_band(&mut self, lo: f32, hi: f32) {
        let sr = self.sr;
        let t = (hi * 3.1415927 / sr).tan();
        let w = (sr / 3.1415927) * t * 6.283;
        self.set_spoly(0.0, w, 0.0, 1.0, lo * 6.283 + w, lo * 6.283 * w);
    }

    /// `SetShelving` [re:12f8e6]: low/high shelf pair; corner arguments are
    /// prewarped Hz, gains are linear ratios (both ≈1 → bypass).
    pub fn set_shelving(&mut self, f_lo: f32, g_lo: f32, f_hi: f32, g_hi: f32) {
        if (g_lo - 1.0).abs() < 0.01 && (g_hi - 1.0).abs() < 0.01 {
            self.c = [1.0, 0.0, 0.0, 0.0, 0.0];
            return;
        }
        let mut w_lo = f_lo * 6.2831855;
        let mut w_hi = f_hi * 6.2831855;
        let p_hi;
        if g_hi < 1.0 {
            p_hi = w_hi * g_hi;
        } else {
            p_hi = w_hi;
            w_hi /= g_hi;
        }
        let p_lo;
        if g_lo < 1.0 {
            p_lo = w_lo / g_lo;
        } else {
            p_lo = w_lo;
            w_lo *= g_lo;
        }
        let pp = p_hi * p_lo;
        let k0 = (g_lo * pp) / (w_hi * w_lo);
        let k = self.sr + self.sr;
        let ps = (p_hi + p_lo) * k;
        let k2 = k * k;
        let zz = w_hi * k0 * w_lo;
        let zs = k0 * (w_hi + w_lo) * k;
        let n0 = k0 * k2 + zz;
        let n2 = zz - k0 * k2;
        let n = 1.0 / (k2 + pp + ps);
        self.c[3] = (pp - k2) * -2.0 * n;
        self.c[1] = (n2 + n2) * n;
        self.c[0] = (n0 + zs) * n;
        self.c[2] = (n0 - zs) * n;
        self.c[4] = n * (ps - (k2 + pp));
    }

    /// `operator()(float*)` [re:9abbc0] (in place).
    pub fn run(&mut self, x: &mut Block) {
        let [b0, b1, b2, a1, a2] = self.c;
        for v in x.iter_mut() {
            let s = *v;
            let y = (((self.y2 * a2 + s * b0) + self.x1 * b1) + self.x2 * b2) + self.y1 * a1;
            self.x2 = self.x1;
            self.x1 = s;
            self.y2 = self.y1;
            self.y1 = y;
            *v = y;
        }
    }

    /// `operator()(float*, const float*)` [re:178666].
    pub fn run2(&mut self, out: &mut Block, x: &Block) {
        let [b0, b1, b2, a1, a2] = self.c;
        let y0 = (((x[0] * b0 + self.x1 * b1) + self.x2 * b2) + self.y1 * a1) + self.y2 * a2;
        out[0] = y0;
        out[1] = (((x[1] * b0 + x[0] * b1) + self.x1 * b2) + y0 * a1) + self.y1 * a2;
        for i in 2..B {
            out[i] = (((x[i] * b0 + x[i - 1] * b1) + x[i - 2] * b2) + out[i - 1] * a1) + out[i - 2] * a2;
        }
        self.x1 = x[31];
        self.x2 = x[30];
        self.y1 = out[31];
        self.y2 = out[30];
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// [re:7054d0]: sample rate +0, x1 +4, y1 +8, b0 b1 a1 +0xc...
#[derive(Clone, Copy, Default)]
pub struct Bilin {
    pub sr: f32,
    pub x1: f32,
    pub y1: f32,
    pub c: [f32; 3],
}

impl Bilin {
    /// `Prewarp` [re:15c2f6]: first-order `(p1 s + p2) / (p3 s + p4)`.
    pub fn prewarp(&mut self, p1: f32, p2: f32, p3: f32, p4: f32) {
        let sr = self.sr;
        let pk = sr * 3.1415927;
        let g = ((pk * p1 * pk * p1 + p2 * p2) / (pk * p3 * pk * p3 + p4 * p4)).sqrt();
        let p1 = if p1.abs() < 1e-9 { 1e-9 } else { p1 };
        let mut z = p2 / p1;
        let mut pl = p4 / p3;
        if z < 1e-6 {
            let k = sr + sr;
            let n = 1.0 / (k + pl * g);
            self.c = [(g * k + 0.0) * n, (0.0 - g * k) * n, n * (k - pl * g)];
            return;
        }
        let den;
        if z < pl {
            if pk <= z {
                z = sr * 2.84;
            }
            z *= g;
            pl = z / (p2 / p4);
            den = sr + sr + pl;
        } else {
            if pk <= pl {
                pl = sr * 2.84;
            }
            z = pl * (p2 / p4);
            den = sr + sr + pl;
        }
        let n = 1.0 / den;
        let gk = g * (sr + sr);
        self.c = [(z + gk) * n, (z - gk) * n, n * ((sr + sr) - pl)];
    }

    /// `SetHPF` [re:b0002d].
    pub fn set_hpf(&mut self, f: f32) {
        let k = self.sr + self.sr;
        let n = 1.0 / (k + f * 6.2831855);
        let b0 = k * n;
        self.c = [b0, -b0, n * (k - f * 6.2831855)];
    }

    /// `operator()(float*)` [re:461c96] (in place).
    pub fn run(&mut self, x: &mut Block) {
        let [b0, b1, a1] = self.c;
        for v in x.iter_mut() {
            let s = *v;
            let y = (self.y1 * a1 + s * b0) + self.x1 * b1;
            self.x1 = s;
            self.y1 = y;
            *v = y;
        }
    }

    pub fn clear(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// [re:fb93db]: control rate +0 (sr/32), block period +4 (32/sr), waveform
/// +8, frequency +0xc, BPM-sync fields +0x10/+0x14/+0x18, increment +0x1c,
/// threshold +0x20, held value +0x28, slopes +0x2c..+0x38, phase +0x3c.
#[derive(Clone, Copy)]
pub struct Lfo {
    pub rate: f32,
    pub period: f32,
    pub wave: i32,
    pub freq: f32,
    pub sync_mult: f32,
    pub sync_bpm: f32,
    pub sync: bool,
    pub inc: i32,
    pub thr: i32,
    pub held: f32,
    pub slope: [f32; 4],
    pub phase: u32,
}

impl Lfo {
    /// The ctor field values every reverb writes, followed by its stray
    /// `GetValue()` (waveform 0 at that point) which advances the phase once.
    pub fn new() -> Lfo {
        let mut l = Lfo {
            rate: 44100.0,
            period: f32::from_bits(0x3a3e37c6),
            wave: 0,
            freq: 1.0,
            sync_mult: 1.0,
            sync_bpm: 120.0,
            sync: false,
            inc: 0x2f8dc2,
            thr: 0,
            held: 0.0,
            slope: [0.0; 4],
            phase: 0x80008000,
        };
        l.value();
        l
    }

    /// `SetLFO` [re:3bd702].
    pub fn set(&mut self, wave: i32, freq: f32, sym: f32) {
        self.freq = freq;
        self.wave = wave;
        let s = (sym + sym) - 1.0;
        self.inc = (self.period * 4.2949018e9 * self.freq) as i32;
        let a = 2.0 / (s + 1.0);
        self.thr = (s * 2.1474509e9) as i32;
        self.slope[0] = a;
        self.slope[1] = a - 1.0;
        self.slope[3] = (s + s) / (1.0 - s) + 1.0;
        self.slope[2] = -2.0 / (1.0 - s);
    }

    /// Increment for frequency `f` (or the BPM-synced rate); the binary
    /// computes `(period · 2^32) · f` (`ChangeSpin` [re:2800d0]).
    pub fn set_freq(&mut self, f: f32) {
        self.freq = f;
        self.inc = if self.sync { ((self.sync_bpm / (self.sync_mult * 240.0)) * self.period * 4.2949018e9) as i32 } else { ((self.period * 4.2949018e9) * f) as i32 };
    }

    /// `ChangeWaveInfo` part shared by the reverbs.
    pub fn set_rate(&mut self, sr: f32) {
        let period = 32.0 / sr;
        self.rate = sr * 0.03125;
        self.period = period;
        self.inc = if self.sync { ((self.sync_bpm / (self.sync_mult * 240.0)) * period * 4.2949018e9) as i32 } else { ((period * 4.2949018e9) * self.freq) as i32 };
    }

    /// `GetValue` [re:80ab06]: evaluate at the current phase, then advance.
    pub fn value(&mut self) -> f32 {
        let p = self.phase as i32;
        let v = match self.wave {
            0 => ((((p as f32) * 4.656613e-10).abs() as f64 - 0.5) * 2.0) as f32,
            1 => {
                if p < 0 {
                    -1.0
                } else {
                    1.0
                }
            }
            2 => p as f32 * 4.656613e-10,
            3 => (p as f32 * 1.4628749e-09).sin(),
            4 => {
                if (p.wrapping_add(self.inc) ^ p) < 0 {
                    self.held = rand_bipolar(randnext());
                }
                self.held
            }
            5 => {
                let x = p as f32 * 4.656613e-10;
                if p < self.thr { x * self.slope[0] + self.slope[1] } else { x * self.slope[2] + self.slope[3] }
            }
            _ => 0.0,
        };
        self.phase = self.phase.wrapping_add(self.inc as u32);
        v
    }
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo::new()
    }
}

/// [re:96908b] (ctor [re:7998fe]) used as [re:58604e] ([re:8faa0f]): an
/// allpass whose read tap is swept by a built-in LFO (16.16 offset,
/// depth +0x98 = samples·65536).
pub struct DelayItemLfo {
    pub d: DelayItem,
    pub rate: f32,
    pub period: f32,
    pub wave: i32,
    pub freq: f32,
    pub inc: i32,
    pub thr: i32,
    pub start: f32,
    pub held: f32,
    pub slope: [f32; 4],
    pub phase: u32,
    pub pos: i64,
    pub step: i64,
    pub depth: f32,
}

impl DelayItemLfo {
    pub fn new(n: i64) -> DelayItemLfo {
        DelayItemLfo {
            d: DelayItem::new(n),
            rate: 44100.0,
            period: f32::from_bits(0x3a3e37c6),
            wave: 0,
            freq: f32::from_bits(0x3c75c28f),
            inc: 0xb69b,
            thr: 0,
            start: 0.0,
            held: 0.0,
            slope: [2.0, 1.0, -2.0, 1.0],
            phase: 0x80300dc2,
            pos: 0,
            step: 0,
            depth: 0.0,
        }
    }

    /// `SetLFO(long depth, float freq)` [re:5604d1] (triangle).
    pub fn set_lfo(&mut self, depth: i64, freq: f32) {
        self.freq = freq;
        self.slope[0] = 2.0;
        self.slope[2] = -2.0;
        self.wave = 0;
        self.thr = 0;
        self.inc = ((self.period * 4.2949018e9) * self.freq) as i32;
        self.slope[1] = 1.0;
        self.slope[3] = 1.0;
        self.depth = depth as f32 * 65536.0;
    }

    /// `ChangeFreq` [re:a15365].
    pub fn change_freq(&mut self, sr: f32) {
        let period = 32.0 / sr;
        self.period = period;
        self.rate = sr * 0.03125;
        self.inc = ((period * 4.2949018e9) * self.freq) as i32;
    }

    fn wave_at(&mut self, p: u32) -> f32 {
        let pi = p as i32;
        match self.wave {
            0 => {
                let v = (pi as f32 * 4.656613e-10).abs() - 0.5;
                v + v
            }
            1 => {
                if pi < 0 {
                    -1.0
                } else {
                    1.0
                }
            }
            2 => pi as f32 * 4.656613e-10,
            3 => (pi as f32 * 1.4628749e-09).sin(),
            4 => {
                if (p.wrapping_add(self.inc as u32) ^ p) as i32 >= 0 {
                    self.held
                } else {
                    self.held = rand_bipolar(randnext());
                    self.held
                }
            }
            5 => {
                if pi < self.thr {
                    pi as f32 * 4.656613e-10 * self.slope[0] + self.slope[1]
                } else {
                    pi as f32 * 4.656613e-10 * self.slope[2] + self.slope[3]
                }
            }
            _ => self.depth,
        }
    }

    /// `Reset(float phase)` [re:9c9808].
    pub fn reset(&mut self, start: f32) {
        self.d.reset();
        self.start = start;
        self.phase = ((start - 0.5) * 4.2949018e9) as i64 as u32;
        let v = self.wave_at(self.phase);
        self.phase = self.phase.wrapping_add(self.inc as u32);
        self.pos = (self.depth * v) as i64;
    }

    /// [re:93dc4a].
    pub fn allpass(&mut self, out: &mut Block, inp: &Block) {
        let v = self.wave_at(self.phase);
        self.phase = self.phase.wrapping_add(self.inc as u32);
        let step = ((self.depth * v) as i64 - self.pos) / 32;
        self.step = step;
        let d = &mut self.d;
        let (m, g) = (d.mask, d.gain);
        for i in 0..B {
            let frac = self.pos & 0xffff;
            let idx = (d.r - (self.pos >> 16)) & m;
            let y0 = d.buf[idx as usize];
            let ym2 = d.buf[((idx - 2) & m) as usize];
            let ym1 = d.buf[((idx - 1) & m) as usize];
            let yp1 = d.buf[((idx + 1) & m) as usize];
            self.pos += step;
            let t = frac as f32 * 1.5258789e-05;
            let y = (((ym1 - yp1) * 0.5 + (y0 * -2.5 + yp1 + ym1 + ym1 + ym2 * -0.5 + ((ym2 - yp1) * 0.5 + (y0 - ym1) * 1.5) * t) * t) * t + y0) - g * inp[i];
            out[i] = y;
            d.buf[d.w as usize] = y * g + inp[i];
            d.w = (d.w + 1) & m;
            d.r = (d.r + 1) & m;
        }
    }
}

/// `SamplesMulAdd(dst, a, b, g)`: `dst = b·g + a`.
#[inline]
pub fn mul_add(dst: &mut Block, a: &Block, b: &Block, g: f32) {
    for i in 0..B {
        dst[i] = b[i] * g + a[i];
    }
}

/// `SamplesSub(dst, src)`: `dst −= src`.
#[inline]
pub fn sub(dst: &mut Block, src: &Block) {
    for i in 0..B {
        dst[i] -= src[i];
    }
}

#[inline]
pub fn gain(x: &mut Block, g: f32) {
    for v in x.iter_mut() {
        *v *= g;
    }
}

/// `SamplesMul(dst, src, g)`.
#[inline]
pub fn mul(dst: &mut Block, src: &Block, g: f32) {
    for i in 0..B {
        dst[i] = src[i] * g;
    }
}

/// `SamplesMixMul(dst, src, g)`: `dst += src·g`.
#[inline]
pub fn mix_mul(dst: &mut Block, src: &Block, g: f32) {
    for i in 0..B {
        dst[i] = src[i] * g + dst[i];
    }
}

/// `SamplesAdd(dst, a, b)`.
#[inline]
pub fn add(dst: &mut Block, a: &Block, b: &Block) {
    for i in 0..B {
        dst[i] = a[i] + b[i];
    }
}

/// `SamplesMix(dst, src)`: `dst += src`.
#[inline]
pub fn mix(dst: &mut Block, src: &Block) {
    for i in 0..B {
        dst[i] += src[i];
    }
}

/// One-pole smoother used for wet/dry/decay: `x + (t − x)·0.08`, flushed
/// to zero below 1e-6.
#[inline]
pub fn smooth(cur: &mut f32, target: f32) -> f32 {
    let v = *cur + (target - *cur) * 0.08;
    *cur = if v.abs() < 1e-6 { 0.0 } else { v };
    *cur
}

/// Predelay in samples: halved until ≤ 0xffff, at least 32.
pub fn predelay_len(seconds: f32, sr: f32) -> i64 {
    let mut n = (seconds * sr) as i64;
    while n > 0xffff {
        n /= 2;
    }
    n.max(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_item_basic_delay_is_len() {
        let mut d = DelayItem::new(0x100);
        d.set_length(64);
        d.reset();
        let mut out = [0.0; B];
        let mut imp = [0.0; B];
        imp[0] = 1.0;
        let z = [0.0; B];
        let mut got = Vec::new();
        for k in 0..5 {
            d.basic(&mut out, if k == 0 { &imp } else { &z });
            got.extend_from_slice(&out);
        }
        assert_eq!(got.iter().position(|&v| v == 1.0), Some(64));
    }

    #[test]
    fn noise_is_tiny() {
        let n = anti_underflow_noise();
        assert!(n.iter().all(|v| v.abs() <= 1e-10) && n.iter().any(|&v| v != 0.0));
    }
}
