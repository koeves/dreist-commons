//! [re:4f768c] (Audio Freezer, kind 38, dual-mono; `Reset`
//! [re:0eea55], `SetMaxDelay` [re:144838], `ProcessBuffer` [re:c43ffb]): while the
//! gate is off the input is recorded into a loop buffer; when it opens the
//! loop length is latched (`LoopLength` beats) and, at the next 16th-note
//! boundary, playback switches to grains read from the frozen loop: each
//! grain starts at the (Speed-drifting) start position plus a manual/random
//! jump of whole grains, plays at the Scratch rate and crossfades from the
//! previous grain over `Smooth · grain`. The dry path is delayed by one
//! sample. The grain length uses `ConvertToUnits(v, mode, [re:88cd77])`,
//! i.e. bpm/60 rather than 60/bpm in beat mode, as in the binary.

use super::math::{clip01, cubic, pow2_at_least};
use crate::fx::convert::time_beat_to_units;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

/// Loop lengths in beats ([re:7bca78], `LoopLength` 0..16).
const LOOP_BEATS: [f32; 17] =
    [0.04166687652468681, 0.0625, 0.08333375304937363, 0.125, 0.16666750609874725, 0.25, 0.3333350121974945, 0.5, 0.666670024394989, 1.0, 2.0, 3.0, 4.0, 8.0, 16.0, 32.0, 856.8172607421875];

struct Voice {
    buf: Vec<f32>,
    mask: u32,
    size: f32,
    max_grain: f32,
    gate_prev: bool,
    armed: bool,
    xfading: bool,
    recording: bool,
    w: i32,
    count: i32,
    loop_int: i32,
    start_ppq: f64,
    prev_in: f32,
    xf: f32,
    fade_len: f32,
    inv_fade: f32,
    loop_len: f32,
    inv_loop: f32,
    start: f32,
    grain: f32,
    grain_prev: f32,
    pos_a: f32,
    pos_b: f32,
    bypass: f32,
    dry: f32,
    wet: f32,
    seed: u32,
}

impl Voice {
    fn new() -> Voice {
        let mut v = Voice {
            buf: Vec::new(),
            mask: 0,
            size: 0.0,
            max_grain: 0.0,
            gate_prev: false,
            armed: false,
            xfading: false,
            recording: true,
            w: 0,
            count: 0,
            loop_int: 0,
            start_ppq: 0.0,
            prev_in: 0.0,
            xf: 0.0,
            fade_len: 0.0,
            inv_fade: 1.0,
            loop_len: 1.0,
            inv_loop: 1.0,
            start: 0.0,
            grain: 0.0,
            grain_prev: 0.0,
            pos_a: 0.0,
            pos_b: 0.0,
            bypass: 1.0,
            dry: 1.0,
            wet: 0.0,
            seed: 0,
        };
        v.set_max_delay(384000.0 + 384000.0);
        v
    }

    /// `SetMaxDelay(2 · 384000)`: power-of-two buffer.
    fn set_max_delay(&mut self, d: f32) {
        self.max_grain = d;
        let n = d.round() as i32;
        let size = pow2_at_least(n as i64);
        if self.buf.len() != size {
            self.buf = vec![0.0; size];
        }
        self.mask = size as u32 - 1;
        self.size = size as f32;
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.gate_prev = false;
        self.armed = false;
        self.xfading = false;
        self.recording = true;
        self.w = 0;
        self.count = 0;
        self.loop_int = 0;
        self.start_ppq = 0.0;
        self.prev_in = 0.0;
        self.bypass = 1.0;
        self.dry = 1.0;
        self.wet = 0.0;
        self.xf = 0.0;
        self.fade_len = 0.0;
        self.inv_fade = 1.0;
        self.start = 0.0;
        self.loop_len = 1.0;
        self.inv_loop = 1.0;
        self.grain = 0.0;
        self.grain_prev = 0.0;
        self.pos_a = 0.0;
        self.pos_b = 0.0;
    }

