//! `Sampler` ([re:cd6e57]): records the audio input (or an internal
//! output) into takes, armed/started immediately, by MIDI, at the next bar,
//! or by a level threshold, optionally timed. The original posts the
//! recorded samples to the editor, which builds audio files; the port keeps
//! the takes in memory. See the dreist-re sampler notes.

/// Pre-roll ring length (+0x20c / +[re:6f3900], 0x4b00 frames).
const PREROLL: usize = 0x4b00;

/// Finished takes held for the editor; later ones are dropped (their
/// buffer is reused) until it collects them.
pub const FINISHED_MAX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Idle,
    Armed,
    Recording,
}

/// [re:5ce144] (+0x1c8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartMode {
    #[default]
    Immediate,
    Midi,
    Transport,
    Threshold,
    MidiAny,
}

/// `TimedMode` (+0x1cc): off, stop after the length, or keep cutting takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimedMode {
    #[default]
    Off,
    Stop,
    Retake,
}

/// `SamplerChannelMode` (+0x1f8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channels {
    #[default]
    Stereo,
    Left,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Take {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Recording room reserved per take buffer. The memory is only committed
/// as it is written, and a longer take grows the buffer.
pub const TAKE_RESERVE_SECONDS: f32 = 600.0;

impl Take {
    /// An empty take with room for `frames` (reserve it off the audio
    /// thread: see [`Sampler::give_spare`]).
    pub fn with_capacity(frames: usize) -> Take {
        Take { left: Vec::with_capacity(frames), right: Vec::with_capacity(frames) }
    }

    fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
    }
}

/// `recordsource` values: the host input and the master output.
pub const SOURCE_EXTERNAL: i32 = 38;
pub const SOURCE_MASTER: i32 = 1;

/// [re:eba363]: the stored `recordsource` name —
/// "External", an output location name, or "MITM".
pub fn source_name(source: i32) -> String {
    match source {
        SOURCE_EXTERNAL => "External".into(),
        s if s < SOURCE_EXTERNAL => crate::engine::Route::from_index(s.max(0) as usize).name(),
        _ => "MITM".into(),
    }
}

/// The source for a stored name ("External", "MITM" and unknown names →
/// the host input).
pub fn source_from_name(name: &str) -> i32 {
    (1..SOURCE_EXTERNAL).find(|&i| crate::engine::Route::from_index(i as usize).name() == name).unwrap_or(SOURCE_EXTERNAL)
}

#[derive(Debug, Clone)]
pub struct Sampler {
    pub state: State,
    /// Engine of the auto-allocation target (`GetAutoSamplerTarget` +[re:6c4598]),
    /// set when arming.
    pub target_engine: Option<usize>,
    pub start_mode: StartMode,
    pub timed: TimedMode,
    pub channels: Channels,
    /// `beatsmode`, `recbars`, `recbeats`, `recsecs` (defaults off, 1, 0, 2 s).
    pub beats_mode: bool,
    pub bars: u32,
    pub beats: u32,
    pub secs: f32,
    /// `inputgain`, `outputgain` (thru level), `inputthru`.
    pub input_gain: f32,
    pub output_gain: f32,
    pub thru: bool,
    /// `startthreshold` (0.1), `stopthreshold` (0 = off).
    pub start_threshold: f32,
    pub stop_threshold: f32,
    pub sample_rate: f32,
    /// Peak meters (+0x1fc / +0x200).
    pub meters: (f32, f32),
    /// Takes kept (`StartTake` stops at 1023); their audio goes to the
    /// editor through `finished`.
    pub takes: usize,
    /// Takes finished since the editor last collected them (the original
    /// posts the audio to [re:95eb92]), at most
    /// [`FINISHED_MAX`] waiting.
    pub finished: Vec<Take>,
    /// A reserved buffer for the next take, handed in off the audio thread
    /// so recording does not allocate.
    spare: Option<Take>,
    /// `recordsource` (+0x1f4, `GetRecordSourceType` [re:c88950]): 38 = the
    /// host input ("External"), below that an output location (1 Master,
    /// 2..33 Sub output 1–32), above it MITM (out of scope). The
    /// constructor ([re:33d1f3]) starts at 1, Master.
    pub source: i32,
    /// Auto-export mode (+[re:ef5002], message 0x100; export itself not ported).
    pub auto_mode: usize,
    /// Timed length in samples (+0x1c0, 0 = untimed).
    length: u32,
    /// Samples until a pending start (+0x1c4, −1 = none).
    countdown: i64,
    /// Current take length (+0x1d8) and trailing silence (+0x1f0).
    take_len: u32,
    silence: u32,
    current: Option<Take>,
    ring: [Vec<f32>; 2],
    ring_pos: usize,
    ring_fill: usize,
    rec: [Vec<f32>; 2],
}

