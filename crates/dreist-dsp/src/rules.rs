//! Behaviour switches an app may set where its instrument behaves
//! differently from the reference (Dreist's): installed once at start-up
//! with [`install`]; until then (and in Dreist) [`get`] returns [`REFERENCE`].

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Seconds below the stop threshold after which an armed recording
    /// stops by itself.
    pub sampler_auto_stop_seconds: f32,
    /// The envelope's end rules of the original v1 (a decay ending at zero
    /// sustain stops at once instead of fading over 32 samples; the release
    /// stops at y <= 0; the decay output is not clamped).
    pub envelope_v1_ends: bool,
    /// The original v1's forward loop: no crossfade, wrap at the loop end
    /// minus 3 frames keeping the fractional position.
    pub loop_v1: bool,
    /// The original v1's step lanes at note start: only the lanes the
    /// voice and sequencer pass through v1's lane variant (track scale,
    /// compression, unrounded randomisation) vary; every other lane, the
    /// sequencer's layer-selection velocity and ForceLayer included, is
    /// used as stored, and the voice's lanes are not snapped.
    pub lanes_v1: bool,
    /// The original v1's one-stage pan: the voice does not pan by the
    /// step's Pan lane; the layer stage pans by the layer pan pushed by the
    /// Pan lane of the voice that closes the layer group.
    pub pan_v1: bool,
    /// The original v1's reverse: the read position mirrors the forward
    /// position inside the start/end window, and the direction follows the
    /// layer's live `IsReversed` (xor the step's Reverse lane) every block.
    pub reverse_v1: bool,
    /// The original v1's end fade: it starts 2 frames earlier (end ≤
    /// position + 34) and lasts to the end marker; cut fades clamp to the
    /// end marker too.
    pub end_fade_v1: bool,
    /// The original v1's aux send summing: a send ramps from its last level
    /// and is skipped while that level is 0 (the target only takes effect
    /// in the next block).
    pub sends_v1: bool,
    /// The original v1's timestretch ratio: the layer's source BPM over the
    /// host BPM (tracking) or its destination BPM, at most 4, and the pitch
    /// ratio at least 0.5.
    pub timestretch_v1: bool,
    /// The original v1's pattern MIDI export: the pattern's own step count
    /// (steps past a shorter track are off), no probability draw, tied
    /// steps always merged, notes not shortened by later ones, step graphs
    /// 0.1 QN before the note (a note at 0 moves 0.007 QN later) and only
    /// v1's graph lanes.
    pub midi_export_v1: bool,
    /// The original v1's default MIDI map: 16 pads per engine channel
    /// (notes 36–51), note repeat / erase on notes 54 / 56, scenes 0–71 on
    /// the scene channel.
    pub midi_map_v1: bool,
    /// Pads per engine the slicer allocates slices to (the original v1:
    /// 16, at most 16 slices, 128 with layers).
    pub slice_pads: usize,
    /// The original v1's scene snapshot: an engine whose current pattern
    /// holds no step is stored with pattern power off.
    pub scene_snapshot_v1: bool,
    /// The original v1's project `Sampler` node also keeps the trigger and
    /// record modes (`startmode` Immediate / MIDI / Host / Threshold,
    /// `timedmode` Free / Timed / Loop).
    pub sampler_modes_v1: bool,
    /// The original v1's sequencer window: each sub-block's play position
    /// starts at `start − 1` samples ([re:01fcac]; v2 at `start`), so
    /// sequenced notes and metronome clicks start one sample later.
    pub sequencer_window_v1: bool,
    /// Voices in the timestretch pool (the original v1: 8 [re:70951b]; at
    /// most [`crate::voices::TIMESTRETCH_VOICES`]).
    pub timestretch_voices: usize,
}

/// The reference behaviour.
pub const REFERENCE: Rules = Rules {
    sampler_auto_stop_seconds: 0.25,
    envelope_v1_ends: false,
    loop_v1: false,
    lanes_v1: false,
    pan_v1: false,
    reverse_v1: false,
    end_fade_v1: false,
    sends_v1: false,
    timestretch_v1: false,
    midi_export_v1: false,
    midi_map_v1: false,
    slice_pads: 64,
    scene_snapshot_v1: false,
    sampler_modes_v1: false,
    sequencer_window_v1: false,
    timestretch_voices: crate::voices::TIMESTRETCH_VOICES,
};

static RULES: OnceLock<Rules> = OnceLock::new();

/// Install the app's rules (the first call wins; later calls return `false`).
pub fn install(r: Rules) -> bool {
    RULES.set(r).is_ok()
}

/// The installed rules, or [`REFERENCE`].
pub fn get() -> &'static Rules {
    RULES.get().unwrap_or(&REFERENCE)
}
