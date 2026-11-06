//! The engine runtime: [re:93ab44] → voice processing [re:e720f7].
//! Splits each host block into sub-blocks of at most 32 frames (cut at song
//! changes), runs the song sequencer and step sequencer, starts voices and
//! mixes layer → pad → engine → master ([re:eba3c6]
//! [re:4a70f5]). See the dreist-re mixer notes.

pub mod doc;
mod messages;
mod midi;

pub use midi::MidiEvent;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dreist_dsp::mix::Mixable;
use dreist_dsp::voice::{LAYER_PARAMS, NoteSetup, PlaybackMode, SOURCES, SampleData, Voice};
use dreist_dsp::voices::{CUT_FADE, CutMode, LayerMode, LayerPicker, LayerSlot, NORMAL_VOICES, Slot, TAIL_SILENCE, TIMESTRETCH_VOICES, VoiceInfo, VoiceState, is_cut, steal};
use dreist_formats::model::samples_dir;
use dreist_formats::params::aux_send_name;
use dreist_formats::{EngineParam, LayerParam, Node, PadParam, Project, TransmodSource};

use crate::audio::SampleCache;
use crate::binding::{BindingMap, Message};
use crate::fx::FxChain;
use crate::live::{MidiMap, PadMode};
use crate::metronome::{Metronome, default_metronomes_dir};
use crate::modulation::{Modulator, TransmodHost, process_transmod, src};
use crate::pattern::{PATTERNS, PatternData, Swing, TRACKS, lane, randomised};
use crate::sampler::{Sampler, SamplerClock};
use crate::scene::SceneManager;
use crate::sequencer::{Clock, EngineState, EngineTiming, NoteRequest, PadInfo, Rng, process_engine};
use crate::song::{Change, ChangeKind, EngineView, Song, SongCursor, phase_mod};
use dreist_dsp::fx::Ctx;

/// Maximum sub-block length (`start + 0x20`).
pub const SUB_BLOCK: usize = 32;

/// Host block the per-block buffers are reserved for at construction (the
/// sampler's input copies); longer host blocks grow them once.
pub const MAX_HOST_BLOCK: usize = 8192;

/// Queued MIDI out events (the original's 0x1ff-entry queue).
const MIDI_PENDING_MAX: usize = 0x1ff;
pub const ENGINES: usize = 8;
pub const LAYERS: usize = 8;

/// A parameter and its transmod depths.
pub type Row = [f32; SOURCES];

fn row(node: &Node, name: &str, default: f32) -> Row {
    let mut r = [0.0; SOURCES];
    r[0] = node.f32(name).unwrap_or(default).clamp(0.0, 1.0);
    for (i, s) in TransmodSource::ALL.iter().enumerate().skip(1) {
        if let Some(k) = s.attr(name) {
            r[i] = node.f32(&k).unwrap_or(0.0).clamp(-1.0, 1.0);
        }
    }
    r
}

/// [re:846d50]: base when unmodulated, else Σ p_i·M_i clamped.
pub fn param_sum(r: &Row, m: &[f32; SOURCES]) -> f32 {
    if r[1..].iter().all(|d| *d == 0.0) {
        return r[0];
    }
    r.iter().zip(m).map(|(a, b)| a * b).sum::<f32>().clamp(0.0, 1.0)
}

/// Number of stereo sub outputs ([re:ebd7d8] locations 2..=33).
pub const SUB_OUTPUTS: usize = 32;

/// [re:7ea458] (names [re:bae6b8]):
/// 0 Parent, 1 Master, 2..=33 "Sub output 1..32", 34..=37 "Aux 1..4".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Parent,
    Master,
    Sub(u8),
    Aux(u8),
}

impl Route {
    /// The output state stores the name; unknown → Parent
    /// ([re:fb17d5]).
    pub fn from_name(s: &str) -> Route {
        if s == "Master" {
            return Route::Master;
        }
        let num = |p: &str| s.strip_prefix(p).and_then(|n| n.parse::<u8>().ok());
        match (num("Sub output "), num("Aux ")) {
            (Some(k @ 1..=32), _) => Route::Sub(k - 1),
            (_, Some(k @ 1..=4)) => Route::Aux(k - 1),
            _ => Route::Parent,
        }
    }

    /// Output location index (the mixer OutputCombo item).
    pub fn from_index(i: usize) -> Route {
        match i {
            1 => Route::Master,
            2..=33 => Route::Sub((i - 2) as u8),
            34..=37 => Route::Aux((i - 34) as u8),
            _ => Route::Parent,
        }
    }

    /// The stored name (not the display name).
    pub fn name(self) -> String {
        match self {
            Route::Parent => "Parent".into(),
            Route::Master => "Master".into(),
            Route::Sub(k) => format!("Sub output {}", k + 1),
            Route::Aux(k) => format!("Aux {}", k + 1),
        }
    }

    fn of(n: &Node) -> Route {
        n.attr("OutputLocation").map(Route::from_name).unwrap_or_default()
    }
}

/// Stage output buffers other than the parent bus.
struct Outs<'a> {
    master: [&'a mut [f32]; 2],
    sub: &'a mut [[Vec<f32>; 2]],
    aux: &'a mut [[Vec<f32>; 2]],
}

fn add_into(dst: [&mut [f32]; 2], src: [&[f32]; 2]) {
    let [dl, dr] = dst;
    for i in 0..src[0].len() {
        dl[i] += src[0][i];
        dr[i] += src[1][i];
    }
}

/// Route a stage's output (layer effects [re:a74178].., global aux
/// [re:25503d]..): Parent → `parent` (the master bus when `None`), Master →
/// master bus, Sub n → output pair n, Aux n → the [re:fd4dd3] bus
/// ([re:21a3c8]). `allow_aux` is false for [re:fd4dd3] returns,
/// whose Aux locations index host channels past the sub outputs (dropped).
fn route(r: Route, src: [&[f32]; 2], parent: Option<[&mut [f32]; 2]>, o: &mut Outs, allow_aux: bool) {
    match r {
        Route::Parent => match parent {
            Some(p) => add_into(p, src),
            None => {
                let [ml, mr] = &mut o.master;
                add_into([ml, mr], src)
            }
        },
        Route::Master => {
            let [ml, mr] = &mut o.master;
            add_into([ml, mr], src)
        }
        Route::Sub(k) => {
            let [l, r] = &mut o.sub[k as usize];
            add_into([l, r], src);
        }
        Route::Aux(k) if allow_aux => {
            let [l, r] = &mut o.aux[k as usize];
            add_into([l, r], src);
        }
        Route::Aux(_) => {}
    }
}

/// `Gain` ([re:d073e0]): linear send gain ramped per sample from the last value.
#[derive(Debug, Clone, Copy, Default)]
struct SendGain {
    last: f32,
}

impl SendGain {
    /// [re:31cd26] + [re:90f68f].
    fn process(&mut self, target: f32, src: [&[f32]; 2], dst: [&mut [f32]; 2]) {
        if dreist_dsp::rules::get().sends_v1 {
            self.process_v1(target, src, dst);
            return;
        }
        if target <= 0.0 {
            return;
        }
        let n = src[0].len();
        let inc = (target - self.last) * (1.0 / n as f32);
        let [dl, dr] = dst;
        if inc.abs() < 1e-6 {
            for i in 0..n {
                dl[i] += src[0][i] * self.last;
                dr[i] += src[1][i] * self.last;
            }
            return;
        }
        for i in 0..n {
            self.last += inc;
            dl[i] += src[0][i] * self.last;
            dr[i] += src[1][i] * self.last;
        }
    }
}

impl SendGain {
    /// The original v1's send ([re:99f309], [re:be9258], [re:f918f1]): from
    /// the last level toward `target` over the block, the first sample at
    /// the last level; nothing while the last level is 0. The last level
    /// becomes `target` either way.
    fn process_v1(&mut self, target: f32, src: [&[f32]; 2], dst: [&mut [f32]; 2]) {
        let n = src[0].len();
        let last = std::mem::replace(&mut self.last, target);
        if n == 0 || last <= 0.0 {
            return;
        }
        let inc = (target - last) / n as f32;
        let [dl, dr] = dst;
        let mut g = last;
        let flat = inc.abs() < 1e-6;
        for i in 0..n {
            dl[i] += g * src[0][i];
            dr[i] += g * src[1][i];
            if !flat {
                g += inc;
            }
        }
    }
}

struct LayerRt {
    sample: Option<Arc<SampleData>>,
    template: Option<NoteSetup>,
    cut: CutMode,
    slot: LayerSlot,
    gain: Row,
    pan: Row,
    aux: [Row; 4],
    sends: [SendGain; 4],
    mix: Mixable,
    audible: bool,
    fx: FxChain,
    route: Route,
    mute: bool,
    solo: bool,
}

struct PadRt {
    layers: Vec<LayerRt>,
    mode: LayerMode,
    picker: LayerPicker,
    gain: Row,
    pan: Row,
    v2a: f32,
    pitch: Row,
    aux: [Row; 4],
    sends: [SendGain; 4],
    mix: Mixable,
    muted: bool,
    fx: FxChain,
    route: Route,
    mute: bool,
    solo: bool,
}

struct EngineRt {
    pads: Vec<PadRt>,
    patterns: Vec<PatternData>,
    timing: EngineTiming,
    state: EngineState,
    gain: Row,
    pan: Row,
    pitch: Row,
    aux: [Row; 4],
    sends: [SendGain; 4],
    mix: Mixable,
    muted: bool,
    fx: FxChain,
    route: Route,
    one_shot: [bool; TRACKS],
    pad_mode: PadMode,
    mute: bool,
    solo: bool,
    links: Box<[[bool; TRACKS]; TRACKS]>,
    /// [re:fe3f2a] dummy pad steps: the lanes
    /// live notes read; set by 0xd9 while the pattern is off. None = the
    /// default step.
    dummy: Vec<Option<dreist_dsp::voice::Lanes>>,
}

struct AuxRt {
    mix: Mixable,
    muted: bool,
    fx: FxChain,
    route: Route,
    /// [re:490645] "IsPreFade", copied to
    /// [re:b02356].
    pre_fade: bool,
}

struct VoiceSlot {
    voice: Voice,
    state: VoiceState,
    id: u32,
    slot: Slot,
    cut: CutMode,
    /// Samples until the note starts ([re:ec8699]).
    delay: f64,
    /// Note length counter, -1 = none.
    duration: f64,
    pending: Option<(Arc<SampleData>, NoteSetup)>,
    /// Effect tail counter while in the tail state ([re:59365b]).
    tail: i64,
    has_step: bool,
    /// Live note and its semitone offset ([re:ea5bc5]).
    live: bool,
    semitone: f64,
    /// Live-note recording (delayed-start phase, MIDI velocity).
    record: Option<f64>,
    velocity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    /// Add the [re:fd4dd3] returns to the master (on by default; un-ported
    /// effect types pass audio through, so a return using one is wrong).
    pub aux_returns: bool,
    /// Seed for the step probability / randomisation generators.
    pub seed: u64,
    /// Option `ForcedLayersFallThrough` (read by [re:3048a1]
    /// [re:a37fb4] into +[re:cab206]).
    pub forced_layers_fall_through: bool,
    /// Option `AuxesArePreFade`: the aux sends' `IsPreFade` when a document
    /// does not say ([re:16ad9d] reads it).
    pub auxes_pre_fade: bool,
    /// Header options ([re:0daa5e]): `PrerollTime`
    /// 0 off / 1, 2, 4 bars, `PrerollOnlyOnRecord` (default on),
    /// `WaitForMIDI`, and metronome only while recording (read from the
    /// same `PrerollOnlyOnRecord` key, default off).
    pub preroll_mode: u32,
    pub preroll_only_rec: bool,
    pub wait_midi: bool,
    pub metro_only_rec: bool,
    /// `PadSelectionFollowsMIDI` (default on) / `EngineSelectionFollowsMIDI`
    /// (default off), read by [re:b7d4aa].
    pub pad_follow: bool,
    pub engine_follow: bool,
    /// Option `PreDelayInMsec` (default on; [re:58734c]):
    /// layer pre-delay in ms, else tempo-synced note values.
    pub predelay_ms: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            aux_returns: true,
            seed: 1,
            forced_layers_fall_through: false,
            auxes_pre_fade: false,
            preroll_mode: 0,
            preroll_only_rec: true,
            wait_midi: false,
            metro_only_rec: false,
            pad_follow: true,
            engine_follow: false,
            predelay_ms: true,
        }
    }
}

/// The step a live note records: on, velocity, coarse pitch and octave
/// (semitones folded into ±12 by octaves, as normalised
/// pitch and octave values).
fn live_step(velocity: u32, semitone: f64) -> crate::pattern::StepData {
    let (mut st, mut oct) = (semitone, 0.0f32);
    while st > 12.0 {
        oct += 1.0;
        st -= 12.0;
    }
    while st < -12.0 {
        oct -= 1.0;
        st += 12.0;
    }
    let mut data = crate::pattern::StepData::off();
    data.on = true;
    data.lanes[lane::VELOCITY] = velocity as f32 * 0.007874016;
    data.lanes[6] = (-12.0 - st as f32) / -24.0;
    data.lanes[5] = (-4.0 - oct) / -8.0;
    data
}

/// A held live note extending its recorded step (duration recording).
#[derive(Clone, Copy, Debug)]
struct DurRec {
    engine: usize,
    pad: usize,
    pattern: usize,
    /// Step the note started on and the current one.
    start: usize,
    step: usize,
    /// Samples into the current step and samples per step.
    accum: f64,
    sps: f64,
    velocity: u32,
    semitone: f64,
}

/// Preview voice: sample, position and the played region.
struct PreviewVoice {
    data: Arc<SampleData>,
    pos: f64,
    start: f64,
    end: f64,
    looping: bool,
}

