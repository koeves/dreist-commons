//! Renders the installed demo projects (skipped when the content is absent;
//! override the location with `DREIST_CONTENT`).

use std::path::PathBuf;

use dreist_engine::{Dreist, RenderOptions, render};

fn presets() -> Vec<PathBuf> {
    let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
    let dir = PathBuf::from(content).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets");
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == dreist_formats::profile::LEGACY.project)).collect();
    v.sort();
    v
}

#[test]
fn demo_projects_render_audio() {
    for p in presets() {
        let mut g = Dreist::load(&p, 44100.0, RenderOptions::default()).unwrap();
        assert!(g.missing_samples.is_empty(), "{}: {:?}", p.display(), g.missing_samples);
        assert!(g.song_active());
        let (l, r) = render(&mut g, 44100 * 4, 512);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        let rms = (l.iter().map(|x| x * x).sum::<f32>() / l.len() as f32).sqrt();
        assert!(rms > 1e-3, "{}: silent (rms {rms})", p.display());
    }
}

#[test]
fn block_size_does_not_change_the_output() {
    let Some(p) = presets().into_iter().next() else { return };
    let opts = RenderOptions::default();
    let (a, _) = render(&mut Dreist::load(&p, 44100.0, opts).unwrap(), 44100 * 2, 512);
    let (b, _) = render(&mut Dreist::load(&p, 44100.0, opts).unwrap(), 44100 * 2, 64);
    // Song changes are cued per host block, and several effects update at
    // block rate (Reverb's delay LFO, [re:f2c879] control ramps), as in
    // the original: outputs differ slightly with the host block size but must
    // stay close overall.
    let err = a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>();
    let rel = (err / a.iter().map(|x| x * x).sum::<f32>()).sqrt();
    assert!(rel < 0.05, "relative difference {rel}");
}

#[test]
fn engines_routed_to_a_sub_output_leave_the_main_mix() {
    let Some(p) = presets().into_iter().next() else { return };
    let mut project = dreist_formats::Project::load(&p).unwrap();
    for e in &mut project.engines {
        e.raw.attrs.retain(|(k, _)| k != "OutputLocation");
        e.raw.attrs.push(("OutputLocation".into(), "Sub output 3".into()));
    }
    let samples = dreist_formats::model::samples_dir(&p);
    let opts = RenderOptions { aux_returns: false, ..RenderOptions::default() };
    let mut g = Dreist::from_project(&project, &samples, 44100.0, opts);
    let (l, _, subs) = dreist_engine::render_multi(&mut g, 44100 * 2, 512, 4);
    let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
    assert!(rms(&subs[2][0]) > 1e-3, "sub output 3 silent");
    assert_eq!(rms(&subs[0][0]), 0.0);
    // Only the master chain's own output (e.g. limiter noise floor) remains.
    assert!(rms(&l) < 1e-4, "main still carries engine audio: {}", rms(&l));
}

#[test]
fn scene_trigger_applies_engine_patterns() {
    let Some(p) = presets().into_iter().find(|p| p.ends_with("Hollow.geist")) else { return };
    let mut g = Dreist::load(&p, 44100.0, RenderOptions::default()).unwrap();
    let s0 = g.scenes().scenes[0];
    assert!(s0.used);
    g.trigger_scene(0, 100, true);
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    g.process(&mut l, &mut r);
    for (e, se) in s0.engines.iter().enumerate() {
        let (pat, power, _) = g.engine_pattern_state(e).unwrap();
        assert_eq!((pat, power), (se.pattern, se.power), "engine {e}");
    }
    assert_eq!(g.scenes().current, 0);
    assert!(g.scenes().pending().is_empty());
}

#[test]
fn metronome_clicks_on_beats() {
    let Some(p) = presets().into_iter().next() else { return };
    let opts = RenderOptions::default();
    let mut a = Dreist::load(&p, 44100.0, opts).unwrap();
    if a.metronome.bar_voice.sample.is_none() {
        return;
    }
    let mut b = Dreist::load(&p, 44100.0, opts).unwrap();
    b.set_metronome(true);
    let n = 44100 * 2;
    let (la, _) = render(&mut a, n, 512);
    let (lb, _) = render(&mut b, n, 512);
    let diff: Vec<f32> = la.iter().zip(&lb).map(|(x, y)| (y - x).abs()).collect();
    // First click on the downbeat at sample 0, the next one beat later.
    let spb = (44100.0 * 60.0 / a.tempo()) as usize;
    assert!(diff[..200].iter().any(|d| *d > 1e-3));
    assert!(diff[spb..spb + 300].iter().any(|d| *d > 1e-3));
}

#[test]
fn empty_instrument_is_silent() {
    let mut g = Dreist::empty(48000.0, RenderOptions::default());
    let (l, r) = render(&mut g, 4800, 256);
    assert!(l.iter().chain(&r).all(|x| x.abs() < 1e-6));
}
