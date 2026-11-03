//! Modulator signal generators (`LFO`, [re:0b7918], `SampleAndHold`,
//! [re:58b842], [re:fcd9b0], `MathModulator`, [re:fe6713],
//! `BouncingBall`) and helpers. All outputs are unipolar `0..1` per sample.
//! See the dreist-re modulation notes.

use crate::analog::sin_tab;
use std::sync::OnceLock;

/// [re:d3662a] ([re:a4c946]): 32-bit LCG; `Seed` mixes the clock,
/// which the port replaces with a caller-supplied seed.
#[derive(Debug, Clone, Copy)]
pub struct Randomiser {
    state: u32,
}

impl Randomiser {
    pub fn new(seed: u32) -> Randomiser {
        Randomiser { state: seed.wrapping_mul(0x0a82_5735).wrapping_add(0x35dc_5a6b) }
    }

    /// `Seed` [re:45d0e3]: `state = (seed + clock)·[re:a1a935] + 0x35dc5a6b`.
    pub fn reseed(&mut self, salt: u32) {
        self.state = self.state.wrapping_add(salt).wrapping_mul(0x0a82_5735).wrapping_add(0x35dc_5a6b);
    }

    /// `GetWhite(lo, hi)` [re:01c8cc].
    pub fn white(&mut self, lo: f32, hi: f32) -> f32 {
        self.state = self.state.wrapping_mul(0x0bb3_8433).wrapping_add(0x3619_636b);
        let f = f32::from_bits((self.state >> 9) | 0x4000_0000) + -3.0;
        (f * 0.5 + 0.5) * (hi - lo) + lo
    }
}

/// [re:177917] (5122 entries): `asin(4t)/π` of the folded triangle.
pub(crate) fn asin_tab() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..0x1402)
            .map(|k| {
                let x = k as f32 * 0.00048828125 - 0.5;
                let x = x - x.floor();
                let x = if x < 0.75 { x } else { x - 1.0 };
                let t = if x < 0.25 { x } else { 0.5 - x };
                (t * 4.0).asin() * 0.31830987
            })
            .collect()
    })
}

/// Linear read of a 2048-per-cycle table starting at `base` ([re:58b121]).
fn tab(t: &[f32], base: usize, p: f64) -> f32 {
    let i = ((p as f32) * 2048.0).floor() as i32;
    let k = (base as i32 + i) as usize;
    t[k] + (t[k + 1] - t[k]) * ((p as f32) * 2048.0 - i as f32)
}

/// LFO table shapes at phase `p` (0..1), centred on 0 (±0.5): 0 sine,
/// 1 cosine, 6 arcsine, 7 arccosine (the [re:58b121] tables).
pub fn lfo_table(shape: u32, p: f64) -> f32 {
    let p = p.clamp(0.0, 0.999_999);
    match shape {
        0 => tab(sin_tab(), 1024, p),
        1 => tab(sin_tab(), 1536, p),
        6 => tab(asin_tab(), 1024, p),
        _ => tab(asin_tab(), 1536, p),
    }
}

fn wrap1(p: f64) -> f64 {
    if p < 1.0 { p } else { p - 1.0 }
}

/// [re:df8a80]: free `250·r⁵ + 1e-5` Hz, or
/// synced to one of 50 note divisions of a 4/4 bar.
pub fn frequency_hz(rate: f32, sync: f32, bpm: f32) -> f32 {
    if sync < 0.5 {
        return rate * rate * rate * rate * rate * 249.99998 + 1e-5;
    }
    const DIV: [f32; 51] = [
        0.00390625, 0.005859375, 0.0026041667, 0.0078125, 0.01171875, 0.0052083335, 0.015625, 0.0234375, 0.010416667, 0.03125, 0.046875, 0.020833334, 0.0625, 0.09375, 0.041666668, 0.125, 0.1875,
        0.083333336, 0.25, 0.375, 0.16666667, 0.5, 0.75, 0.33333334, 1.0, 1.5, 0.6666667, 2.0, 3.0, 1.3333334, 4.0, 6.0, 2.6666667, 8.0, 12.0, 5.3333335, 16.0, 24.0, 10.666667, 32.0, 48.0, 21.333334,
        64.0, 96.0, 42.666668, 128.0, 192.0, 85.333336, 256.0, 384.0, 170.66667,
    ];
    let i = ((rate * 50.0).floor() as i32).clamp(0, 50) as usize;
    1.0 / ((4.0 / DIV[i]) * (60.0 / bpm))
}

