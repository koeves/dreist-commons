//! On-disk names of the legacy instrument that the apps read for
//! compatibility: where its content is installed, its marker files, its
//! document root element and the names its skins use. They must stay
//! byte-identical.

use std::path::PathBuf;

/// The legacy documents folder under the home folder (user content).
pub const DOCUMENTS_DIR: &str = "Documents/FXpansion/Geist2";

/// The installed factory content under the home folder.
pub const CONTENT_DIR: &str = "Documents/FXpansion/Geist2/Content";

/// The demo library inside the factory content.
pub const DEMO_FACTORY: &str = "Geist2 Demo Factory";

/// The lite edition's factory content under the home folder (REX loops).
pub const LITE_FACTORY: &str = "Documents/ableton/FXpansion/GeistLite/AudioData/GeistLite Factory";

/// A folder's "do not slice" marker file.
pub const NOSLICE_FILE: &str = "Geist.noslice";

/// The document root element of project and part files.
pub const ROOT_ELEMENT: &str = "Geist";

/// The stored type name of the Reverb effect.
pub const REVERB_TYPE: &str = "FXpansion Reverb";

/// Icon font families named by legacy skins.
pub const ICON_FAMILIES: [&str; 2] = ["FXpansion-Geist2-Icons", "FXpansion-Geist2-Browser"];

/// The installed factory content (`DREIST_CONTENT` overrides it).
pub fn content_dir() -> PathBuf {
    std::env::var_os("DREIST_CONTENT").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(CONTENT_DIR))
}

/// A file of the demo library (`rel` inside [`DEMO_FACTORY`]).
pub fn demo(rel: &str) -> PathBuf {
    content_dir().join(DEMO_FACTORY).join(rel)
}