impl Default for Sampler {
    /// [re:d4f785].
    fn default() -> Self {
        Sampler {
            state: State::Idle,
            start_mode: StartMode::Immediate,
            timed: TimedMode::Off,
            channels: Channels::Stereo,
            beats_mode: false,
            bars: 1,
            beats: 0,
            secs: 2.0,
            input_gain: 1.0,
            output_gain: 1.0,
            thru: false,
            start_threshold: 0.1,
            stop_threshold: 0.0,
            sample_rate: 44100.0,
            meters: (0.0, 0.0),
            takes: 0,
            finished: Vec::with_capacity(FINISHED_MAX),
            spare: None,
            source: SOURCE_MASTER,
            auto_mode: 0,
            target_engine: None,
            length: 0,
            countdown: -1,
            take_len: 0,
            silence: 0,
            current: None,
            ring: [vec![0.0; PREROLL], vec![0.0; PREROLL]],
            ring_pos: 0,
            ring_fill: 0,
            rec: [Vec::with_capacity(crate::engine::MAX_HOST_BLOCK), Vec::with_capacity(crate::engine::MAX_HOST_BLOCK)],
        }
    }
}

/// Host timing the sampler needs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SamplerClock {
    pub playing: bool,
    /// Transport just stopped this block.
    pub stopped: bool,
    pub samples_per_beat: f64,
    pub samples_per_bar: f64,
    /// Samples to the bar where playback started ([re:812732]).
    pub to_bar: i64,
}

impl Sampler {
    /// Whether the next take still needs a reserved buffer.
    pub fn needs_spare(&self) -> bool {
        self.spare.is_none()
    }

    /// The buffer the next take records into (see [`Take::with_capacity`]).
    pub fn give_spare(&mut self, mut t: Take) {
        t.clear();
        self.spare = Some(t);
    }

    /// A buffer for [`Sampler::give_spare`] (allocates: not on the audio thread).
    pub fn new_spare(&self) -> Take {
        Take::with_capacity((self.sample_rate * TAKE_RESERVE_SECONDS) as usize)
    }

    /// A finished or discarded take's buffer: kept as the spare when there
    /// is none (so the audio thread does not free it).
    fn recycle(&mut self, mut t: Take) {
        if self.spare.is_none() {
            t.clear();
            self.spare = Some(t);
        }
    }

    /// The take being recorded (pre-roll included).
    pub fn current_take(&self) -> Option<&Take> {
        self.current.as_ref()
    }

    /// `ArmRecord` [re:df1a89].
    pub fn arm(&mut self) {
        if self.state == State::Idle {
            self.state = State::Armed;
            self.countdown = -1;
        }
    }

    /// `DisarmRecord` [re:c36352].
    pub fn disarm(&mut self) {
        match self.state {
            State::Recording => self.stop_recording(),
            State::Armed => self.state = State::Idle,
            State::Idle => {}
        }
    }

    /// `ProcessMIDI` [re:157b1c]: a note-on starts an armed MIDI-mode sampler
    /// `delta` samples into the block.
    pub fn midi(&mut self, status: u8, delta: u32) {
        if self.state == State::Armed && status == 0x90 && matches!(self.start_mode, StartMode::Midi | StartMode::MidiAny) {
            self.countdown = delta as i64;
        }
    }

