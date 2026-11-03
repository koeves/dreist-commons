//! Readers for v2 file formats. See the dreist-re formats notes.

pub mod file;
pub mod legacy;
pub mod model;
pub mod node;
pub mod params;
pub mod paths;
pub mod profile;

pub use model::{Engine, Kit, Layer, Markers, Pad, Project, Step, StepPattern, Track};
pub use node::{Modulated, Node};
pub use params::{Curve, EFFECT_TYPES, EngineParam, LayerParam, ModulatorType, PadParam, STEP_LANES, TransmodSource};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("unexpected structure: {0}")]
    Structure(String),
}
