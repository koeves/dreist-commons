//! Sampler voice DSP ported from the v2 2.1.2.13 engine. Each item cites
//! the original function; see the dreist-re voice notes for the surrounding logic.
//!
//! Arithmetic follows the original's f32/f64 split. Where the original uses an
//! SSE estimate refined by one Newton step (`rsqrtss`, `rcpps`) this port uses
//! the exact operation, so results can differ in the last bits.

/// Build the lazily made lookup tables now, so the first block that needs
/// one does not build it on the audio thread (milliseconds: a dropout).
/// Engines call this when they are built.
pub fn warm_tables() {
    let _ = analog::analog();
    let _ = analog::sin_tab();
    let _ = analog::tri_tab();
    let _ = modulators::asin_tab();
    fx::nodes::amber::warm();
}

pub mod analog;
pub mod comb;
pub mod convert;
pub mod envelope;
pub mod filter;
pub mod fx;
pub mod interp;
pub mod mix;
pub mod model;
pub mod modulators;
pub mod ms20;
pub mod rules;
pub mod sh101;
pub mod sp1200;
pub mod svf;
pub mod voice;
pub mod voices;

pub use envelope::{AhrEnvelope, Envelope, EnvelopeParams};
pub mod slice;
pub mod stretch;