    /// `StartRecording` [re:d8f427]: timed length, new take, and (with
    /// `fade`) the last ≤ 1 ms of input faded in ahead of the take.
    fn start_recording(&mut self, clock: &SamplerClock, fade: bool) {
        self.length = match self.timed {
            TimedMode::Off => 0,
            _ if self.beats_mode => (clock.samples_per_beat * self.beats as f64 + clock.samples_per_bar * self.bars as f64) as i64 as u32,
            _ => (self.secs * self.sample_rate) as i64 as u32,
        };
        self.state = State::Recording;
        self.start_take();
        if fade {
            let m = self.ring_fill.min((self.sample_rate * 0.001) as usize);
            if let Some(t) = self.current.as_mut().filter(|_| m > 0) {
                let k = 10f32.powf(4.69897 / (m >> 1).max(1) as f32);
                let mut g = 0.99999f32;
                // Filled backwards from the newest pre-roll frame.
                let at = t.left.len();
                t.left.resize(at + m, 0.0);
                t.right.resize(at + m, 0.0);
                let mut p = self.ring_pos;
                for i in 0..m {
                    p = if p == 0 { PREROLL - 1 } else { p - 1 };
                    t.left[at + m - 1 - i] = self.ring[0][p] * g;
                    t.right[at + m - 1 - i] = self.ring[1][p] * g;
                    if i > m >> 1 { g *= 1.0 / k } else { g = (g - 1.0) * k + 1.0 }
                }
            }
        }
    }

    /// `StartTake` [re:822ac7] (at most 1023 takes).
    fn start_take(&mut self) {
        if self.takes >= 0x3ff {
            self.state = State::Idle;
            return;
        }
        self.take_len = 0;
        self.silence = 0;
        self.current = Some(self.spare.take().unwrap_or_default());
    }

    /// `StopTake` [re:0f92f1]: keep the take, cut to `len` recorded samples
    /// (after any pre-roll).
    fn stop_take(&mut self, len: u32) {
        if let Some(mut t) = self.current.take() {
            let pre = t.left.len().saturating_sub(self.take_len as usize);
            t.left.truncate(pre + len as usize);
            t.right.truncate(pre + len as usize);
            if len > 0 {
                self.takes += 1;
                if self.finished.len() < FINISHED_MAX {
                    self.finished.push(t);
                    return;
                }
            }
            self.recycle(t);
        }
    }

    /// `StopRecording` [re:26cdcb]: untimed takes lose their trailing silence.
    pub fn stop_recording(&mut self) {
        if self.state != State::Recording {
            return;
        }
        if self.length == 0 {
            self.stop_take(self.take_len - self.silence.min(self.take_len));
        } else {
            self.stop_take(self.length);
        }
        self.state = State::Idle;
        self.countdown = -1;
    }

    /// Manual start (message 0xf0): restart and record now.
    pub fn start_now(&mut self, clock: &SamplerClock) {
        self.stop_recording();
        self.arm();
        self.start_recording(clock, true);
    }

    /// Armed → record now with fade (message 0xef/0xf0 on an armed
    /// sampler, `StartRecording(true)`).
    pub fn start_armed(&mut self, clock: &SamplerClock) {
        if self.state == State::Armed {
            self.start_recording(clock, true);
        }
    }

    /// `ClearTake` (0xfb): drop one take (not while recording).
    pub fn clear_take(&mut self, n: usize) {
        if self.state != State::Recording && n < self.takes {
            self.takes -= 1;
        }
    }

    /// `ClearTakes` (0xfc).
    pub fn clear_takes(&mut self) {
        if self.state != State::Recording {
            self.takes = 0;
        }
    }

    /// `HandleMessage` 0xfe: channel mode change clears takes and idles.
    pub fn set_channels(&mut self, mode: i32) {
        if let Some(t) = self.current.take() {
            self.recycle(t);
        }
        self.state = State::Idle;
        self.countdown = -1;
        self.takes = 0;
        self.channels = match mode {
            1 => Channels::Left,
            2 => Channels::Right,
            _ => Channels::Stereo,
        };
    }

