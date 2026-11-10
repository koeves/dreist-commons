//! The apps' side of the system, shared by Dreist and Einst: the
//! standalone's audio devices ([`audio`], cpal) and MIDI ports ([`midi`],
//! midir), native file dialogs (`rfd`, re-exported), the session file a
//! relaunched standalone starts from ([`session`]; serde and serde_json
//! re-exported) and local civil time ([`time`], the C library).
//!
//! This crate owns those third-party dependencies and their versions, so
//! the apps declare none of their own. Each group is behind a feature
//! (`audio`, `midi`, `dialog`, `session`, `clap-sys`); [`devices`] and
//! [`time`] are always there.

pub mod devices;
pub mod time;

#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "midi")]
pub mod midi;
#[cfg(feature = "session")]
pub mod session;

#[cfg(feature = "clap-sys")]
pub use clap_sys;
#[cfg(feature = "dialog")]
pub use rfd;
#[cfg(feature = "session")]
pub use serde;
#[cfg(feature = "session")]
pub use serde_json;
