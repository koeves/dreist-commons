//! A voice parameter model an app may install in place of the reference
//! pitch and filter parameter computation (`Voice::update_parameters`):
//! once per block it sees the layer's parameter rows, the note's lanes,
//! the velocity and the block's envelope buffers, and returns the playback
//! step (optionally per sample), the semitones (for the timestretcher) and
//! the filter cutoff, resonance and drive. Without an installed model the
//! voice is unchanged.

use crate::voice::{LAYER_PARAMS, Lanes, SOURCES};
use std::sync::OnceLock;

pub struct ModelInput<'a> {
    /// The layer's parameter matrix: `[param][0]` the value, `[param][s]`
    /// the depth from transmod source `s`.
    pub layer: &'a [[f32; SOURCES]; LAYER_PARAMS],
    pub lanes: &'a Lanes,
    pub velocity: f32,
    /// The block's amp and free envelope output (empty at note start).
    pub amp_env: &'a [f32],
    pub free_env: &'a [f32],
    /// Pad, engine and master pitch in semitones, as the reference sums them.
    pub outer_semitones: f64,
    /// The note's semitone offset (root key).
    pub semitone_offset: f64,
    /// The step at 0 semitones (file rate / output rate).
    pub base: f64,
}

#[derive(Default)]
pub struct ModelOutput {
    /// The playback step for the block.
    pub step: f64,
    /// Per-sample steps for the block, overriding `step` when not empty.
    pub steps: Vec<f64>,
    /// The pitch in semitones (the timestretcher's transpose).
    pub semitones: f64,
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
}

pub trait VoiceModel: Send + Sync {
    /// Fill `out` and return `true`, or return `false` to leave this
    /// voice's block to the reference computation.
    fn update(&self, input: &ModelInput, out: &mut ModelOutput) -> bool;
}

static MODEL: OnceLock<&'static dyn VoiceModel> = OnceLock::new();

/// Install the app's voice model (once, at start-up; later calls return
/// `false`).
pub fn install(m: &'static dyn VoiceModel) -> bool {
    MODEL.set(m).is_ok()
}

/// The installed model, if any.
pub fn get() -> Option<&'static dyn VoiceModel> {
    MODEL.get().copied()
}
