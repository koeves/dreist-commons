//! Small DSP building blocks shared by the original's newer processors
//! ([re:4e6c52], [re:f4d05c], [re:d8c888], [re:e590e8],
//! [re:76aea3], `Compander`, [re:d3662a], `Pan`,
//! [re:cde049]). Addresses in each item's doc.

use super::math::clip01;
use super::math::sat;
use super::ssl_filter::DENORMAL;
use crate::fx::scratch;

/// [re:d70ecb]: width `g` ramped by `dg`
/// (incremented before each sample).
pub fn ms_spacing(mut g: f32, dg: f32, l: &mut [f32], r: &mut [f32]) {
    for i in 0..l.len() {
        g += dg;
        let m = (l[i] + r[i]) * 0.5;
        let s = (r[i] - l[i]) * g * 0.5;
        l[i] = m - s;
        r[i] = s + m;
    }
}

/// [re:4e6c52] ([re:08a88c]/[re:c467f9]/[re:a63cde]): one-pole DC blocker at 10 Hz.
#[derive(Debug, Clone, Copy, Default)]
pub struct DcFilter {
    k: f32,
    s: [f32; 2],
}

impl DcFilter {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.k = 1.0 - ((1.0 / sr) * -62.831856).exp();
        self.s = [0.0; 2];
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.s[0] -= DENORMAL;
        self.s[1] += DENORMAL;
        for i in 0..l.len() {
            let y = l[i] - self.s[0];
            self.s[0] += self.k * y;
            l[i] = y;
            let y = r[i] - self.s[1];
            self.s[1] += self.k * y;
            r[i] = y;
        }
    }
}

/// [re:388c85] (RBJ-style; `q` 0 → Q ≈ 0.707).
/// Modes: 0 low-pass, 1 high-pass, 2 band-pass, 3 band-pass (peak 0 dB).
pub fn iir_coeffs(f: f32, q: f32, mode: u32, inv_sr: f32) -> [f32; 5] {
    let w = (f * 6.2831855 * inv_sr) as f64;
    let (sn, cs) = (w.sin(), w.cos());
    if mode > 3 {
        return [0.0; 5];
    }
    let a = ((1.0 - q) * 0.5893446 + 0.11786893) as f64 * sn;
    let (b0, b1, b2) = match mode {
        0 => ((1.0 - cs) * 0.5, 1.0 - cs, (1.0 - cs) * 0.5),
        1 => ((cs + 1.0) * 0.5, -(cs + 1.0), (cs + 1.0) * 0.5),
        2 => (sn * 0.5, 0.0, sn * -0.5),
        _ => (a, 0.0, -a),
    };
    let n = 1.0 / (a + 1.0);
    [(b0 * n) as f32, (b1 * n) as f32, (b2 * n) as f32, (cs * -2.0 * n) as f32, (n * (1.0 - a)) as f32]
}

/// Stereo direct-form-I biquad state.
#[derive(Debug, Clone, Copy, Default)]
struct Df1 {
    x1: [f32; 2],
    x2: [f32; 2],
    y1: [f32; 2],
    y2: [f32; 2],
}

impl Df1 {
    #[inline]
    fn tick(&mut self, c: &[f32; 5], ch: usize, x: f32) -> f32 {
        let y = ((self.x2[ch] * c[2] + c[1] * self.x1[ch] + c[0] * x) - c[3] * self.y1[ch]) - self.y2[ch] * c[4];
        self.x2[ch] = self.x1[ch];
        self.x1[ch] = x;
        self.y2[ch] = self.y1[ch];
        self.y1[ch] = y;
        y
    }
}

/// Parameter glide used by the stereo filters: the per-sample step is
/// `ceil((target − cur)/len·1000)·0.001`, fixed for the block.
fn glide_step(target: f32, cur: f32, inv_len: f64) -> f64 {
    ((target as f64 - cur as f64) * inv_len * 1000.0).ceil() * 0.0010000000474974513
}

