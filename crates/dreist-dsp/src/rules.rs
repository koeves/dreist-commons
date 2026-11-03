//! Behaviour switches an app may set where its instrument behaves
//! differently from the reference (Dreist's): installed once at start-up
//! with [`install`]; until then (and in Dreist) [`get`] returns [`REFERENCE`].

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Seconds below the stop threshold after which an armed recording
    /// stops by itself.
    pub sampler_auto_stop_seconds: f32,
}

/// The reference behaviour.
pub const REFERENCE: Rules = Rules { sampler_auto_stop_seconds: 0.25 };

static RULES: OnceLock<Rules> = OnceLock::new();

/// Install the app's rules (the first call wins; later calls return `false`).
pub fn install(r: Rules) -> bool {
    RULES.set(r).is_ok()
}

/// The installed rules, or [`REFERENCE`].
pub fn get() -> &'static Rules {
    RULES.get().unwrap_or(&REFERENCE)
}