/// `LFO::LFOShape` (`CreateLFOShapeMap`): Sin, Cos, Triangle, SawUp, SawDown,
/// Square, ArcSin, ArcCos, Noise; selected by `lround(Shape·8)`.
#[derive(Debug, Clone)]
pub struct Lfo {
    pub phase: f64,
    inv_sr: f32,
    noise: Randomiser,
}

impl Lfo {
    pub fn new(sample_rate: f32, seed: u32) -> Lfo {
        Lfo { phase: 0.0, inv_sr: 1.0 / sample_rate, noise: Randomiser::new(seed) }
    }

    /// [re:447294].
    pub fn reset_phase(&mut self, phase: f64) {
        self.phase = phase;
    }

    /// [re:693224].
    pub fn process(&mut self, out: &mut [f32], hz: f32, shape: u32) {
        let inc = (hz * self.inv_sr) as f64;
        let s = sin_tab();
        let a = asin_tab();
        for o in out.iter_mut() {
            let p = self.phase;
            *o = match shape {
                0 => tab(s, 1024, p) + 0.5,
                1 => tab(s, 1536, p) + 0.5,
                2 => {
                    let t = if p < 0.25 {
                        p + p
                    } else if p < 0.75 {
                        1.0 - (p + p)
                    } else {
                        p + p - 2.0
                    };
                    t as f32 + 0.5
                }
                3 => (p - 0.5) as f32 + 0.5,
                4 => (0.5 - p) as f32 + 0.5,
                5 => {
                    if p < 0.5 {
                        1.0
                    } else {
                        0.0
                    }
                }
                6 => tab(a, 1024, p) + 0.5,
                7 => tab(a, 1536, p) + 0.5,
                // Noise: the original draws from an MT19937; any uniform source matches in distribution.
                _ => self.noise.white(0.0, 1.0),
            };
            self.phase = wrap1(self.phase + inc);
        }
    }
}

/// Morphing LFO ([re:42210b]): saw → square → sine → triangle →
/// arcsine by Asymmetry, with Swing skewing the two half-cycle rates.
#[derive(Debug, Clone)]
pub struct MorphLfo {
    pub phase: f64,
    inv_sr: f32,
}

impl MorphLfo {
    pub fn new(sample_rate: f32) -> MorphLfo {
        MorphLfo { phase: 0.0, inv_sr: 1.0 / sample_rate }
    }

    pub fn reset_phase(&mut self, phase: f32) {
        self.phase = phase as f64;
    }

    pub fn process(&mut self, out: &mut [f32], hz: f32, asym: f32, swing: f32) {
        let d = asym as f64 * 4.0;
        // Weights: saw, square, sine, arcsine, triangle.
        let (mut w_saw, mut w_sq, mut w_sin, mut w_asin, mut w_tri) = (0.0, 0.0, 0.0, 0.0, 0.0);
        if d < 1.0 {
            w_saw = 1.0 - d;
            w_sq = d;
        } else if d < 2.0 {
            w_sq = 2.0 - d;
            w_sin = d - 1.0;
        } else if d < 3.0 {
            w_sin = 3.0 - d;
            w_tri = d - 2.0;
        } else {
            w_tri = 4.0 - d;
            w_asin = d - 3.0;
        }
        let s = sin_tab();
        let a = asin_tab();
        let sw = swing as f64 * 1.5;
        let skew = (sw - 0.75).abs() >= 1e-6;
        let step = self.inv_sr as f64 * hz as f64;
        for o in out.iter_mut() {
            let inc = if skew { (1.0 / if self.phase < 0.5 { sw + 0.25 } else { 1.75 - sw }) * step } else { step };
            let mut p = self.phase + inc;
            if 1.0 < p {
                p -= 1.0;
            }
            self.phase = p;
            let tri = if p < 0.25 {
                p + p
            } else if p < 0.75 {
                1.0 - (p + p)
            } else {
                p + p - 2.0
            };
            let sq = if p < 0.5 { 0.5 } else { -0.5 };
            let v = w_saw as f32 * (0.5 - p) as f32 + w_tri as f32 * tri as f32 + tab(a, 1024, p) * w_asin as f32 + tab(s, 1024, p) * w_sin as f32 + sq * w_sq as f32 + 0.5;
            *o = v.clamp(0.0, 1.0);
        }
    }
}