/// [re:f4d05c] ([re:9e3038]..[re:02ae58]): input soft-saturated, cutoff and
/// Q glide over 512 samples.
#[derive(Debug, Clone, Copy, Default)]
pub struct StereoIir {
    inv_sr: f32,
    mode: u32,
    dirty: bool,
    f_cur: f32,
    pub f_tgt: f32,
    q_cur: f32,
    pub q_tgt: f32,
    c: [f32; 5],
    st: Df1,
}

impl StereoIir {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.inv_sr = 1.0 / sr;
    }

    /// Mode setup (also marks the coefficients stale).
    pub fn set_mode(&mut self, mode: u32) {
        self.mode = mode;
        self.dirty = true;
    }

    pub fn set_hz(&mut self, f: f32, q: f32) {
        self.f_tgt = f;
        self.q_tgt = q;
    }

    /// Setup with `(f01, q)`: 0..1 maps to 20 Hz..20 kHz logarithmically.
    pub fn set_01(&mut self, f: f32, q: f32) {
        self.f_tgt = (f * 9.965784 + 3.321928 + 1.0).exp2();
        self.q_tgt = q;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut df = glide_step(self.f_tgt, self.f_cur, 0.001953125);
        let mut dq = glide_step(self.q_tgt, self.q_cur, 0.001953125);
        let settled_f = df.abs() < 0.0001;
        if settled_f {
            self.f_cur = self.f_tgt;
            df = 0.0;
        }
        if dq.abs() < 0.0001 {
            self.q_cur = self.q_tgt;
            dq = 0.0;
            if settled_f && !self.dirty {
                for i in 0..l.len() {
                    l[i] = self.st.tick(&self.c, 0, sat(l[i]));
                    r[i] = self.st.tick(&self.c, 1, sat(r[i]));
                }
                return;
            }
        }
        for i in 0..l.len() {
            self.f_cur += df as f32;
            self.q_cur += dq as f32;
            self.c = iir_coeffs(self.f_cur, self.q_cur, self.mode, self.inv_sr);
            l[i] = self.st.tick(&self.c, 0, sat(l[i]));
            r[i] = self.st.tick(&self.c, 1, sat(r[i]));
        }
        self.dirty = false;
    }
}

/// [re:d8c888] ([re:1e80fe]..[re:684e82]) in high-shelf mode ([re:fd8169] mode 4,
/// [re:b91b49]): frequency and gain glide over 128 samples.
#[derive(Debug, Clone, Copy, Default)]
pub struct StereoSslShelf {
    sr: f32,
    f: [f32; 2],
    q: [f32; 2],
    g: [f32; 2],
    c: [f32; 5],
    st: Df1,
}

impl StereoSslShelf {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
    }

    pub fn init(&mut self, f: f32, q: f32, gain: f32) {
        self.f[0] = f;
        self.q[0] = q;
        self.g[0] = gain;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let mut df = glide_step(self.f[0], self.f[1], 0.0078125);
        let mut dq = glide_step(self.q[0], self.q[1], 0.0078125);
        let mut dg = glide_step(self.g[0], self.g[1], 0.0078125);
        let sf = df.abs() < 0.0001;
        if sf {
            self.f[1] = self.f[0];
            df = 0.0;
        }
        let sq = dq.abs() < 0.0001;
        if sq {
            self.q[1] = self.q[0];
            dq = 0.0;
        }
        if dg.abs() < 0.0001 {
            self.g[1] = self.g[0];
            dg = 0.0;
            if sf && sq {
                for i in 0..l.len() {
                    l[i] = self.st.tick(&self.c, 0, sat(l[i]));
                    r[i] = self.st.tick(&self.c, 1, sat(r[i]));
                }
                return;
            }
        }
        for i in 0..l.len() {
            self.f[1] += df as f32;
            self.q[1] += dq as f32;
            self.g[1] += dg as f32;
            let (b, a) = super::ssl_eq::ssl_shelf(self.f[1] as f64, self.g[1] as f64, self.sr as f64, true);
            self.c = [b[0] as f32, b[1] as f32, b[2] as f32, a[1] as f32, a[2] as f32];
            l[i] = self.st.tick(&self.c, 0, sat(l[i]));
            r[i] = self.st.tick(&self.c, 1, sat(r[i]));
        }
    }
}

