//! One playing layer: [re:7914d6] ([re:b7314b] .. [re:da5301]) without the
//! voice-manager, step-sequencer and effect plumbing. The caller supplies the
//! layer's parameter matrix, the step's lane values and the transmod source
//! values; this module does everything from the voice setup for playback to
//! the output stage.

use crate::convert::{cutoff_with_velocity, filter_type_from_step, fine_semitones, modulate_pos_neg, octave, semitones};
use crate::envelope::{AhrEnvelope, Envelope, EnvelopeParams};
use crate::filter::VoiceFilter;
use crate::interp::catmull_rom;
use crate::mix::Mixable;
use crate::sp1200::{Sp1200, vintage_tap};
use std::sync::Arc;

/// Number of transmod sources ([re:56009e]).
pub const SOURCES: usize = 29;
/// Number of Layer parameters.
pub const LAYER_PARAMS: usize = 42;

/// Layer parameter indices used here (layer parameter enum order).
pub mod lp {
    pub const VOICE_GAIN: usize = 2;
    pub const VOICE_PAN: usize = 3;
    pub const OCTAVE: usize = 4;
    pub const COARSE: usize = 5;
    pub const FINE: usize = 6;
    pub const CUTOFF: usize = 8;
    pub const RESONANCE: usize = 9;
    pub const FILTER_TYPE: usize = 10;
    pub const DRIVE: usize = 11;
    pub const ALGORITHM: usize = 12;
    pub const FILTER_ACTIVE: usize = 13;
    pub const AMP_ENV: usize = 14;
    pub const FREE_ENV: usize = 22;
    pub const SAMPLE_START: usize = 30;
    pub const SAMPLE_END: usize = 31;
    pub const LOOP_START: usize = 32;
    pub const LOOP_END: usize = 33;
    pub const LOOP_IN_RELEASE: usize = 34;
    pub const ALT_LOOP: usize = 35;
    pub const XFADE_TIME: usize = 36;
    pub const XFADE_CURVE: usize = 37;
    pub const TS_ACTIVE: usize = 38;
    pub const TS_AMOUNT: usize = 39;
    pub const TS_TRACKS_BPM: usize = 40;
    pub const TS_MODE: usize = 41;
}

/// Step lanes the voice reads (step graph lanes).
pub mod lane {
    pub const VELOCITY: usize = 0;
    pub const PAN: usize = 1;
    pub const REVERSE: usize = 4;
    pub const OCTAVE: usize = 5;
    pub const COARSE: usize = 6;
    pub const FINE: usize = 7;
    pub const START: usize = 8;
    pub const CUTOFF: usize = 11;
    pub const RESONANCE: usize = 12;
    pub const FILTER_TYPE: usize = 13;
    pub const DRIVE: usize = 14;
    pub const LOOP_START: usize = 17;
    pub const LOOP_END: usize = 18;
    pub const AMP_ENV: usize = 23;
    pub const FREE_ENV: usize = 31;
}

/// A parameter row at its centre (0.5) with no modulation.
pub fn centre_row() -> [f32; SOURCES] {
    let mut r = [0.0; SOURCES];
    r[0] = 0.5;
    r
}

/// Parameter vector sum: Σ row·M clamped to 0..1 (the base value alone when
/// nothing modulates it).
fn row_sum(row: &[f32; SOURCES], m: &[f32; SOURCES]) -> f32 {
    if row[1..].iter().all(|d| *d == 0.0) {
        return row[0];
    }
    row.iter().zip(m).map(|(a, b)| a * b).sum::<f32>().clamp(0.0, 1.0)
}

/// Layer playback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    OneShot,
    OneShotLengthBound,
    Gated,
    Looping,
}

/// `Layer` playback mode enum names, by index.
pub const PLAYBACK_MODE_NAMES: [&str; 4] = ["OneShot", "OneShotLengthBound", "Gated", "Looping"];
/// Display names (replaced in [re:c38326]).
/// The replacement rewrites the map [re:87acba] uses, so these are
/// also the names stored in files.
pub const PLAYBACK_MODE_LABELS: [&str; 4] = ["One shot", "Length bound", "Gated", "Looping"];

impl PlaybackMode {
    fn index(self) -> u32 {
        self as u32
    }

    /// A stored `PlaybackMode` (the display name; enum names are accepted
    /// too, as older port builds wrote them). Unknown → OneShot.
    pub fn from_name(s: &str) -> PlaybackMode {
        let s = s.trim();
        match PLAYBACK_MODE_LABELS.iter().position(|n| *n == s).or_else(|| PLAYBACK_MODE_NAMES.iter().position(|n| *n == s)) {
            Some(1) => PlaybackMode::OneShotLengthBound,
            Some(2) => PlaybackMode::Gated,
            Some(3) => PlaybackMode::Looping,
            _ => PlaybackMode::OneShot,
        }
    }

    /// The enum name (`PLAYBACK_MODE_NAMES`) of a stored value.
    pub fn canonical(s: &str) -> &'static str {
        PLAYBACK_MODE_NAMES[PlaybackMode::from_name(s).index() as usize]
    }
}

/// Audio for one layer, de-interleaved.
#[derive(Debug, Clone)]
pub struct SampleData {
    pub left: Vec<f32>,
    pub right: Option<Vec<f32>>,
    pub sample_rate: f32,
}

impl SampleData {
    fn frames(&self) -> u32 {
        self.left.len() as u32
    }
}

/// A step's 93 graph lane values.
pub type Lanes = [f32; 93];