    /// `SetStartMode` [re:b66a15] (0xf2): immediate is timed-stop, MIDI and
    /// threshold untimed, host and timed MIDI take the auto mode's timing
    /// (table [re:6b0523] = [stop, stop, retake, retake, retake, stop]).
    pub fn set_start_mode(&mut self, mode: i32) {
        let new = match mode {
            1 => StartMode::Midi,
            2 => StartMode::Transport,
            3 => StartMode::Threshold,
            4 => StartMode::MidiAny,
            _ => StartMode::Immediate,
        };
        if new == self.start_mode {
            return;
        }
        self.start_mode = new;
        const AUTO: [TimedMode; 6] = [TimedMode::Stop, TimedMode::Stop, TimedMode::Retake, TimedMode::Retake, TimedMode::Retake, TimedMode::Stop];
        self.timed = match new {
            StartMode::Immediate => TimedMode::Stop,
            StartMode::Midi | StartMode::Threshold => TimedMode::Off,
            StartMode::Transport | StartMode::MidiAny => AUTO.get(self.auto_mode).copied().unwrap_or(TimedMode::Off),
        };
    }

    fn preroll(&mut self, from: usize, n: usize) {
        for i in from..from + n {
            self.ring[0][self.ring_pos] = self.rec[0][i];
            self.ring[1][self.ring_pos] = self.rec[1][i];
            self.ring_pos = (self.ring_pos + 1) % PREROLL;
        }
        self.ring_fill = (self.ring_fill + n).min(PREROLL);
    }

    fn loud(&self, i: usize, thr: f32) -> bool {
        match self.channels {
            Channels::Stereo => self.rec[0][i].abs() > thr || self.rec[1][i].abs() > thr,
            Channels::Left => self.rec[0][i].abs() > thr,
            Channels::Right => self.rec[1][i].abs() > thr,
        }
    }