    fn process(&mut self, ctx: &Ctx, p: &[f32], x: &[f32], out: &mut [f32]) {
        let gate = p[0];
        let li = (((p[1].abs() + 16.0) - (p[1] + -16.0).abs()) * 0.5).round() as usize;
        let sr = ctx.sample_rate;
        let g = time_beat_to_units(p[2], p[3], ctx.bpm / 60.0) * sr;
        let mg = self.max_grain;
        let grain_len = ((mg + 2.0 + (g + -2.0).abs()) - (g - mg).abs()) * 0.5;
        let fade = clip01(p[4]) * grain_len;
        if self.fade_len != fade {
            self.fade_len = fade;
            self.inv_fade = if 1e-06 <= fade.abs() { 1.0 / fade } else { 1.0 };
        }
        let ppq4 = ctx.ppq * 4.0;
        let open = 0.5 <= gate;
        let latch = if !open {
            self.bypass = 1.0;
            true
        } else if !self.gate_prev {
            self.start_ppq = ppq4;
            self.armed = true;
            true
        } else {
            false
        };
        if latch {
            self.recording = true;
            let l = sr * (LOOP_BEATS[li.min(16)] * (60.0 / ctx.bpm.max(1e-8)));
            let s = self.size;
            self.loop_len = ((l + s) - (s - l).abs()) * 0.5;
            self.loop_int = self.loop_len.round() as i32;
        }
        self.gate_prev = open;
        let frac = (self.start_ppq - self.start_ppq.floor()) as f32;
        if (frac < 0.1 || 1.0 < ppq4 - self.start_ppq.floor()) && self.armed {
            self.bypass = 0.0;
            self.armed = false;
            self.count = 0;
            self.inv_loop = if 1.0 <= self.loop_len { 1.0 / self.loop_len } else { 1.0 };
            let s = (self.w - 1) as f32;
            self.start = s;
            self.grain = 0.0;
            self.grain_prev = 0.0;
            self.pos_a = s;
            self.pos_b = s;
        }
        self.dry = self.dry + 1e-08 + -1e-08;
        self.wet = self.wet + 1e-08 + -1e-08;
        let k = {
            let v = (1.0 / sr) * 62.831856;
            ((v + 1.0) - (v + -1.0).abs()) * 0.5
        };
        let wet_t = clip01(p[9]);
        let (speed, scratch) = (p[6], p[5]);
        let (jump_m, jump_r) = (p[7], p[8]);
        let m = self.mask;
        for i in 0..x.len() {
            self.w += 1;
            if self.loop_int <= self.w {
                self.w = 0;
            }
            let xi = x[i];
            if self.recording {
                self.buf[(m & self.w as u32) as usize] = xi;
                if open {
                    let c = self.count;
                    self.count = c + 1;
                    if self.loop_int <= c {
                        self.recording = false;
                    }
                }
            }
            if grain_len < self.grain {
                self.xfading = true;
                self.xf = 0.0;
                self.grain_prev = self.grain;
                self.grain = 0.0;
                self.pos_b = self.pos_a;
                self.seed = self.seed.wrapping_mul(0xbb38433).wrapping_add(0x3619636b);
                let r = f32::from_bits(self.seed >> 9 | 0x4000_0000) + -3.0;
                let j = (r * 0.5 * jump_r + jump_m).round() as i32;
                self.pos_a = j as f32 * grain_len + self.start;
            }
            let (l, il) = (self.loop_len, self.inv_loop);
            let a = self.pos_a * il;
            self.pos_a = (a - a.floor()) * l;
            let b = self.pos_b * il;
            self.pos_b = (b - b.floor()) * l;
            let v = if self.xfading {
                self.xf += self.inv_fade;
                if self.xf < 1.0 {
                    cubic(&self.buf, m, self.pos_a) * self.xf + cubic(&self.buf, m, self.pos_b) * (1.0 - self.xf)
                } else {
                    self.xfading = false;
                    cubic(&self.buf, m, self.pos_a)
                }
            } else {
                cubic(&self.buf, m, self.pos_a)
            };
            let wt = (1.0 - self.bypass) * wet_t;
            self.wet = (wt - self.wet) * k + self.wet;
            self.dry = ((1.0 - wt) - self.dry) * k + self.dry;
            out[i] = self.wet * v + self.dry * self.prev_in;
            self.prev_in = xi;
            self.pos_b += scratch;
            self.start += speed;
            self.grain += scratch.abs();
            self.grain_prev += scratch.abs();
            self.pos_a += scratch;
        }
    }
}

pub struct AudioFreeze {
    ports: Vec<Port>,
    voices: [Voice; 2],
}

impl AudioFreeze {
    pub fn new() -> AudioFreeze {
        let mut ports = ports_from(ports::AUDIO_FREEZER);
        // Float-form enum: Sync max = item count (2).
        ports[3] = Port::new(1.0, 0.0, 2.0);
        AudioFreeze { ports, voices: [Voice::new(), Voice::new()] }
    }
}

impl Default for AudioFreeze {
    fn default() -> Self {
        AudioFreeze::new()
    }
}

impl Node for AudioFreeze {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let p = crate::fx::port_values(&self.ports);
        for (k, o) in out.into_iter().enumerate() {
            self.voices[k].process(ctx, &p, inp[k], o);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
    }
}
