//! Editor parts Dreist and Einst share. Each app's editor crate re-exports
//! these modules under its own paths and keeps its panels, layouts and
//! menus.

pub mod command;
pub mod fxtext;
pub mod knob;
pub mod options;
pub mod sampling;
pub mod swing;
pub mod tabs;
pub mod timeline;
pub mod transmodvis;
pub mod uistate;
pub mod undo;
pub mod zoomslider;
