//! [re:a42e25] (Bloom; port setup [re:59ad5a], stream setup
//! [re:d41488], parameter update [re:c67536], processing [re:c44d47],
//! feedback modelling [re:a88d43]): a modelled stereo delay sharing one
//! `DelayBuffer` between three front ends — Digital (bit/sample-rate
//! reduction), BBD (compander + clock decimation) and Tape (saturation,
//! shelving, wow and hiss). Each model also runs a buffer-less copy of
//! itself inside the feedback path.
//!
//! Ping Pong, Freeze and Direction (Reverse) drive the buffer's ping-pong
//! push and frozen / backwards pulls.

use super::blocks::{Compander, DcFilter, Interp, Rng, StereoIir, StereoSslShelf, ms_spacing};
use super::math::pow2_at_least;
use super::math::{cubic, sat};
use crate::fx::convert::{bpm_from_text, time_beat_to_units};
use crate::fx::{Ctx, Node, Port, ports, ports_from, scratch};

fn ms_to_samples(ms: f32, sr: f32) -> f32 {
    ms * 0.001 * sr
}

/// `DelayBuffer` ([re:8760a4]..[re:a03e74]): per-channel write / read heads over
/// power-of-two rings, gliding delay times, and the four pulls (forward,
/// backwards, frozen forward, frozen backwards) plus the ping-pong push.
struct DelayBuffer {
    sr: f32,
    buf: [Vec<f32>; 2],
    mask: u32,
    /// Write and read heads.
    w: [u32; 2],
    r: [u32; 2],
    /// Target and current delay (samples).
    tgt: [f64; 2],
    cur: [f64; 2],
    /// Glide speeds (1/samples) when lengthening/shortening.
    k_up: f32,
    k_down: f32,
    /// Backwards countdowns, read gains, gain fading in.
    count: [i32; 2],
    gain: [f32; 2],
    ramp: [bool; 2],
    /// Frozen loop starts and ends.
    frz_start: [i32; 2],
    frz_end: [i32; 2],
    /// Frozen, ping-pong, backwards.
    frozen: bool,
    ping_pong: bool,
    backwards: bool,
    iir: StereoIir,
    fb_g: [f32; 2],
    xs: [f32; 2],
    phase: f32,
    width: [f32; 2],
    out: [Vec<f32>; 2],
    fb: [Vec<f32>; 2],
}

impl DelayBuffer {
    fn new() -> DelayBuffer {
        DelayBuffer {
            sr: 44100.0,
            buf: [Vec::new(), Vec::new()],
            mask: 0,
            w: [0; 2],
            r: [0; 2],
            tgt: [0.0; 2],
            cur: [0.0; 2],
            k_up: 0.03125,
            k_down: 0.03125,
            count: [0; 2],
            gain: [1.0; 2],
            ramp: [false; 2],
            frz_start: [0; 2],
            frz_end: [0; 2],
            frozen: false,
            ping_pong: false,
            backwards: false,
            iir: StereoIir::default(),
            fb_g: [1.0, 1.0],
            xs: [0.0, 0.0],
            phase: 1.0,
            width: [0.0, 0.0],
            out: [scratch(), scratch()],
            fb: [scratch(), scratch()],
        }
    }

    fn size(&self) -> i32 {
        self.buf[0].len() as i32
    }

    /// Backwards: read just ahead of the write heads and count a delay down.
    fn restart_backwards(&mut self) {
        for c in 0..2 {
            self.count[c] = self.tgt[c] as i32;
            self.r[c] = self.w[c].wrapping_add(1) & self.mask;
        }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        let d = ms_to_samples(5.0, sr) as f64;
        self.cur = [d, d];
        let size = pow2_at_least(ms_to_samples(4001.0, sr).ceil() as i64);
        if self.buf[0].len() != size {
            self.buf = [vec![0.0; size], vec![0.0; size]];
            self.mask = size as u32 - 1;
        }
        self.iir.set_sample_rate(sr);
        self.iir.set_mode(0);
        self.iir.set_hz(0.01, 0.0);
        self.clear();
    }