/// `SampleAndHold` [re:f7b9ad] / [re:53da28]: a new random target every period,
/// approached along `ramp^(4·slew+1)` and mixed with the held value by Slew.
#[derive(Debug, Clone)]
pub struct SampleAndHold {
    sr: f32,
    rng: Randomiser,
    count: i32,
    period: i32,
    prev: f32,
    next: f32,
    inc: f32,
    lo: f32,
    range: f32,
    ramp: f32,
    up: bool,
}

impl SampleAndHold {
    pub fn new(sample_rate: f32, seed: u32) -> SampleAndHold {
        SampleAndHold { sr: sample_rate, rng: Randomiser::new(seed), count: 0, period: 1, prev: 0.0, next: 0.0, inc: 0.0, lo: 0.0, range: 0.0, ramp: 0.0, up: false }
    }

    fn new_target(&mut self) {
        self.rng.reseed(0x9e37);
        self.count = 0;
        self.prev = self.next;
        self.next = self.rng.white(0.0, 1.0);
        self.range = (self.next - self.prev).abs();
        self.up = self.prev < self.next;
        self.lo = if self.up { self.prev } else { self.next };
        let step = 1.0 / self.period as f32;
        if self.up {
            self.ramp = 0.0;
            self.inc = step;
        } else {
            self.ramp = 1.0;
            self.inc = -step;
        }
    }

    pub fn reset(&mut self, phase: f32) {
        self.rng.reseed(0x51ed);
        self.next = self.rng.white(0.0, 1.0);
        self.new_target();
        self.count = (self.period as f32 * phase).floor() as i32;
    }

    pub fn process(&mut self, out: &mut [f32], hz: f32, slew: f32) {
        self.period = (((1.0 / hz) * self.sr) as i32).max(1);
        let step = 1.0 / self.period as f32;
        self.inc = if self.up { step } else { -step };
        for o in out.iter_mut() {
            let r = self.ramp + self.inc;
            self.ramp = r;
            let k = if r < 1.0 { r } else { 1.0 }.max(0.0);
            let y = k.powf(slew * 4.0 + 1.0);
            *o = (y * self.range + self.lo) * slew + (1.0 - slew) * self.prev;
            self.count += 1;
            if self.period <= self.count {
                self.new_target();
            }
        }
    }
}

/// [re:eb61c1] / [re:a19c7e] (times up to 5.5 s, curve exponent
/// `1.5c + 0.25` below 0.5, `6c − 2` above).
#[derive(Debug, Clone)]
pub struct AdEnvelope {
    sr: f32,
    level: f32,
    peak: f32,
    active: bool,
    attacking: bool,
}

fn ad_curve(c: f32) -> f32 {
    if 0.5 < c { c * 6.0 - 2.0 } else { c * 1.5 + 0.25 }
}

impl AdEnvelope {
    pub fn new(sample_rate: f32) -> AdEnvelope {
        AdEnvelope { sr: sample_rate, level: 0.0, peak: 1.0, active: false, attacking: false }
    }