pub struct Dreist {
    sr: f32,
    bpm: f64,
    ppq: f64,
    /// The song's last stop time ([re:4834a6]): where a stop
    /// left the transport, or `e_setCurrentTime` 0x1c while stopped; the
    /// next start with the song active begins there.
    locate: f64,
    engines: Vec<EngineRt>,
    aux: Vec<AuxRt>,
    master: Mixable,
    master_fx: FxChain,
    /// [re:78f636] `Pitch` (default 0.5).
    master_pitch: f32,
    ctx: Ctx,
    song: Song,
    cursor: SongCursor,
    scenes: SceneManager,
    time_sig: (u32, u32),
    /// Transport running ([re:a7ef14]).
    playing: bool,
    /// Host playing flag of the previous block (the transport
    /// compares against it: only host starts / stops move the transport).
    host_playing: bool,
    /// Record armed ([re:458d51] / [re:abd5a0]) and record quantize
    /// ([re:527c20], saved as [re:aa2b5e]).
    recording: bool,
    /// Pre-roll / record options ([re:781091]).
    preroll_mode: u32,
    preroll_only_rec: bool,
    wait_midi: bool,
    metro_only_rec: bool,
    /// Counting in ([re:ed1b21]) with the samples left (+[re:d54c45]).
    in_preroll: bool,
    preroll_left: i64,
    /// Started, waiting for a MIDI pad note ([re:21794e]).
    waiting_midi: bool,
    /// Selection follows live MIDI notes ([re:dd8304] pad, engine).
    pad_follow: bool,
    engine_follow: bool,
    /// Layer pre-delay in ms (else tempo-synced), `PreDelayInMsec`.
    predelay_ms: bool,
    /// Song recording / retro reporting ([re:c7a961]..).
    pub songrec: crate::songrec::SongRecorder,
    /// Pattern retro recording input (drained by the host for the editor).
    pub retro_events: Vec<crate::songrec::RetroEvent>,
    /// Transmod sources whose modulator retriggered on a pad since the
    /// editor last read them (0x169 `e_updateModulationSourceTriggered`).
    pub mods_triggered: u32,
    /// Steps the engine wrote itself (recording, graph record), for the
    /// editor's copy of the document (the original posts step messages to
    /// the editor).
    pub step_writes: Vec<StepWrite>,
    /// `SendMIDINotes` / `SendMIDICCs` ([re:d93946] / +[re:2e6a02], set by
    /// 0x3 `e_registryMIDIOptionsChanged`).
    pub send_notes: bool,
    /// A step's ForceLayer naming an empty layer falls through to the
    /// pad's layer mode ([re:6395b3] passes +[re:cab206] to
    /// [re:f32b6d]).
    forced_fallthrough: bool,
    pub send_ccs: bool,
    /// MIDI out send buffer ([re:504644], 511 events): (frame from the
    /// current block start, bytes); later blocks' events wait here.
    midi_pending: Vec<(f64, [u8; 3])>,
    /// MIDI out due this block, sorted by frame.
    pub midi_out: Vec<(u32, [u8; 3])>,
    /// Sub-block start within the host block (for MIDI out timing).
    sub_pos: usize,
    /// Live notes being duration-recorded ([re:085301]).
    dur_recs: Vec<DurRec>,
    /// Per engine: (gate, pattern) last seen by the recorder, and whether
    /// the song changed it since (song changes are not recorded).
    rec_seen: [(bool, usize); ENGINES],
    rec_skip: [bool; ENGINES],
    quantize: bool,
    midi_map: MidiMap,
    pub metronome: Metronome,
    /// Audio-input recorder ([re:ed0d24]).
    pub sampler: Sampler,
    /// Output peak levels since the last [`Meters::clear`].
    pub meters: Meters,
    bindings: BindingMap,
    /// Last MIDI CC received (channel, controller) and a running count, for
    /// the editor's MIDI learn.
    pub last_cc: (u32, u8, u8),
    /// Incoming MIDI for the MIDI console (bounded; the host drains it).
    pub midi_log: Vec<[u8; 3]>,
    /// Browser / sampler preview voice ([re:c1c238]).
    preview: Option<PreviewVoice>,
    /// Messages from editor-only bindings for the editor to act on.
    pub editor_messages: Vec<Message>,
    /// Automated (bound) parameter messages for the editor's document,
    /// latest per target (`echo_automation`).
    pub automation_out: Vec<Message>,
    /// The sampler's output-location source for this block.
    sampler_src: [Vec<f32>; 2],
    /// [re:3da8cc] (`e_tapTempo`).
    tap: crate::tap::TapTempo,
    /// `e_setPreviewVolume` ([re:b967d0]), fader value.
    pub preview_volume: f32,
    /// Preview output pair (`e_setPreviewOutputLocation` 0xe7, [re:9cd5bc]):
    /// 1 main, n > 1 sub outputs 2n−3 / 2n−2.
    pub preview_output: usize,
    /// Synth message queue: applied at the next block start.
    messages: Vec<Message>,
    /// The project document (saved by `save`).
    doc: Node,
    /// Keep `doc` in step with the messages (off in the plugin, whose
    /// editor owns the document: mirroring allocates on the audio thread).
    mirror_doc: bool,
    /// Selected engine ([re:9af29c]; "selected" map items, note repeat).
    selected_engine: usize,
    /// Host MIDI events for the next block (`processEvents`).
    midi_in: Vec<MidiEvent>,
    /// [re:d662aa] queue ([re:64046f]), deltas from the block start.
    automation: Vec<MidiEvent>,
    /// [re:ee1e94]: current sub-block start.
    sub_start: u32,
    voices: Vec<VoiceSlot>,
    /// Active voices in processing order (scratch, kept between blocks).
    voice_order: Vec<usize>,
    counter: u32,
    rng: Rng,
    options: RenderOptions,
    changes: Vec<Change>,
    requests: Vec<NoteRequest>,
    triggers: Vec<usize>,
    host: TransmodHost,
    mods: Vec<Option<Modulator>>,
    pub missing_samples: Vec<String>,
    buf: Buffers,
}

/// Per-sub-block mixing buffers, sized for a [`SUB_BLOCK`] up front.
struct Buffers {
    layer: [Vec<f32>; 2],
    pad: [Vec<f32>; 2],
    engine: [Vec<f32>; 2],
    aux: Vec<[Vec<f32>; 2]>,
    sub: Vec<[Vec<f32>; 2]>,
    metro: [Vec<f32>; 2],
}

impl Buffers {
    /// The layer (0), pad (1) or engine (2) stage's buffers.
    fn stage(&mut self, stage: u32) -> &mut [Vec<f32>; 2] {
        match stage {
            0 => &mut self.layer,
            1 => &mut self.pad,
            _ => &mut self.engine,
        }
    }

    fn new() -> Buffers {
        let pair = || [Vec::with_capacity(SUB_BLOCK), Vec::with_capacity(SUB_BLOCK)];
        Buffers { layer: pair(), pad: pair(), engine: pair(), aux: (0..4).map(|_| pair()).collect(), sub: (0..SUB_OUTPUTS).map(|_| pair()).collect(), metro: pair() }
    }
}

fn layer_mode(s: &str) -> LayerMode {
    match s {
        "Velocity split" => LayerMode::VelocitySplit,
        "Round robin" => LayerMode::RoundRobin,
        "Round robin reset" => LayerMode::RoundRobinReset,
        "Random" => LayerMode::Random,
        _ => LayerMode::AllLayers,
    }
}

fn aux_rows(n: &Node) -> [Row; 4] {
    std::array::from_fn(|i| row(n, &aux_send_name(i), 0.0))
}

fn solo_pass(solo_any: bool, solo: bool, mute: bool) -> bool {
    if solo_any { solo } else { !mute }
}

/// Peak-hold output levels (`EngineState`/[re:e46e77]
/// [re:8c0459] keep the running max of |sample| per channel; the editor reads
/// them through the output level getters and resets them
/// [re:cfaf98]). Levels are taken post-fader, before routing.
/// A step the engine wrote (see [re:f53b53]).
#[derive(Clone, Debug)]
pub struct StepWrite {
    pub engine: usize,
    pub pattern: usize,
    pub track: usize,
    pub step: usize,
    pub data: crate::pattern::StepData,
}

/// An effect's control output after a block ([re:414b35]): the chain at
/// `at` (`[engine, pad, layer]`, −1 unused; global aux `[−4, aux, −1]`,
/// master `[−3, −1, −1]`), its slot and the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxOut {
    pub at: [i8; 3],
    pub slot: u8,
    pub value: f32,
}

/// Room for this many [`FxOut`]s without allocating on the audio thread.
const FX_OUTS: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub struct Meters {
    /// Master channel ([re:8805c2]/[re:5327eb]).
    pub master: [f32; 2],
    /// Global aux returns.
    pub aux: [[f32; 2]; 4],
    pub engine: [[f32; 2]; ENGINES],
    /// `engine * TRACKS + pad`.
    pub pad: Vec<[f32; 2]>,
    /// `(engine * TRACKS + pad) * LAYERS + layer`.
    pub layer: Vec<[f32; 2]>,
    /// A block carried host MIDI ([re:ed467d] sets [re:a59474]
    /// when the VstEvents count is positive; the received check
    /// [re:3fd4a6] reads and clears it for the header's MIDIIndi).
    pub midi_received: bool,
    /// [re:6395b3] had to steal a voice (posts
    /// `e_updateVoiceStolen` 0x18a for the header's VoiceIndi).
    pub voice_stolen: bool,
    /// The effects' control outputs (gain-reduction meters), the latest per slot.
    pub fx_out: Vec<FxOut>,
}

impl Default for Meters {
    fn default() -> Self {
        Meters {
            master: [0.0; 2],
            aux: [[0.0; 2]; 4],
            engine: [[0.0; 2]; ENGINES],
            pad: vec![[0.0; 2]; ENGINES * TRACKS],
            layer: vec![[0.0; 2]; ENGINES * TRACKS * LAYERS],
            midi_received: false,
            voice_stolen: false,
            fx_out: Vec::with_capacity(FX_OUTS),
        }
    }
}

impl Meters {
    pub fn clear(&mut self) {
        self.master = [0.0; 2];
        self.aux = [[0.0; 2]; 4];
        self.engine = [[0.0; 2]; ENGINES];
        self.pad.fill([0.0; 2]);
        self.layer.fill([0.0; 2]);
        self.midi_received = false;
        self.voice_stolen = false;
        self.fx_out.clear();
    }

    /// Fold `o` into these levels (running max).
    pub fn merge(&mut self, o: &Meters) {
        let mx = |a: &mut [f32; 2], b: &[f32; 2]| {
            a[0] = a[0].max(b[0]);
            a[1] = a[1].max(b[1]);
        };
        mx(&mut self.master, &o.master);
        self.aux.iter_mut().zip(&o.aux).for_each(|(a, b)| mx(a, b));
        self.engine.iter_mut().zip(&o.engine).for_each(|(a, b)| mx(a, b));
        self.pad.iter_mut().zip(&o.pad).for_each(|(a, b)| mx(a, b));
        self.layer.iter_mut().zip(&o.layer).for_each(|(a, b)| mx(a, b));
        self.midi_received |= o.midi_received;
        self.voice_stolen |= o.voice_stolen;
        // These are newer: only the slots they lack.
        for f in &o.fx_out {
            if self.fx_out.len() < FX_OUTS && !self.fx_out.iter().any(|g| g.at == f.at && g.slot == f.slot) {
                self.fx_out.push(*f);
            }
        }
    }

    /// The latest control output of slot `slot` of the chain at `at`.
    pub fn fx_output(&self, at: [i8; 3], slot: usize) -> Option<f32> {
        self.fx_out.iter().find(|f| f.at == at && f.slot as usize == slot).map(|f| f.value)
    }

    pub fn pad_level(&self, e: usize, p: usize) -> [f32; 2] {
        self.pad.get(e * TRACKS + p).copied().unwrap_or_default()
    }

    pub fn layer_level(&self, e: usize, p: usize, l: usize) -> [f32; 2] {
        self.layer.get((e * TRACKS + p) * LAYERS + l).copied().unwrap_or_default()
    }
}

/// Record the control outputs of `fx`'s running effects ([re:414b35]).
fn fx_outs(acc: &mut Vec<FxOut>, at: [i8; 3], fx: &FxChain) {
    for (slot, e) in fx.chain.slots.iter().enumerate() {
        let Some(value) = e.as_ref().filter(|e| e.active && e.kind != 0).and_then(|e| e.output(0)) else { continue };
        if let Some(f) = acc.iter_mut().find(|f| f.at == at && f.slot as usize == slot) {
            f.value = value;
        } else if acc.len() < FX_OUTS {
            acc.push(FxOut { at, slot: slot as u8, value });
        }
    }
}

/// Running max of |sample| per channel into `acc`.
fn meter(acc: &mut [f32; 2], l: &[f32], r: &[f32]) {
    acc[0] = l.iter().fold(acc[0], |a, x| a.max(x.abs()));
    acc[1] = r.iter().fold(acc[1], |a, x| a.max(x.abs()));
}

impl Dreist {
    /// Bytes of decoded sample audio the engine holds, each buffer counted
    /// once (the master display's memory readout).
    pub fn sample_bytes(&self) -> u64 {
        let mut seen = std::collections::HashSet::new();
        let mut n = 0u64;
        for l in self.engines.iter().flat_map(|e| &e.pads).flat_map(|p| &p.layers) {
            if let Some(s) = &l.sample {
                if seen.insert(Arc::as_ptr(s)) {
                    n += s.bytes();
                }
            }
        }
        n
    }

    /// [re:2f3340]: play a sample once at
    /// the preview volume (port: plain resampled playback after the master).
    pub fn start_preview(&mut self, data: Option<Arc<SampleData>>) {
        self.preview = data.map(|d| {
            let end = d.left.len() as f64;
            PreviewVoice { data: d, pos: 0.0, start: 0.0, end, looping: false }
        });
    }

    /// Preview frames `start..end` of `data`, optionally looping (the
    /// slicer's PreviewWave / slice audition, file preview with
    /// markers; `e_setPreviewLoopMode` 0xdd toggles the loop).
    pub fn start_preview_region(&mut self, data: Arc<SampleData>, start: u32, end: u32, looping: bool) {
        let n = data.left.len() as u32;
        let end = end.min(n);
        let start = start.min(end);
        self.preview = (end > start).then_some(PreviewVoice { data, pos: start as f64, start: start as f64, end: end as f64, looping });
    }

    pub fn set_preview_loop(&mut self, on: bool) {
        if let Some(p) = &mut self.preview {
            p.looping = on;
        }
    }

    pub fn stop_preview(&mut self) {
        self.preview = None;
    }

    /// Report user pattern starts / stops to the song recorder
    /// (`e_updatePatternStarted` / `Stopped` / `Power` → [re:380ce6]
    ///  [re:b4c0f9]), from each engine's gate and pattern.
    fn song_record_poll(&mut self, ppq_now: f64) {
        if !self.playing {
            return;
        }
        let time = self.song.effective_ppq(ppq_now);
        for e in 0..ENGINES.min(self.engines.len()) {
            let eng = &self.engines[e];
            let now = (eng.state.gate, eng.state.current_pattern);
            let was = self.rec_seen[e];
            self.rec_seen[e] = now;
            if std::mem::take(&mut self.rec_skip[e]) || now == was {
                continue;
            }
            match (was.0, now.0) {
                (_, true) if !was.0 || was.1 != now.1 => {
                    let pat = eng.patterns.get(now.1);
                    let empty = pat.is_none_or(|p| p.tracks.iter().all(|t| t.steps.iter().all(|s| !s.on)));
                    let phase = pat
                        .filter(|p| !p.sync.restarts())
                        .map(|p| {
                            let cycle = p.length_16ths() * 0.25 * eng.timing.tempo_multiplier;
                            if cycle > 0.0 { ppq_now.rem_euclid(cycle) } else { 0.0 }
                        })
                        .unwrap_or(0.0);
                    if !was.0 {
                        self.songrec.power_on(e, time);
                    }
                    self.songrec.pattern_started(e, now.1, empty, time, phase);
                }
                (true, false) => self.songrec.pattern_stopped(e, time),
                _ => {}
            }
        }
    }

    /// Song position in quarter notes (internal loop applied).
    pub fn song_position(&self) -> f64 {
        self.song.effective_ppq(self.ppq)
    }

    pub fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    fn render_preview(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Some(pv) = self.preview.as_mut() else { return };
        let g = dreist_dsp::convert::gain_to_amp(self.preview_volume);
        let d = &*pv.data;
        let step = d.sample_rate as f64 / self.sr as f64;
        let n = (d.left.len() as f64).min(pv.end);
        let mut ended = false;
        for (ol, or) in l.iter_mut().zip(r.iter_mut()) {
            // A step longer than a short loop wraps more than once.
            while pv.pos + 1.0 >= n {
                if pv.looping && n - pv.start > 1.0 {
                    pv.pos = pv.start + (pv.pos + 1.0 - n);
                } else {
                    ended = true;
                    break;
                }
            }
            if ended {
                break;
            }
            let i = pv.pos as usize;
            let f = (pv.pos - i as f64) as f32;
            let a = d.left[i] + (d.left[i + 1] - d.left[i]) * f;
            let b = d.right.as_ref().map(|rr| rr[i] + (rr[i + 1] - rr[i]) * f).unwrap_or(a);
            *ol += a * g;
            *or += b * g;
            pv.pos += step;
        }
        if ended {
            self.preview = None;
        }
    }

    /// Add a binding (MIDI learn) without rebuilding or allocating (see
    /// [`crate::binding::BindingMap::add_prepared`]).
    pub fn add_binding(&mut self, b: &mut crate::binding::NewBinding) {
        self.bindings.add_prepared(b);
    }

    pub fn load(path: &Path, sample_rate: f32, options: RenderOptions) -> Result<Dreist, dreist_formats::Error> {
        Dreist::load_cached(path, sample_rate, options, &mut SampleCache::default())
    }