/// Everything a note needs at start.
#[derive(Debug, Clone)]
pub struct NoteSetup {
    /// `[parameter][source]`; source 0 is the base value (normalized, clamped as loaded).
    pub layer: [[f32; SOURCES]; LAYER_PARAMS],
    pub playback: PlaybackMode,
    pub vintage: bool,
    /// [re:f79f64] start/end (StartEndMarkers) and loop markers, frames.
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub loop_len: u32,
    /// Graph lane values of the triggering step (93), or `None` for a live note.
    pub step: Option<Lanes>,
    /// Live-note velocity 0..=127 (`None` = sequenced; then the step's Velocity lane is used).
    pub midi_velocity: Option<u32>,
    /// Pad `V2A` (velocity-to-amp depth).
    pub pad_v2a: f32,
    /// Extra semitones from the caller (global tuning / note offset).
    pub semitone_offset: f64,
    /// Layer per-voice output scalar (1.0 unless set).
    pub output_scale: f32,
    /// Layer `IsReversed`; XORed with the step's Reverse lane.
    pub reversed: bool,
    /// Layer `EnvTracksPitch` (Env Mode "Track Pitch"): the
    /// (non-AHR) envelopes run at the playback rate ([re:97a3c3]
    /// [re:c43a03]: rate = 1 / (1 / step), or the timestretch factor's
    /// inverse).
    pub env_tracks: bool,
    /// Sample tempo for TracksBPM timestretch (`.gsa` metadata BPM, else
    /// the BPM guessed from the audio; 0 when no guess fits).
    pub file_bpm: f32,
    /// The layer's stored `SourceBPM` and `DestinationBPM` (documents of
    /// the original v1); [`crate::rules::Rules::timestretch_v1`] tracks the
    /// host tempo from the source BPM.
    pub stretch_bpm: Option<(f32, f32)>,
}

/// [`crate::rules::Rules::reverse_v1`] reads a one-shot or gated note
/// (not a loop) as the mirror of its forward position.
fn mirrors(setup: &NoteSetup) -> bool {
    crate::rules::get().reverse_v1 && setup.playback != PlaybackMode::Looping
}

/// Neutral graph values used when a note has no step
/// ([re:265aa1], table [re:672bdd]).
fn neutral_lane(i: usize) -> f32 {
    const T: [f32; 18] = [0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.5, 0.0, 0.0, 0.5, 0.5];
    if (23..=38).contains(&i) {
        0.5
    } else if (1..=18).contains(&i) {
        T[i - 1]
    } else {
        0.0
    }
}

/// [re:365a03]: position, markers and loop state.
#[derive(Debug, Clone, Default)]
struct Playback {
    frames: u32,
    start: u32,
    end: u32,
    loop_start: u32,
    loop_end: u32,
    loop_len: u32,
    xfade_len: u32,
    xfade_exp: f32,
    in_loop: bool,
    reverse: bool,
    /// [`crate::rules::Rules::reverse_v1`]: the position runs forward and
    /// the read point mirrors it (`start + end − pos`).
    mirror: bool,
    pos: f64,
    step: f64,
    base: f64,
}

impl Playback {
    /// Playback position advance [re:471307].
    fn advance(&mut self) {
        if self.reverse {
            self.pos = (self.pos - self.step).max(0.0);
        } else {
            self.pos = (self.step + self.pos).min((self.frames as i32 - 1) as f64);
        }
    }

    /// Playback advance in a loop [re:86fa41] (`ping_pong` = AltLoopMode).
    fn advance_in_loop(&mut self, idx: i32, ping_pong: bool) {
        let (ls, le) = (self.loop_start as i32, self.loop_end as i32);
        if ls <= idx && idx <= le {
            self.in_loop = true;
        }
        let new = if ping_pong {
            if !self.reverse {
                (idx >= le).then(|| {
                    self.reverse = true;
                    (le * 2 - idx) as f64
                })
            } else {
                (ls >= idx).then(|| {
                    self.reverse = false;
                    (ls * 2 - idx) as f64
                })
            }
        } else if !self.reverse {
            (idx >= le).then(|| ((idx + ls) - le) as f64)
        } else {
            let d = idx - ls;
            (!(d != 0 && ls <= idx)).then(|| (d + le) as f64)
        };
        if let Some(p) = new {
            self.pos = p;
        }
        let last = (self.frames as i32 - 1) as f64;
        let p = if self.pos < last { self.pos } else { last };
        self.pos = if 0.0 < p { p } else { 0.0 };
    }

    /// v1's forward loop: wrap at `le − 3` to `ls + (pos − le) + 3`, keeping
    /// the fractional position.
    fn advance_in_loop_v1(&mut self) {
        let (ls, le) = (self.loop_start as f64, self.loop_end as f64);
        if ls <= self.pos && self.pos <= le {
            self.in_loop = true;
        }
        if self.pos >= le - 3.0 {
            self.pos = ls + (self.pos - le) + 3.0;
        }
        let last = (self.frames as i32 - 1) as f64;
        self.pos = self.pos.min(last).max(0.0);
    }

    /// Crossfade setup [re:2a490d].
    fn init_xfade(&mut self, curve: f32, time: f32) {
        let n = (time * 0.5 * self.loop_len as f32).max(8.0).floor() as i32 as u32;
        let room = self.loop_start.wrapping_sub(self.start);
        self.xfade_len = n.min(room);
        self.xfade_exp = if (curve - 0.5).abs() <= 1e-6 {
            1.0
        } else if 0.5 <= curve {
            curve * 4.0 - 1.0
        } else {
            curve + 0.5
        };
    }
}