    /// Delay clear [re:5136c2] (a frozen buffer is kept).
    fn clear(&mut self) {
        if self.frozen {
            return;
        }
        for b in &mut self.buf {
            b.fill(0.0);
        }
        if self.backwards {
            self.restart_backwards();
        } else {
            self.r = [0; 2];
            self.w = [0; 2];
        }
    }

    /// `SetModellingParameters(ms_up, ms_down, hz)`.
    fn set_modelling(&mut self, up: u32, down: u32, hz: f32) {
        let a = ms_to_samples(up as f32, self.sr).max(32.0);
        let b = ms_to_samples(down as f32, self.sr).max(32.0);
        self.k_up = 1.0 / a;
        self.k_down = 1.0 / b;
        self.iir.set_hz(hz, 0.0);
    }

    /// Setup [re:63ec3d].
    #[allow(clippy::too_many_arguments)]
    fn init(&mut self, l_ms: f32, r_ms: f32, freeze: bool, reverse: bool, fb: f32, phase: bool, xfb: f32, ping_pong: bool, width: f32) {
        let size = self.size();
        let old = self.tgt;
        for (c, ms) in [l_ms, r_ms].into_iter().enumerate() {
            let s = ms_to_samples(ms, self.sr) as f64;
            self.tgt[c] = s.max(0.0).min(size as f64);
        }
        if (old[0] - self.tgt[0]).abs() >= 1e-6 || (old[1] - self.tgt[1]).abs() >= 1e-6 || self.backwards != reverse {
            if reverse {
                self.restart_backwards();
            } else {
                self.r = self.w;
            }
            self.backwards = reverse;
        }
        // Freezing loops the span one delay behind the read heads; unfreezing
        // reads at the write heads again.
        let changed = self.frozen != freeze;
        let wrap = |x: i32| if x < 0 { x - 1 + size } else { x };
        if changed {
            self.frz_end[0] = self.r[0] as i32;
        }
        self.frz_start[0] = wrap((self.frz_end[0] as f64 - self.cur[0]) as i32);
        if changed {
            self.r[0] = if freeze { self.frz_start[0] as u32 } else { self.w[0] };
            self.frz_end[1] = self.r[1] as i32;
        }
        self.frz_start[1] = wrap((self.frz_end[1] as f64 - self.cur[1]) as i32);
        if changed {
            self.r[1] = if freeze { self.frz_start[1] as u32 } else { self.w[1] };
        }
        self.frozen = freeze;
        self.xs[1] = 1.0 - xfb;
        self.fb_g[1] = fb * fb * 1.5;
        self.phase = if phase { 1.0 } else { -1.0 };
        self.width[1] = width;
        self.ping_pong = ping_pong;
    }

    /// The read gains' clamp to 0..1 (`> 1` also stops the fade-in).
    fn clamp_gain(&mut self, c: usize) -> f32 {
        if self.gain[c] > 1.0 {
            self.gain[c] = 1.0;
            self.ramp[c] = false;
        }
        self.gain[c] = if self.gain[c] > 0.0 { self.gain[c].min(1.0) } else { 0.0 };
        self.gain[c]
    }