    /// [[re:527fcd]] reusing (and filling) a sample cache.
    pub fn load_cached(path: &Path, sample_rate: f32, options: RenderOptions, cache: &mut SampleCache) -> Result<Dreist, dreist_formats::Error> {
        let project = Project::load(path)?;
        Ok(Dreist::from_project_cached(&project, &samples_dir(path), sample_rate, options, cache))
    }

    /// An empty instrument (no project loaded).
    pub fn empty(sr: f32, options: RenderOptions) -> Dreist {
        let root = Node::parse("<root version=\"3000\" />").expect("static XML");
        let project = Project::from_node(root).expect("empty project");
        Dreist::from_project(&project, Path::new("."), sr, options)
    }

    pub fn from_project(project: &Project, samples: &Path, sr: f32, options: RenderOptions) -> Dreist {
        Dreist::from_project_cached(project, samples, sr, options, &mut SampleCache::default())
    }

    /// Build from an edited document tree (editor structural changes).
    pub fn from_document(doc: &Node, samples: &Path, sr: f32, options: RenderOptions, cache: &mut SampleCache) -> Option<Dreist> {
        let project = Project::from_node(doc.clone()).ok()?;
        Some(Dreist::from_project_cached(&project, samples, sr, options, cache))
    }

    /// [[re:9f37ad]] reusing already decoded samples.
    pub fn from_project_cached(project: &Project, samples: &Path, sr: f32, options: RenderOptions, cache: &mut SampleCache) -> Dreist {
        dreist_dsp::fx::nodes::reset_shared_rng();
        // Shared lookup tables: built here, not on the audio thread.
        dreist_dsp::analog::warm();
        let engine_nodes: Vec<Option<&dreist_formats::Engine>> = (0..ENGINES).map(|i| project.engines.iter().find(|e| e.index as usize == i)).collect();
        let solo_eng = engine_nodes.iter().flatten().any(|e| e.raw.bool("Solo").unwrap_or(false));
        let engines = engine_nodes
            .iter()
            .map(|e| match e {
                Some(e) => load_engine(e, samples, sr, solo_eng, cache),
                None => empty_engine(),
            })
            .collect();
        let aux = (0..4)
            .map(|i| {
                let n = project.raw.children_of("GlobalAux").find(|n| n.index() == Some(i));
                AuxRt {
                    mix: n.map(|n| Mixable::new(row(n, "Gain", 0.5f32.cbrt())[0], row(n, "Pan", 0.5)[0])).unwrap_or(Mixable::new(0.5f32.cbrt(), 0.5)),
                    muted: n.and_then(|n| n.bool("Mute")).unwrap_or(false),
                    fx: FxChain::from_node(n.and_then(|n| n.children_of("EffectChain").next()), sr),
                    route: n.map(Route::of).unwrap_or_default(),
                    pre_fade: n.and_then(|n| n.bool("IsPreFade")).unwrap_or(options.auxes_pre_fade),
                }
            })
            .collect();
        let master = project.raw.children_of("MasterChannel").next().map(|n| Mixable::new(row(n, "Gain", 0.5f32.cbrt())[0], row(n, "Pan", 0.5)[0])).unwrap_or(Mixable::new(0.5f32.cbrt(), 0.5));
        let master_fx = FxChain::from_node(project.raw.children_of("MasterChannel").next().and_then(|n| n.children_of("EffectChain").next()), sr);
        let song = project.raw.children_of("SongSequencer").next().map(Song::from_node).unwrap_or_default();
        let voices = (0..NORMAL_VOICES + TIMESTRETCH_VOICES)
            .map(|i| VoiceSlot {
                voice: {
                    let mut v = Voice::new(sr);
                    if i >= NORMAL_VOICES {
                        v.prepare_timestretch();
                    }
                    v
                },
                state: VoiceState::Idle,
                id: 0,
                slot: Slot { engine: 0, pad: 0, layer: 0 },
                cut: CutMode::NotCut,
                delay: 0.0,
                duration: -1.0,
                pending: None,
                tail: 0,
                has_step: false,
                live: false,
                semitone: 0.0,
                record: None,
                velocity: 0,
            })
            .collect();
        // Built stopped (the original starts with the transport off);
        // renders call `start`.
        Dreist {
            sr,
            bpm: project.tempo.unwrap_or(120.0) as f64,
            ppq: 0.0,
            locate: 0.0,
            engines,
            aux,
            master,
            master_fx,
            master_pitch: project.raw.children_of("MasterChannel").next().and_then(|n| n.f32("Pitch")).unwrap_or(0.5),
            ctx: Ctx::new(sr),
            song,
            cursor: SongCursor::new(),
            scenes: project.raw.children_of("SceneManager").next().map(SceneManager::from_node).unwrap_or_default(),
            time_sig: {
                let t = project.raw.children_of("TimeInfo").next();
                (t.and_then(|t| t.u32("TSN")).unwrap_or(4).max(1), t.and_then(|t| t.u32("TSD")).unwrap_or(4).max(1))
            },
            playing: false,
            host_playing: false,
            recording: false,
            preroll_mode: options.preroll_mode.min(3),
            preroll_only_rec: options.preroll_only_rec,
            wait_midi: options.wait_midi,
            metro_only_rec: options.metro_only_rec,
            in_preroll: false,
            preroll_left: -1,
            waiting_midi: false,
            pad_follow: options.pad_follow,
            engine_follow: options.engine_follow,
            predelay_ms: options.predelay_ms,
            songrec: {
                let mut r = crate::songrec::SongRecorder::default();
                r.events.reserve(64);
                r
            },
            retro_events: Vec::with_capacity(64),
            mods_triggered: 0,
            step_writes: Vec::with_capacity(64),
            send_notes: false,
            forced_fallthrough: options.forced_layers_fall_through,
            send_ccs: false,
            midi_pending: Vec::with_capacity(MIDI_PENDING_MAX),
            midi_out: Vec::with_capacity(MIDI_PENDING_MAX),
            sub_pos: 0,
            dur_recs: Vec::with_capacity(64),
            rec_seen: [(false, 0); ENGINES],
            rec_skip: [false; ENGINES],
            quantize: project.raw.children_of("TimeInfo").next().and_then(|t| t.bool("Quantize")).unwrap_or(false),
            midi_map: MidiMap::default(),
            last_cc: (0, 0, 0),
            midi_log: Vec::with_capacity(256),
            preview: None,
            tap: Default::default(),
            sampler_src: [Vec::with_capacity(MAX_HOST_BLOCK), Vec::with_capacity(MAX_HOST_BLOCK)],
            editor_messages: Vec::with_capacity(256),
            automation_out: Vec::with_capacity(256),
            preview_volume: 0.793_700_5,
            preview_output: 1,
            meters: Meters::default(),
            sampler: {
                // [re:55ac26].
                let mut sm = Sampler::default();
                sm.sample_rate = sr;
                if let Some(n) = project.raw.children_of("Sampler").next() {
                    sm.input_gain = n.f32("inputgain").unwrap_or(sm.input_gain);
                    sm.output_gain = n.f32("outputgain").unwrap_or(sm.output_gain);
                    sm.start_threshold = n.f32("startthreshold").unwrap_or(sm.start_threshold);
                    sm.stop_threshold = n.f32("stopthreshold").unwrap_or(sm.stop_threshold);
                    sm.thru = n.bool("inputthru").unwrap_or(false);
                    sm.beats_mode = n.bool("beatsmode").unwrap_or(false);
                    sm.bars = n.u32("recbars").unwrap_or(sm.bars);
                    sm.beats = n.u32("recbeats").unwrap_or(sm.beats);
                    sm.secs = n.f32("recsecs").unwrap_or(sm.secs).clamp(0.0, 3600.0);
                    if let Some(rs) = n.attr("recordsource") {
                        sm.source = crate::sampler::source_from_name(rs.trim());
                    }
                }
                sm
            },
            bindings: project.raw.children_of("BindingMap").next().map(BindingMap::from_node).unwrap_or_default(),
            messages: Vec::with_capacity(256),
            doc: project.raw.clone(),
            mirror_doc: true,
            metronome: {
                let mut m = project.raw.children_of("Metronome").next().map(Metronome::from_node).unwrap_or_default();
                // [re:8ca869] `Metro`: metronome on → mute off (message 0x18).
                if let Some(on) = project.raw.children_of("TimeInfo").next().and_then(|t| t.bool("Metro")) {
                    m.muted = !on;
                }
                m.load_sounds(&default_metronomes_dir(), "Default");
                m
            },
            selected_engine: 0,
            midi_in: Vec::with_capacity(512),
            automation: Vec::with_capacity(1024),
            sub_start: 0,
            voice_order: Vec::with_capacity(512),
            voices,
            counter: 0,
            rng: Rng::new(options.seed),
            options,
            changes: Vec::with_capacity(64),
            requests: Vec::with_capacity(256),
            triggers: Vec::with_capacity(256),
            host: TransmodHost::from_root(&project.raw),
            mods: (0..16).map(|i| project.raw.children_of("Modulator").find(|n| n.index() == Some(i)).and_then(|n| Modulator::from_node(n, sr, options.seed as u32 ^ (i * 0x9e37)))).collect(),
            missing_samples: std::mem::take(&mut cache.errors),
            buf: Buffers::new(),
        }
    }

