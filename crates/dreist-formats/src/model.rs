//! Typed view over v2 project (`.geist`) and kit (`.geistkit`) files.
//!
//! Parameter values are kept as the normalized `0..=1` floats stored on disk;
//! [`Layer::param`] and [`Pad::param`] fill in omitted defaults and
//! [`crate::params::Curve`] converts to display units. Each typed struct keeps
//! its source [`Node`] so nothing is lost.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::node::Node;
use crate::params::{LayerParam, PadParam, TransmodSource};

#[derive(Debug, Clone)]
pub struct Project {
    pub version: Option<u32>,
    pub current_engine: Option<u32>,
    pub tempo: Option<f32>,
    pub audio_files: Vec<String>,
    pub engines: Vec<Engine>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Kit {
    pub pads: Vec<Pad>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Engine {
    pub index: u32,
    pub label: Option<String>,
    pub pads: Vec<Pad>,
    pub patterns: Vec<StepPattern>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub index: u32,
    pub layers: Vec<Layer>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub index: u32,
    pub name: Option<String>,
    /// Path relative to the owning file's `.samples` folder, `/`-separated.
    pub audio_file: Option<String>,
    pub start_end: Option<Markers>,
    pub loop_markers: Option<Markers>,
    pub raw: Node,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markers {
    pub start: u64,
    pub end: u64,
    pub length: u64,
}

#[derive(Debug, Clone)]
pub struct StepPattern {
    pub index: u32,
    pub name: Option<String>,
    pub steps: Option<u32>,
    /// Step length as written, e.g. `1/16`.
    pub resolution: Option<String>,
    pub tracks: Vec<Track>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Track {
    /// Pad this track triggers (track N drives pad N).
    pub index: u32,
    pub length: Option<u32>,
    pub steps: Vec<Step>,
    pub raw: Node,
}

#[derive(Debug, Clone)]
pub struct Step {
    pub index: u32,
    pub on: bool,
    /// Normalized velocity; `None` = class default.
    pub velocity: Option<f32>,
    pub raw: Node,
}

impl Project {
    pub fn load(path: &Path) -> Result<Project, Error> {
        Self::from_node(Node::parse(&std::fs::read_to_string(path)?)?)
    }

    pub fn from_node(raw: Node) -> Result<Project, Error> {
        if raw.class != "root" {
            return Err(Error::Structure(format!("expected <root>, found <{}>", raw.class)));
        }
        let audio_files = raw.children_of("AudioFileManager").flat_map(|m| m.children.iter()).filter_map(|c| c.attr("string")).map(normalize_path).collect();
        let engines = raw.children_of("Engine").map(Engine::from_node).collect();
        let tempo = raw.children_of("TimeInfo").next().and_then(|t| t.f32("Tempo"));
        Ok(Project { version: raw.version, current_engine: raw.u32("CurrentEngine"), tempo, audio_files, engines, raw })
    }
}

impl Kit {
    pub fn load(path: &Path) -> Result<Kit, Error> {
        let raw = Node::parse(&std::fs::read_to_string(path)?)?;
        Ok(Kit { pads: raw.children_of("Pad").map(Pad::from_node).collect(), raw })
    }
}

impl Engine {
    fn from_node(n: &Node) -> Engine {
        Engine {
            index: n.index().unwrap_or(0),
            label: n.attr("Name").map(str::to_owned),
            pads: n.children_of("Pad").map(Pad::from_node).collect(),
            patterns: n.children_of("StepPattern").map(StepPattern::from_node).collect(),
            raw: n.clone(),
        }
    }
}

impl Pad {
    /// Normalized value, or the class default when the file omits it.
    pub fn param(&self, p: PadParam) -> f32 {
        self.raw.f32(p.name()).unwrap_or(p.default_value())
    }

    fn from_node(n: &Node) -> Pad {
        Pad { index: n.index().unwrap_or(0), layers: n.children_of("Layer").map(Layer::from_node).collect(), raw: n.clone() }
    }
}

impl Layer {
    /// Normalized value as [re:87acba] loads it: the class default when
    /// omitted, clamped to `0..=1`.
    pub fn param(&self, p: LayerParam) -> f32 {
        self.raw.f32(p.name()).or_else(|| self.raw.f32(p.enum_name())).unwrap_or(p.default_value()).clamp(0.0, 1.0)
    }

    /// Modulation depth of `src` on `p`, clamped to `-1..=1`; 0 when omitted.
    pub fn depth(&self, p: LayerParam, src: TransmodSource) -> f32 {
        let read = |name: &str| src.attr(name).and_then(|k| self.raw.f32(&k));
        read(p.name()).or_else(|| read(p.enum_name())).unwrap_or(0.0).clamp(-1.0, 1.0)
    }

    /// Every depth the layer stores, in one pass over its attributes:
    /// `set(param, source, depth)` with the value [`Layer::depth`] reads
    /// (the first attribute of each pair; unparsable reads as 0).
    pub fn depths(&self, mut set: impl FnMut(LayerParam, TransmodSource, f32)) {
        let mut seen = vec![0u64; LayerParam::ALL.len()];
        for (k, v) in &self.raw.attrs {
            let Some((p, s)) = k.split_once('_') else { continue };
            let (Some(p), Some(s)) = (LayerParam::from_name(p), TransmodSource::from_name(s)) else { continue };
            let bit = 1u64 << s as u32;
            if s == TransmodSource::ActualParameter || seen[p as usize] & bit != 0 {
                continue;
            }
            seen[p as usize] |= bit;
            set(p, s, v.parse().unwrap_or(0.0f32).clamp(-1.0, 1.0));
        }
    }

    fn from_node(n: &Node) -> Layer {
        Layer {
            index: n.index().unwrap_or(0),
            name: n.string("LayerName").map(str::to_owned),
            audio_file: n.attr("AudioFileName").map(normalize_path),
            start_end: n.child("AudioFileMarkers", "StartEndMarkers").and_then(Markers::from_node),
            loop_markers: n.child("AudioFileMarkers", "LoopMarkers").and_then(Markers::from_node),
            raw: n.clone(),
        }
    }
}

impl Markers {
    fn from_node(n: &Node) -> Option<Markers> {
        Some(Markers { start: n.attr("StartSample")?.parse().ok()?, end: n.attr("EndSample")?.parse().ok()?, length: n.attr("SampleLength")?.parse().ok()? })
    }
}

impl StepPattern {
    fn from_node(n: &Node) -> StepPattern {
        StepPattern {
            index: n.index().unwrap_or(0),
            name: n.string("PatternName").map(str::to_owned),
            steps: n.u32("PatternSteps"),
            resolution: n.attr("Resolution").map(str::to_owned),
            tracks: n.children_of("Track").map(Track::from_node).collect(),
            raw: n.clone(),
        }
    }
}

impl Track {
    fn from_node(n: &Node) -> Track {
        Track {
            index: n.index().unwrap_or(0),
            length: n.u32("TrackLength"),
            steps: n.children_of("Step").map(|s| Step { index: s.index().unwrap_or(0), on: s.bool("State").unwrap_or(false), velocity: s.f32("Velocity"), raw: s.clone() }).collect(),
            raw: n.clone(),
        }
    }
}

/// v2 writes Windows separators on every platform.
pub fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// Folder holding samples saved alongside `file`: `Foo.geist` -> `Foo.samples/`.
pub fn samples_dir(file: &Path) -> PathBuf {
    file.with_extension("samples")
}

/// A layer's `AudioFileName` as a path. Relative names resolve in the
/// file's samples folder (v2's own files); names that are not there
/// resolve next to the file itself ([re:b00098] Lite kits refer to their
/// library as `../../Hits/...` from the kit's folder).
pub fn resolve_sample(samples: &Path, name: &str) -> PathBuf {
    let rel = normalize_path(name);
    let p = Path::new(&rel);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let exists = |c: &Path| c.exists() || (c.extension().is_some_and(|e| e.eq_ignore_ascii_case("truncated")) && c.with_extension("").exists());
    let first = samples.join(p);
    if !exists(&first) {
        if let Some(alt) = samples.parent().map(|d| d.join(p)).filter(|a| exists(a)) {
            return alt;
        }
    }
    first
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn samples_folder_first_then_the_files_folder() {
        let d = std::env::temp_dir().join(format!("dreist-resolve-{}", std::process::id()));
        let kit = d.join("Presets/Kits/K.geistkit");
        std::fs::create_dir_all(d.join("Presets/Kits/K.samples/Kick")).unwrap();
        std::fs::create_dir_all(d.join("Hits/Kicks")).unwrap();
        std::fs::write(d.join("Presets/Kits/K.samples/Kick/a.wav"), b"").unwrap();
        std::fs::write(d.join("Hits/Kicks/b.wav"), b"").unwrap();
        let s = samples_dir(&kit);
        assert_eq!(resolve_sample(&s, "Kick\\a.wav"), s.join("Kick/a.wav"));
        assert_eq!(resolve_sample(&s, "../../Hits/Kicks/b.wav"), d.join("Presets/Kits/../../Hits/Kicks/b.wav"));
        // Missing everywhere: the samples folder path (for the missing files list).
        assert_eq!(resolve_sample(&s, "x.wav"), s.join("x.wav"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