    pub fn note_on(&mut self, peak: f32) {
        self.level = 0.0;
        self.peak = peak;
        self.active = peak.abs() >= 1e-6;
        self.attacking = true;
    }

    pub fn process(&mut self, out: &mut [f32], attack: f32, decay: f32, attack_curve: f32, decay_curve: f32) {
        if !self.active {
            out.fill(0.0);
            return;
        }
        let rate = |t: f32| if t.abs() >= 1e-6 { self.peak / (t * 5.5 * self.sr) } else { 0.0 };
        let mut up = rate(attack);
        let mut down = rate(decay);
        if up.abs() <= 1e-6 {
            up = self.peak;
        }
        if down.abs() <= 1e-6 {
            down = self.peak;
        }
        for o in out.iter_mut() {
            if !self.active {
                *o = 0.0;
                continue;
            }
            if self.attacking {
                self.level += up;
                if self.peak <= self.level {
                    self.level = self.peak;
                    self.attacking = false;
                    *o = self.peak;
                    continue;
                }
                *o = self.level.powf(ad_curve(attack_curve));
            } else {
                self.level -= down;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.active = false;
                    *o = 0.0;
                    continue;
                }
                *o = self.level.powf(ad_curve(decay_curve));
            }
        }
    }
}

/// [re:d042b1]: one-pole follower of `|x|` with separate
/// rise/fall times in seconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Slew {
    y: f32,
}

impl Slew {
    pub fn process(&mut self, buf: &mut [f32], up: f32, down: f32, sr: f32) {
        let cu = (-1.0 / (up.max(1e-8) * sr)).exp();
        let cd = (-1.0 / (down.max(1e-8) * sr)).exp();
        for x in buf.iter_mut() {
            let a = x.abs();
            let c = if self.y < a { cu } else { cd };
            self.y = c * (self.y - a) + a;
            *x = self.y;
        }
    }
}

/// [re:e6bada]..: 16-step sequence (Mini Sequencer).
#[derive(Debug, Clone)]
pub struct MiniSequencer {
    pub sr: f32,
    step: u32,
    counter: u32,
    pub running: bool,
    pub values: [f32; 16],
    rng: Randomiser,
    slew: Slew,
    trig: bool,
    reverse: bool,
    drunk: bool,
    last: u32,
    samples_per_step: u32,
}

impl MiniSequencer {
    pub fn new(sample_rate: f32, seed: u32) -> MiniSequencer {
        MiniSequencer {
            sr: sample_rate,
            step: 0,
            counter: 0,
            running: false,
            values: [0.0; 16],
            rng: Randomiser::new(seed),
            slew: Slew::default(),
            trig: false,
            reverse: false,
            drunk: false,
            last: 15,
            samples_per_step: 1,
        }
    }

    fn up(&self, last: u32) -> u32 {
        if self.step < last { self.step + 1 } else { 0 }
    }

    fn down(&self, last: u32) -> u32 {
        if self.step == 0 { last } else { self.step - 1 }
    }

    /// Drunk walk: forward with probability `1 − p` where p = 0.4 forward / 0.6 reverse.
    fn drunk_step(&mut self, last: u32, threshold: f32) -> u32 {
        let w = self.rng.white(0.0, 1.0);
        if threshold <= w { self.up(last) } else { self.down(last) }
    }

    /// `Reset(start)` [re:0a0de3]: in trigger mode advance one step, else jump to `start`.
    pub fn reset(&mut self, start: u32) {
        let last = self.last;
        if self.trig && self.samples_per_step != 0 {
            self.step = match (self.reverse, self.drunk) {
                (false, true) => self.drunk_step(last, 0.4),
                (false, false) => self.up(last),
                (true, true) => self.drunk_step(last, 0.6),
                (true, false) => self.down(last),
            };
        } else {
            self.step = start;
        }
        self.counter = 0;
    }

    /// The step being played (+0x20, [re:ee0d71]
    /// [re:0d75ba]).
    pub fn current_step(&self) -> u32 {
        self.step
    }