    pub fn tempo(&self) -> f64 {
        self.bpm
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    /// Export: the engine's current pattern steps and tempo multiplier when
    /// its patterns are on ([re:a61043]).
    pub fn export_pattern_steps(&self, e: usize) -> Option<(u32, f64)> {
        let eng = self.engines.get(e)?;
        if !eng.state.pattern_power {
            return None;
        }
        let p = eng.patterns.get(eng.state.current_pattern)?;
        Some((p.steps, eng.timing.tempo_multiplier))
    }

    /// `e_setEngineSwingTemplate` (0x96): the engine's swing template.
    /// Swap in engine `e`'s swing template; the old one is left in `swing`
    /// (for the sender to free: this runs on the audio thread).
    pub fn set_swing_template(&mut self, e: usize, swing: &mut crate::pattern::Swing) {
        if let Some(x) = self.engines.get_mut(e) {
            std::mem::swap(&mut x.timing.swing, swing);
        }
    }

    /// `e_songReplaceTrack`: replace whole song tracks (editor edits),
    /// swapping (see [`crate::song::Song::swap_track`]).
    pub fn replace_song_tracks(&mut self, tracks: &mut [(usize, Vec<crate::song::SongPart>)]) {
        for (t, parts) in tracks {
            self.song.swap_track(*t, parts);
        }
    }

    pub fn song_active(&self) -> bool {
        self.song.active
    }

    /// Song length in samples when the song is active.
    pub fn song_length_samples(&self) -> Option<u64> {
        (self.song.active && self.song.length_qn() > 0.0).then(|| (self.song.length_qn() * 60.0 / self.bpm * self.sr as f64).ceil() as u64)
    }

    /// Transport start ([re:755e2b]): the song's last
    /// stop time with the song active, else 0.
    pub fn start(&mut self) {
        let at = if self.song.active { self.locate } else { 0.0 };
        self.start_at(at);
    }

    /// The internal play button ([re:04f8ef]
    /// [re:755e2b]). With the song inactive it waits for a MIDI pad note when
    /// `WaitForMIDI` is set (playing already reports it), else counts in
    /// 1, 2 or 4 bars (`PrerollTime`; not when pre-rolling only while
    /// recording and not recording): the transport runs from
    /// −bars·beats-per-bar QN before the start (samples per bar·bars
    /// samples, rounded) with the patterns stopped, so only the metronome
    /// sounds, and the transport start follows at the start.
    pub fn play(&mut self) {
        self.in_preroll = false;
        self.preroll_left = -1;
        self.waiting_midi = false;
        if !self.song.active {
            if self.wait_midi {
                self.ppq = 0.0;
                self.waiting_midi = true;
                return;
            }
            let bars = [0.0, 1.0, 2.0, 4.0][self.preroll_mode.min(3) as usize];
            if bars > 0.0 && !(self.preroll_only_rec && !self.recording) {
                let beats = self.beats_per_bar() * bars;
                self.preroll_left = (self.sr as f64 * 60.0 / self.bpm * beats).round() as i64;
                self.in_preroll = true;
                self.playing = true;
                self.ppq = -beats;
                return;
            }
        }
        self.start();
    }

    /// [re:a6c67b]: numerator · 4 / denominator.
    pub(crate) fn beats_per_bar(&self) -> f64 {
        self.time_sig.0 as f64 * (4.0 / self.time_sig.1.max(1) as f32) as f64
    }

    /// Counting in before the transport starts.
    pub fn in_preroll(&self) -> bool {
        self.in_preroll
    }

    /// MIDI note-on [re:c0544f]: a mapped pad note while waiting for
    /// MIDI starts the transport.
    pub(crate) fn end_midi_wait(&mut self) {
        if self.waiting_midi {
            self.waiting_midi = false;
            self.start_at(0.0);
        }
    }

    /// Transport start [re:1ba742] at host position `ppq`.
    pub fn start_at(&mut self, ppq: f64) {
        self.in_preroll = false;
        self.preroll_left = -1;
        self.waiting_midi = false;
        self.playing = true;
        self.ppq = ppq;
        self.cursor = SongCursor::new();
        // [re:27886a]: modulators, then the Transport source.
        let (bpm, host) = (self.bpm as f32, self.host.clone());
        for m in self.mods.iter_mut().flatten() {
            m.transport_started(0.0, bpm, &host);
        }
        self.host.values[src::TRANSPORT] = 1.0;
        let song = self.song.active;
        let pos = self.song.effective_ppq(ppq);
        self.songrec.transport(true, pos, song);
        self.rec_seen = [(false, 0); ENGINES];
        self.rec_skip = [false; ENGINES];
        for e in &mut self.engines {
            e.state.reset_playback();
            let sync = e.patterns.get(e.state.current_pattern).map(|p| p.sync);
            if song {
                e.state.gate = false;
            } else if let Some(sync) = sync {
                e.state.gate = !sync.gated() && e.state.pattern_power;
                if sync == crate::pattern::SyncMode::SlaveRestart {
                    e.state.needs_resync = true;
                }
            }
        }
    }

    /// Transport stop [re:0fd382]: every engine's gate off and
    /// steps reset, pending scene changes dropped, Transport source 0.
    pub fn stop(&mut self) {
        // A count-in never reached the transport start.
        let started = !self.in_preroll && !self.waiting_midi;
        self.in_preroll = false;
        self.preroll_left = -1;
        self.waiting_midi = false;
        if started {
            let pos = self.song.effective_ppq(self.ppq);
            self.songrec.transport(false, pos, self.song.active);
            // [re:831825] 0x2b8xx: the
            // last stop time, wrapped into the loop with the song active.
            self.locate = if self.song.active { pos } else { self.ppq };
        }
        self.playing = false;
        for e in &mut self.engines {
            e.state.gate = false;
            e.state.process_offset = 0.0;
            e.state.last_16ths = -1;
            e.state.restart_steps();
        }
        self.scenes.transport_stopped();
        self.host.values[src::TRANSPORT] = 0.0;
        self.cursor = SongCursor::new();
    }

    /// Playing, counting in, or waiting for MIDI to start.
    pub fn is_playing(&self) -> bool {
        self.playing || self.waiting_midi
    }

    /// `e_panic` 0x12 ([re:d6b694]):
    /// [re:bf71ff] returns every pad voice, the preview
    /// voice and both metronome voices at once; the effects panic [re:a67204]
    /// resets every effect (engine, pad, layer chains, the four global
    /// auxes and the master chain); the internal transport stops.
    pub fn panic(&mut self) {
        for v in &mut self.voices {
            v.state = VoiceState::Idle;
            v.pending = None;
            v.record = None;
        }
        self.preview = None;
        self.metronome.bar_voice.kill();
        self.metronome.beat_voice.kill();
        for e in &mut self.engines {
            e.fx.chain.panic();
            for p in &mut e.pads {
                p.fx.chain.panic();
                for l in &mut p.layers {
                    l.fx.chain.panic();
                }
            }
        }
        for a in &mut self.aux {
            a.fx.chain.panic();
        }
        self.master_fx.chain.panic();
        if self.is_playing() {
            self.stop();
        }
    }

    /// [re:84504f] (`e_setCurrentTime` 0x1c): the
    /// transport position; the song resyncs when active,
    /// else every engine needs a resync. Stopped, it is also where the
    /// next start begins.
    pub fn set_current_time(&mut self, t: f64) {
        let t = t.max(0.0);
        if t == self.ppq && (self.playing || t == self.locate) {
            return;
        }
        self.ppq = t;
        if self.song.active {
            self.cursor = SongCursor::new();
        } else {
            for e in &mut self.engines {
                e.state.needs_resync = true;
            }
        }
        if !self.playing {
            self.locate = t;
        }
    }

    /// `e_returnToZero` 0x1d (song mode only): with a loop active, a
    /// playing transport goes to the loop start, or to 0 when already
    /// within tempo/120 QN (half a second) of it; a stopped one goes to the
    /// loop start when past it. Otherwise to 0.
    pub fn return_to_zero(&mut self) {
        if !self.song.active {
            return;
        }
        if self.song.loop_active() {
            let pos = self.song.effective_ppq(self.ppq);
            let start = self.song.loop_range().0;
            if self.playing {
                let t = if pos <= self.bpm / 120.0 + start { 0.0 } else { start };
                self.set_current_time(t);
                return;
            }
            if start < pos {
                self.set_current_time(start);
                return;
            }
        }
        self.set_current_time(0.0);
    }

    /// `e_tapTempo` 0x1b: a value above 0.5 taps; once the tap tempo has
    /// three intervals it becomes the tempo ([re:c3d862]).
    pub fn tap_tempo(&mut self, v: f32) {
        if v > 0.5 && self.tap.tap(std::time::Instant::now()) {
            self.bpm = (self.tap.tempo() as f64).clamp(20.0, 999.0);
        }
    }

    /// Host loop reported this block ([re:a26535]).
    pub fn host_loop_active(&self) -> bool {
        self.song.host_loop.is_some()
    }

    /// Host cycle (QN) when the host reports an active loop ([re:5cc709]);
    /// it replaces the internal song loop meanwhile.
    pub fn set_host_loop(&mut self, range: Option<(f64, f64)>) {
        self.song.host_loop = range.filter(|(s, e)| e > s);
    }

    /// [re:814f3a] with option `MetronomeSound`
    /// (call when the engine is built, not on the audio thread).
    pub fn set_metronome_sound(&mut self, name: &str) {
        self.metronome.load_sounds(&default_metronomes_dir(), name);
    }

    /// Host transport for the next block (time info / transport
    /// [re:b8baf0]): tempo (clamped 20..999), play state, and the host position
    /// when known (a jump relocates the song).
    pub fn set_transport(&mut self, playing: bool, bpm: f64, ppq: Option<f64>) {
        if bpm > 0.0 {
            self.bpm = bpm.clamp(20.0, 999.0);
        }
        match (playing, self.host_playing) {
            (true, false) => self.start_at(ppq.unwrap_or(self.locate)),
            (false, true) if self.playing => self.stop(),
            (true, true) if self.playing => {
                if let Some(p) = ppq {
                    if (p - self.ppq).abs() > 1e-6 {
                        self.ppq = p;
                    }
                }
            }
            _ => {}
        }
        self.host_playing = playing;
    }

    /// Continue from the engine this one replaces (an editor rebuild,
    /// swapped in on the audio thread: moves and copies only, no
    /// allocation). The document defines the new engine; what exists only
    /// at run time carries over: the transport, song and record state, each
    /// engine's playback and live state, round-robin pickers, modulator
    /// phases and transmod values, sounding voices and the preview, the
    /// metronome, queued MIDI out and the editor-set options that are not
    /// in the document. What moves out of `old` is dropped with it.
    pub fn adopt(&mut self, old: &mut Dreist) {
        use std::mem::swap;
        self.host_playing = old.host_playing;
        self.locate = old.locate;
        self.waiting_midi = old.waiting_midi;
        self.playing = old.playing;
        self.ppq = old.ppq;
        self.in_preroll = old.in_preroll;
        self.preroll_left = old.preroll_left;
        self.recording = old.recording;
        self.rec_seen = old.rec_seen;
        self.rec_skip = old.rec_skip;
        swap(&mut self.cursor, &mut old.cursor);
        swap(&mut self.songrec, &mut old.songrec);
        self.scenes.adopt(&mut old.scenes);
        self.host.values = old.host.values;
        for (n, o) in self.engines.iter_mut().zip(old.engines.iter_mut()) {
            swap(&mut n.state, &mut o.state);
            if n.state.current_pattern >= n.patterns.len() {
                n.state.current_pattern = 0;
            }
            n.mix.continue_from(&o.mix);
            n.sends = o.sends;
            for (np, op) in n.pads.iter_mut().zip(o.pads.iter_mut()) {
                swap(&mut np.picker, &mut op.picker);
                np.mix.continue_from(&op.mix);
                np.sends = op.sends;
                for (nl, ol) in np.layers.iter_mut().zip(&op.layers) {
                    nl.mix.continue_from(&ol.mix);
                    nl.sends = ol.sends;
                }
            }
        }
        for (n, o) in self.aux.iter_mut().zip(&old.aux) {
            n.mix.continue_from(&o.mix);
        }
        self.master.continue_from(&old.master);
        for (n, o) in self.mods.iter_mut().zip(old.mods.iter_mut()) {
            if let (Some(n), Some(o)) = (n, o) {
                n.adopt(o);
            }
        }
        swap(&mut self.voices, &mut old.voices);
        // Voices of pads / layers the new document does not have stop:
        // their strips are gone.
        let engines = &self.engines;
        for v in self.voices.iter_mut().filter(|v| v.state != VoiceState::Idle) {
            let s = v.slot;
            if !engines.get(s.engine as usize).and_then(|e| e.pads.get(s.pad as usize)).is_some_and(|p| (s.layer as usize) < p.layers.len()) {
                v.state = VoiceState::Idle;
            }
        }
        swap(&mut self.voice_order, &mut old.voice_order);
        swap(&mut self.dur_recs, &mut old.dur_recs);
        self.counter = old.counter;
        swap(&mut self.rng, &mut old.rng);
        swap(&mut self.preview, &mut old.preview);
        self.preview_volume = old.preview_volume;
        self.preview_output = old.preview_output;
        swap(&mut self.metronome, &mut old.metronome);
        swap(&mut self.midi_pending, &mut old.midi_pending);
        swap(&mut self.tap, &mut old.tap);
        self.send_notes = old.send_notes;
        self.send_ccs = old.send_ccs;
        self.selected_engine = old.selected_engine;
        self.last_cc = old.last_cc;
        // What the old engine produced for the host / editor and nobody
        // has collected yet.
        swap(&mut self.step_writes, &mut old.step_writes);
        swap(&mut self.retro_events, &mut old.retro_events);
        swap(&mut self.editor_messages, &mut old.editor_messages);
        swap(&mut self.automation_out, &mut old.automation_out);
        swap(&mut self.midi_log, &mut old.midi_log);
        self.meters.merge(&old.meters);
        self.mods_triggered |= old.mods_triggered;
    }

    /// Render one host block (any length) into `left`/`right` (overwritten).
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_multi(left, right, &mut []);
    }

    /// As `process`, also writing "Sub output k" into `subs[k]` (overwritten).
    /// Sub outputs beyond `subs.len()` are dropped, like host channels that
    /// do not exist.
    pub fn process_multi(&mut self, left: &mut [f32], right: &mut [f32], subs: &mut [[&mut [f32]; 2]]) {
        self.process_with_input(left, right, subs, None);
    }

    /// Host timing for the sampler at the current tempo and signature.
    pub(crate) fn sampler_clock(&self) -> SamplerClock {
        let spb = self.sr as f64 * 60.0 / self.bpm;
        SamplerClock { playing: self.playing, stopped: false, samples_per_beat: spb, samples_per_bar: spb * self.beats_per_bar(), to_bar: 0 }
    }

    /// As `process_multi`, with the host's audio input for the sampler
    /// ([re:37bdbe]; its thru signal goes to the main output).
    pub fn process_with_input(&mut self, left: &mut [f32], right: &mut [f32], subs: &mut [[&mut [f32]; 2]], input: Option<[&[f32]; 2]>) {
        let n = left.len();
        left.fill(0.0);
        right.fill(0.0);
        // Messages posted last block, then the binding
        // interpolators (interpolated parameters at the block start).
        self.apply_messages();
        // Messages queued from here on are this block's (for the next).
        let block_from = self.messages.len();
        let mut msgs = std::mem::take(&mut self.messages);
        self.bindings.update(&mut msgs, block_from);
        self.messages = msgs;
        self.echo_automation(block_from);
        for p in subs.iter_mut() {
            p[0].fill(0.0);
            p[1].fill(0.0);
        }
        let spbeat = self.sr as f64 * 60.0 / self.bpm;
        self.changes.clear();
        if self.song.active && self.playing {
            let mut views = [EngineView::default(); 8];
            for (v, e) in views.iter_mut().zip(&self.engines) {
                *v = EngineView { gate: e.state.gate, current_pattern: e.state.current_pattern };
            }
            let nv = self.engines.len().min(8);
            self.cursor.cue(&self.song, self.ppq, n as u32, spbeat, &views[..nv], &mut self.changes);
        }
        // Host MIDI at the block start.
        self.process_midi_in(n);
        let mut ppq_now = self.ppq;
        let mut pos = 0usize;
        let mut next_change = 0usize;
        // The sampler records from the end of a count-in.
        let mut sampler_from = if self.in_preroll { n } else { 0 };
        while pos < n {
            let mut end = (pos + SUB_BLOCK).min(n);
            // Block end (scenes, automation; the song's below), then
            // [re:02c086], [re:cb7749].
            end = self.scenes.block_end(pos as u32, end as u32) as usize;
            end = self.automation_block_end(pos as u32, end as u32) as usize;
            // Voice processing [re:2ee984]: sub-blocks end where the count-in does.
            if self.in_preroll && self.preroll_left > 0 && (self.preroll_left as usize) < end - pos {
                end = pos + self.preroll_left as usize;
            }
            self.process_automation(pos as u32, end as u32);
            let mut msgs = std::mem::take(&mut self.messages);
            self.bindings.update(&mut msgs, block_from);
            self.messages = msgs;
            self.echo_automation(block_from);
            while let Some(c) = self.scenes.pop_due(pos as u32) {
                if c.on {
                    self.scene_on(c.scene);
                } else {
                    self.scene_off(c.scene);
                }
            }
            // [re:ef295e]: stop at the next change.
            while next_change < self.changes.len() && (self.changes[next_change].offset as usize) <= pos {
                let c = self.changes[next_change].clone();
                self.activate(&c, ppq_now);
                next_change += 1;
            }
            if let Some(c) = self.changes.get(next_change) {
                end = end.min(c.offset as usize).max(pos + 1);
            }
            self.song_record_poll(ppq_now);
            process_transmod(&mut self.mods, &mut self.host, end - pos, self.bpm as f32);
            let clock = Clock { bpm: self.bpm, sample_rate: self.sr, ppq_block: self.ppq, ppq_now };
            for e in 0..ENGINES {
                if !self.playing {
                    break;
                }
                self.requests.clear();
                self.triggers.clear();
                let eng = &mut self.engines[e];
                let pattern = eng.patterns.get(eng.state.current_pattern);
                let pads = PadInfo { one_shot: &eng.one_shot, links: &eng.links };
                let mut click = None;
                let metro = (e == self.selected_engine).then_some(&mut click);
                process_engine(&mut eng.state, pattern, &eng.timing, &clock, pos as f64, (end - pos) as f64, &pads, &mut self.rng, &mut self.requests, &mut self.triggers, metro);
                // [re:b46d06]: silent outside recording
                // with "Metronome only when recording" ([re:d22b11]).
                if let Some((n16, delay)) = click.filter(|_| !(self.metro_only_rec && !self.recording)) {
                    self.metronome.progress_click(n16, delay);
                }
                self.apply_graph_writes(e);
                let eng = &mut self.engines[e];
                if eng.state.wrapped && self.retro_events.len() < 4096 {
                    self.retro_events.push(crate::songrec::RetroEvent::Cycled { engine: e });
                }
                if eng.state.wrapped {
                    for p in &mut eng.pads {
                        if p.mode == LayerMode::RoundRobinReset {
                            p.picker = LayerPicker::new();
                        }
                    }
                }
                let bpm = self.bpm as f32;
                for &pad in &self.triggers {
                    for (i, m) in self.mods.iter_mut().enumerate() {
                        // [re:927da0]: 0x169
                        // [slot + 5] to the editor when it retriggers.
                        if m.as_mut().is_some_and(|m| m.note_on(e, pad, &self.host, bpm)) {
                            self.mods_triggered |= 1 << (i + 5);
                        }
                    }
                }
                let reqs = std::mem::take(&mut self.requests);
                self.sub_pos = pos;
                for r in &reqs {
                    self.note_on(e, r);
                }
                self.requests = reqs;
            }
            self.ctx.bpm = self.bpm as f32;
            self.ctx.ppq = ppq_now;
            self.process_voices(&mut left[pos..end], &mut right[pos..end]);
            self.duration_record_tick(end - pos);
            for (k, p) in subs.iter_mut().take(SUB_OUTPUTS).enumerate() {
                p[0][pos..end].copy_from_slice(&self.buf.sub[k][0]);
                p[1][pos..end].copy_from_slice(&self.buf.sub[k][1]);
            }
            ppq_now += (end - pos) as f64 * self.bpm * (1.0 / 60.0) / self.sr as f64;
            // Voice processing [re:f5a5d0]: the count-in ends → the transport
            // starts at the start position.
            if self.in_preroll {
                self.preroll_left -= (end - pos) as i64;
                if self.preroll_left <= 0 {
                    let at = 0.0;
                    self.start_at(at);
                    // The block's QN advance below lands on `at` + the rest.
                    self.ppq = at - end as f64 * self.bpm * (1.0 / 60.0) * (1.0f32 / self.sr) as f64;
                    ppq_now = at;
                    sampler_from = end;
                }
            }
            pos = end;
        }
        // Notes started before the next block's sub-block loop (editor pad
        // hits, messages, MIDI in) are at its start.
        self.sub_pos = 0;
        self.sub_start = 0;
        self.flush_midi_out(n);
        self.scenes.end_of_block(n as u32);
        self.automation_end_of_block(n as u32);
        // Voice processing → [re:897d27] after the
        // master channel: the host input, or an output location's channels
        // ([re:bbe355]: Master → the main pair,
        // Sub output k → its pair); MITM sources (out of scope) record
        // nothing.
        let clock = self.sampler_clock();
        let src = self.sampler.source;
        let k = sampler_from;
        if k >= n {
            // Counting in: nothing to record.
        } else if src == crate::sampler::SOURCE_EXTERNAL {
            if let Some([il, ir]) = input {
                let (il, ir) = (&il[k.min(il.len())..], &ir[k.min(ir.len())..]);
                self.sampler.process([il, ir], [&mut left[k..], &mut right[k..]], &clock);
            }
        } else if (0..34).contains(&src) {
            let [sl, sr] = &mut self.sampler_src;
            sl.clear();
            sr.clear();
            if src < 2 {
                sl.extend_from_slice(left);
                sr.extend_from_slice(right);
            } else if let Some(p) = subs.get((src - 2) as usize) {
                sl.extend_from_slice(&p[0][..n.min(p[0].len())]);
                sr.extend_from_slice(&p[1][..n.min(p[1].len())]);
            }
            sl.resize(n, 0.0);
            sr.resize(n, 0.0);
            let (sl, sr) = (std::mem::take(sl), std::mem::take(sr));
            self.sampler.process([&sl[k..], &sr[k..]], [&mut left[k..], &mut right[k..]], &clock);
            self.sampler_src = [sl, sr];
        }
        if self.playing {
            self.ppq += n as f64 * self.bpm * (1.0 / 60.0) * (1.0f32 / self.sr) as f64;
        }
    }

