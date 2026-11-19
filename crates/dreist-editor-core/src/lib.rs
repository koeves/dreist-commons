//! Editor parts Dreist and Einst share. Each app's editor crate re-exports
//! these modules under its own paths and keeps its panels, layouts and
//! menus.

pub mod command;
pub mod tabs;
pub mod transmodvis;
pub mod undo;