    /// Buffer read [re:5fbb9a].
    fn pull(&mut self, n: usize) {
        let mut rate = [0.0f64; 2];
        for c in 0..2 {
            let k = if self.cur[c] < self.tgt[c] { self.k_up } else { self.k_down };
            rate[c] = (k as f64 * (self.tgt[c] - self.cur[c]) * 1000.0).ceil() * 0.0010000000474974513;
            if rate[c].abs() < 1e-6 {
                self.cur[c] = self.tgt[c];
                rate[c] = 0.0;
            }
        }
        let mask = self.mask;
        let size = self.size();
        for i in 0..n {
            for c in 0..2 {
                let y = match (self.frozen, self.backwards) {
                    // PullFromBufferNotFrozenNotBackwards [re:ff9da7].
                    (false, false) => {
                        self.r[c] = self.r[c].wrapping_add(1) & mask;
                        self.w[c] = self.w[c].wrapping_add(1) & mask;
                        self.cur[c] += rate[c];
                        cubic(&self.buf[c], mask, self.r[c] as i32 as f32 - self.cur[c] as f32)
                    }
                    // PullFromBufferNotFrozenBackwards [re:a75764]: the read head
                    // walks back from the write head for one delay, 16-sample
                    // fades at both ends.
                    (false, true) => {
                        let r = self.r[c] as i32;
                        let mut r = if r < 1 { (r - 2 + size) as u32 } else { (r - 1) as u32 };
                        if self.count[c] < 18 {
                            self.gain[c] = (self.count[c] - 1) as f32 * 0.0625;
                            self.ramp[c] = false;
                        }
                        self.count[c] -= 1;
                        if self.ramp[c] {
                            self.gain[c] += 0.0625;
                        }
                        self.w[c] = self.w[c].wrapping_add(1) & mask;
                        if self.count[c] == 0 {
                            self.gain[c] = 0.0;
                            self.ramp[c] = true;
                            self.count[c] = self.tgt[c] as i32;
                            r = self.w[c].wrapping_add(1) & mask;
                        } else {
                            r &= mask;
                        }
                        self.r[c] = r;
                        self.cur[c] += rate[c];
                        let g = self.clamp_gain(c);
                        let mut pos = r as i32 as f64 - self.cur[c];
                        if pos < 0.0 {
                            pos += (size - 1) as f64;
                        }
                        cubic(&self.buf[c], mask, pos as f32) * g
                    }
                    // PullFromBufferFrozenNotBackwards [re:f3fdd1]: loop the
                    // frozen span forwards.
                    (true, false) => {
                        let mut r = self.r[c].wrapping_add(1) & mask;
                        let (start, end) = (self.frz_start[c], self.frz_end[c]);
                        let d = end - r as i32;
                        if (r as i32) < end && d < 17 && (c == 1 || d != 0) {
                            self.gain[c] = d as f32 / 17.0;
                            self.ramp[c] = false;
                        }
                        if self.ramp[c] {
                            self.gain[c] += 0.0625;
                        }
                        if end <= r as i32 {
                            self.gain[c] = 0.0;
                            self.ramp[c] = true;
                            r = start as u32;
                        }
                        if (r as i32) < start {
                            r = start as u32;
                        }
                        self.r[c] = r;
                        let g = self.clamp_gain(c);
                        self.cur[c] += rate[c];
                        g * cubic(&self.buf[c], mask, r as i32 as f32)
                    }
                    // PullFromBufferFrozenBackwards [re:a03e74].
                    (true, true) => {
                        let mut r = self.r[c] as i32;
                        if r < 1 {
                            r = size;
                        }
                        let mut r = (r - 1) as u32 & mask;
                        let (start, end) = (self.frz_start[c], self.frz_end[c]);
                        let d = r as i32 - start;
                        if start < r as i32 && d < 17 {
                            self.gain[c] = d as f32 * 0.0625;
                            self.ramp[c] = false;
                        }
                        if self.ramp[c] {
                            self.gain[c] += 0.0625;
                        }
                        if r as i32 <= start {
                            self.gain[c] = 0.0;
                            self.ramp[c] = true;
                            r = end as u32;
                        }
                        self.r[c] = r;
                        let g = self.clamp_gain(c);
                        self.cur[c] += rate[c];
                        g * cubic(&self.buf[c], mask, r as i32 as f32)
                    }
                };
                self.out[c][i] = y;
            }
        }
    }