/// [re:e590e8] ([re:a5ad38]..[re:95f1fa]): mono high-pass (Q 0.707) whose
/// cutoff glides by 1/512 of the remaining distance per sample.
#[derive(Debug, Clone, Copy)]
pub struct SideChain {
    inv_sr: f32,
    f_cur: f32,
    f_tgt: f32,
    alt: f32,
    c: [f32; 5],
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Default for SideChain {
    fn default() -> Self {
        SideChain { inv_sr: 0.0, f_cur: 0.0, f_tgt: 0.0, alt: DENORMAL, c: [0.0; 5], x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
}

impl SideChain {
    pub fn init(&mut self, sr: f32, f: f32) {
        self.inv_sr = 1.0 / sr;
        self.f_tgt = f;
    }

    pub fn process(&mut self, x: &[f32], out: &mut [f32]) {
        let step = (self.f_tgt - self.f_cur) * 0.001953125;
        let glide = step.abs() >= 1e-6;
        for i in 0..x.len() {
            if glide {
                self.f_cur += step;
                let w = self.inv_sr as f64 * self.f_cur as f64 * std::f64::consts::TAU;
                let (sn, cs) = (w.sin(), w.cos());
                let a0 = sn * 0.707213579 + 1.0;
                let b0 = ((cs + 1.0) * 0.5 / a0) as f32;
                self.c = [b0, (-(cs + 1.0) / a0) as f32, b0, (cs * -2.0 / a0) as f32, ((1.0 - sn * 0.707213579) / a0) as f32];
            }
            let d = self.alt;
            self.alt = -d;
            let xi = x[i] - d;
            let y = ((self.c[2] * self.x2 + self.c[1] * self.x1 + self.c[0] * xi) - self.c[3] * self.y1) - self.c[4] * self.y2;
            self.x2 = self.x1;
            self.x1 = xi;
            self.y2 = self.y1;
            self.y1 = y;
            out[i] = y;
        }
    }
}

/// [re:76aea3] ([re:142a06]..[re:35ca5e]), parabolic mode: peak
/// hold with an accelerating release, then a one-pole attack smoother.
#[derive(Debug, Clone, Copy)]
pub struct EnvFollower {
    level: f32,
    w: f32,
    accel: f32,
    att_t: f32,
    att: f32,
    rel_t: f32,
    rel: f32,
    k: f32,
    peak: f32,
    smooth: f32,
}

impl Default for EnvFollower {
    fn default() -> Self {
        EnvFollower { level: 0.0, w: 0.0, accel: 1.0, att_t: 0.0, att: 0.0, rel_t: 0.0, rel: 0.0, k: 0.0, peak: 0.0, smooth: 0.0 }
    }
}

impl EnvFollower {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.w = (1.0 / sr) * 6.2831855;
        self.accel = (sr / 88200.0) * 1e-5 + 1.0;
    }

    /// Setup with `(attack, release, level, fast)`: times are `2^(12x − 9)` s.
    pub fn init(&mut self, attack: f32, release: f32, level: f32, fast: bool) {
        self.level = level;
        let a = (attack * 12.0 - 8.0 - 1.0).exp2();
        let r = (release * 12.0 - 8.0 - 1.0).exp2();
        if (self.att_t - a).abs() >= 1e-6 {
            self.att_t = a;
            self.att = clip01(self.w / ((a + a) * 0.5 + 1e-8));
            if fast {
                self.att *= 8.0;
            }
        }
        if (self.rel_t - r).abs() >= 1e-6 {
            self.rel_t = r;
            self.rel = clip01(self.w / ((r + r) * 0.5 + 1e-8));
        }
        let rt = self.rel_t;
        self.k = clip01((self.w * 0.05) / ((rt.abs() + rt) * 0.5 + 1e-8));
    }

    /// `Process` [re:55b42d]: instant-attack peak of `|level·1.5·x|` with a
    /// one-pole release, then the attack smoother (`x` → `out`).
    pub fn process(&mut self, x: &[f32], out: &mut [f32]) {
        self.peak -= DENORMAL;
        self.smooth += DENORMAL;
        for i in 0..x.len() {
            let a = (self.level * 1.5 * x[i]).abs();
            if self.peak < a {
                self.peak = a;
            }
            self.peak += (a - self.peak) * self.rel;
            self.smooth += (self.peak - self.smooth) * self.att;
            let s = if self.smooth < 1e-15 { 0.0 } else { self.smooth };
            out[i] = s.min(1.0);
        }
    }

    pub fn process_parabolic_from(&mut self, x: &[f32], out: &mut [f32]) {
        out.copy_from_slice(x);
        self.process_parabolic(out);
    }

    /// In place: each sample is read before it is replaced.
    pub fn process_parabolic(&mut self, x: &mut [f32]) {
        self.peak -= DENORMAL;
        self.smooth += DENORMAL;
        let rt = self.rel_t;
        for i in 0..x.len() {
            let a = if x[i] != 0.0 { x[i].abs() } else { 0.0 }.min(1.0);
            let k = if a <= self.peak {
                self.k
            } else {
                self.peak = a;
                self.k = clip01((self.w * 0.05) / ((rt.abs() + rt) * 0.5 + 1e-8));
                self.k
            };
            self.peak += (a - self.peak) * k;
            self.smooth += (self.peak - self.smooth) * self.att;
            self.k = k * self.accel;
            let s = if self.smooth < 1e-15 { 0.0 } else { self.smooth };
            x[i] = s.min(1.0);
        }
    }
}

/// `Compander` ([re:621951]..[re:72e2d8]), compress mode (ratio 2): 75 Hz
/// side-chain high-pass, parabolic envelope, soft knee from −15 to +9 dB,
/// gain smoothed 1/256 per sample, soft-saturated output.
#[derive(Debug, Clone)]
pub struct Compander {
    sc: SideChain,
    env: EnvFollower,
    buf: Vec<f32>,
    gain: f32,
}

impl Compander {
    pub fn new() -> Compander {
        Compander { sc: SideChain::default(), env: EnvFollower::default(), buf: scratch(), gain: 1.0 }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.env.set_sample_rate(sr);
        self.sc.init(sr, 75.0);
        self.env.init(0.0, 0.1, 1.0, false);
    }

    pub fn compress(&mut self, x: &mut [f32]) {
        let n = x.len();
        let ratio = 2.0f32;
        self.buf.resize(n, 0.0);
        self.sc.process(x, &mut self.buf);
        self.env.process_parabolic(&mut self.buf);
        let m = self.buf[..n.saturating_sub(1)].iter().fold(None, |m: Option<f32>, v| Some(m.map_or(*v, |m| m.max(*v)))).unwrap_or(0.0);
        let db = 20.0 * m.abs().max(1e-6).log10();
        let g = if db <= -15.0 {
            1.0
        } else {
            let r = if db < -3.0 {
                let t = ((-3.0 - db) / 12.0).clamp(0.0, 1.0);
                (ratio * 0.5 - 1.0) * t + 1.0
            } else if db <= 9.0 {
                let t = ((9.0 - db) / 12.0).clamp(0.0, 1.0);
                (ratio - ratio * 0.5) * t + ratio * 0.5
            } else {
                ratio
            };
            let red = ((1.0 / r) * (db + 3.0) - (db + 3.0)).min(0.0);
            ((red / 20.0) * 3.321928).exp2()
        };
        let d = g - self.gain;
        for v in x.iter_mut() {
            self.gain += d * 0.00390625;
            *v = sat(self.gain * *v);
        }
    }
}

impl Default for Compander {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// The shared seed counter [re:5d62ba]: process-wide in the original;
    /// per thread here and reset with the engine ([`reset_seeds`]).
    static MS_SEED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static VRAND_SEED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn next_seed(c: &'static std::thread::LocalKey<std::cell::Cell<u32>>) -> u32 {
    c.with(|c| {
        let s = c.get().wrapping_mul(0xa825735).wrapping_add(0x35dc5a6b);
        c.set(s);
        s
    })
}

pub(crate) fn reset_seeds() {
    MS_SEED.with(|c| c.set(0));
    VRAND_SEED.with(|c| c.set(0));
}

/// [re:d3662a] ([re:a4c946]..[re:80360c]). The original seeds from
/// the clock; here the clock term is 0 so runs are reproducible.
#[derive(Debug, Clone, Copy)]
pub struct Rng {
    s: u32,
    count: u32,
    sum: f32,
    rows: [f32; 5],
}

impl Default for Rng {
    fn default() -> Self {
        Rng::new()
    }
}

impl Rng {
    pub fn new() -> Rng {
        Rng { s: next_seed(&MS_SEED), count: 1, sum: 0.0, rows: [0.0; 5] }
    }

    /// [re:4a18d4] (own `s_seed`, clock term 0).
    pub fn vrand() -> Rng {
        Rng::with_seed(next_seed(&VRAND_SEED))
    }

    /// Re-seed in place ([re:06ac8e]), keeping the pink rows.
    pub fn reseed(&mut self) {
        self.s = Rng::vrand().s;
    }

    /// One raw white value in [-0.5, 0.5).
    pub fn white_raw(&mut self) -> f32 {
        self.next()
    }

    pub fn with_seed(s: u32) -> Rng {
        Rng { s, count: 1, sum: 0.0, rows: [0.0; 5] }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        self.s = self.s.wrapping_mul(0xbb38433).wrapping_add(0x3619636b);
        (f32::from_bits(self.s >> 9 | 0x4000_0000) - 3.0) * 0.5
    }

    pub fn white(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() + 0.5) * (hi - lo)
    }

    /// [re:b968f7]: the same generator, returning
    /// `(white + sum)·0.2` (about ±0.5).
    pub fn pink_raw(&mut self) -> f32 {
        self.pink_core() * 0.2
    }

    /// Pink noise: Voss–McCartney over five rows with a ±2 running-sum limit.
    pub fn pink(&mut self, lo: f32, hi: f32) -> f32 {
        (hi - lo) * (self.pink_core() * 0.2 + 0.5) + lo
    }

    /// One Voss–McCartney step; returns `white + sum`.
    fn pink_core(&mut self) -> f32 {
        let c = self.count;
        let k = if c & 1 != 0 {
            0
        } else if c & 2 != 0 {
            1
        } else if c & 4 != 0 {
            2
        } else if c & 8 == 0 {
            4
        } else {
            3
        };
        let old = self.rows[k];
        let mut sum = self.sum;
        for _ in 0..10 {
            let w = self.next();
            let t = (w - old) + self.sum;
            self.rows[k] = w;
            if (-2.0..=2.0).contains(&t) {
                self.sum = t;
                sum = t;
                break;
            }
        }
        self.count = c.wrapping_add(1);
        let w = self.next();
        w + sum
    }
}

/// [re:cde049] ([re:279222]..[re:60399a]) + [re:f438c9]:
/// a per-block linear ramp, `out[i] = start + (i + 1)·step`.
#[derive(Debug, Clone)]
pub struct Interp {
    pub cur: f32,
    pub target: f32,
    buf: Vec<f32>,
}

impl Default for Interp {
    fn default() -> Self {
        Interp { cur: 0.0, target: 0.0, buf: scratch() }
    }
}

impl Interp {
    pub fn process(&mut self, n: usize) -> &[f32] {
        let step = (self.target - self.cur) * (1.0 / n as f32);
        self.buf.resize(n, 0.0);
        let start = self.cur;
        for (i, v) in self.buf.iter_mut().enumerate() {
            *v = (i + 1) as f32 * step + start;
        }
        if n > 0 {
            self.cur = self.buf[n - 1];
        }
        &self.buf
    }
}

/// `Gain` ([re:d073e0]..[re:d18c04]): linear gain ramped per sample over a block.
#[derive(Debug, Clone, Copy)]
pub struct Gain {
    pub cur: f32,
    pub target: f32,
}

impl Default for Gain {
    fn default() -> Self {
        Gain { cur: 1.0, target: 1.0 }
    }
}

impl Gain {
    /// Setup with `g` and [re:e1649c] (2g³).
    pub fn init(&mut self, g: f32) {
        let c = g * g * g;
        self.target = c + c;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        let step = (self.target - self.cur) * (1.0 / n as f32);
        if step.abs() < 1e-6 {
            let g = self.cur;
            l.iter_mut().for_each(|v| *v *= g);
            r.iter_mut().for_each(|v| *v *= g);
            return;
        }
        for i in 0..n {
            self.cur += step;
            l[i] *= self.cur;
            r[i] *= self.cur;
        }
    }
}

/// `DrumDynamics` ([re:01c332]..[re:06f54b]): transient/sustain shaper driven by
/// four envelope followers on a side-chain.
#[derive(Debug, Clone)]
pub struct DrumDynamics {
    env: [EnvFollower; 4],
    transient: f32,
    sustain: f32,
    buf: [Vec<f32>; 4],
    /// Side-chain copy for `process_mono`.
    sc: Vec<f32>,
}

impl DrumDynamics {
    pub fn new() -> DrumDynamics {
        DrumDynamics { env: [EnvFollower::default(); 4], transient: 0.0, sustain: 0.5, buf: std::array::from_fn(|_| scratch()), sc: scratch() }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        for e in &mut self.env {
            e.set_sample_rate(sr);
        }
        self.env[0].init(0.0, 0.755, 1.0, true);
        self.env[1].init(0.58, 0.755, 1.0, true);
        self.env[2].init(0.2, 0.55, 1.0, false);
        self.env[3].init(0.25, 0.45, 1.0, false);
    }

    /// Setup with `(attack, sustain, release)` (each 0..1, 0.5 = neutral).
    pub fn init(&mut self, attack: f32, sustain: f32, release: f32) {
        self.transient = attack + attack + -1.0;
        self.sustain = sustain + sustain + -1.0;
        let a = if 0.5 <= attack { (attack + -0.5 + attack + -0.5) * 0.15 } else { 0.0 };
        let r = a * 0.015 + 0.755;
        self.env[1].init(a + 0.58, r, 1.0, true);
        self.env[0].init(0.0, r, 1.0, true);
        self.env[2].init(0.2, release * 0.3 + 0.55, 1.0, false);
        self.env[3].init(0.25, release * 0.3 + 0.45, 1.0, false);
    }

    /// Gain curve for the side-chain block (`Process` [re:9291a2]/[re:06f54b]).
    fn gains(&mut self, sc: &[f32]) {
        let n = sc.len();
        for b in &mut self.buf {
            b.resize(n, 0.0);
        }
        let [b0, b1, b2, b3] = &mut self.buf;
        self.env[0].process(sc, b0);
        self.env[1].process(sc, b1);
        self.env[2].process(sc, b2);
        self.env[3].process_parabolic_from(sc, b3);
        let z = |v: f32| if v < 1e-5 { 0.0 } else { v };
        for i in 0..n {
            let a = z(b0[i]);
            let mut t = (a - z(b1[i])).max(0.0);
            if 0.0 < a {
                t /= a;
            }
            let p = z(b3[i]);
            let mut s = (p - z(b2[i])).max(0.0);
            if 0.0 < p {
                s /= p;
            }
            let s = (s / (t * 1.5 + 1.0)) * self.sustain;
            let g1 = (t * self.transient).max(-1.0) + 1.0;
            let g2 = s.max(-1.0) + 1.0;
            b0[i] = g2 * g2 * g2 * g1 * g1 * g1;
        }
    }

    /// Stereo: `out = in · gain(sc)`.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: &[f32]) {
        self.gains(sc);
        for i in 0..l.len() {
            let g = self.buf[0][i];
            l[i] *= g;
            r[i] *= g;
        }
    }

    /// Mono, side-chained by its own input.
    pub fn process_mono(&mut self, x: &mut [f32]) {
        // The side chain copy reuses its buffer between blocks.
        let mut sc = std::mem::take(&mut self.sc);
        sc.clear();
        sc.extend_from_slice(x);
        self.gains(&sc);
        self.sc = sc;
        for i in 0..x.len() {
            x[i] *= self.buf[0][i];
        }
    }
}

impl Default for DrumDynamics {
    fn default() -> Self {
        Self::new()
    }
}

/// [re:908df9] ([re:b1a120]..[re:fa470c]): ramped crossfade and output gain.
#[derive(Debug, Clone, Copy)]
pub struct WetDry {
    mix: [f32; 2],
    gain: [f32; 2],
}

impl Default for WetDry {
    fn default() -> Self {
        WetDry { mix: [0.75, 0.75], gain: [1.0, 1.0] }
    }
}

impl WetDry {
    pub fn init(&mut self, mix: f32, gain: f32) {
        self.mix[1] = mix;
        let c = gain * gain * gain;
        self.gain[1] = c + c;
    }

