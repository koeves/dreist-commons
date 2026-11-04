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
    /// position + 34), at the sample that reaches it, and lasts to the end
    /// marker; cut fades clamp to the end marker too.
    pub end_fade_v1: bool,
    /// The original v1's aux send summing: a send ramps from its last level
    /// and is skipped while that level is 0 (the target only takes effect
    /// in the next block).
    pub sends_v1: bool,
}

/// The reference behaviour.
pub const REFERENCE: Rules =
    Rules { sampler_auto_stop_seconds: 0.25, envelope_v1_ends: false, loop_v1: false, lanes_v1: false, pan_v1: false, reverse_v1: false, end_fade_v1: false, sends_v1: false };

static RULES: OnceLock<Rules> = OnceLock::new();

/// Install the app's rules (the first call wins; later calls return `false`).
pub fn install(r: Rules) -> bool {
    RULES.set(r).is_ok()
}

/// The installed rules, or [`REFERENCE`].
pub fn get() -> &'static Rules {
    RULES.get().unwrap_or(&REFERENCE)
}