    fn process(&mut self, inp: [&[f32]; 2], out: [&mut [f32]; 2], fbproc: &mut dyn FnMut(&mut [f32], &mut [f32])) {
        let n = inp[0].len();
        let inv_n = 1.0 / n as f32;
        for v in self.out.iter_mut().chain(self.fb.iter_mut()) {
            v.resize(n, 0.0);
        }
        let start = self.w;
        self.pull(n);
        // ProcessFeedback [re:469c5c] (feedback-processor position 1: output
        // unfiltered); a frozen buffer takes no input.
        if !self.frozen {
            let [fl, fr] = &mut self.fb;
            fl.copy_from_slice(&self.out[0]);
            fr.copy_from_slice(&self.out[1]);
            self.iir.process(fl, fr);
            // PushInToBuffer [re:db7fb9].
            let dxs = (self.xs[1] - self.xs[0]) * inv_n;
            ms_spacing(self.xs[0], dxs, fl, fr);
            fbproc(fl, fr);
            let dg = (self.fb_g[1] - self.fb_g[0]) * inv_n;
            let mask = self.mask;
            let mut w = start;
            for i in 0..n {
                self.fb_g[0] += dg;
                let g = self.fb_g[0];
                w = [w[0].wrapping_add(1) & mask, w[1].wrapping_add(1) & mask];
                let (l, r) = if self.ping_pong {
                    // The left line takes the mono input plus the right
                    // feedback; the right line the left output.
                    (g * 0.8 * fr[i] * self.phase + (inp[0][i] + inp[1][i]) * 0.5, self.out[0][i])
                } else {
                    (fl[i] * g * self.phase + inp[0][i], g * fr[i] * self.phase + inp[1][i])
                };
                self.buf[0][w[0] as usize] = sat(l);
                self.buf[1][w[1] as usize] = sat(r);
            }
            self.xs[0] += dxs * n as f32;
        }
        let [ol, or] = out;
        ol.copy_from_slice(&self.out[0]);
        or.copy_from_slice(&self.out[1]);
        let dw = (self.width[1] - self.width[0]) * inv_n;
        ms_spacing(self.width[0], dw, ol, or);
        self.width[0] += n as f32 * dw;
    }
}

/// `DigitalDistortion` ([re:dea3e3]..[re:4ac2ae]): sample-and-hold rate reduction
/// and amplitude quantisation, then a DC blocker.
#[derive(Default)]
struct DigitalDistortion {
    sr: f32,
    levels: f32,
    held: [f32; 2],
    count: f32,
    hold: f32,
    dc: DcFilter,
}

impl DigitalDistortion {
    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.hold = 1.0;
        self.dc.set_sample_rate(sr);
    }

    fn init(&mut self, bits: f32, srd: f32) {
        self.levels = bits.max(1.0).exp2();
        self.hold = self.sr / srd.max(100.0);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            self.count += 1.0;
            if self.hold <= self.count {
                self.count = 0.0;
                self.held[0] = (l[i] * self.levels).floor() / self.levels;
                self.held[1] = (r[i] * self.levels).floor() / self.levels;
            }
            l[i] = self.held[0];
            r[i] = self.held[1];
        }
        self.dc.process(l, r);
    }
}

/// [re:812a39].
fn tone_to_bits_and_rate(tone: f32, sr: f32) -> (f32, f32) {
    let b = tone.powf(1.5) * 16.0 + 8.0;
    let srd = if b < 12.0 {
        11025.0 + ((b - 8.0) * 0.25).clamp(0.0, 1.0) * 15015.0
    } else if b < 16.0 {
        26040.0 + ((b - 12.0) * 0.25).clamp(0.0, 1.0) * 18060.0
    } else {
        44100.0 + (sr - 44100.0) * ((b - 16.0) * 0.25).clamp(0.0, 1.0)
    };
    (b, srd)
}

/// [re:b56fd2] clock decimation (input decimation [re:0d7e95]).
#[derive(Default)]
struct Decimator {
    hold: [u32; 2],
    count: [u32; 2],
    prev: [f32; 2],
    cur: [f32; 2],
    dc: DcFilter,
}

impl Decimator {
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (c, x) in [&mut *l, &mut *r].into_iter().enumerate() {
            let h = self.hold[c];
            if h == 0 {
                continue;
            }
            for v in x.iter_mut() {
                self.count[c] += 1;
                if self.count[c] >= h {
                    self.prev[c] = self.cur[c];
                    self.cur[c] = *v;
                    self.count[c] = 0;
                }
                let f = self.count[c] as f32 / h as f32;
                *v = self.cur[c] * 0.5 + (f * self.cur[c] + (1.0 - f) * self.prev[c]) * 0.5;
            }
        }
        self.dc.process(l, r);
    }
}

/// [re:a607e0] hold lengths.
fn bbd_hold(ms: f32, stages: f32) -> u32 {
    (8192.0 / (stages / (ms * 0.001 + ms * 0.001))).floor() as i64 as u32
}