    /// `out = (dry·(1 − m) + m·wet)·g`, written into `wet`.
    pub fn process(&mut self, dry: [&[f32]; 2], wet: [&mut [f32]; 2]) {
        let n = dry[0].len();
        if n == 0 {
            return;
        }
        let inv = 1.0 / n as f32;
        let dm = self.mix[1] - self.mix[0];
        let dg = self.gain[1] - self.gain[0];
        let [wl, wr] = wet;
        for i in 0..n {
            self.gain[0] += dg * inv;
            self.mix[0] += dm * inv;
            let (m, g) = (self.mix[0], self.gain[0]);
            wl[i] = (dry[0][i] * (1.0 - m) + m * wl[i]) * g;
            wr[i] = ((1.0 - m) * dry[1][i] + wr[i] * m) * g;
        }
    }
}

/// `StereoLimiter` ([re:ee7b0d]..[re:95d1f9]): per channel, an instant-attack
/// peak follower with a 600 ms release; above 1 the signal is divided by
/// the envelope and soft-saturated.
#[derive(Debug, Clone, Copy)]
pub struct StereoLimiter {
    env: [f32; 2],
    alt: [f32; 2],
    rel: f32,
}

impl Default for StereoLimiter {
    fn default() -> Self {
        StereoLimiter { env: [0.0; 2], alt: [DENORMAL; 2], rel: 0.0 }
    }
}

impl StereoLimiter {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.rel = 0.01f32.powf(1.0 / (sr * 600.0 * 0.001));
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (c, x) in [l, r].into_iter().enumerate() {
            let d = self.alt[c];
            self.alt[c] = -d;
            self.env[c] -= d;
            for v in x.iter_mut() {
                let a = v.abs();
                self.env[c] = if self.env[c] < a { a } else { a + (self.env[c] - a) * self.rel };
                let e = if self.env[c] < 0.0039810715 { 0.0 } else { self.env[c] };
                if 1.0 <= e {
                    *v = sat(*v / e);
                }
            }
        }
    }
}