    /// `ResetFromHostOrPattern`.
    pub fn set_step(&mut self, step: u32) {
        self.step = step;
        self.counter = 0;
    }

    /// `Advance` [re:d13eae].
    fn advance(&mut self) {
        let last = self.last;
        if !self.reverse {
            self.counter += 1;
            if self.counter < self.samples_per_step {
                return;
            }
            self.counter = 0;
            self.step = if self.drunk { self.drunk_step(last, 0.6) } else { self.up(last) };
        } else {
            if self.counter != 0 {
                self.counter -= 1;
                return;
            }
            self.counter = self.samples_per_step.saturating_sub(1);
            self.step = if self.drunk { self.drunk_step(last, 0.4) } else { self.down(last) };
        }
    }

    /// `Process` [re:ca4345].
    #[allow(clippy::too_many_arguments)]
    pub fn process(&mut self, out: &mut [f32], samples_per_step: u32, drunk: bool, reverse: bool, trig: bool, last: u32, slew: f32) {
        self.trig = trig;
        self.reverse = reverse;
        self.drunk = drunk;
        self.last = last;
        self.samples_per_step = samples_per_step;
        if !(trig || self.running) {
            return;
        }
        for o in out.iter_mut() {
            *o = self.values[(self.step as usize).min(15)];
            if !self.trig {
                self.advance();
            }
        }
        let s = slew + slew;
        if s.abs() >= 1e-6 {
            self.slew.process(out, s, s, self.sr);
        }
    }
}

/// `MathModulator` [re:52138f]..: combines two transmod sources once per block.
#[derive(Debug, Clone, Default)]
pub struct MathModulator {
    held: f32,
    prev_b: f32,
    flip: bool,
}

impl MathModulator {
    pub fn reset(&mut self) {
        self.held = 0.0;
        self.flip = true;
    }

    /// `Process(a, b, op)` [re:f86080] (asm-checked): returns the unipolar output.
    pub fn op(&mut self, a: f32, b: f32, op: u32) -> f32 {
        let aa = a + a - 1.0;
        let bb = b + b - 1.0;
        let tp = 6.2831855f32;
        let div = |x: f32| x / (b + b + -0.999999);
        let sign = |c: bool| if c { 1.0f32 } else { -1.0 };
        let r = match op {
            0 => aa + bb,
            1 => aa - bb,
            2 => return b * a,
            3 => aa * b,
            4 => div(aa),
            5 => (bb + aa) * 0.5,
            6 => a + -0.5 + bb,
            7 => b + -0.75 + a * 0.5,
            8 => (b * 4.0 + -2.0 + aa) * 0.3,
            9 => -(bb * aa),
            10 => aa * 10.0f32.powf(bb),
            11 => (aa + bb).abs(),
            12 => (aa - bb).abs(),
            13 => aa.min(bb),
            14 => aa.max(bb),
            15 => {
                let q = b * b * 30.0 + 2.0;
                let v = ((a * q).floor() as i32) as f32 / q;
                return v;
            }
            16 => bb / (2.000001 - (a + a)),
            17 => (aa + bb) * (aa + bb),
            18 => ((aa + bb) * tp).sin(),
            19 => ((aa - bb) * tp).sin(),
            20 => ((aa * bb) * tp).sin(),
            21 => (div(aa) * tp).sin(),
            22 => ((aa + bb) * tp).cos(),
            23 => ((aa - bb) * tp).cos(),
            24 => ((aa * bb) * tp).cos(),
            25 => (div(aa) * tp).cos(),
            26 => {
                if 0.0 <= aa {
                    sign(0.0 <= bb)
                } else {
                    -1.0
                }
            }
            27 => {
                if 0.0 <= aa {
                    1.0
                } else {
                    sign(0.0 <= bb)
                }
            }
            28 => {
                if 0.0 <= aa && bb < 0.0 {
                    1.0
                } else if aa < 0.0 {
                    sign(0.0 <= bb)
                } else {
                    -1.0
                }
            }
            29 => {
                if self.flip {
                    self.prev_b = 1.0 - self.prev_b;
                    self.flip = false;
                }
                return self.prev_b;
            }
            30 => return 1.0 - a,
            31 | 32 => {
                let edge = if op == 31 { b >= 0.5 && self.prev_b < 0.5 } else { b < 0.5 && self.prev_b >= 0.5 };
                self.prev_b = b;
                if edge {
                    self.held = aa;
                }
                self.held
            }
            33 => {
                if 0.0 <= bb {
                    self.held = aa;
                }
                self.held
            }
            34 => {
                if bb < 0.0 {
                    self.held = aa;
                }
                self.held
            }
            _ => 0.0,
        };

        (r + 1.0) * 0.5
    }

