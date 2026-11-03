//! Sampler voice DSP ported from the v2 2.1.2.13 engine. Each item cites
//! the original function; see the dreist-re voice notes for the surrounding logic.
//!
//! Arithmetic follows the original's f32/f64 split. Where the original uses an
//! SSE estimate refined by one Newton step (`rsqrtss`, `rcpps`) this port uses
//! the exact operation, so results can differ in the last bits.

pub mod analog;
pub mod comb;
pub mod convert;
pub mod envelope;
pub mod filter;
pub mod fx;
pub mod interp;
pub mod mix;
pub mod modulators;
pub mod ms20;
pub mod sh101;
pub mod sp1200;
pub mod svf;
pub mod voice;
pub mod voices;

pub use envelope::{AhrEnvelope, Envelope, EnvelopeParams};
pub mod slice;
pub mod stretch;