/// [re:e2af9b] ([re:e1855a]..[re:442be6]): one or two
/// [re:f4d05c] pairs; in dual mode the pairs sit `width/2` above and
/// below the cutoff (band-pass when set to low-pass + high-pass).
#[derive(Debug, Clone, Copy)]
pub struct DualPole {
    f: [StereoIir; 4],
    pub dual: bool,
    steep: [bool; 2],
}

impl Default for DualPole {
    fn default() -> Self {
        DualPole { f: [StereoIir::default(); 4], dual: true, steep: [false; 2] }
    }
}

impl DualPole {
    pub fn set_sample_rate(&mut self, sr: f32) {
        for f in &mut self.f {
            f.set_sample_rate(sr);
        }
    }

    pub fn set_mode(&mut self, a: u32, b: u32) {
        self.f[0].set_mode(a);
        self.f[1].set_mode(a);
        self.f[2].set_mode(b);
        self.f[3].set_mode(b);
    }

    /// Setup with `(f01, q, width, steep)`.
    pub fn init(&mut self, f: f32, q: f32, width: f32, steep: bool) {
        let q = if steep { 0.8 } else { 1.0 } * q;
        if !self.dual {
            self.f[0].set_01(f, q);
            self.f[1].set_01(f, q);
            self.steep[0] = steep;
        } else {
            let w = width.min(0.75).max(0.025);
            let hi = (w * 0.5 + f).min(1.0);
            let lo = (f - w * 0.5).max(0.0);
            self.f[0].set_01(hi, q);
            self.f[1].set_01(hi, q);
            self.steep[0] = steep;
            self.f[2].set_01(lo, q);
            self.f[3].set_01(lo, q);
            self.steep[1] = steep;
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.f[0].process(l, r);
        if self.steep[0] {
            self.f[1].process(l, r);
        }
        if self.dual {
            self.f[2].process(l, r);
            if self.steep[1] {
                self.f[3].process(l, r);
            }
        }
    }
}

/// `0x322bcc77` (≈1e-8): the flipping denormal offset of [re:f2c879] voices.
pub const ALT: u32 = 0x322bcc77;

/// Stereo biquad section of the [re:f2c879] RBJ-style filters (Dirty DAC,
/// Enhancer): `x1 = x − alt` (direct form I with a flipping denormal
/// offset on the input history).
#[derive(Clone, Copy, Default)]
pub struct PolySect {
    x1: [f32; 2],
    x2: [f32; 2],
    y1: [f32; 2],
    y2: [f32; 2],
    alt: f32,
}

impl PolySect {
    pub fn new() -> PolySect {
        PolySect { alt: f32::from_bits(ALT), ..Default::default() }
    }