    /// `Process(buf, n, a, b, op)` [re:92d7f4]: one value per block, clamped.
    pub fn process(&mut self, out: &mut [f32], a: f32, b: f32, op: u32) {
        let v = self.op(a, b, op).clamp(0.0, 1.0);
        out.fill(v);
    }
}

/// `BouncingBall` [re:55e546] / [re:207bca]: falling height that loses energy on
/// each bounce (`Gravity` sets the loss, `Speed` the fall time).
#[derive(Debug, Clone)]
pub struct BouncingBall {
    sr: f32,
    active: bool,
    height: f32,
    level: f32,
}

impl BouncingBall {
    pub fn new(sample_rate: f32) -> BouncingBall {
        BouncingBall { sr: sample_rate, active: false, height: 0.0, level: 0.0 }
    }

    pub fn note_on(&mut self, v: f32) {
        self.active = true;
        self.height = v;
        self.level = v;
    }

    pub fn process(&mut self, out: &mut [f32], gravity: f32, speed: f32) {
        out.fill(0.0);
        if !self.active {
            return;
        }
        let t = speed * 5.4 + 0.1;
        let mut step = self.height / (self.sr * t);
        let mut i = 0;
        while i < out.len() {
            self.level -= step;
            out[i] = self.level;
            if self.level <= 0.0 || self.level.abs() <= 1e-6 {
                out[i] = 0.0;
                self.height = self.height + gravity * -0.49 + -0.01;
                if self.height < 0.01 {
                    self.active = false;
                }
                self.level = self.height;
                step = self.height / (self.sr * t);
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_sine_and_square() {
        let mut l = Lfo::new(1000.0, 1);
        let mut out = [0.0; 1000];
        l.process(&mut out, 1.0, 0);
        assert!((out[250] - 1.0).abs() < 1e-3 && (out[750] - 0.0).abs() < 1e-3 && (out[0] - 0.5).abs() < 1e-3);
        let mut l = Lfo::new(1000.0, 1);
        l.process(&mut out, 1.0, 5);
        assert_eq!((out[100], out[600]), (1.0, 0.0));
    }

    #[test]
    fn synced_rate() {
        // Index 24 = one beat (quarter note) → bpm/60 Hz... DIV[24]=1 bar fraction 1 → 1/(4·60/bpm).
        assert!((frequency_hz(24.0 / 50.0, 1.0, 120.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn math_ops() {
        let mut m = MathModulator::default();
        assert!((m.op(0.75, 0.75, 0) - 1.0).abs() < 1e-6);
        assert!((m.op(0.6, 0.5, 2) - 0.3).abs() < 1e-6);
        assert_eq!(m.op(1.0, 0.0, 26), 0.0);
        assert_eq!(m.op(1.0, 0.0, 27), 1.0);
    }

    #[test]
    fn ad_envelope_rises_and_falls() {
        let mut e = AdEnvelope::new(1000.0);
        e.note_on(1.0);
        let mut out = vec![0.0; 2000];
        e.process(&mut out, 0.1, 0.1, 0.5, 0.5);
        let peak = out.iter().cloned().fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
        assert_eq!(*out.last().unwrap(), 0.0);
    }
}