    /// Track change [re:dce201]: song changes post 0xa8/0xa9/0xaa
    /// (start) or 0xab (stop) to the engine, song-triggered.
    fn activate(&mut self, c: &Change, _ppq_now: f64) {
        if c.engine >= self.engines.len() {
            return;
        }
        self.rec_skip[c.engine] = true;
        match c.kind {
            // Patterns above 0x18 (the recorder's stopped parts) are ignored.
            ChangeKind::Start { pattern, .. } if pattern > 24 => {}
            ChangeKind::Start { pattern, phase } => {
                self.msg_power(c.engine, true);
                if pattern < PATTERNS {
                    self.select_pattern(c.engine, pattern, true);
                }
                self.msg_phase(c.engine, Some(phase), true);
            }
            ChangeKind::Stop => self.msg_stop(c.engine, true),
            ChangeKind::Resync => {
                // Song resync [re:662b0c].
                if self.song.active {
                    for e in &mut self.engines {
                        e.state.gate = false;
                    }
                }
            }
        }
    }

    /// Pattern message 0xa8: pattern power. Turning power on gates a
    /// free-running (sync < 2) current pattern when the song is inactive, and
    /// for non-restart patterns may suppress every pad for one step.
    fn msg_power(&mut self, e: usize, on: bool) {
        let (bpm, sr, ppq) = (self.bpm, self.sr, self.ppq);
        let song_active = self.song.active;
        let eng = &mut self.engines[e];
        let st = &mut eng.state;
        if st.pattern_power == on {
            return;
        }
        st.pattern_power = on;
        let Some(p) = eng.patterns.get(st.current_pattern) else { return };
        if on && !song_active && p.sync.index() < 2 {
            st.gate = true;
        }
        if on && !p.sync.restarts() {
            // [re:91fdc9]: m = 1/((len16/steps)·mult), d = 16ths per sample.
            let m = 1.0 / ((p.length_16ths() / p.steps.max(1) as f64) * eng.timing.tempo_multiplier);
            let d = bpm / 60.0 / sr as f64 * 4.0;
            if (d * m + ppq).floor() == ((ppq - d) * m).floor() {
                st.pad_suppression = [1; TRACKS];
            }
        }
    }

    /// Select an engine's pattern [re:87ab86] (message 0xa9).
    fn select_pattern(&mut self, e: usize, pattern: usize, song: bool) {
        let (host, bpm) = (&self.host, self.bpm as f32);
        for m in self.mods.iter_mut().flatten() {
            m.engine_changed_pattern(e, host, bpm);
        }
        let eng = &mut self.engines[e];
        let st = &mut eng.state;
        if eng.patterns.get(st.current_pattern).is_some_and(|p| p.sync.gated()) {
            st.gate = false;
        }
        st.current_pattern = pattern;
        st.song_triggered = song;
        // Pattern trigger position: the trigger, rounded down to a pattern
        // cycle unless the pattern restarts.
        st.trigger_ppq = self.ppq;
        if let Some(p) = eng.patterns.get(pattern) {
            if !p.sync.restarts() {
                let cycle = p.length_16ths() * 0.25 * eng.timing.tempo_multiplier;
                if cycle > 0.0 {
                    st.trigger_ppq -= st.trigger_ppq % cycle;
                }
            }
        }
        match eng.patterns.get(pattern).map(|p| p.sync.index()) {
            Some(0) => st.gate = true,
            Some(1) => {
                st.gate = true;
                st.restart_steps();
                st.needs_resync = true;
            }
            _ => {}
        }
    }

    /// 0xaa: start the current pattern, optionally at a song part phase
    /// (`None` = the scene's -999 "no phase").
    fn msg_phase(&mut self, e: usize, phase: Option<f64>, _song: bool) {
        let bpm = self.bpm;
        let eng = &mut self.engines[e];
        let st = &mut eng.state;
        let Some(p) = eng.patterns.get(st.current_pattern) else { return };
        if let Some(phase) = phase {
            // Part phase to last play position [re:219452].
            let len = p.length_16ths();
            st.phase_resync = if len >= 0.001 { phase_mod(phase * 4.0 * bpm * eng.timing.tempo_multiplier * (1.0 / bpm), len) } else { 0.0 };
        }
        st.gate = true;
        if p.sync.restarts() {
            st.restart_steps();
        }
        if phase.is_some() {
            st.needs_resync = false;
        }
    }

    /// 0xab: stop. Song-triggered patterns, song-sent stops and gated
    /// patterns lose the gate.
    fn msg_stop(&mut self, e: usize, song: bool) {
        let eng = &mut self.engines[e];
        let st = &mut eng.state;
        let gated = eng.patterns.get(st.current_pattern).is_some_and(|p| p.sync.gated());
        if st.song_triggered || song || gated {
            st.gate = false;
        }
    }

    /// [re:413596].
    fn scene_on(&mut self, scene: usize) {
        let Some((used, engines)) = self.scenes.scenes.get(scene).map(|s| (s.used, s.engines)) else { return };
        for e in 0..ENGINES.min(self.engines.len()) {
            if e == self.scenes.bypass {
                continue;
            }
            let on = used && engines[e].power;
            if self.engines[e].state.pattern_power != on {
                self.msg_power(e, on);
            }
            if used {
                self.select_pattern(e, engines[e].pattern, false);
            }
            if on {
                self.msg_phase(e, None, false);
            }
        }
        // The tail of scene activation: current scene, 0x1ee, then
        // 0x1ec.
        self.scenes.current = scene as i32;
        self.scenes.notify(crate::scene::SceneEvent::Selected(scene));
        self.scenes.notify(crate::scene::SceneEvent::Activated(scene));
    }

    /// [re:9c8f38]: release a held scene's
    /// gated patterns.
    fn scene_off(&mut self, scene: usize) {
        if !self.scenes.scenes.get(scene).is_some_and(|s| s.used) {
            return;
        }
        for e in 0..ENGINES.min(self.engines.len()) {
            if e == self.scenes.bypass {
                continue;
            }
            let eng = &self.engines[e];
            let st = &eng.state;
            if st.pattern_power && st.gate && eng.patterns.get(st.current_pattern).is_some_and(|p| p.sync.gated()) {
                self.msg_stop(e, false);
            }
        }
    }

    /// Queue a scene trigger ([re:d3287e]): `on` = note
    /// on/click, `false` = release. `delta` is the offset into the next block.
    pub fn trigger_scene(&mut self, scene: usize, delta: u32, on: bool) {
        let clock = self.scene_clock();
        self.scenes.add(scene, delta, on, Some(&clock));
    }

    /// (current pattern, pattern power, gate) of engine `e`.
    pub fn engine_pattern_state(&self, e: usize) -> Option<(usize, bool, bool)> {
        self.engines.get(e).map(|x| (x.state.current_pattern, x.state.pattern_power, x.state.gate))
    }

    /// The engine's current (track-limited) step while playing, for the
    /// pattern editor's position marker.
    /// Per transmod slot, [re:cf6b89] (the mini
    /// sequencer's playing step, −1 otherwise / empty).
    pub fn seq_steps(&self) -> [i32; 16] {
        let mut v = [-1; 16];
        for (k, m) in self.mods.iter().enumerate().take(16) {
            if let Some(m) = m {
                v[k] = m.visualization();
            }
        }
        v
    }

    pub fn current_step(&self, e: usize) -> Option<u32> {
        let eng = self.engines.get(e)?;
        (self.playing && eng.state.pattern_power).then_some(eng.state.current_limited_step)
    }

    /// [re:0d0d93].
    pub fn set_note_repeat_sync(&mut self, e: usize, on: bool) {
        if let Some(x) = self.engines.get_mut(e) {
            x.state.note_repeat_synced = on;
        }
    }

    pub fn scenes(&self) -> &SceneManager {
        &self.scenes
    }

    pub fn scenes_mut(&mut self) -> &mut SceneManager {
        &mut self.scenes
    }

    /// [re:483907] / internal loop range
    /// [re:a489b9] (range rounded to 1/960 QN).
    pub fn set_song_loop(&mut self, enabled: bool, start_qn: f64, end_qn: f64) {
        self.song.loop_enabled = enabled;
        self.song.loop_start = crate::song::quantize_round(start_qn);
        self.song.loop_end = crate::song::quantize_round(end_qn);
    }

    /// Record arm (message 0x14 `e_toggleRecord`).
    pub fn set_record(&mut self, on: bool) {
        self.recording = on;
    }

    /// Record quantize (message 0x19 `e_toggleQuantize`).
    pub fn set_quantize(&mut self, on: bool) {
        self.quantize = on;
    }

    /// [re:0b7874] + the recording message
    /// [re:0691ab]: a live note starting while playing writes a step at the
    /// engine's current step (velocity, coarse pitch/octave, and the timing
    /// offset in Shift unless quantize is on), mono-izing the column.
    fn record_step(&mut self, e: usize, pad: usize, phase: f64, semitone: f64, velocity: u32) {
        if !self.playing {
            return;
        }
        let quantize = self.quantize;
        let Some(eng) = self.engines.get_mut(e) else { return };
        if !eng.state.pattern_power || eng.pad_mode == PadMode::X0x {
            return;
        }
        let step_index = eng.state.current_limited_step as usize;
        let pattern_index = eng.state.current_pattern;
        let mut data = live_step(velocity, semitone);
        data.lanes[lane::SHIFT] = if quantize { 0.5 } else { (phase as f32 + 0.5).clamp(0.0, 1.0) };
        // Held notes of gated (not one-shot) pads extend the step while the
        // pattern plays (duration recording [re:802b5d]).
        if !eng.one_shot.get(pad).copied().unwrap_or(true) && eng.state.gate {
            if let Some(p) = eng.patterns.get(pattern_index) {
                let step_qn = p.length_16ths() * 0.25 * eng.timing.tempo_multiplier / p.steps.max(1) as f64;
                let sps = step_qn * 60.0 / self.bpm * self.sr as f64;
                if sps > 0.0 {
                    self.dur_recs.retain(|r| !(r.engine == e && r.pad == pad));
                    self.dur_recs.push(DurRec { engine: e, pad, pattern: pattern_index, start: step_index, step: step_index, accum: 0.0, sps, velocity, semitone });
                }
            }
        }
        self.write_recorded_step(e, pattern_index, pad, step_index, data);
    }