pub struct Voice {
    sr: f32,
    data: Option<Arc<SampleData>>,
    setup: Option<NoteSetup>,
    lanes: Lanes,
    pb: Playback,
    amp_env: Envelope,
    free_env: Envelope,
    amp_ahr: AhrEnvelope,
    free_ahr: AhrEnvelope,
    filter: VoiceFilter,
    mix: Mixable,
    sp1200: Sp1200,
    /// Transmod raw values, scales and offsets (transmod update [re:eb94fb]).
    pub transmod_values: [f32; SOURCES],
    pub transmod_scale: [f32; SOURCES],
    pub transmod_offset: [f32; SOURCES],
    modv: [f32; SOURCES],
    velocity: f32,
    vel_gain: f32,
    prev: [f32; 2],
    first_block: bool,
    releasing: bool,
    active: bool,
    cut: Option<(f32, f32)>,
    /// The block sample a cut fade starts at ([`crate::rules::Rules::end_fade_v1`]).
    cut_from: usize,
    buf: [Vec<f32>; 4],
    /// Timestretch (Signalsmith substitute for DIRAC), its transpose and
    /// stretch factors, and the host tempo for TracksBPM.
    /// Default and Fast instances, built once ([`Voice::prepare_timestretch`])
    /// and reset at each note start.
    stretchers: [Option<Box<crate::stretch::Stretcher>>; 2],
    /// The instance the current note uses.
    stretch: Option<usize>,
    /// Stretcher input (reused).
    ts_in: [Vec<f32>; 2],
    ts_pitch: f32,
    /// The installed parameter model's output for the block (reused).
    model_out: crate::model::ModelOutput,
    ts_factor: f32,
    pub host_bpm: f32,
    /// [re:78f636] `Pitch` (normalized; read through the delegate in the
    /// pitch parameter update [re:ecbc78]).
    pub master_pitch: f32,
    /// Engine and pad Pitch with their modulation depths, set by the host
    /// every block: the pitch parameter update sums them against the voice's
    /// modulation vector (parameter vector sum) ev_04ca7b2c….
    pub engine_pitch: [f32; SOURCES],
    pub pad_pitch: [f32; SOURCES],
}