/// [re:27f502] ([re:95de87]..[re:5fba91]) front/back end.
struct Tape {
    sr: f32,
    hiss: Interp,
    rng: Rng,
    lp: StereoIir,
    pre: StereoSslShelf,
    post: StereoSslShelf,
    dc: DcFilter,
    wow_rate: f32,
    wow_phase: f32,
    wow_rising: bool,
    wow_depth: f32,
}

impl Tape {
    fn new() -> Tape {
        Tape {
            sr: 44100.0,
            hiss: Interp::default(),
            rng: Rng::new(),
            lp: StereoIir::default(),
            pre: StereoSslShelf::default(),
            post: StereoSslShelf::default(),
            dc: DcFilter::default(),
            wow_rate: f32::from_bits(0x393e37c6),
            wow_phase: 0.0,
            wow_rising: true,
            wow_depth: 0.0,
        }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sr = sr;
        self.dc.set_sample_rate(sr);
        self.lp.set_sample_rate(sr);
        self.lp.set_mode(0);
        self.pre.set_sample_rate(sr);
        self.post.set_sample_rate(sr);
        self.pre.init(500.0, 0.0, 6.0);
        self.post.init(500.0, 0.0, -6.0);
        self.wow_rate = Rng::new().white(0.0625, 2.0) * (1.0 / sr);
    }

    /// Setup with `(1 − tone, 1 − slew)`; returns the buffer modelling values.
    fn init(&mut self, a: f32, b: f32) -> (u32, u32, f32) {
        let a3 = a * a * a;
        self.hiss.target = a3 * 0.1;
        self.wow_depth = a3;
        let t = 1.0 - a;
        self.lp.set_hz(t * t * t * 10000.0 + 5000.0, 0.0);
        (((1.0 - b) * 997.0 + 3.0) as i64 as u32, ((1.0 - b) * 1997.0 + 3.0) as i64 as u32, t * 6500.0 + 1000.0)
    }

    /// `GenerateWow(n)`: delay modulation (ms) for this block.
    fn wow(&mut self, n: usize) -> f32 {
        let mut v = -100.0f32;
        let mut prev = -100.0f32;
        for _ in 0..n {
            self.wow_phase += self.wow_rate;
            if 1.0 < self.wow_phase {
                self.wow_phase -= 1.0;
                self.wow_rising = true;
            }
            if self.wow_rising {
                v = self.wow_phase * 1.2;
                if 1.0 <= v {
                    self.wow_rising = false;
                    v = 1.0;
                }
            } else {
                v = (1.0 - self.wow_phase) * 4.15 * 1.2;
                if v <= 0.0 {
                    self.wow_rising = true;
                    v = 0.0;
                }
            }
            v = v.max(prev);
            prev = v;
        }
        let e = if n > 0 && v > 0.0 { v } else { 0.0 };
        (e.min(1.0) * 4.0 + 1.0) * self.wow_depth
    }
}

/// [re:e78a64].
pub struct Bloom {
    ports: Vec<Port>,
    sr: f32,
    buffer: DelayBuffer,
    dd_main: DigitalDistortion,
    dd_fb: DigitalDistortion,
    dc_digital: [DcFilter; 2],
    comp: [Compander; 2],
    dec_main: Decimator,
    dec_fb: Decimator,
    dc_bbd: [DcFilter; 2],
    tape: Tape,
    tmp: [Vec<f32>; 2],
}

impl Bloom {
    pub fn new() -> Bloom {
        let mut ports = ports_from(ports::BLOOM);
        // Ping Pong, Freeze and Direction are on/off switches: the original
        // reads them as ≥ 0.5 of the front-panel value (a stored
        // Direction="1" plays the echoes backwards) [runtime].
        for i in [2, 12, 13] {
            ports[i] = Port::new(0.0, 0.0, 1.0);
        }
        let t = (bpm_from_text("1/64 note"), bpm_from_text("1/64 note"), bpm_from_text("2 note"));
        ports[3] = Port::new(t.0, t.1, t.2);
        Bloom {
            ports,
            sr: 0.0,
            buffer: DelayBuffer::new(),
            dd_main: DigitalDistortion::default(),
            dd_fb: DigitalDistortion::default(),
            dc_digital: [DcFilter::default(); 2],
            comp: [Compander::new(), Compander::new()],
            dec_main: Decimator::default(),
            dec_fb: Decimator::default(),
            dc_bbd: [DcFilter::default(); 2],
            tape: Tape::new(),
            tmp: [scratch(), scratch()],
        }
    }