    /// Step playback [re:9496b8]: queued graph record values that reached a
    /// step go straight into it ([re:28d242], no linkage).
    fn apply_graph_writes(&mut self, e: usize) {
        let eng = &mut self.engines[e];
        if eng.state.graph_writes.is_empty() {
            return;
        }
        let writes = std::mem::take(&mut eng.state.graph_writes);
        let pattern_index = eng.state.current_pattern;
        for &(t, sidx, l, v) in &writes {
            let Some(step) = self.engines[e].patterns.get_mut(pattern_index).and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.steps.get_mut(sidx as usize)) else { continue };
            step.lanes[l] = v;
            self.doc_step(e, pattern_index, t, sidx as usize);
        }
        let mut writes = writes;
        writes.clear();
        self.engines[e].state.graph_writes = writes;
    }

    /// Set steps as another engine wrote them ([`StepWrite`]s carried
    /// across a rebuild whose document predates them); no new events.
    pub fn restore_steps<'a>(&mut self, writes: impl IntoIterator<Item = &'a StepWrite>) {
        for w in writes {
            let step = self.engines.get_mut(w.engine).and_then(|x| x.patterns.get_mut(w.pattern)).and_then(|p| p.tracks.get_mut(w.track)).and_then(|t| t.steps.get_mut(w.step));
            if let Some(step) = step {
                *step = w.data.clone();
            }
        }
    }

    /// A recorded step ([re:4e20da]): always to the retro recorder,
    /// into the pattern when record is on (the recording message
    /// [re:0691ab], mono-izing the column).
    fn write_recorded_step(&mut self, e: usize, pattern_index: usize, pad: usize, step_index: usize, data: crate::pattern::StepData) {
        if self.retro_events.len() < 4096 {
            self.retro_events.push(crate::songrec::RetroEvent::Step { engine: e, track: pad, step: step_index, data: data.clone() });
        }
        if !self.recording {
            return;
        }
        let eng = &mut self.engines[e];
        let Some(pattern) = eng.patterns.get_mut(pattern_index) else { return };
        let Some(track) = pattern.tracks.get_mut(pad) else { return };
        let Some(step) = track.steps.get_mut(step_index) else { return };
        *step = data;
        pattern.monoize(step_index, pad);
        self.doc_step(e, pattern_index, pad, step_index);
        // Mono-izing may have turned off this column on other tracks.
        let mono = self.engines[e].patterns[pattern_index].mono;
        if mono {
            for t in 0..TRACKS {
                if t != pad {
                    self.doc_step(e, pattern_index, t, step_index);
                }
            }
        }
    }

    /// [re:4ee51d] after `n` samples: a held
    /// note that fills its step moves on and writes the next step tied at
    /// full duration; on release the current step gets the played fraction
    /// as its Duration (tied unless it is the first). Stops when the
    /// engine's gate closes or its pattern changes.
    fn duration_record_tick(&mut self, n: usize) {
        // In place (audio thread): records dropped here are removed, the
        // rest keep their order.
        let mut i = 0;
        for _ in 0..self.dur_recs.len() {
            let mut r = self.dur_recs[i];
            let keep = self.duration_record_step(&mut r, n);
            if keep {
                self.dur_recs[i] = r;
                i += 1;
            } else {
                self.dur_recs.remove(i);
            }
        }
    }

    /// One record's sub-block: extends its step; false when it ends.
    fn duration_record_step(&mut self, r: &mut DurRec, n: usize) -> bool {
        let Some(eng) = self.engines.get(r.engine) else { return false };
        if !self.playing || !eng.state.gate || eng.state.current_pattern != r.pattern {
            return false;
        }
        let held = eng.state.held.get(r.pad).copied().unwrap_or(false);
        let steps = eng.patterns.get(r.pattern).map(|p| p.steps as usize).unwrap_or(1).max(1);
        r.accum += n as f64;
        let frac = if held {
            if r.accum < r.sps {
                return true;
            }
            r.step += 1;
            r.accum = 0.0;
            1.0
        } else {
            if r.accum > r.sps {
                r.step += 1;
                r.accum -= r.sps.floor();
            }
            r.accum / r.sps
        };
        r.step = r.step.min(steps - 1);
        let mut data = live_step(r.velocity, r.semitone);
        data.lanes[lane::DURATION] = frac as f32;
        if r.step != r.start {
            data.lanes[lane::LINK] = 1.0;
        }
        self.write_recorded_step(r.engine, r.pattern, r.pad, r.step, data);
        held
    }

    /// Metronome on/off (message 0x18: mute = !on).
    pub fn set_metronome(&mut self, on: bool) {
        self.metronome.muted = !on;
    }

    /// Host time signature (numerator, denominator; time info
    /// [re:c9d833] takes VstTimeInfo's when non-zero); else the document's.
    pub fn set_time_signature(&mut self, num: u32, den: u32) {
        self.time_sig = (num.max(1), den.max(1));
    }

    /// [re:59d08c] / voice MIDI CC [re:5c9583]:
    /// queue a MIDI out event `at` samples from the block start (dropped
    /// past 511 pending, like the original's buffer).
    fn queue_midi(&mut self, at: f64, data: [u8; 3]) {
        if self.midi_pending.len() < MIDI_PENDING_MAX {
            self.midi_pending.push((at.max(0.0), data));
        }
    }

    /// [re:e95690] (and
    /// [re:70dcfb] for pads that
    /// start no voice): note on (channel = engine, note = pad + 36), a CC
    /// for every graph lane with a MIDI binding (value·127 on the binding's
    /// channel / CC), and the note off `length` samples later.
    fn midi_out_note(&mut self, e: usize, r: &NoteRequest, lanes: &[f32], at: f64, length: f64) {
        if self.send_notes {
            let note = crate::live::PAD_NOTE_BASE as usize + r.pad;
            if note < 128 {
                let ch = (e & 15) as u8;
                self.queue_midi(at, [0x90 | ch, note as u8, r.velocity.clamp(1, 127) as u8]);
                self.queue_midi(at + length, [0x80 | ch, note as u8, 0]);
            }
        }
        if self.send_ccs && self.bindings.has_graph_bindings() {
            for (l, v) in lanes.iter().enumerate() {
                if let Some((ch, cc)) = self.bindings.graph_binding(e, r.pattern, r.pad, l) {
                    self.queue_midi(at, [0xb0 | (ch & 15), cc & 127, ((v * 127.0) as i32).clamp(0, 127) as u8]);
                }
            }
        }
    }

    /// [re:fd19be]: events due within this
    /// block go out sorted by time; the rest move to the next block.
    fn flush_midi_out(&mut self, n: usize) {
        self.midi_out.clear();
        if self.midi_pending.is_empty() {
            return;
        }
        let n = n as f64;
        let out = &mut self.midi_out;
        self.midi_pending.retain_mut(|(t, d)| {
            if *t < n {
                out.push((*t as u32, *d));
                false
            } else {
                *t -= n;
                true
            }
        });
        // Stable by time, in place (insertion sort: the queue is short and
        // mostly ordered).
        for i in 1..out.len() {
            let mut j = i;
            while j > 0 && out[j - 1].0 > out[j].0 {
                out.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// [re:6395b3] for a sequenced step.
    fn note_on(&mut self, e: usize, r: &NoteRequest) {
        let sr = self.sr;
        let eng = &mut self.engines[e];
        let Some(pattern) = eng.patterns.get(r.pattern) else { return };
        let track = &pattern.tracks[r.pad];
        // Live notes read the default step ([re:af32c0] / dummy pad step).
        let default_step = crate::pattern::StepData::off();
        let step = if r.step < 0 || r.live { &default_step } else { track.step(r.step as u32) };
        let mut rand = || self.rng.next_f32();
        let v1 = dreist_dsp::rules::get().lanes_v1;
        let force = if v1 {
            // v1 reads ForceLayer as stored [re:166a87].
            (step.lanes[lane::FORCE_LAYER] * 8.0).floor() as i32 - 1
        } else {
            (randomised(step.lanes[lane::FORCE_LAYER], track.rand[lane::FORCE_LAYER], &mut rand) * 8.0).floor() as i32 - 1
        };
        // Note-on parameters: each lane randomised, then stored through
        // [re:28d242] (on/off lanes snap at 0.5, so a randomised
        // Reverse is a coin flip, not almost never on) [decomp] ev_59538a6c….
        let lanes: dreist_dsp::voice::Lanes = if v1 {
            std::array::from_fn(|l| crate::pattern::lane_at_note_v1(step, track, l, &mut rand))
        } else {
            let mut lanes = std::array::from_fn(|l| randomised(step.lanes[l], track.rand[l], &mut rand));
            crate::pattern::snap_lanes(&mut lanes);
            lanes
        };
        let Some(pad) = eng.pads.get_mut(r.pad) else { return };
        let slots: [LayerSlot; 8] = std::array::from_fn(|i| pad.layers[i].slot);
        let picked = match r.layer {
            Some(li) => pad.picker.pick(LayerMode::AllLayers, &slots, r.velocity, li as i32, false, &mut rand),
            None => pad.picker.pick(pad.mode, &slots, r.velocity, force, self.forced_fallthrough, &mut rand),
        };
        let v2a = pad.v2a;
        // At most one start per layer (a fixed array: audio thread).
        let mut starts: [Option<(usize, Arc<SampleData>, NoteSetup, f64, CutMode, i64)>; 8] = Default::default();
        let mut nstarts = 0;
        for &li in picked.iter() {
            let layer = &pad.layers[li];
            let (Some(sample), Some(template)) = (&layer.sample, &layer.template) else { continue };
            let mut setup = template.clone();
            if r.live {
                setup.step = eng.dummy.get(r.pad).copied().flatten();
                setup.midi_velocity = Some(r.velocity);
            } else if v1 && nstarts > 0 {
                // v1 draws each voice's lane variants itself [re:bef782].
                setup.step = Some(std::array::from_fn(|l| crate::pattern::lane_at_note_v1(step, track, l, &mut rand)));
            } else {
                setup.step = Some(lanes);
            }
            if !r.live {
                setup.routing_raw = std::array::from_fn(|k| step.lanes[lane::ROUTING_DEPTH + k]);
            }
            setup.semitone_offset += r.semitone;
            setup.pad_v2a = v2a;
            // [re:b5f726]: PreDelay in ms, int(1000·v³)
            // (normalised value to ms), or with
            // `PreDelayInMsec` off a note value: floor(15v) indexes
            // 1/128d .. 1/8t, 240000/(BPM·div)·factor ms.
            let v = setup.layer[LayerParam::PreDelay as usize][0];
            let pre = if self.predelay_ms {
                ((v * v * 1000.0 * v) as i64) as f32 * 0.001 * sr
            } else {
                match ((v * 15.0).floor() as usize).min(15) {
                    0 => 0.0,
                    i => {
                        let (div, f) = PREDELAY_NOTES[i - 1];
                        240000.0 / (self.bpm as f32 * div) * f * 0.001 * sr
                    }
                }
            };
            let tail = (layer.fx.tail(&self.ctx) + pad.fx.tail(&self.ctx) + eng.fx.tail(&self.ctx)) as i64;
            starts[nstarts] = Some((li, sample.clone(), setup, pre as f64, layer.cut, tail));
            nstarts += 1;
        }
        if self.send_notes || self.send_ccs {
            let eng = &self.engines[e];
            let step_len = eng.patterns.get(r.pattern).map(|p| p.length_16ths() * 0.25 * eng.timing.tempo_multiplier / p.steps.max(1) as f64 * 60.0 / self.bpm * sr as f64).unwrap_or(1024.0);
            let out_lanes = if r.live { eng.dummy.get(r.pad).copied().flatten().unwrap_or_else(|| crate::pattern::StepData::off().lanes) } else { lanes };
            let at = self.sub_pos as f64 + r.start;
            if nstarts == 0 {
                // Non-sounding note as MIDI out: off after the step's Duration.
                let dur = out_lanes.get(lane::DURATION).copied().unwrap_or(1.0) as f64;
                self.midi_out_note(e, r, &out_lanes, at, dur * step_len);
            } else {
                for st in starts.iter().flatten() {
                    self.midi_out_note(e, r, &out_lanes, at + st.3, step_len);
                }
            }
        }
        for (li, sample, setup, pre, cut, tail) in starts.into_iter().flatten() {
            let slot = Slot { engine: e as u32, pad: r.pad as u32, layer: li as u32 };
            // [re:58d360]: timestretch layers use their own 16-voice
            // pool ([re:701ae1], stealing in [re:f48101]).
            let stretch = setup.layer[LayerParam::TimestretchActive as usize][0] >= 0.5;
            let pool = if stretch { NORMAL_VOICES..NORMAL_VOICES + TIMESTRETCH_VOICES } else { 0..NORMAL_VOICES };
            let base = pool.start;
            let idx = match self.voices[pool.clone()].iter().position(|v| v.state == VoiceState::Idle) {
                Some(i) => base + i,
                None => {
                    let info = self.voices[pool].iter().map(|v| VoiceInfo { state: v.state, id: v.id });
                    match steal(info, self.counter) {
                        Some(i) => {
                            self.meters.voice_stolen = true;
                            base + i
                        }
                        None => continue,
                    }
                }
            };
            let v = &mut self.voices[idx];
            v.state = VoiceState::Pending;
            v.id = self.counter;
            self.counter = self.counter.wrapping_add(1);
            v.slot = slot;
            v.cut = cut;
            v.delay = r.start + pre;
            v.duration = if r.duration < 0.0 { -1.0 } else { pre + r.duration };
            v.pending = Some((sample, setup));
            v.tail = tail;
            v.has_step = !r.live;
            v.live = r.live;
            v.semitone = r.semitone;
            v.record = r.record;
            v.velocity = r.velocity;
        }
    }

    /// [re:2aff70] for one sub-block.
    fn process_voices(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for b in [&mut self.buf.layer, &mut self.buf.pad, &mut self.buf.engine].into_iter().chain(self.buf.aux.iter_mut()).chain(self.buf.sub.iter_mut()) {
            for c in b.iter_mut() {
                c.clear();
                c.resize(n, 0.0);
            }
        }
        // Reused between blocks (no allocation on the audio thread).
        let mut order = std::mem::take(&mut self.voice_order);
        order.clear();
        order.extend((0..self.voices.len()).filter(|&i| self.voices[i].state != VoiceState::Idle));
        // Voice order: by engine, pad, layer, then voice index (an
        // unstable sort with the index in the key: no merge buffer).
        order.sort_unstable_by_key(|&i| {
            let s = self.voices[i].slot;
            (s.engine, s.pad, s.layer, i)
        });
        for (k, &vi) in order.iter().enumerate() {
            self.run_voice(vi, n);
            let s = self.voices[vi].slot;
            let next = order.get(k + 1).map(|&j| self.voices[j].slot);
            let (layer_end, pad_end, engine_end) = match next {
                None => (true, true, true),
                Some(t) if t.engine == s.engine && t.pad == s.pad && t.layer == s.layer => (false, false, false),
                Some(t) if t.engine == s.engine && t.pad == s.pad => (true, false, false),
                Some(t) => (true, true, t.engine != s.engine),
            };
            let m = *self.voices[vi].voice.modulation();
            if layer_end {
                self.layer_stage(vi, s, &m, out_l, out_r);
            }
            if pad_end {
                self.pad_stage(vi, s, &m, out_l, out_r);
            }
            if engine_end {
                self.engine_stage(vi, s, &m, out_l, out_r);
            }
        }
        self.voice_order = order;
        // Metronome voices (end of the voice processing), routed by its OutputLocation.
        {
            let Buffers { aux, sub, metro, .. } = &mut self.buf;
            let [ml, mr] = metro;
            ml.clear();
            ml.resize(n, 0.0);
            mr.clear();
            mr.resize(n, 0.0);
            self.metronome.render(self.sr, [&mut ml[..], &mut mr[..]]);
            let mut o = Outs { master: [&mut *out_l, &mut *out_r], sub, aux };
            route(self.metronome.route, [&ml[..], &mr[..]], None, &mut o, false);
        }
        self.metronome.process(n);
        // Global aux [re:733339]: effects with the host transmod vector, gain/pan, into the master.
        let hm = self.host.m();
        if self.options.aux_returns {
            for (i, a) in self.aux.iter_mut().enumerate() {
                let Buffers { aux, sub, .. } = &mut self.buf;
                let [l, r] = &mut aux[i];
                a.fx.update(&hm, None);
                a.fx.process(&self.ctx, l, r);
                fx_outs(&mut self.meters.fx_out, [-4, i as i8, -1], &a.fx);
                if a.muted {
                    continue;
                }
                a.mix.process(l, r);
                meter(&mut self.meters.aux[i], l, r);
                let (l, r) = (std::mem::take(l), std::mem::take(r));
                let mut o = Outs { master: [&mut *out_l, &mut *out_r], sub, aux };
                route(a.route, [&l, &r], None, &mut o, false);
                aux[i] = [l, r];
            }
        }
        // Master channel [re:3e19f6].
        self.master_fx.update(&hm, None);
        self.master_fx.process(&self.ctx, out_l, out_r);
        fx_outs(&mut self.meters.fx_out, [-3, -1, -1], &self.master_fx);
        self.master.process(out_l, out_r);
        let po = self.preview_output;
        if po > 1 && po - 2 < self.buf.sub.len() {
            let [mut l, mut r] = std::mem::take(&mut self.buf.sub[po - 2]);
            let n = out_l.len().min(l.len());
            self.render_preview(&mut l[..n], &mut r[..n]);
            self.buf.sub[po - 2] = [l, r];
        } else {
            self.render_preview(out_l, out_r);
        }
        meter(&mut self.meters.master, out_l, out_r);
    }

    /// [re:0d9e3a] / [re:bfc66d] into the layer buffer.
    fn run_voice(&mut self, vi: usize, n: usize) {
        if self.voices[vi].state == VoiceState::Tail {
            return;
        }
        if self.voices[vi].state == VoiceState::Pending {
            if self.voices[vi].delay > n as f64 {
                self.voices[vi].delay -= n as f64;
                return;
            }
            let (data, setup) = self.voices[vi].pending.take().expect("pending voice");
            // Voice setup: cut voices this note cuts.
            let (slot, mode) = (self.voices[vi].slot, self.voices[vi].cut);
            for (j, o) in self.voices.iter_mut().enumerate() {
                if j != vi && o.state == VoiceState::Playing && is_cut(o.slot, o.cut, slot, mode) {
                    o.voice.cut(CUT_FADE);
                }
            }
            let random = self.rng.next_f32();
            let v = &mut self.voices[vi];
            // Voice setup: per-note Random source.
            v.voice.transmod_values[src::RANDOM] = random;
            v.voice.transmod_values[src::SLOT1..].copy_from_slice(&self.host.values[src::SLOT1..]);
            v.voice.transmod_scale = self.host.scale();
            v.voice.transmod_offset = self.host.offset();
            if let Some(e) = self.engines.get(slot.engine as usize) {
                v.voice.engine_pitch = e.pitch;
                v.voice.pad_pitch = e.pads.get(slot.pad as usize).map_or(dreist_dsp::voice::centre_row(), |p| p.pitch);
            }
            v.voice.start(data, setup);
            v.state = VoiceState::Playing;
            if let Some(phase) = v.record {
                let (slot, semitone, velocity) = (v.slot, v.semitone, v.velocity);
                self.record_step(slot.engine as usize, slot.pad as usize, phase, semitone, velocity);
            }
        }
        let v = &mut self.voices[vi];
        // [re:d67556]: host sources 5..28 and polarity every block.
        v.voice.transmod_values[src::SLOT1..].copy_from_slice(&self.host.values[src::SLOT1..]);
        v.voice.transmod_scale = self.host.scale();
        v.voice.transmod_offset = self.host.offset();
        v.voice.host_bpm = self.bpm as f32;
        v.voice.master_pitch = self.master_pitch;
        if let Some(e) = self.engines.get(v.slot.engine as usize) {
            v.voice.engine_pitch = e.pitch;
            v.voice.pad_pitch = e.pads.get(v.slot.pad as usize).map_or(dreist_dsp::voice::centre_row(), |p| p.pitch);
        }
        let [bl, br] = &mut self.buf.layer;
        let mut pos = (v.delay.round() as i64).clamp(0, n as i64) as usize;
        v.delay = 0.0;
        while pos < n {
            let len = n - pos;
            if v.duration != -1.0 {
                let d = v.duration as i64 as u32 as i32;
                if (len as i32) < d {
                    v.duration -= len as f64;
                    v.voice.process(&mut bl[pos..n], &mut br[pos..n]);
                    pos = n;
                    continue;
                }
                v.duration = -1.0;
                if d != len as i32 {
                    if d <= 0 {
                        v.voice.release();
                    } else {
                        let m = (d as usize).min(len);
                        v.voice.process(&mut bl[pos..pos + m], &mut br[pos..pos + m]);
                        v.voice.release();
                        pos += m;
                        continue;
                    }
                } else {
                    v.voice.process(&mut bl[pos..n], &mut br[pos..n]);
                    v.voice.release();
                    pos = n;
                    continue;
                }
            }
            v.voice.process(&mut bl[pos..n], &mut br[pos..n]);
            pos = n;
        }
        // Voice kill [re:fad130]: the note ended; effect tails keep the voice alive.
        if !v.voice.is_active() {
            v.state = VoiceState::Tail;
        }
    }

    /// [re:409ea3] (stage 0 layer, 1 pad, 2 engine).
    fn kill_status(&mut self, vi: usize, stage: u32, buf: [&[f32]; 2]) {
        let n = buf[0].len() as i64;
        if self.voices[vi].state != VoiceState::Tail {
            return;
        }
        self.voices[vi].tail -= n;
        if self.voices[vi].tail > 0 {
            return;
        }
        let s = self.voices[vi].slot;
        if stage != 2 {
            let shared = self.voices.iter().enumerate().any(|(j, o)| j != vi && o.state != VoiceState::Idle && o.slot.engine == s.engine && (stage == 1 || o.slot.pad == s.pad));
            if !shared {
                return;
            }
        }
        let peak = |b: &[f32]| b.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        let (pl, pr) = (peak(buf[0]), peak(buf[1]));
        if (pl < TAIL_SILENCE && pr < TAIL_SILENCE) || (pl == 0.0 && pr == 0.0) {
            self.voices[vi].state = VoiceState::Idle;
        }
    }

    fn voice_lanes(&self, vi: usize) -> Option<dreist_dsp::voice::Lanes> {
        let v = &self.voices[vi];
        v.has_step.then(|| v.voice.lanes())
    }

    fn pre_flags(&self) -> [bool; 4] {
        std::array::from_fn(|k| self.aux.get(k).is_some_and(|a| a.pre_fade))
    }

    /// [re:acf267].
    fn layer_stage(&mut self, vi: usize, s: Slot, m: &[f32; SOURCES], out_l: &mut [f32], out_r: &mut [f32]) {
        let (ctx, pre) = (self.ctx, self.pre_flags());
        let layer = &mut self.engines[s.engine as usize].pads[s.pad as usize].layers[s.layer as usize];
        let Buffers { layer: [l, r], pad: [pl, pr], aux, sub, .. } = &mut self.buf;
        // An inaudible layer still ramps its gain, then is silenced.
        let silence = (!layer.audible).then_some(Silence::AfterGain);
        // v1 pans once, at the layer, by the closing voice's Pan lane [re:f87c3f].
        let pan_lane = dreist_dsp::rules::get().pan_v1.then(|| self.voices[vi].voice.lanes()[dreist_dsp::voice::lane::PAN]);
        let strip = Strip { fx: &mut layer.fx, sends: &mut layer.sends, aux_rows: &layer.aux, gain: &layer.gain, pan: &layer.pan, mix: &mut layer.mix, route: layer.route, silence, pan_lane };
        let acc = &mut self.meters.layer[(s.engine as usize * TRACKS + s.pad as usize) * LAYERS + s.layer as usize];
        mix_strip(strip, &ctx, m, None, pre, [l, r], Some([pl, pr]), aux, sub, [out_l, out_r], acc);
        fx_outs(&mut self.meters.fx_out, [s.engine as i8, s.pad as i8, s.layer as i8], &layer.fx);
        self.end_stage(vi, 0);
    }

    /// [re:8eb098].
    fn pad_stage(&mut self, vi: usize, s: Slot, m: &[f32; SOURCES], out_l: &mut [f32], out_r: &mut [f32]) {
        let (ctx, pre) = (self.ctx, self.pre_flags());
        let lanes = self.voice_lanes(vi);
        let pad = &mut self.engines[s.engine as usize].pads[s.pad as usize];
        let Buffers { pad: [l, r], engine: [el, er], aux, sub, .. } = &mut self.buf;
        let silence = pad.muted.then_some(Silence::InsteadOfGain);
        let strip = Strip { fx: &mut pad.fx, sends: &mut pad.sends, aux_rows: &pad.aux, gain: &pad.gain, pan: &pad.pan, mix: &mut pad.mix, route: pad.route, silence, pan_lane: None };
        let acc = &mut self.meters.pad[s.engine as usize * TRACKS + s.pad as usize];
        mix_strip(strip, &ctx, m, lanes.as_ref().map(|l| &l[..]), pre, [l, r], Some([el, er]), aux, sub, [out_l, out_r], acc);
        fx_outs(&mut self.meters.fx_out, [s.engine as i8, s.pad as i8, -1], &pad.fx);
        self.end_stage(vi, 1);
    }

    /// [re:342518].
    fn engine_stage(&mut self, vi: usize, s: Slot, m: &[f32; SOURCES], out_l: &mut [f32], out_r: &mut [f32]) {
        let (ctx, pre) = (self.ctx, self.pre_flags());
        let eng = &mut self.engines[s.engine as usize];
        let Buffers { engine: [l, r], aux, sub, .. } = &mut self.buf;
        let silence = eng.muted.then_some(Silence::InsteadOfGain);
        let strip = Strip { fx: &mut eng.fx, sends: &mut eng.sends, aux_rows: &eng.aux, gain: &eng.gain, pan: &eng.pan, mix: &mut eng.mix, route: eng.route, silence, pan_lane: None };
        mix_strip(strip, &ctx, m, None, pre, [l, r], None, aux, sub, [out_l, out_r], &mut self.meters.engine[s.engine as usize]);
        fx_outs(&mut self.meters.fx_out, [s.engine as i8, -1, -1], &eng.fx);
        self.end_stage(vi, 2);
    }

    /// After a stage (0 layer, 1 pad, 2 engine): the voice's tail check on
    /// the stage's output, then the buffer is cleared for the next voice.
    fn end_stage(&mut self, vi: usize, stage: u32) {
        let [mut l, mut r] = std::mem::take(self.buf.stage(stage));
        self.kill_status(vi, stage, [&l, &r]);
        l.fill(0.0);
        r.fill(0.0);
        *self.buf.stage(stage) = [l, r];
    }
}

/// How a strip is silenced (layer effects zero an inaudible layer
/// after its gain stage; a muted pad or engine skips the gain stage).
#[derive(Clone, Copy)]
enum Silence {
    AfterGain,
    InsteadOfGain,
}

/// One mixer strip (layer, pad or engine) as [`mix_strip`] needs it.
struct Strip<'a> {
    fx: &'a mut FxChain,
    sends: &'a mut [SendGain; 4],
    aux_rows: &'a [Row; 4],
    gain: &'a Row,
    pan: &'a Row,
    mix: &'a mut Mixable,
    route: Route,
    silence: Option<Silence>,
    /// The step's Pan lane pushing the pan ([`dreist_dsp::rules::Rules::pan_v1`], layers).
    pan_lane: Option<f32>,
}

/// A strip's processing: effects (with the voice's step lanes for pads),
/// pre-fade sends, gain / pan (or silence), the meter, routing to `parent`
/// or an output, post-fade sends.
#[allow(clippy::too_many_arguments)]
fn mix_strip(
    st: Strip,
    ctx: &Ctx,
    m: &[f32; SOURCES],
    lanes: Option<&[f32]>,
    pre: [bool; 4],
    [l, r]: [&mut Vec<f32>; 2],
    parent: Option<[&mut Vec<f32>; 2]>,
    aux: &mut [[Vec<f32>; 2]],
    sub: &mut [[Vec<f32>; 2]],
    out: [&mut [f32]; 2],
    acc: &mut [f32; 2],
) {
    if !st.fx.is_empty() {
        st.fx.update(m, lanes);
        st.fx.process(ctx, l, r);
    }
    send(st.sends, st.aux_rows, m, [l, r], aux, |k| pre[k]);
    let pan = param_sum(st.pan, m);
    let pan = st.pan_lane.map_or(pan, |l| dreist_dsp::convert::modulate_pos_neg(pan, l));
    st.mix.set(param_sum(st.gain, m), pan);
    match st.silence {
        None => st.mix.process(l, r),
        Some(Silence::AfterGain) => {
            st.mix.process(l, r);
            l.fill(0.0);
            r.fill(0.0);
        }
        Some(Silence::InsteadOfGain) => {
            l.fill(0.0);
            r.fill(0.0);
        }
    }
    meter(acc, l, r);
    let mut o = Outs { master: out, sub, aux };
    route(st.route, [l, r], parent.map(|[pl, pr]| [&mut pl[..], &mut pr[..]]), &mut o, true);
    send(st.sends, st.aux_rows, m, [l, r], o.aux, |k| !pre[k]);
}

/// Aux sends whose pre/post flag matches `pick` ([re:591274]).
fn send(sends: &mut [SendGain; 4], rows: &[Row; 4], m: &[f32; SOURCES], src: [&[f32]; 2], aux: &mut [[Vec<f32>; 2]], pick: impl Fn(usize) -> bool) {
    for k in 0..4 {
        if pick(k) {
            let [al, ar] = &mut aux[k];
            sends[k].process(param_sum(&rows[k], m), src, [al, ar]);
        }
    }
}

fn empty_engine() -> EngineRt {
    EngineRt {
        pads: Vec::new(),
        patterns: Vec::new(),
        timing: EngineTiming::default(),
        state: EngineState { pattern_power: false, ..EngineState::default() },
        gain: [0.0; SOURCES],
        pan: [0.0; SOURCES],
        pitch: dreist_dsp::voice::centre_row(),
        aux: [[0.0; SOURCES]; 4],
        sends: Default::default(),
        mix: Mixable::default(),
        muted: true,
        fx: FxChain::from_node(None, 48000.0),
        route: Route::Parent,
        one_shot: [true; TRACKS],
        pad_mode: PadMode::Normal,
        mute: false,
        solo: false,
        links: Box::new([[false; TRACKS]; TRACKS]),
        dummy: vec![None; TRACKS],
    }
}

fn load_engine(e: &dreist_formats::Engine, samples: &Path, sr: f32, solo_eng: bool, cache: &mut SampleCache) -> EngineRt {
    let n = &e.raw;
    let engine_audible = solo_pass(solo_eng, n.bool("Solo").unwrap_or(false), n.bool("Mute").unwrap_or(false));
    let pad_nodes: Vec<Option<&dreist_formats::Pad>> = (0..TRACKS).map(|i| e.pads.iter().find(|p| p.index as usize == i)).collect();
    let solo_pad = pad_nodes.iter().flatten().any(|p| p.raw.bool("Solo").unwrap_or(false));
    let mut one_shot = [true; TRACKS];
    let mut links = Box::new([[false; TRACKS]; TRACKS]);
    let pads = pad_nodes
        .iter()
        .enumerate()
        .map(|(pi, p)| {
            let Some(p) = p else { return empty_pad() };
            let pn = &p.raw;
            let pad_audible = solo_pass(solo_pad, pn.bool("Solo").unwrap_or(false), pn.bool("Mute").unwrap_or(false));
            for (j, l) in links[pi].iter_mut().enumerate() {
                *l = pn.bool(&format!("Link{j}")).unwrap_or(false);
            }
            let solo_layer = p.layers.iter().any(|l| l.raw.bool("Solo").unwrap_or(false));
            let layers: Vec<LayerRt> = (0..LAYERS)
                .map(|li| match p.layers.iter().find(|l| l.index as usize == li) {
                    None => empty_layer(),
                    Some(l) => {
                        let audible = engine_audible && pad_audible && solo_pass(solo_layer, l.raw.bool("Solo").unwrap_or(false), l.raw.bool("Mute").unwrap_or(false));
                        load_layer(l, samples, sr, audible, cache)
                    }
                })
                .collect();
            if layers.iter().any(|l| l.slot.valid && l.template.as_ref().is_some_and(|t| t.playback as u32 > 1)) {
                one_shot[pi] = false;
            }
            PadRt {
                layers,
                mode: pn.attr("PadMode").map(layer_mode).unwrap_or(LayerMode::AllLayers),
                picker: LayerPicker::new(),
                gain: row(pn, "Gain", PadParam::Gain.default_value()),
                pan: row(pn, "Pan", 0.5),
                v2a: p.param(PadParam::V2A),
                pitch: row(pn, "Pitch", PadParam::Pitch.default_value()),
                aux: aux_rows(pn),
                sends: Default::default(),
                mix: Mixable::new(p.param(PadParam::Gain), p.param(PadParam::Pan)),
                muted: false,
                fx: FxChain::from_node(pn.children_of("EffectChain").next(), sr),
                route: Route::of(pn),
                mute: pn.bool("Mute").unwrap_or(false),
                solo: pn.bool("Solo").unwrap_or(false),
            }
        })
        .collect();
    let patterns: Vec<PatternData> =
        (0..PATTERNS).map(|i| n.children_of("StepPattern").filter(|sp| sp.index() == Some(i as u32)).last().map(PatternData::from_node).unwrap_or_else(PatternData::empty)).collect();
    let es = n.children_of("EngineState").next();
    // [re:4299d3].
    let state = EngineState {
        current_pattern: es.and_then(|s| s.u32("CurrentPattern")).unwrap_or(0) as usize,
        pattern_power: es.and_then(|s| s.bool("PatternPower")).unwrap_or(true),
        full_level_velocity: es.and_then(|s| s.attr("FullLevels")).and_then(|v| v.parse().ok()).unwrap_or(127),
        full_level: es.and_then(|s| s.bool("FullLevelEnabled")).unwrap_or(false),
        note_erase: es.and_then(|s| s.bool("NoteEraseEnabled")).unwrap_or(false),
        note_repeat: es.and_then(|s| s.bool("NoteRepeatEnabled")).unwrap_or(false),
        note_repeat_synced: es.and_then(|s| s.bool("NoteRepeatSynced")).unwrap_or(true),
        visible_pads: es.and_then(|s| s.u32("VisiblePads")).unwrap_or(0).min(4),
        current_pad: es.and_then(|s| s.attr("padSelAnchor")).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0).clamp(0, 63) as usize,
        ..EngineState::default()
    };
    let num = n.u32("TempoNumerator").unwrap_or(1).max(1) as f64;
    let den = n.u32("TempoDenominator").unwrap_or(1).max(1) as f64;
    EngineRt {
        pads,
        patterns,
        timing: EngineTiming {
            tempo_multiplier: num / den,
            tempo_num: num as u32,
            tempo_den: den as u32,
            swing: n.children_of("SwingTemplate").next().map(Swing::from_node).unwrap_or_default(),
            swing_depth: n.f32("SwingTemplateDepth").unwrap_or(0.0),
        },
        state,
        gain: row(n, "Gain", EngineParam::Gain.default_value()),
        pan: row(n, "Pan", 0.5),
        pitch: row(n, "Pitch", 0.5),
        aux: aux_rows(n),
        sends: Default::default(),
        mix: Mixable::new(n.f32("Gain").unwrap_or(EngineParam::Gain.default_value()), n.f32("Pan").unwrap_or(0.5)),
        muted: false,
        fx: FxChain::from_node(n.children_of("EffectChain").next(), sr),
        route: Route::of(n),
        one_shot,
        pad_mode: n.attr("EnginePadMode").map(PadMode::from_name).unwrap_or_default(),
        mute: n.bool("Mute").unwrap_or(false),
        solo: n.bool("Solo").unwrap_or(false),
        links,
        dummy: vec![None; TRACKS],
    }
}

