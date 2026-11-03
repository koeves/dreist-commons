//! Project saving: an unedited project writes back identically, and edits
//! made through messages are persisted.

use std::path::PathBuf;

use dreist_engine::binding::{Message, id};
use dreist_engine::{Dreist, RenderOptions};

fn hollow() -> Option<PathBuf> {
    let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
    let p = PathBuf::from(content).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
    p.exists().then_some(p)
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dreist-save-{}-{name}", std::process::id()))
}

#[test]
fn unedited_project_saves_identically() {
    let Some(p) = hollow() else { return };
    let g = Dreist::load(&p, 44100.0, RenderOptions::default()).unwrap();
    let out = tmp("same.geist");
    g.save(&out).unwrap();
    let a = std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n");
    let b = std::fs::read_to_string(&out).unwrap();
    std::fs::remove_file(&out).ok();
    assert!(a == b, "saved project differs from the original");
}

#[test]
fn message_edits_are_saved() {
    let Some(p) = hollow() else { return };
    let mut g = Dreist::load(&p, 44100.0, RenderOptions::default()).unwrap();
    // Layer param 0 of engine 0 / pad 0 / layer 0, base value and a transmod depth.
    g.apply_message(&Message::new(id::SET_LAYER_PARAMETER, &[0, 0, 0], &[0.0, 0.25, 0.0]));
    g.apply_message(&Message::new(id::SET_LAYER_PARAMETER, &[0, 0, 0], &[0.0, 0.75, 1.0]));
    g.apply_message(&Message::new(id::SET_TEMPO, &[], &[133.0]));
    g.apply_message(&Message::new(id::MUTE_ENGINE, &[1], &[1.0]));
    let out = tmp("edit.geist");
    g.save(&out).unwrap();
    let project = dreist_formats::Project::load(&out).unwrap();
    std::fs::remove_file(&out).ok();

    let param = dreist_formats::LayerParam::ALL[0].name();
    let depth = dreist_formats::TransmodSource::ALL[1].attr(param).unwrap();
    let layer = project.raw.child("Engine", "Engine0").and_then(|e| e.child("Pad", "Pad0")).and_then(|p| p.child("Layer", "Layer0")).unwrap();
    assert_eq!(layer.attr(param), Some("0.25"));
    assert_eq!(layer.attr(&depth), Some("0.75"));
    assert_eq!(project.raw.child("TimeInfo", "TimeInfo").and_then(|t| t.f32("Tempo")), Some(133.0));
    assert_eq!(project.raw.child("Engine", "Engine1").and_then(|e| e.bool("Mute")), Some(true));
}

#[test]
fn step_edits_reach_pattern_and_document() {
    let Some(p) = hollow() else { return };
    let mut g = Dreist::load(&p, 44100.0, RenderOptions::default()).unwrap();
    let pat = g.engine_pattern_state(0).unwrap().0 as i32;
    g.apply_message(&Message::new(id::SET_STEP_ON, &[0, -1, -1, pat, 5, 3], &[0.5, 1.0, 0.0]));
    g.apply_message(&Message::new(id::SET_PATTERN_STEP_COUNT, &[0, -1, -1, pat], &[24.0]));
    let out = tmp("steps.geist");
    g.save(&out).unwrap();
    let project = dreist_formats::Project::load(&out).unwrap();
    std::fs::remove_file(&out).ok();
    let sp = project.raw.child("Engine", "Engine0").and_then(|e| e.child("StepPattern", &format!("StepPattern{pat}"))).unwrap();
    assert_eq!(sp.u32("PatternSteps"), Some(24));
    let step = sp.child("Track", "Track5").and_then(|t| t.child("Step", "S3")).unwrap();
    assert_eq!(step.bool("State"), Some(true));
    assert_eq!(step.f32("Velocity"), Some(0.5));
}
