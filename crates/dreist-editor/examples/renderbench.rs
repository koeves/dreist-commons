//! Time full editor frames: `renderbench [project.geist] [scale] [frames]`.

use dreist_editor::{Editor, Model};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let doc = args.get(1).filter(|s| !s.is_empty()).map(|p| dreist_formats::Project::load(std::path::Path::new(p)).expect("project").raw).unwrap_or_else(|| dreist_formats::Node::new("root", None));
    let scale: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    if std::env::var("PLAYBENCH").is_ok() {
        playback(args.get(1).map(String::as_str).unwrap_or(""), scale);
        return;
    }
    let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    // SYNCBENCH=1: only editor syncs (for profiling).
    if std::env::var("SYNCBENCH").is_ok() {
        let mut ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(doc)).expect("editor");
        let t = std::time::Instant::now();
        for _ in 0..n {
            ed.sync();
        }
        println!("sync {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
        return;
    }
    let mut ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(doc)).expect("editor");
    ed.sync();
    let _ = ed.render(scale);
    let t = std::time::Instant::now();
    for _ in 0..n {
        ed.app.dirty = true;
        let _ = ed.render(scale);
    }
    let per = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("render {per:.1} ms/frame at scale {scale}");
    let t = std::time::Instant::now();
    for _ in 0..n {
        let _ = ed.take_commands();
        ed.animate();
    }
    println!("frame logic {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = std::time::Instant::now();
    for _ in 0..n {
        ed.sync();
    }
    println!("sync {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    // Host automation moving a layer knob every frame (the plugin's
    // editor link: model update, sync, repaint of what changed).
    let _ = ed.render_frame(scale);
    let t = std::time::Instant::now();
    for i in 0..n {
        let e = ed.model.engine as i32;
        let m = dreist_engine::binding::Message::new(dreist_engine::binding::id::SET_LAYER_PARAMETER, &[e, 0, 0], &[0.0, (i % 50) as f32 / 50.0, 0.0]);
        ed.automation(&[m]);
        let _ = ed.render_frame(scale);
    }
    println!("automation frame {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    // Mouse moves across the window.
    let t = std::time::Instant::now();
    let mut evs = 0;
    for i in 0..400 {
        let (x, y) = ((i * 37 % 1400) as f32, (i * 53 % 850) as f32);
        for ev in ed.app.mouse_move(x, y) {
            ed.handle(&ev);
            evs += 1;
        }
    }
    println!("mouse move {:.3} ms ({evs} events)", t.elapsed().as_secs_f64() * 1000.0 / 400.0);
}

/// Playback frames as the plugin's editor link drives them: `PLAYBENCH=1`.
#[allow(dead_code)]
pub fn playback(path: &str, scale: f32) {
    let p = std::path::Path::new(path);
    let doc = dreist_formats::Project::load(p).expect("project").raw;
    let mut ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(doc.clone())).expect("editor");
    if let Some(t) = std::env::var("TAB").ok().and_then(|t| t.parse().ok()) {
        ed.model.editor_tab = t;
    }
    ed.sync();
    let mut cache = dreist_engine::audio::SampleCache::default();
    let mut g = dreist_engine::Dreist::from_document(&doc, &dreist_formats::model::samples_dir(p), 44100.0, dreist_engine::RenderOptions::default(), &mut cache).expect("engine");
    g.start();
    let (mut l, mut r) = (vec![0.0f32; 735], vec![0.0f32; 735]);
    let _ = ed.render_frame(scale);
    let (mut full, mut partial, mut idle) = (0, 0, 0);
    let t = std::time::Instant::now();
    let frames = 300;
    for _ in 0..frames {
        g.process(&mut l, &mut r);
        let steps: [Option<u32>; 8] = std::array::from_fn(|e| g.current_step(e));
        ed.set_playback(g.is_playing(), steps, g.is_playing().then(|| g.song_position()));
        ed.update_meters(&g.meters);
        g.meters.clear();
        ed.animate();
        if ed.app.dirty {
            full += 1;
        } else if ed.app.needs_redraw() {
            partial += 1;
        } else {
            idle += 1;
        }
        let _ = ed.render_frame(scale);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("playback: {ms:.1} ms/frame (engine included); {full} full, {partial} partial, {idle} idle of {frames}");
}
