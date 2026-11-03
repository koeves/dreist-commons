//! Validation harness pieces: the demo presets and the stress scenarios that
//! rewrite their layers and steps, and the golden check apps run with
//! their own profile installed ([`check_scenarios`]).

use std::path::{Path, PathBuf};

use crate::audio::SampleCache;
use crate::{Dreist, RenderOptions};
use dreist_formats::Node;
use dreist_testkit::Stats;

pub fn presets() -> Vec<PathBuf> {
    let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
    let Ok(rd) = std::fs::read_dir(PathBuf::from(content).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets")) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == dreist_formats::profile::LEGACY.project)).collect();
    v.sort();
    v
}

/// Deterministic values in 0..1.
pub struct Lcg(pub u32);

#[allow(clippy::should_implement_trait)]
impl Lcg {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }
}

pub fn walk(n: &mut Node, f: &mut dyn FnMut(&mut Node)) {
    f(n);
    for c in &mut n.children {
        walk(c, f);
    }
}

/// Every layer node with its (pad, layer) numbers.
pub fn layers(doc: &mut Node, mut f: impl FnMut(&mut Node, usize, usize)) {
    for e in doc.children.iter_mut().filter(|c| c.class == "Engine") {
        for p in e.children.iter_mut().filter(|c| c.class == "Pad") {
            let pi = p.index().unwrap_or(0) as usize;
            for l in p.children.iter_mut().filter(|c| c.class == "Layer") {
                let li = l.index().unwrap_or(0) as usize;
                f(l, pi, li);
            }
        }
    }
}

pub type Scenario = (&'static str, fn(&mut Node));

pub fn scenarios() -> Vec<Scenario> {
    vec![
        ("plain", |_| {}),
        ("filters", |doc| {
            layers(doc, |l, p, _| {
                l.set("FilterActive", 1.0)
                    .set("FilterAlgorithm", (p % 4) as f32 / 3.0)
                    .set("FilterType", ((p * 7) % 10) as f32 / 10.0)
                    .set("FilterResonance", 0.95)
                    .set("FilterCutoff", 0.25 + 0.1 * (p % 7) as f32)
                    .set("FilterDrive", 0.8);
            })
        }),
        ("loops", |doc| {
            layers(doc, |l, p, _| {
                l.set("PlaybackMode", if p % 3 == 0 { "Gated" } else { "Looping" })
                    .set("LoopInRelease", (p % 2) as f32)
                    .set("AltLoopMode", ((p / 2) % 2) as f32)
                    .set("XFadeLoopTime", 0.3)
                    .set("SampleLoopStartPoint", 0.2)
                    .set("SampleLoopEndPoint", 0.9);
            })
        }),
        ("pitch", |doc| {
            layers(doc, |l, p, _| {
                l.set("CoarseTune", if p % 2 == 0 { 0.0 } else { 1.0 })
                    .set("Octave", if p % 4 < 2 { 0.0 } else { 1.0 })
                    .set("FineTune", 0.9)
                    .set("TimestretchActive", (p % 2) as f32)
                    .set("TimestretchAmount", if p % 3 == 0 { 0.05 } else { 0.95 })
                    .set("TimestretchMode", ((p / 3) % 2) as f32);
            })
        }),
        ("envelopes", |doc| {
            let mut r = Lcg(42);
            layers(doc, |l, p, _| {
                for k in ["AmpEnvAttack", "AmpEnvDecay", "AmpEnvSustain", "AmpEnvHold", "AmpEnvRelease", "FreeEnvAttack", "FreeEnvDecay", "FreeEnvSustain", "FreeEnvHold", "FreeEnvRelease"] {
                    l.set(k, r.next());
                }
                for k in ["AmpEnvAttackCurve", "AmpEnvDecayCurve", "AmpEnvReleaseCurve", "FreeEnvAttackCurve", "FreeEnvDecayCurve", "FreeEnvReleaseCurve"] {
                    l.set(k, if r.next() < 0.5 { 0.02 } else { 0.98 });
                }
                if p % 2 == 1 {
                    l.set("PlaybackMode", "OneShotLengthBound");
                }
            })
        }),
        ("vintage", |doc| {
            // The vintage sampler path, forward and reversed, transposed.
            layers(doc, |l, p, _| {
                l.set("SynthesisMode", if p % 2 == 0 { "VintageSampler" } else { "Sampler" }).set("IsReversed", if p % 3 == 0 { "true" } else { "false" }).set("CoarseTune", [0.5, 0.0, 1.0][p % 3]);
            })
        }),
        ("regions", |doc| {
            // Tiny and inverted sample regions, start offsets near the end.
            layers(doc, |l, p, _| {
                let (s, e) = [(0.0, 0.0), (0.0, 0.002), (0.99, 1.0), (0.5, 0.1), (1.0, 1.0), (0.3, 0.7)][p % 6];
                l.set("SampleStartPoint", s).set("SampleEndPoint", e);
            })
        }),
        ("lanes", |doc| {
            let mut r = Lcg(7);
            walk(doc, &mut |n| {
                if n.class == "Step" && n.attr("State").is_some_and(|v| v.trim() == "true") {
                    for lane in dreist_formats::params::STEP_LANES {
                        n.set(lane, r.next());
                    }
                }
            })
        }),
    ]
}

/// Render `seconds` of `doc` at `sr` in host blocks of `block`.
pub fn render(doc: &Node, samples: &Path, sr: f32, block: usize, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let mut cache = SampleCache::default();
    let mut g = Dreist::from_document(doc, samples, sr, RenderOptions::default(), &mut cache).expect("document builds");
    crate::render(&mut g, (sr * seconds) as usize, block)
}

pub fn doc_for(preset: &Path, scenario: fn(&mut Node)) -> Node {
    let mut doc = dreist_formats::Project::load(preset).expect("preset loads").raw;
    scenario(&mut doc);
    doc
}

pub fn name(p: &Path) -> String {
    p.file_stem().unwrap().to_string_lossy().into_owned()
}

/// A render that is not finite or peaks past +40 dB is broken.
pub fn sane(tag: &str, l: &[f32], r: &[f32]) -> Option<String> {
    let s = Stats::of(l.iter().chain(r));
    if !s.finite {
        Some(format!("{tag}: non-finite output"))
    } else if s.peak > 100.0 {
        Some(format!("{tag}: peak {:.1}", s.peak))
    } else {
        None
    }
}

/// A built (stopped) engine for `doc`.
pub fn engine(doc: &Node, samples: &Path, sr: f32) -> Dreist {
    let mut cache = SampleCache::default();
    Dreist::from_document(doc, samples, sr, RenderOptions::default(), &mut cache).expect("document builds")
}

/// Render every demo preset in every scenario at 44.1 kHz: all renders
/// stay sane and match the golden set `set` (in the calling crate's
/// `tests/golden`; nothing to check without the factory content).
pub fn check_scenarios(set: &str) {
    let mut golden = dreist_testkit::Golden::open(set);
    let mut failures = Vec::new();
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        for (sname, f) in scenarios() {
            let doc = doc_for(&p, f);
            let tag = format!("{}-{sname}", name(&p));
            let (l, r) = render(&doc, &samples, 44_100.0, 512, 6.0);
            failures.extend(sane(&tag, &l, &r));
            golden.check(&tag, dreist_testkit::fnv(l.iter().chain(&r)), &l);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    golden.finish();
}