    /// Voice `Reset`: clears the history, keeps the alternator phase.
    pub fn clear(&mut self) {
        *self = PolySect { alt: self.alt, ..Default::default() };
    }

    #[inline]
    pub fn run(&mut self, c: &[f32; 5], x: [f32; 2]) -> [f32; 2] {
        let [b0, b1, b2, a1, a2] = *c;
        let mut y = [0.0; 2];
        for ch in 0..2 {
            y[ch] = ((self.x2[ch] * b2 + b1 * self.x1[ch] + x[ch] * b0) - a1 * self.y1[ch]) - self.y2[ch] * a2;
        }
        let a = self.alt;
        self.alt = -a;
        for ch in 0..2 {
            self.x2[ch] = self.x1[ch];
            self.y2[ch] = self.y1[ch];
            self.x1[ch] = x[ch] - a;
            self.y1[ch] = y[ch];
        }
        y
    }
}

/// Polynomial tan/cos approximation shared by the LP and HP designs
/// (Q 0.707): returns (w, c).
pub fn design_poly(f: f32) -> (f32, f32) {
    let p = 0.5 - (((f + -0.001).abs() + 0.45099998) - (f + -0.45).abs()) * 0.5;
    let a = p.abs();
    let s = p * p;
    let c = -1.0 - (((a * 71.34317 + -124.85512) * s + a * 12.23143 + 62.747192) * s + a * 0.21205398 + -19.747784) * s;
    let w = (((((a * 55.079624 + -110.15925) * s + a * 11.302022 + 79.43631) * s + a * 0.21989952 + -41.350807) * s + 6.2831855) * p) / 1.4139404;
    (w, c)
}

pub fn lowpass_poly(f: f32) -> [f32; 5] {
    let (w, c) = design_poly(f);
    let n = 1.0 / (w + 1.0);
    let g = (1.0 - c) * n;
    [g * 0.5, g, g * 0.5, c * -2.0 * n, n * (1.0 - w)]
}

pub fn highpass_poly(f: f32) -> [f32; 5] {
    let (w, c) = design_poly(f);
    let n = 1.0 / (w + 1.0);
    let g = (c + 1.0) * n;
    [g * 0.5, -g, g * 0.5, c * -2.0 * n, (1.0 - w) * n]
}