fn empty_pad() -> PadRt {
    PadRt {
        layers: (0..LAYERS).map(|_| empty_layer()).collect(),
        mode: LayerMode::AllLayers,
        picker: LayerPicker::new(),
        gain: [0.0; SOURCES],
        pan: [0.0; SOURCES],
        v2a: 1.0,
        pitch: dreist_dsp::voice::centre_row(),
        aux: [[0.0; SOURCES]; 4],
        sends: Default::default(),
        mix: Mixable::default(),
        muted: true,
        fx: FxChain::from_node(None, 48000.0),
        route: Route::Parent,
        mute: false,
        solo: false,
    }
}

/// [re:a96674]: (bpm, bars, beats) of a
/// file without a `.gsa` ([re:3df5f2] builds
/// its Master analysis from it): 1, 2, 4 or 8 bars of the whole buffer
/// (240 / 480 / 960 / 1920 ÷ seconds, single rounding) in 90..=180, else
/// a tempo in the file name ([re:49b646]: the
/// first digit run, 85..=180; bars = rounded length in bars, beats =
/// tenths of the remainder), else 120 / 1 bar (and 440 Hz: the last
/// flag).
pub fn guess_key_data(frames: u32, sample_rate: f32, path: Option<&Path>) -> (f32, f32, f32, bool) {
    let len = if sample_rate > 0.0 { frames as f32 / sample_rate } else { 0.0 };
    let round = |x: f32| (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32;
    for (bars, k) in [(1.0, 240.0), (2.0, 480.0), (4.0, 960.0), (8.0, 1920.0)] {
        let i = round(k / len);
        if (90..=180).contains(&i) {
            return (i as f32, bars, 0.0, false);
        }
    }
    let stem = path.and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let digits: String = stem.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    let t: f32 = digits.parse().unwrap_or(0.0);
    if (85.0..=180.0).contains(&t) && sample_rate > 0.0 {
        let spb = 4.0 * 60.0 / t as f64 * sample_rate as f64;
        let x = frames as f64 / spb;
        let bars = (x + 0.5).floor() as i32;
        let beats = ((x - bars as f64) * 10.0).floor().max(0.0);
        return (t, bars as f32, beats as f32, false);
    }
    (120.0, 1.0, 0.0, true)
}

/// Tempo-synced pre-delays ([re:6eecd9] table copied from
/// [re:fa9455]): (note division, factor) for 1/128d, 1/128, 1/128t .. 1/8t.
const PREDELAY_NOTES: [(f32, f32); 15] = [
    (128.0, 1.5),
    (128.0, 1.0),
    (128.0, 2.0 / 3.0),
    (64.0, 1.5),
    (64.0, 1.0),
    (64.0, 2.0 / 3.0),
    (32.0, 1.5),
    (32.0, 1.0),
    (32.0, 2.0 / 3.0),
    (16.0, 1.5),
    (16.0, 1.0),
    (16.0, 2.0 / 3.0),
    (8.0, 1.5),
    (8.0, 1.0),
    (8.0, 2.0 / 3.0),
];

fn empty_layer() -> LayerRt {
    LayerRt {
        sample: None,
        template: None,
        cut: CutMode::NotCut,
        slot: LayerSlot { valid: false, vel_low: 0, vel_high: 127 },
        gain: [0.0; SOURCES],
        pan: [0.0; SOURCES],
        aux: [[0.0; SOURCES]; 4],
        sends: Default::default(),
        mix: Mixable::default(),
        audible: false,
        fx: FxChain::from_node(None, 48000.0),
        route: Route::Parent,
        mute: false,
        solo: false,
    }
}

fn load_layer(l: &dreist_formats::Layer, samples: &Path, _sr: f32, audible: bool, cache: &mut SampleCache) -> LayerRt {
    let n = &l.raw;
    let path: Option<PathBuf> = l.audio_file.as_ref().map(|f| dreist_formats::model::resolve_sample(samples, f));
    let sample = path.as_deref().and_then(|p| cache.get(p));
    let mut matrix = [[0.0f32; SOURCES]; LAYER_PARAMS];
    for (pi, p) in LayerParam::ALL.iter().enumerate() {
        matrix[pi][0] = l.param(*p);
    }
    l.depths(|p, s, v| matrix[p as usize][s as usize] = v);
    // [re:87acba] → hold times reset for layers
    // without (or with negative) holds and pre-3000 layers.
    if let Some(s) = sample.as_ref().filter(|_| crate::hold::needs_reset_on_load(n)) {
        let region = l.start_end.map(|m| (m.end as f64 - m.start as f64).max(0.0)).unwrap_or(0.0);
        let m = matrix;
        let h = crate::hold::reset_hold_time(&|q: LayerParam| m[q as usize][0], false, region, s.left.len() as f64, s.sample_rate, true);
        matrix[LayerParam::AmpEnvHold as usize][0] = h;
        matrix[LayerParam::FreeEnvHold as usize][0] = h;
    }
    let split = n.children.iter().find(|c| c.name.as_deref() == Some("VelocitySplit"));
    let vel_low = split.and_then(|s| s.attr("LowPoint")).and_then(|v| v.parse().ok()).unwrap_or(0u32).min(127);
    let vel_high = split.and_then(|s| s.attr("Highest")).and_then(|v| v.parse().ok()).unwrap_or(127u32).min(127);
    let sidecar_bpm = sample.as_ref().and(path.as_deref()).and_then(|p| cache.sidecar_bpm(p));
    let template = sample.as_ref().map(|s| {
        let frames = s.left.len() as u32;
        let (start, end) = l.start_end.map(|m| (m.start as u32, (m.end as u32).min(frames))).unwrap_or((0, frames));
        let (loop_start, loop_end) = l.loop_markers.map(|m| (m.start as u32, m.end as u32)).unwrap_or((start, end));
        NoteSetup {
            layer: matrix,
            playback: n.attr("PlaybackMode").map(PlaybackMode::from_name).unwrap_or(PlaybackMode::OneShot),
            vintage: n.attr("SynthesisMode") == Some("VintageSampler"),
            start,
            end,
            loop_start,
            loop_end,
            loop_len: loop_end.saturating_sub(loop_start),
            step: None,
            midi_velocity: None,
            pad_v2a: 1.0,
            semitone_offset: 0.0,
            // `PhasePosition` (1 normal, −1 inverted; the original's
            // [re:75d14c] [re:55cc56] scales the voice gain by it).
            output_scale: n.f32("PhasePosition").unwrap_or(1.0),
            reversed: n.bool("IsReversed").unwrap_or(false),
            env_tracks: n.bool("EnvTracksPitch").unwrap_or(false),
            file_bpm: sidecar_bpm.unwrap_or_else(|| guess_key_data(s.left.len() as u32, s.sample_rate, path.as_deref()).0),
            stretch_bpm: n.f32("SourceBPM").map(|src| (src, n.f32("DestinationBPM").unwrap_or(120.0))),
            routings: dreist_formats::legacy::routing_slots(n),
            routing_raw: [0.5; 4],
        }
    });
    LayerRt {
        slot: LayerSlot { valid: sample.is_some(), vel_low, vel_high },
        sample,
        template,
        // [re:87acba] reads CutMode with the default 1
        // ([re:f08395]): an omitted value is "Self".
        cut: n.attr("CutMode").map(CutMode::from_name).unwrap_or(CutMode::BySelf),
        gain: matrix[LayerParam::Gain as usize],
        pan: matrix[LayerParam::Pan as usize],
        aux: aux_rows(n),
        sends: Default::default(),
        mix: Mixable::new(matrix[LayerParam::Gain as usize][0], matrix[LayerParam::Pan as usize][0]),
        audible,
        fx: FxChain::from_node(n.children_of("EffectChain").next(), _sr),
        route: Route::of(n),
        mute: n.bool("Mute").unwrap_or(false),
        solo: n.bool("Solo").unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::{Dreist, RenderOptions, Route};

    /// v1's send summing: skipped while the last level is 0, then a ramp
    /// from the last level with the first sample at it.
    #[test]
    fn v1_send_ramps_from_the_last_level() {
        let mut s = super::SendGain { last: 0.0 };
        let src = [1.0f32; 4];
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        s.process_v1(0.8, [&src, &src], [&mut l, &mut r]);
        assert_eq!(l, [0.0; 4]);
        assert_eq!(s.last, 0.8);
        s.process_v1(0.4, [&src, &src], [&mut l, &mut r]);
        let want = [0.8, 0.7, 0.6, 0.5];
        assert!(l.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "{l:?}");
        assert_eq!(r, l);
    }

    /// A looping preview region shorter than the playback step (an 88.2 kHz
    /// file at 44.1 kHz, two frames at the file's end) wraps in place.
    #[test]
    fn short_looping_preview_stays_in_bounds() {
        let mut g = Dreist::empty(44_100.0, RenderOptions::default());
        let data = std::sync::Arc::new(dreist_dsp::voice::SampleData { left: vec![0.5; 100], right: None, sample_rate: 88_200.0 });
        g.start_preview_region(data, 98, 100, true);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        g.process(&mut l, &mut r);
        assert!(l.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn restored_steps_land_without_events() {
        let doc = dreist_formats::Node::parse(r#"<root version="3000"><Engine name="Engine2" /></root>"#).unwrap();
        let mut g = Dreist::from_document(&doc, std::path::Path::new("."), 44_100.0, RenderOptions::default(), &mut Default::default()).unwrap();
        let data = crate::pattern::StepData { on: true, ..crate::pattern::StepData::off() };
        let w = super::StepWrite { engine: 2, pattern: 5, track: 7, step: 3, data };
        let beyond = super::StepWrite { step: 999, ..w.clone() };
        g.restore_steps([&w, &beyond]);
        assert!(g.engines[2].patterns[5].tracks[7].steps[3].on);
        assert!(g.step_writes.is_empty() && g.retro_events.is_empty());
    }

    #[test]
    fn fx_outputs_keep_the_latest_per_slot() {
        use super::{FX_OUTS, FxOut, Meters};
        let mut m = Meters::default();
        m.fx_out.push(FxOut { at: [0, -1, -1], slot: 2, value: 0.4 });
        let mut old = Meters::default();
        old.fx_out.push(FxOut { at: [0, -1, -1], slot: 2, value: 0.9 });
        old.fx_out.push(FxOut { at: [-3, -1, -1], slot: 0, value: 0.7 });
        m.merge(&old);
        assert_eq!((m.fx_output([0, -1, -1], 2), m.fx_output([-3, -1, -1], 0), m.fx_output([1, -1, -1], 2)), (Some(0.4), Some(0.7), None));
        m.clear();
        assert!(m.fx_out.is_empty() && m.fx_out.capacity() >= FX_OUTS);
    }

    /// Output the old engine produced but nobody collected survives a
    /// rebuild.
    #[test]
    fn adopt_keeps_uncollected_output() {
        let mut old = Dreist::empty(44_100.0, RenderOptions::default());
        old.step_writes.push(super::StepWrite { engine: 1, pattern: 2, track: 3, step: 4, data: crate::pattern::StepData::off() });
        old.midi_log.push([0x90, 36, 100]);
        old.mods_triggered = 0b100;
        old.meters.master = [0.5, 0.25];
        let mut new = Dreist::empty(44_100.0, RenderOptions::default());
        new.adopt(&mut old);
        assert_eq!((new.step_writes.len(), new.midi_log.len(), new.mods_triggered, new.meters.master), (1, 1, 0b100, [0.5, 0.25]));
    }

    #[test]
    fn host_transport_edges_only() {
        let mut g = Dreist::empty(48000.0, RenderOptions::default());
        // A stopped host leaves the original's own Play alone.
        g.set_transport(false, 120.0, Some(0.0));
        g.start();
        g.set_transport(false, 120.0, Some(0.0));
        assert!(g.is_playing());
        // Host start / stop move the transport.
        g.stop();
        g.set_transport(true, 120.0, Some(4.0));
        assert!(g.is_playing());
        g.set_transport(false, 120.0, Some(4.0));
        assert!(!g.is_playing());
        // A rebuilt engine keeps playing.
        g.start();
        let mut n = Dreist::empty(48000.0, RenderOptions::default());
        n.adopt(&mut g);
        assert!(n.is_playing());
    }

    fn run(g: &mut Dreist, samples: usize, block: usize) {
        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        for _ in 0..samples / block {
            g.process(&mut l, &mut r);
        }
    }

    #[test]
    fn preroll_counts_in_then_starts() {
        let opts = RenderOptions { preroll_mode: 1, preroll_only_rec: false, ..RenderOptions::default() };
        let mut g = Dreist::empty(48000.0, opts);
        g.set_transport(false, 120.0, None);
        g.play();
        assert!(g.is_playing() && g.in_preroll());
        assert_eq!(g.ppq, -4.0);
        // One 4/4 bar at 120 BPM: 96000 samples; the metronome counts in.
        g.metronome.muted = false;
        let mut clicked = false;
        let (mut l, mut r) = (vec![0.0; 500], vec![0.0; 500]);
        for _ in 0..191 {
            g.process(&mut l, &mut r);
            clicked |= g.metronome.clicking();
            assert!(g.in_preroll());
        }
        assert!(clicked);
        g.process(&mut l, &mut r);
        assert!(!g.in_preroll() && g.is_playing());
        assert!(g.ppq.abs() < 1e-9, "{}", g.ppq);
        // A sub-block split mid-block lands on the start too.
        let mut g = Dreist::empty(48000.0, opts);
        g.play();
        run(&mut g, 96256, 512);
        assert!(!g.in_preroll());
        assert!((g.ppq - 256.0 * 2.0 / 48000.0).abs() < 1e-9, "{}", g.ppq);
        // Only when recording: not recording plays straight away.
        let mut g = Dreist::empty(48000.0, RenderOptions { preroll_mode: 3, ..RenderOptions::default() });
        g.play();
        assert!(g.is_playing() && !g.in_preroll() && g.ppq == 0.0);
        g.stop();
        g.recording = true;
        g.play();
        assert!(g.in_preroll() && g.ppq == -16.0);
        g.stop();
        assert!(!g.is_playing() && !g.in_preroll());
    }

    #[test]
    fn wait_for_midi_starts_on_a_pad_note() {
        let mut g = Dreist::empty(48000.0, RenderOptions { wait_midi: true, ..RenderOptions::default() });
        g.stop();
        g.play();
        assert!(g.is_playing() && !g.playing);
        run(&mut g, 4096, 512);
        assert_eq!(g.ppq, 0.0);
        // Pattern notes do not count; pad notes (36..) do.
        g.push_midi(0, [0x90, 20, 100]);
        run(&mut g, 512, 512);
        assert!(!g.playing);
        g.push_midi(0, [0x90, 40, 100]);
        run(&mut g, 512, 512);
        assert!(g.playing && g.ppq > 0.0);
        // Stop while waiting.
        g.stop();
        g.play();
        g.stop();
        assert!(!g.is_playing());
    }

    #[test]
    fn metronome_only_when_recording() {
        let mut g = Dreist::empty(48000.0, RenderOptions { metro_only_rec: true, ..RenderOptions::default() });
        g.metronome.muted = false;
        g.start();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        let mut clicked = false;
        for _ in 0..200 {
            g.process(&mut l, &mut r);
            clicked |= g.metronome.clicking();
        }
        assert!(!clicked);
        g.recording = true;
        for _ in 0..200 {
            g.process(&mut l, &mut r);
            clicked |= g.metronome.clicking();
        }
        assert!(clicked);
    }

    #[test]
    fn selection_follows_midi_notes() {
        use crate::binding::id;
        let mut g = Dreist::empty(48000.0, RenderOptions { engine_follow: true, ..RenderOptions::default() });
        // Channel 3 pad 5 → engine 3 selected, then its pad.
        g.push_midi(0, [0x92, 41, 100]);
        run(&mut g, 512, 512);
        assert_eq!(g.selected_engine(), 2);
        assert_eq!(g.engines[2].state.current_pad, 5);
        let kinds: Vec<u32> = g.editor_messages.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, [id::UPDATE_ENGINE_SELECTION_CHANGED, id::UPDATE_PAD_SELECTION_CHANGED]);
        // Pad follow alone leaves other engines' notes alone.
        let mut g = Dreist::empty(48000.0, RenderOptions::default());
        g.push_midi(0, [0x92, 41, 100]);
        g.push_midi(0, [0x90, 43, 100]);
        run(&mut g, 512, 512);
        assert_eq!((g.selected_engine(), g.engines[2].state.current_pad, g.engines[0].state.current_pad), (0, 0, 7));
    }

    #[test]
    fn output_location_names() {
        assert_eq!(Route::from_name("Parent"), Route::Parent);
        assert_eq!(Route::from_name("Master"), Route::Master);
        assert_eq!(Route::from_name("Sub output 1"), Route::Sub(0));
        assert_eq!(Route::from_name("Sub output 32"), Route::Sub(31));
        assert_eq!(Route::from_name("Aux 4"), Route::Aux(3));
        assert_eq!(Route::from_name("Aux 5"), Route::Parent);
        assert_eq!(Route::from_name("bogus"), Route::Parent);
    }
}