impl Voice {
    pub fn new(sample_rate: f32) -> Voice {
        Voice {
            sr: sample_rate,
            data: None,
            setup: None,
            lanes: std::array::from_fn(neutral_lane),
            pb: Playback::default(),
            amp_env: Envelope::new(sample_rate),
            free_env: Envelope::new(sample_rate),
            amp_ahr: AhrEnvelope::default(),
            free_ahr: AhrEnvelope::default(),
            filter: VoiceFilter::new(sample_rate),
            mix: Mixable::default(),
            sp1200: Sp1200::new(sample_rate),
            transmod_values: [0.0; SOURCES],
            transmod_scale: [1.0; SOURCES],
            transmod_offset: [0.0; SOURCES],
            modv: [0.0; SOURCES],
            velocity: 1.0,
            vel_gain: 1.0,
            prev: [0.0; 2],
            first_block: false,
            releasing: false,
            active: false,
            cut: None,
            cut_from: 0,
            buf: std::array::from_fn(|_| Vec::with_capacity(256)),
            stretch: None,
            stretchers: [None, None],
            ts_in: [Vec::with_capacity(1024), Vec::with_capacity(1024)],
            ts_pitch: 1.0,
            model_out: crate::model::ModelOutput::default(),
            ts_factor: 1.0,
            host_bpm: 120.0,
            master_pitch: 0.5,
            engine_pitch: centre_row(),
            pad_pitch: centre_row(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Current transmod multipliers `M_i` (used for layer/pad/engine gain and sends).
    pub fn modulation(&self) -> &[f32; SOURCES] {
        &self.modv
    }

    /// The note's step lanes after randomisation (pad FX read these).
    pub fn lanes(&self) -> Lanes {
        self.lanes
    }

    fn layer(&self) -> &[[f32; SOURCES]; LAYER_PARAMS] {
        &self.setup.as_ref().expect("voice started").layer
    }

    /// [re:b35cce] via the parameter vector sum.
    fn modulated(&self, p: usize) -> f32 {
        let row = &self.layer()[p];
        if row[1..].iter().all(|d| *d == 0.0) {
            return row[0];
        }
        row.iter().zip(&self.modv).map(|(a, b)| a * b).sum::<f32>().clamp(0.0, 1.0)
    }

    fn lane(&self, i: usize) -> f32 {
        self.lanes[i]
    }

    /// Build both timestretch instances and size their buffers up front, so
    /// notes in the timestretch pool start without allocating.
    pub fn prepare_timestretch(&mut self) {
        for (i, st) in self.stretchers.iter_mut().enumerate() {
            st.get_or_insert_with(|| Box::new(crate::stretch::Stretcher::new(self.sr, i == 1)));
        }
        let need = self.stretchers.iter().flatten().map(|s| s.max_input(crate::fx::NODE_BLOCK)).max().unwrap_or(0);
        for b in &mut self.ts_in {
            b.reserve(need);
        }
    }

    /// Voice setup for playback [re:2c96ac] for a sequenced or live layer note.
    pub fn start(&mut self, data: Arc<SampleData>, setup: NoteSetup) {
        self.lanes = match &setup.step {
            Some(s) => *s,
            None => std::array::from_fn(neutral_lane),
        };
        // SetupVelocity [re:08e1f4].
        self.velocity = match setup.midi_velocity {
            Some(v) => v as f32 * (1.0 / 127.0),
            None if setup.step.is_some() => self.lanes[lane::VELOCITY],
            None => 1.0,
        };
        let v2a = setup.pad_v2a;
        let amp_vel = if v2a.abs() > 1e-6 { ((self.velocity * v2a + (1.0 - v2a)) * 127.0) as i32 } else { 127 };
        self.vel_gain = amp_vel as f32 * 0.007874016;
        // SetupSamplePlaybackPoints [re:9b4ba6] (step StartPoint lane shifts the start).
        let mut start = setup.start as f64;
        let end = setup.end as f64;
        if setup.step.is_some() {
            let o = self.lanes[lane::START] - 0.5 + self.lanes[lane::START] - 0.5;
            if o.abs() >= 1e-6 {
                start = if o >= 0.0 { ((o as f64) * ((end - 1.0) - start) + start).min(end - 1.0) } else { (o as f64 * start + start).max(0.0) };
            }
        }
        self.pb = Playback {
            frames: data.frames(),
            start: start.floor() as u32,
            end: (end.floor() as u32).saturating_sub(1),
            loop_start: setup.loop_start,
            loop_end: setup.loop_end,
            loop_len: setup.loop_len,
            base: data.sample_rate as f64 / self.sr as f64,
            ..Default::default()
        };
        // Parameter update: reverse = layer IsReversed xor step Reverse lane.
        self.pb.reverse = ((self.lane(lane::REVERSE) - 1.0).abs() <= 1e-6) ^ setup.reversed;
        if mirrors(&setup) {
            self.pb.mirror = self.pb.reverse;
            self.pb.reverse = false;
        }
        self.pb.pos = if self.pb.reverse { self.pb.end.saturating_sub(1) as f64 } else { self.pb.start as f64 };
        self.data = Some(data);
        // InitializeTimeStretch [re:440f34]: Default/Fast instance by TimestretchMode.
        self.stretch = None;
        if setup.layer[lp::TS_ACTIVE][0] >= 0.5 {
            let fast = setup.layer[lp::TS_MODE][0] >= 0.5;
            let sr = self.sr;
            self.stretchers[fast as usize].get_or_insert_with(|| Box::new(crate::stretch::Stretcher::new(sr, fast))).reset();
            self.stretch = Some(fast as usize);
        }
        self.setup = Some(setup);
        self.sp1200.reset();
        self.filter.reset();
        self.amp_env.note_on();
        self.free_env.note_on();
        self.amp_ahr.note_on();
        self.free_ahr.note_on();
        self.prev = [0.0; 2];
        self.first_block = true;
        self.releasing = false;
        self.cut = None;
        self.cut_from = 0;
        self.active = true;
        self.update_parameters(0);
    }

    /// Release phase start [re:9c044b].
    /// Live layer-parameter change ([re:34ea12]): the voice
    /// reads its layer every block, so active voices follow.
    pub fn set_layer_param(&mut self, param: usize, source: usize, value: f32) {
        if let Some(s) = self.setup.as_mut() {
            if param < LAYER_PARAMS && source < SOURCES {
                s.layer[param][source] = value;
            }
        }
    }

    /// The layer's live `IsReversed` (read every block under
    /// [`crate::rules::Rules::reverse_v1`]).
    pub fn set_reversed(&mut self, on: bool) {
        if let Some(s) = self.setup.as_mut() {
            s.reversed = on;
        }
    }

    pub fn release(&mut self) {
        if !self.active || self.releasing {
            return;
        }
        self.releasing = true;
        if self.setup.as_ref().is_some_and(|s| s.playback.index() > 1) {
            self.amp_env.note_off();
            self.amp_ahr.note_off();
            self.free_env.note_off();
            self.free_ahr.note_off();
        }
    }

    /// Cut phase start [re:490648]: linear fade over `len` frames (cut groups use 32).
    pub fn cut(&mut self, len: f64) {
        if self.cut.is_some() {
            return;
        }
        let len = if 0.0 < len { len } else { 1.0 };
        let len = if crate::rules::get().end_fade_v1 && !self.pb.reverse {
            // v1 [re:093661]: up to the end marker, from the position + 2.
            let over = self.pb.pos + 2.0 + len - self.pb.end as f64;
            if 0.0 <= over { len - over } else { len }
        } else {
            let remain = self.pb.frames as f64 - self.pb.pos;
            if len + self.pb.pos < self.pb.frames as f64 { len } else { remain }
        };
        let n = if 0.0 < len { len as i32 - 1 } else { 0 };
        self.cut = Some((1.0, 1.0 / n as f32));
    }

    fn env_params(&self, base: usize, lane0: usize) -> EnvelopeParams {
        let v = |i: usize| (self.modulated(base + i) + (self.lane(lane0 + i) * 2.0 - 1.0)).clamp(0.0, 1.0);
        EnvelopeParams { attack: v(0), decay: v(1), sustain: v(2), hold: v(3), release: v(4), attack_curve: v(5), decay_curve: v(6), release_curve: v(7) }
    }

    /// Parameter update [re:086a94] (`n` = block length, 0 at note start).
    fn update_parameters(&mut self, n: usize) {
        // Transmod update: envelope maxima over the block, velocity, external sources.
        let mut t = self.transmod_values;
        t[0] = 1.0;
        if n > 0 {
            t[1] = self.buf[2][..n].iter().fold(0.0f32, |a, b| a.max(*b));
            t[2] = self.buf[3][..n].iter().fold(0.0f32, |a, b| a.max(*b));
        }
        t[3] = self.velocity;
        for i in 0..SOURCES {
            self.modv[i] = t[i] * self.transmod_scale[i] - self.transmod_offset[i];
        }
        self.modv[0] = 1.0;

        // The playback points update runs once, in the first block
        // (flagged); the note-start call (n = 0) must not move the
        // points, or they compound.
        if self.first_block && n > 0 {
            self.update_sample_points();
        }
        self.update_loop_points();

        // v1 [re:18d53d]: the direction follows the live layer every block.
        if self.setup.as_ref().is_some_and(mirrors) {
            let rev = self.setup.as_ref().is_some_and(|s| s.reversed);
            self.pb.mirror = ((self.lane(lane::REVERSE) - 1.0).abs() <= 1e-6) ^ rev;
        }

        // Borrowed field by field (not copied: ~5 KB per block).
        let setup = self.setup.as_ref().expect("voice started");

        // An installed parameter model replaces the pitch and filter
        // parameters below.
        let mut model = crate::model::get();
        if let Some(m) = model {
            let outer = semitones(row_sum(&self.pad_pitch, &self.modv), true) + semitones(row_sum(&self.engine_pitch, &self.modv), true) + semitones(self.master_pitch, true);
            let input = crate::model::ModelInput {
                layer: &setup.layer,
                lanes: &self.lanes,
                velocity: self.velocity,
                amp_env: &self.buf[2][..n],
                free_env: &self.buf[3][..n],
                outer_semitones: outer,
                semitone_offset: setup.semitone_offset,
                base: self.pb.base,
            };
            if !m.update(&input, &mut self.model_out) {
                model = None;
                self.model_out.steps.clear();
            }
            // The timestretcher pulls the source at its own rate.
            if self.stretch.is_some() {
                self.model_out.steps.clear();
            }
        }

        // Pitch parameter update [re:c6d172].
        let l = &setup.layer;
        let depth = |p: usize| l[p][1..].iter().zip(&self.modv[1..]).map(|(a, b)| a * b).sum::<f32>();
        let st = fine_semitones(l[lp::FINE][0])
            + semitones(l[lp::COARSE][0], true)
            + semitones((depth(lp::COARSE) + 1.0) * 0.5, false)
            + fine_semitones((depth(lp::FINE) + 1.0) * 0.5)
            + fine_semitones(self.lane(lane::FINE))
            + semitones(row_sum(&self.pad_pitch, &self.modv), true)
            + setup.semitone_offset
            + (octave((depth(lp::OCTAVE) + 1.0) * 0.5, false) + octave(l[lp::OCTAVE][0], true)) * 12.0
            + semitones(row_sum(&self.engine_pitch, &self.modv), true)
            + semitones(self.master_pitch, true)
            + semitones(self.lane(lane::COARSE), true)
            + octave(self.lane(lane::OCTAVE), true) * 12.0;
        let st = if model.is_some() { self.model_out.semitones } else { st };
        self.pb.step = if model.is_some() { self.model_out.step } else { crate::convert::pitch_step(st, self.pb.base) };
        if self.stretch.is_some() {
            // Timestretch parameter update [re:e9b827]: DIRAC pulls the source at
            // step 1.0, pitch 2^(st/12) (property 100), stretch
            // factor (property 101) from the amount or file/host tempo.
            self.pb.step = 1.0;
            let tracks = l[lp::TS_TRACKS_BPM][0] >= 0.5;
            if crate::rules::get().timestretch_v1 {
                // v1 [re:d8985e]: tracking, the layer's source BPM over the
                // host's; otherwise source over destination (the amount);
                // at most 4. The pitch ratio is at least 0.5.
                let ratio = match setup.stretch_bpm {
                    Some((src, _)) if tracks => src as f64 / self.host_bpm as f64,
                    _ if tracks => (setup.file_bpm / self.host_bpm.max(1.0)) as f64,
                    _ => crate::stretch::amount_factor(self.modulated(lp::TS_AMOUNT)) as f64,
                };
                self.ts_factor = ratio.min(4.0) as f32;
                self.ts_pitch = 2f64.powf(st / 12.0).max(0.5) as f32;
            } else {
                self.ts_pitch = (st / 12.0).exp2() as f32;
                self.ts_factor = if tracks { setup.file_bpm / self.host_bpm.max(1.0) } else { crate::stretch::amount_factor(self.modulated(lp::TS_AMOUNT)) };
            }
        }

        // UpdateFilterParameters [re:9f92ff].
        let algo = l[lp::ALGORITHM][0];
        let ftype = filter_type_from_step(self.lane(lane::FILTER_TYPE), algo, l[lp::FILTER_TYPE][0]);
        let (cut, res, drive) = if n == 0 {
            let c = cutoff_with_velocity(l[lp::CUTOFF][0], l[lp::CUTOFF][3], self.velocity);
            (modulate_pos_neg(c, self.lane(lane::CUTOFF)), modulate_pos_neg(l[lp::RESONANCE][0], self.lane(lane::RESONANCE)), l[lp::DRIVE][0])
        } else {
            (modulate_pos_neg(self.modulated(lp::CUTOFF), self.lane(lane::CUTOFF)), modulate_pos_neg(self.modulated(lp::RESONANCE), self.lane(lane::RESONANCE)), self.modulated(lp::DRIVE))
        };
        let drive = modulate_pos_neg(drive, self.lane(lane::DRIVE));
        let (cut, res, drive) = if model.is_some() { (self.model_out.cutoff, self.model_out.resonance, self.model_out.drive) } else { (cut, res, drive) };
        self.filter.initialize(cut, res, ftype, drive, algo);

        // UpdateEnvelopeParameters [re:ea1070].
        let amp = self.env_params(lp::AMP_ENV, lane::AMP_ENV);
        let free = self.env_params(lp::FREE_ENV, lane::FREE_ENV);
        let mode = setup.playback.index();
        let length = self.pb.end.saturating_sub(self.pb.start);
        if setup.playback == PlaybackMode::OneShotLengthBound {
            self.amp_ahr.initialize(&amp, length, mode, 1.0);
            self.free_ahr.initialize(&free, length, mode, 1.0);
        } else {
            let rate = if !setup.env_tracks {
                1.0
            } else if self.stretch.is_some() {
                1.0 / self.ts_factor.max(1e-6)
            } else {
                self.pb.step as f32
            };
            self.amp_env.initialize(&amp, mode < 2, rate);
            self.free_env.initialize(&free, mode < 2, rate);
        }

        // UpdateMixParameters [re:2847ff].
        let g = self.modulated(lp::VOICE_GAIN);
        // v1 pans by the Pan lane at the layer instead (`Rules::pan_v1`).
        let p = if crate::rules::get().pan_v1 { self.modulated(lp::VOICE_PAN) } else { modulate_pos_neg(self.modulated(lp::VOICE_PAN), self.lane(lane::PAN)) };
        self.mix.set(g, p);
    }

    /// Sample start/end offsets at note start (playback points update).
    fn update_sample_points(&mut self) {
        let (s, e) = crate::convert::sample_points(self.pb.start, self.pb.end + 1, self.pb.frames, self.modulated(lp::SAMPLE_START), self.modulated(lp::SAMPLE_END));
        self.pb.start = s;
        self.pb.end = e.saturating_sub(1);
        self.pb.pos = if self.pb.reverse { self.pb.end.saturating_sub(1) as f64 } else { self.pb.start as f64 };
    }

    /// Loop points inside the start/end window, at least 32 frames apart.
    fn update_loop_points(&mut self) {
        let setup = self.setup.as_ref().expect("voice started");
        if setup.playback != PlaybackMode::Looping {
            return;
        }
        let (loop_start, loop_end, loop_len) = (setup.loop_start, setup.loop_end, setup.loop_len);
        let off = |base: f32, lanev: f32| (base + -1.0 + lanev + lanev).clamp(0.0, 1.0);
        let fs = off(self.modulated(lp::LOOP_START), self.lane(lane::LOOP_START));
        let fe = off(self.modulated(lp::LOOP_END), self.lane(lane::LOOP_END));
        let shift = |x: f32, base: u32, toward_end: u32, toward_start: u32| -> u32 {
            let o = x + x - 1.0;
            if o.abs() < 1e-6 {
                return base;
            }
            let (hi, lo) = if o >= 0.0 { (toward_end, base) } else { (base, toward_start) };
            let k = if o >= 0.0 { o * o * o } else { (1.0 - o.abs()).powi(3) };
            ((hi as f64 - lo as f64) * k as f64 + lo as f64).floor() as u32
        };
        let mut ls = shift(fs, loop_start, self.pb.end, self.pb.start);
        let mut le = shift(fe, loop_end, self.pb.end, ls);
        le += (le == ls) as u32;
        ls = ls.max(self.pb.start);
        le = le.min(self.pb.end);
        let (a, b) = if le < ls { (le, ls) } else { (ls, le) };
        let b = if b - a > 0x1f { b } else { a + 0x20 };
        self.pb.loop_start = a;
        self.pb.loop_end = b;
        self.pb.loop_len = loop_len;
        let curve = self.modulated(lp::XFADE_CURVE);
        let time = self.modulated(lp::XFADE_TIME);
        self.pb.init_xfade(curve, time);
    }

    fn looping(&self) -> Option<bool> {
        let s = self.setup.as_ref()?;
        if s.playback != PlaybackMode::Looping {
            return None;
        }
        if self.releasing && s.layer[lp::LOOP_IN_RELEASE][0] < 0.5 {
            return None;
        }
        Some(s.layer[lp::ALT_LOOP][0] >= 0.5)
    }

    /// Crossfade processing [re:83b67b] for forward wrap-around loops (the common case).
    fn xfade(&self, data: &SampleData, idx: i32, l: &mut f32, r: &mut f32) {
        let Some(ping_pong) = self.looping() else { return };
        if crate::rules::get().loop_v1 {
            return;
        }
        let pb = &self.pb;
        let n = pb.xfade_len as i32;
        if ping_pong || pb.reverse || n == 0 || pb.end == 0 {
            return;
        }
        let from = pb.loop_end as i32 - n;
        let u = idx - from;
        if u <= 0 || idx < pb.loop_start as i32 {
            return;
        }
        let u = u.min(n);
        let remain = n - u;
        let w = (1.0 - u as f32 / (n as f32).max(1.0)).clamp(0.0, 1.0);
        let src = ((pb.loop_start as i32 - n + u) % pb.end as i32) as usize;
        let k = if remain < 3 { if remain != 1 { w } else { 0.0 } } else { w.powf(pb.xfade_exp) };
        *l = data.left[src] * (1.0 - w) + *l * k;
        if let Some(rd) = &data.right {
            *r = (1.0 - w) * rd[src] + k * *r;
        }
    }

    /// Audio processing (standard and vintage), forward and reverse.
    fn render(&mut self, n: usize) -> usize {
        let Some(mut st) = self.stretch.and_then(|i| self.stretchers[i].take()) else {
            let mut out = std::mem::take(&mut self.buf[0]);
            let mut out_r = std::mem::take(&mut self.buf[1]);
            let w = self.read_source(n, &mut out, &mut out_r);
            self.buf[0] = out;
            self.buf[1] = out_r;
            return w;
        };
        let k = st.input_needed(n, self.ts_factor);
        let [mut il, mut ir] = std::mem::take(&mut self.ts_in);
        il.clear();
        il.resize(k, 0.0);
        ir.clear();
        ir.resize(k, 0.0);
        let got = self.read_source(k, &mut il, &mut ir);
        let mut out = std::mem::take(&mut self.buf[0]);
        let mut out_r = std::mem::take(&mut self.buf[1]);
        st.process([&il, &ir], [&mut out[..n], &mut out_r[..n]], self.ts_pitch, self.ts_factor);
        self.ts_in = [il, ir];
        self.buf[0] = out;
        self.buf[1] = out_r;
        let i = st.fast as usize;
        self.stretchers[i] = Some(st);
        if got < k { got.min(n) } else { n }
    }

    /// Source reader (audio processing / stretch source callback [re:5dc8dd]): `n` frames at
    /// the current step with loops, crossfades and the end fade.
    fn read_source(&mut self, n: usize, out_l: &mut [f32], out_r: &mut [f32]) -> usize {
        // An empty region (end point at 0: a region of at most one frame,
        // or a 0 / 1-frame file) has nothing to play; the reads below index
        // modulo the end.
        if self.pb.end == 0 {
            self.active = false;
            return 0;
        }
        let data = self.data.take().unwrap();
        let vintage = self.setup.as_ref().unwrap().vintage;
        let mut written = 0;
        let end_fade_v1 = crate::rules::get().end_fade_v1;
        for i in 0..n {
            let fl = self.pb.pos.floor();
            let end = self.pb.end as i32;
            let start = self.pb.start as i32;
            let past = if self.pb.reverse { (fl as i32) < start } else { end < fl as i32 };
            // The read point: the position, or its mirror in the window.
            let (fl, frac, rev) = if self.pb.mirror {
                let m = ((start + end) as f64 - self.pb.pos).max(start as f64);
                (m.floor(), (m - m.floor()) as f32, true)
            } else {
                (fl, (self.pb.pos - fl) as f32, self.pb.reverse)
            };
            let idx = fl as i32;
            if past {
                // ProcessEndFrameCheckWithLoop [re:5ca7d0].
                if let Some(pp) = self.looping() {
                    self.pb.advance_in_loop(idx, pp);
                    if (self.pb.pos as i32) < end {
                        continue;
                    }
                }
                self.active = false;
                break;
            }
            let at = |ch: &[f32], k: i32| ch[k as usize];
            let (mut l, mut r);
            if vintage {
                let k1 = if rev { (idx - 1).max(start) } else { (idx + 1) % end };
                let k0 = if rev { idx } else { idx % end };
                let a = vintage_tap(at(&data.left, k0), at(&data.left, k1), frac);
                let b = data.right.as_ref().map(|rd| vintage_tap(at(rd, k0), at(rd, k1), frac));
                (l, r) = self.sp1200.process(a, b);
            } else {
                let taps = |ch: &[f32]| -> [f32; 3] {
                    if rev { [at(ch, idx), at(ch, (idx - 1).max(start)), at(ch, (idx - 2).max(start))] } else { [at(ch, idx % end), at(ch, (idx + 1) % end), at(ch, (idx + 2) % end)] }
                };
                let [a0, a1, a2] = taps(&data.left);
                l = catmull_rom(self.prev[0], a0, a1, a2, frac);
                r = 0.0;
                if let Some(rd) = &data.right {
                    let [b0, b1, b2] = taps(rd);
                    r = catmull_rom(self.prev[1], b0, b1, b2, frac);
                    self.prev[1] = rd[idx as usize];
                }
                self.prev[0] = data.left[idx as usize];
            }
            self.xfade(&data, idx, &mut l, &mut r);
            out_l[i] = l;
            out_r[i] = if data.right.is_some() { r } else { l };
            written += 1;
            if let Some(&st) = self.model_out.steps.get(i) {
                self.pb.step = st;
            }
            self.pb.advance();
            if let Some(pp) = self.looping() {
                if !pp && !self.pb.reverse && crate::rules::get().loop_v1 {
                    self.pb.advance_in_loop_v1();
                } else {
                    self.pb.advance_in_loop(self.pb.pos.floor() as i32, pp);
                }
            } else if self.cut.is_none() {
                // 32-frame fade before the end of a one-shot; v1
                // [re:fece0b] starts it at end ≤ position + 34, from this
                // sample on.
                let near = if self.pb.reverse {
                    self.pb.pos.floor() < self.pb.start as f64 + 32.0
                } else if end_fade_v1 {
                    self.pb.end as f64 <= self.pb.pos + 2.0 + 32.0
                } else {
                    self.pb.end as f64 - 32.0 <= self.pb.pos.floor()
                };
                if near {
                    self.cut(32.0);
                    if end_fade_v1 && self.stretch.is_none() {
                        self.cut_from = i;
                    }
                }
            }
        }
        self.data = Some(data);
        written
    }

    /// Block processing [re:fd90be]: render `left/right.len()` frames, mixing into the outputs.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.active {
            return;
        }
        let n = left.len();
        for b in &mut self.buf {
            b.clear();
            b.resize(n, 0.0);
        }
        let ahr = self.setup.as_ref().unwrap().playback == PlaybackMode::OneShotLengthBound;
        let env_alive = if ahr {
            let a = self.amp_ahr.process(&mut self.buf[2]);
            self.free_ahr.process(&mut self.buf[3]);
            a
        } else {
            let a = self.amp_env.process(&mut self.buf[2]);
            self.free_env.process(&mut self.buf[3]);
            a
        };
        self.update_parameters(n);
        self.first_block = false;
        let written = self.render(n);
        if self.layer()[lp::FILTER_ACTIVE][0] >= 0.5 {
            let (a, rest) = self.buf.split_at_mut(1);
            self.filter.process(&mut a[0][..n], &mut rest[0][..n], 1.0 / n as f32);
        }
        if let Some((mut g, dg)) = self.cut {
            for i in std::mem::take(&mut self.cut_from).min(n)..n {
                self.buf[2][i] *= g;
                g -= dg;
                if g < 0.0 {
                    self.buf[2][i..].fill(0.0);
                    self.active = false;
                    break;
                }
            }
            self.cut = Some((g.max(0.0), dg));
        }
        // Output stage [re:55cc56].
        let (a, rest) = self.buf.split_at_mut(1);
        self.mix.process(&mut a[0][..n], &mut rest[0][..n]);
        let scale = self.setup.as_ref().unwrap().output_scale * self.vel_gain;
        for i in 0..n.min(written.max(n)) {
            let e = self.buf[2][i] * scale;
            left[i] += self.buf[0][i] * e;
            right[i] += self.buf[1][i] * e;
        }
        if !env_alive {
            self.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(frames: u32) -> NoteSetup {
        let mut layer = [[0.0; SOURCES]; LAYER_PARAMS];
        for (p, row) in layer.iter_mut().enumerate() {
            row[0] = match p {
                0 | 2 => 0.5f32.cbrt(),
                8 | 16 | 24 => 1.0,
                11 | 12 => 0.3,
                15 | 23 => 0.02,
                // Saved one-shot layers always carry a hold sized to the sample.
                17 | 25 => 1.0,
                18 | 26 => 0.001,
                7 | 9 | 10 | 13 | 14 | 22 | 34..=36 | 38 | 40 | 41 => 0.0,
                _ => 0.5,
            };
        }
        NoteSetup {
            layer,
            playback: PlaybackMode::OneShot,
            vintage: false,
            start: 0,
            end: frames,
            loop_start: 0,
            loop_end: frames,
            loop_len: frames,
            step: None,
            midi_velocity: Some(127),
            pad_v2a: 1.0,
            semitone_offset: 0.0,
            output_scale: 1.0,
            env_tracks: false,
            reversed: false,
            file_bpm: 0.0,
            stretch_bpm: None,
        }
    }

    #[test]
    fn plays_a_one_shot_at_unity() {
        let sr = 44100.0;
        let src: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let data = SampleData { left: src.clone(), right: None, sample_rate: sr };
        let mut v = Voice::new(sr);
        v.start(Arc::new(data), setup(4000));
        let mut out = vec![0.0; 6000];
        let mut out_r = vec![0.0; 6000];
        for (l, r) in out.chunks_mut(64).zip(out_r.chunks_mut(64)) {
            v.process(l, r);
        }
        assert!(!v.is_active());
        // Sustain 1 after a ~4-sample attack: the middle reproduces the source.
        for i in 100..3000 {
            assert!((out[i] - src[i]).abs() < 1e-3, "frame {i}: {} vs {}", out[i], src[i]);
        }
        assert!(out[4500].abs() < 1e-6);
    }

    /// The playback points update moves the region once per note (first
    /// block), not again from the moved points.
    #[test]
    fn sample_start_applies_once() {
        let sr = 44100.0;
        let mut s = setup(10000);
        s.layer[lp::SAMPLE_START][0] = 0.75;
        let end = s.layer[lp::SAMPLE_END][0];
        let mut v = Voice::new(sr);
        v.start(Arc::new(SampleData { left: vec![0.1; 10000], right: None, sample_rate: sr }), s);
        let (mut l, mut r) = ([0.0; 32], [0.0; 32]);
        v.process(&mut l, &mut r);
        v.process(&mut l, &mut r);
        let (want, _) = crate::convert::sample_points(0, 10000, 10000, 0.75, end);
        assert_eq!(v.pb.start, want);
    }

    #[test]
    fn octave_up_halves_duration() {
        let sr = 44100.0;
        let src = vec![0.25f32; 4000];
        let mut s = setup(4000);
        s.layer[4][0] = 0.625; // Octave +1 (floor(8·0.625 − 4) = 1).
        let mut v = Voice::new(sr);
        v.start(Arc::new(SampleData { left: src, right: None, sample_rate: sr }), s);
        let mut out = vec![0.0; 4000];
        let mut r = vec![0.0; 4000];
        for (a, b) in out.chunks_mut(64).zip(r.chunks_mut(64)) {
            v.process(a, b);
        }
        assert!(out[1500].abs() > 0.1);
        assert!(out[2200].abs() < 1e-6);
    }

    #[test]
    fn timestretch_doubles_duration() {
        let sr = 44100.0;
        let n = 20000;
        let src: Vec<f32> = (0..n).map(|i| (i as f32 * 0.06).sin() * 0.5).collect();
        let mut s = setup(n as u32);
        s.layer[lp::TS_ACTIVE][0] = 1.0;
        s.layer[lp::TS_AMOUNT][0] = 0.5 + 1.0 / 14.0; // factor 2
        let mut v = Voice::new(sr);
        v.start(Arc::new(SampleData { left: src, right: None, sample_rate: sr }), s);
        let total = 3 * n;
        let mut out = vec![0.0; total];
        let mut r = vec![0.0; total];
        let mut stopped = None;
        for (k, (a, b)) in out.chunks_mut(64).zip(r.chunks_mut(64)).enumerate() {
            v.process(a, b);
            if stopped.is_none() && !v.is_active() {
                stopped = Some(k * 64);
            }
        }
        let stop = stopped.expect("voice ends");
        // ~2·n output frames (minus the pre-roll the stretcher consumed).
        assert!(stop > n * 3 / 2 && stop < n * 5 / 2, "stopped at {stop}");
        assert!(out[n].abs() + out[n + 7].abs() + out[n + 13].abs() > 1e-3);
    }
}