    fn prepare(&mut self, sr: f32) {
        if self.sr == sr {
            return;
        }
        self.sr = sr;
        self.buffer.set_sample_rate(sr);
        for d in [&mut self.dd_main, &mut self.dd_fb] {
            d.set_sample_rate(sr);
        }
        for d in self.dc_digital.iter_mut().chain(self.dc_bbd.iter_mut()) {
            d.set_sample_rate(sr);
        }
        self.dec_main.dc.set_sample_rate(sr);
        self.dec_fb.dc.set_sample_rate(sr);
        for c in &mut self.comp {
            c.set_sample_rate(sr);
        }
        self.tape.set_sample_rate(sr);
    }

    fn model(&self) -> u32 {
        let v = self.ports[0].value * 2.0;
        (((v.abs() + 2.0) - (v - 2.0).abs()) * 0.5).round() as u32
    }

    /// Parameter update.
    fn update(&mut self, ctx: &Ctx, n: usize) {
        let p = |i: usize| self.ports[i].value;
        let model = self.model();
        let slew = p(1);
        let tone = p(11);
        let wow = if model == 2 && p(12) < 0.5 && p(13) < 0.5 { self.tape.wow(n) } else { 0.0 };
        let t = time_beat_to_units(p(3), if p(4) >= 0.5 { 1.0 } else { 0.0 }, 60.0 / ctx.bpm.max(1e-8));
        let off = (p(5) * 50.0 - 25.0).round();
        let lim = |x: f32| {
            if x <= 5.0 {
                5.0
            } else if 4000.0 <= x {
                4000.0
            } else {
                x
            }
        };
        let l = lim(t * 1000.0 + off + wow);
        let r = lim((t * 1000.0 - off) + wow);
        let (bits, srd) = tone_to_bits_and_rate(tone, self.sr);
        match model {
            2 => {
                let (up, down, hz) = self.tape.init(1.0 - tone, 1.0 - slew);
                self.buffer.set_modelling(up, down, hz);
            }
            1 => {
                let stages = tone * 3840.0 + 256.0;
                let h = [bbd_hold(l, stages), bbd_hold(r, stages)];
                self.dec_main.hold = h;
                self.dec_fb.hold = h;
                let b = 1.0 - (1.0 - slew);
                self.buffer.set_modelling((b * 207.0 + 3.0) as i64 as u32, (b * 187.0 + 3.0) as i64 as u32, 10000.0);
            }
            _ => {
                self.dd_main.init(bits, srd);
                self.dd_fb.init(bits, srd);
                let ms = ((1.0 - (1.0 - slew)) * 47.0 + 3.0) as i64 as u32;
                self.buffer.set_modelling(ms, ms, srd * 0.5);
            }
        }
        self.buffer.init(l, r, p(12) >= 0.5, p(13) >= 0.5, p(7), p(9) >= 0.5, p(8), p(2) >= 0.5, p(6));
    }
}

impl Default for Bloom {
    fn default() -> Self {
        Bloom::new()
    }
}

impl Node for Bloom {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn port_value(&self, index: usize) -> Option<f32> {
        self.ports.get(index).map(|p| p.value)
    }