    /// `ProcessSamples` [re:08bdb0] for one block of `input`; adds the thru
    /// signal into `out`.
    pub fn process(&mut self, input: [&[f32]; 2], out: [&mut [f32]; 2], clock: &SamplerClock) {
        let n = input[0].len();
        if n == 0 || n > 0x15888 {
            return;
        }
        for r in &mut self.rec {
            r.clear();
            r.resize(n, 0.0);
        }
        // Gain, channel selection and meters.
        let g = self.input_gain;
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            let (l, r) = match self.channels {
                Channels::Stereo => (input[0][i] * g, input[1][i] * g),
                Channels::Left => (input[0][i] * g, 0.0),
                Channels::Right => (input[1][i] * g, input[1][i] * g),
            };
            self.rec[0][i] = l;
            self.rec[1][i] = if self.channels == Channels::Stereo { r } else { l };
            pl = pl.max(l.abs());
            pr = pr.max(r.abs());
        }
        self.meters = (self.meters.0.max(pl), self.meters.1.max(pr));
        // Thru goes to the main outputs, except when recording the master
        // itself (`ProcessSamples`: +0x208 = source != 1).
        if self.thru && self.source != SOURCE_MASTER {
            let og = self.output_gain;
            let [ol, or] = out;
            for i in 0..n {
                ol[i] += self.rec[0][i] * og;
                or[i] += self.rec[1][i] * og;
            }
        }
        match self.state {
            State::Recording => {
                if self.start_mode == StartMode::Transport && clock.stopped {
                    self.stop_recording();
                    self.arm();
                }
            }
            State::Idle => {
                self.preroll(0, n);
                return;
            }
            State::Armed => match self.start_mode {
                StartMode::Immediate => self.start_recording(clock, true),
                StartMode::Transport if clock.playing && self.countdown == -1 => {
                    self.countdown = clock.to_bar;
                }
                _ => {}
            },
        }
        let mut pos = 0usize;
        let mut remaining = n;
        while remaining > 0 {
            let mut used;
            if self.state == State::Armed {
                if self.start_mode == StartMode::Threshold {
                    if let Some(k) = (0..remaining).find(|&k| self.loud(pos + k, self.start_threshold)) {
                        self.countdown = k as i64;
                    }
                }
                if self.countdown == 0 {
                    self.start_recording(clock, self.start_mode == StartMode::Threshold);
                    used = 0;
                } else {
                    used = if self.countdown > 0 && (self.countdown as usize) < remaining { self.countdown as usize } else { remaining };
                    self.preroll(pos, used);
                }
            } else {
                used = 0;
            }
            if self.state == State::Recording {
                let mut chunk = remaining - used;
                if self.timed != TimedMode::Off && self.length != 0 {
                    chunk = chunk.min((self.length - self.take_len.min(self.length)) as usize);
                }
                let at = pos + used;
                if let Some(t) = self.current.as_mut() {
                    t.left.extend_from_slice(&self.rec[0][at..at + chunk]);
                    t.right.extend_from_slice(&self.rec[1][at..at + chunk]);
                }
                self.take_len += chunk as u32;
                if chunk < remaining - used {
                    if self.timed == TimedMode::Retake {
                        self.stop_take(self.take_len);
                        self.start_take();
                    } else {
                        self.stop_recording();
                    }
                }
                // Auto-stop after 0.25 s below the stop threshold (the
                // app's rules may set another time).
                if self.state == State::Recording && self.start_mode == StartMode::Threshold && self.stop_threshold > 0.0 {
                    let last = (0..chunk).rev().find(|&k| self.loud(at + k, self.stop_threshold));
                    self.silence = match last {
                        Some(j) => (chunk - 1 - j) as u32,
                        None => self.silence + n as u32,
                    };
                    if self.silence as f32 > self.sample_rate * dreist_dsp::rules::get().sampler_auto_stop_seconds {
                        self.stop_recording();
                        self.arm();
                    }
                }
                used += chunk;
            }
            if self.countdown >= 0 {
                self.countdown = (self.countdown - used as i64).max(-1);
            }
            if used == 0 && self.state != State::Recording {
                break;
            }
            pos += used;
            remaining -= used;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_start_and_auto_stop() {
        let mut s = Sampler { start_mode: StartMode::Threshold, stop_threshold: 0.05, ..Sampler::default() };
        s.arm();
        let clock = SamplerClock::default();
        let mut sig = vec![0.0f32; 44100];
        for v in &mut sig[1000..11000] {
            *v = 0.5;
        }
        let mut out = (vec![0.0; 512], vec![0.0; 512]);
        for c in sig.chunks(512) {
            let (l, r) = (&mut out.0[..c.len()], &mut out.1[..c.len()]);
            s.process([c, c], [l, r], &clock);
        }
        assert_eq!(s.takes, 1);
        let t = &s.finished[0];
        // Starts at the first loud sample (plus ≤ 1 ms faded pre-roll), and
        // the trailing silence is trimmed.
        let loud = t.left.iter().filter(|v| **v > 0.4).count();
        assert_eq!(loud, 10000);
        assert!(t.left.len() < 10000 + 45 + 10);
        assert_eq!(s.state, State::Armed);
    }

    #[test]
    fn timed_take_in_seconds() {
        let mut s = Sampler { timed: TimedMode::Stop, secs: 0.1, ..Sampler::default() };
        s.arm();
        let clock = SamplerClock::default();
        let sig = vec![0.25f32; 44100];
        let mut out = (vec![0.0; 256], vec![0.0; 256]);
        for c in sig.chunks(256) {
            let (l, r) = (&mut out.0[..c.len()], &mut out.1[..c.len()]);
            s.process([c, c], [l, r], &clock);
        }
        assert_eq!(s.takes, 1);
        assert_eq!(s.finished[0].left.len(), 4410);
        assert_eq!(s.state, State::Idle);
    }
}