    fn prepare_rate(&mut self, sr: f32) {
        self.prepare(sr);
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        self.prepare(ctx.sample_rate);
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        self.update(ctx, n);
        let model = self.model();
        let [tl, tr] = &mut self.tmp;
        tl.clear();
        tl.extend_from_slice(inp[0]);
        tr.clear();
        tr.extend_from_slice(inp[1]);
        let [ol, or] = out;
        match model {
            2 => {
                let tape = &mut self.tape;
                for v in tl.iter_mut().chain(tr.iter_mut()) {
                    *v = sat(*v);
                }
                tape.pre.process(tl, tr);
                let hiss = tape.hiss.process(n);
                for i in 0..n {
                    tl[i] += tape.rng.pink(-1.0, 1.0) * 0.01 * hiss[i];
                    tr[i] += tape.rng.pink(-1.0, 1.0) * 0.01 * hiss[i];
                }
                self.buffer.process([tl, tr], [&mut *ol, &mut *or], &mut |l, r| {
                    for v in l.iter_mut().chain(r.iter_mut()) {
                        *v = sat(*v);
                    }
                });
                tape.lp.process(ol, or);
                tape.post.process(ol, or);
                tape.dc.process(ol, or);
            }
            1 => {
                self.comp[0].compress(tl);
                self.comp[1].compress(tr);
                self.dec_main.process(tl, tr);
                for v in tl.iter_mut().chain(tr.iter_mut()) {
                    *v = sat(*v);
                }
                let (dec_fb, dc_fb) = (&mut self.dec_fb, &mut self.dc_bbd[1]);
                self.buffer.process([tl, tr], [&mut *ol, &mut *or], &mut |l, r| {
                    dec_fb.process(l, r);
                    for v in l.iter_mut().chain(r.iter_mut()) {
                        *v = sat(*v);
                    }
                    dc_fb.process(l, r);
                });
                for v in ol.iter_mut().chain(or.iter_mut()) {
                    *v = sat(*v);
                }
                self.dc_bbd[0].process(ol, or);
            }
            _ => {
                self.dd_main.process(tl, tr);
                let (dd_fb, dc_fb) = (&mut self.dd_fb, &mut self.dc_digital[1]);
                self.buffer.process([tl, tr], [&mut *ol, &mut *or], &mut |l, r| {
                    dd_fb.process(l, r);
                    dc_fb.process(l, r);
                });
                self.dc_digital[0].process(ol, or);
            }
        }
    }

    fn reset(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_models_echo_and_stay_finite() {
        for model in [0.0, 0.5, 1.0] {
            let mut b = Bloom::new();
            let ctx = Ctx::new(48000.0);
            b.ports[0].value = model;
            b.ports[4].value = 0.0;
            // 100 ms free time.
            b.ports[3].value = 0.1f32.log2() + 1.0;
            b.ports[7].value = 0.5;
            let n = 256;
            let mut energy_late = 0.0f32;
            for blk in 0..200 {
                let mut x = vec![0.0f32; n];
                if blk == 50 {
                    for (i, v) in x.iter_mut().enumerate().take(128) {
                        *v = 0.8 * (i as f32 * 0.2).sin();
                    }
                }
                let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
                b.process(&ctx, [&x, &x], [&mut l, &mut r]);
                assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "model {model}");
                if blk > 60 {
                    energy_late += l.iter().map(|v| v * v).sum::<f32>();
                }
            }
            assert!(energy_late > 1e-6, "model {model}: no echo");
        }
    }

    /// Direction = Reverse: a decaying burst comes back as a swell (the
    /// read head walks backwards from the write head), so the echo's energy
    /// rises across the delay instead of falling.
    #[test]
    fn reverse_plays_echoes_backwards() {
        let ctx = Ctx::new(48000.0);
        let mut b = Bloom::new();
        b.ports[0].value = 0.0;
        b.ports[4].value = 0.0;
        b.ports[3].value = 0.2f32.log2() + 1.0;
        b.ports[13].value = 1.0;
        let n = 240;
        let mut env = Vec::new();
        for blk in 0..400 {
            let x: Vec<f32> = (0..n).map(|i| if blk < 10 { 0.8 * (-((blk * n + i) as f32) / 600.0).exp() * ((i as f32) * 0.3).sin() } else { 0.0 }).collect();
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            b.process(&ctx, [&x, &x], [&mut l, &mut r]);
            assert!(l.iter().all(|v| v.is_finite()));
            env.push(l.iter().map(|v| v * v).sum::<f32>());
        }
        // Somewhere after the dry burst the wet energy grows block over block.
        let rising = env[12..].windows(4).any(|w| w[0] > 1e-6 && w[0] < w[1] && w[1] < w[2] && w[2] < w[3]);
        assert!(rising, "no reversed swell");
    }
}
