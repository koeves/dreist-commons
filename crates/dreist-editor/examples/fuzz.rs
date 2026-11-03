//! Randomized UI bughunt: `fuzz [project.geist] [seed] [steps]`.
//!
//! Drives the editor through the real mouse / keyboard paths (clicks,
//! right clicks, double clicks, drags, wheel, shortcuts, menu items) with
//! an engine attached: commands are applied, rebuilds rebuilt, audio
//! rendered. Reports panics (with the action that caused them), non-finite
//! audio and documents that no longer build.

use std::panic::{AssertUnwindSafe, catch_unwind};

use dreist_editor::{Command, Editor, Model};
use dreist_gui::Ctrl;
use dreist_gui::app::MouseButton;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn f(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

/// Visible, enabled controls with their absolute rects.
fn targets(c: &Ctrl, ox: i32, oy: i32, out: &mut Vec<(String, i32, i32, i32, i32)>) {
    if !c.visible {
        return;
    }
    let (x, y) = (ox + c.rect.x, oy + c.rect.y);
    if c.enabled && c.rect.w > 0 && c.rect.h > 0 && (!c.name.is_empty() || !c.overload.is_empty() || c.class != "CtrlPanel") {
        out.push((format!("{}:{}:{}", c.class, c.name, c.overload), x, y, c.rect.w, c.rect.h));
    }
    let mut kids: Vec<&Ctrl> = c.children.iter().collect();
    kids.sort_by_key(|k| k.zorder);
    for k in kids {
        targets(k, x, y, out);
    }
}

/// Random content files for drops.
fn drop_files(rng: &mut Rng) -> Vec<std::path::PathBuf> {
    let root = dreist_editor::options::documents_dir().join("Content");
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| ["wav", "aif", "geist", "geistkit", "mid"].contains(&x.to_string_lossy().to_lowercase().as_str())) {
                out.push(p);
            }
        }
    }
    thread_local!(static ALL: std::cell::RefCell<Vec<std::path::PathBuf>> = const { std::cell::RefCell::new(Vec::new()) });
    ALL.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_empty() {
            walk(&root, &mut a);
            a.sort();
        }
        let n = 1 + rng.below(3);
        (0..n).map(|_| a[rng.below(a.len())].clone()).collect()
    })
}

/// A throwaway settings folder whose documents folder holds an APFS clone
/// of the factory content, so menu actions (save default preset, delete
/// slice data, favourites, ...) never touch the user's files.
fn sandbox() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("dreist-fuzz-{}", std::process::id()));
    let docs = base.join("Documents");
    let _ = std::fs::create_dir_all(&docs);
    let real = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(dreist_formats::legacy::CONTENT_DIR);
    let ok = std::process::Command::new("cp").arg("-cR").arg(&real).arg(docs.join("Content")).status().is_ok_and(|s| s.success());
    assert!(ok, "content clone failed");
    std::fs::write(base.join("Options.txt"), format!("LocalDocumentsPath={}\n", docs.display())).unwrap();
    // SAFETY: single-threaded at this point.
    unsafe {
        std::env::set_var("DREIST_SETTINGS_DIR", &base);
        std::env::remove_var("DREIST_CONTENT");
    }
    base
}

/// First difference between two documents (attribute order ignored).
fn first_diff(a: &dreist_formats::Node, b: &dreist_formats::Node, path: &str) -> Option<String> {
    let here = format!("{path}/{}{}", a.class, a.name.as_deref().map(|n| format!("[{n}]")).unwrap_or_default());
    // Editor-side selection state.
    let keep = |x: &(String, String)| x.0 != "CurrentEngine";
    let mut aa: Vec<_> = a.attrs.iter().filter(|x| keep(x)).cloned().collect();
    let mut bb: Vec<_> = b.attrs.iter().filter(|x| keep(x)).cloned().collect();
    aa.sort();
    bb.sort();
    if a.class != b.class || aa != bb {
        let only_a: Vec<_> = aa.iter().filter(|x| !bb.contains(x)).collect();
        let only_b: Vec<_> = bb.iter().filter(|x| !aa.contains(x)).collect();
        return Some(format!("{here}: editor {only_a:?} engine {only_b:?}"));
    }
    if a.children.len() != b.children.len() {
        let ca: Vec<_> = a.children.iter().map(|c| c.class.as_str()).collect();
        let cb: Vec<_> = b.children.iter().map(|c| c.class.as_str()).collect();
        return Some(format!("{here}: children editor {ca:?} engine {cb:?}"));
    }
    a.children.iter().zip(&b.children).find_map(|(x, y)| first_diff(x, y, &here))
}

/// The document without engine pattern state (the editor shows a cued
/// pattern at once; the engine switches at the boundary).
fn comparable(n: &dreist_formats::Node) -> dreist_formats::Node {
    let mut n = n.clone();
    fn strip(n: &mut dreist_formats::Node) {
        n.children.retain(|c| c.class != "EngineState");
        n.children.iter_mut().for_each(strip);
    }
    strip(&mut n);
    n
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() {
    let sandbox = sandbox();
    let _cleanup = Cleanup(sandbox.clone());
    let args: Vec<String> = std::env::args().collect();
    // Projects from the factory content are opened from the clone.
    let real_content = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(dreist_formats::legacy::CONTENT_DIR);
    let path = args.get(1).cloned().unwrap_or_default();
    let path = match std::path::Path::new(&path).strip_prefix(&real_content) {
        Ok(rel) => sandbox.join("Documents/Content").join(rel).to_string_lossy().into_owned(),
        Err(_) => path,
    };
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let steps: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let load = || -> (dreist_formats::Node, Option<std::path::PathBuf>) {
        if path.is_empty() {
            (dreist_editor::project::new_document(), None)
        } else {
            let p = std::path::Path::new(&path);
            (dreist_formats::Project::load(p).expect("project").raw, Some(dreist_formats::model::samples_dir(p)))
        }
    };
    let (doc, samples) = load();
    let mut model = Model::new(doc.clone());
    model.samples = samples.clone();
    let mut ed = Editor::new(dreist_gui::skin::default_theme(), model).expect("editor");
    ed.sync();
    let sr = 44100.0;
    let mut cache = dreist_engine::audio::SampleCache::default();
    let mut engine = dreist_engine::Dreist::from_document(&doc, samples.as_deref().unwrap_or(std::path::Path::new(".")), sr, dreist_engine::RenderOptions::default(), &mut cache).expect("engine");
    engine.start();
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut log: Vec<String> = Vec::new();
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    let mut problems = 0;
    let (mut ncmd, mut nrebuild, mut nmenu) = (0usize, 0usize, 0usize);
    let mut tabs = std::collections::BTreeSet::new();
    for step in 0..steps {
        let mut list = Vec::new();
        targets(&ed.app.root, -ed.app.root.rect.x, -ed.app.root.rect.y, &mut list);
        let menu = ed.app.menu.clone();
        let res = catch_unwind(AssertUnwindSafe(|| {
            if let Some(m) = &menu {
                let usable: Vec<usize> = (0..m.items.len()).filter(|&i| m.item_at(m.x + 4.0, m.y + (i as f32 + 0.5) * 18.0) == Some(i)).collect();
                if !usable.is_empty() && rng.below(10) < 8 {
                    let i = usable[rng.below(usable.len())];
                    let (x, y) = (m.x + 4.0, m.y + (i as f32 + 0.5) * 18.0);
                    let action = format!("menu item {i} {:?} of {:?}", m.items[i], m.anchor);
                    let mut evs = ed.app.mouse_move(x, y);
                    evs.extend(ed.app.mouse_down(MouseButton::Left));
                    evs.extend(ed.app.mouse_up(MouseButton::Left));
                    for e in evs {
                        ed.handle(&e);
                    }
                    return action;
                }
            }
            let (name, x, y, w, h) = list[rng.below(list.len())].clone();
            let (px, py) = (x as f32 + rng.f() * w as f32, y as f32 + rng.f() * h as f32);
            let kind = rng.below(100);
            let mut evs = ed.app.mouse_move(px, py);
            let action = match kind {
                0..=49 => {
                    evs.extend(ed.app.mouse_down(MouseButton::Left));
                    evs.extend(ed.app.mouse_up(MouseButton::Left));
                    format!("click {name} at ({px:.0},{py:.0})")
                }
                50..=61 => {
                    evs.extend(ed.app.mouse_down(MouseButton::Right));
                    evs.extend(ed.app.mouse_up(MouseButton::Right));
                    format!("right click {name} at ({px:.0},{py:.0})")
                }
                62..=79 => {
                    let (dx, dy) = ((rng.f() - 0.5) * 300.0, (rng.f() - 0.5) * 300.0);
                    evs.extend(ed.app.mouse_down(MouseButton::Left));
                    for k in 1..=5 {
                        evs.extend(ed.app.mouse_move(px + dx * k as f32 / 5.0, py + dy * k as f32 / 5.0));
                    }
                    evs.extend(ed.app.mouse_up(MouseButton::Left));
                    format!("drag {name} from ({px:.0},{py:.0}) by ({dx:.0},{dy:.0})")
                }
                80..=85 => {
                    for _ in 0..2 {
                        evs.extend(ed.app.mouse_down(MouseButton::Left));
                        evs.extend(ed.app.mouse_up(MouseButton::Left));
                    }
                    format!("double click {name} at ({px:.0},{py:.0})")
                }
                91 => {
                    // A menu bar item (standalone).
                    let (app_items, menus) = ed.menu_bar_items();
                    fn tags(items: &[dreist_gui::menubar::Item], out: &mut Vec<(isize, String)>) {
                        for i in items {
                            if i.submenu.is_empty() {
                                if !i.title.is_empty() {
                                    out.push((i.tag, i.title.clone()));
                                }
                            } else {
                                tags(&i.submenu, out);
                            }
                        }
                    }
                    let mut all = Vec::new();
                    tags(&app_items, &mut all);
                    for (_, m) in &menus {
                        tags(m, &mut all);
                    }
                    // Not the panels that relaunch the app.
                    all.retain(|t| t.0 < 1001);
                    let (tag, title) = all[rng.below(all.len())].clone();
                    ed.menu_command(tag);
                    format!("menu bar {title:?}")
                }
                92..=94 => {
                    // A Finder drop of content files.
                    let files = drop_files(&mut rng);
                    let a = format!("drop {files:?} on {name}");
                    evs.extend(ed.app.files(px, py, files.clone(), false));
                    evs.extend(ed.app.files(px, py, files, true));
                    a
                }
                95..=96 => {
                    // Type into the text box under the pointer.
                    evs.extend(ed.app.mouse_down(MouseButton::Left));
                    evs.extend(ed.app.mouse_up(MouseButton::Left));
                    evs.extend(ed.app.mouse_down(MouseButton::Left));
                    evs.extend(ed.app.mouse_up(MouseButton::Left));
                    const TEXT: [&str; 8] = ["", "0", "-1", "9999", "abc", "1.5", "-inf", "120"];
                    let t = TEXT[rng.below(TEXT.len())];
                    let editing = ed.app.is_editing();
                    for ch in t.chars() {
                        ed.app.key_text(&ch.to_string());
                    }
                    let (more, _) = ed.app.key_named("Enter");
                    evs.extend(more);
                    format!("type {t:?} into {name} (editing {editing})")
                }
                86..=90 => {
                    let dy = (rng.f() - 0.5) * 6.0;
                    evs.extend(ed.app.mouse_wheel(dy));
                    format!("wheel {dy:.1} over {name}")
                }
                _ => {
                    const KEYS: [&str; 27] =
                        [" ", "r", "m", "k", "s", "0", "l", "1", "2", "3", "4", "5", "6", "7", "8", "9", "delete", "left", "right", "up", "down", "enter", "a", "c", "x", "v", "d"];
                    let k = KEYS[rng.below(KEYS.len())];
                    ed.app.mods.cmd = rng.below(4) == 0;
                    ed.app.mods.shift = rng.below(4) == 0;
                    evs.extend(ed.app.shortcut(k));
                    let a = format!("key {k:?} cmd={} shift={}", ed.app.mods.cmd, ed.app.mods.shift);
                    ed.app.mods = Default::default();
                    a
                }
            };
            for e in evs {
                ed.handle(&e);
            }
            // The window asks for a tooltip after every pointer event.
            let path = ed.app.tooltip_target();
            ed.handle(&dreist_gui::app::UiEvent::ToolTip { path });
            action
        }));
        let action = match res {
            Ok(a) => a,
            Err(e) => {
                let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                println!("PANIC step {step}: {msg}\n  last actions:");
                for a in log.iter().rev().take(6).rev() {
                    println!("    {a}");
                }
                return;
            }
        };
        if action.starts_with("menu item") {
            nmenu += 1;
        }
        tabs.insert((ed.model.editor_tab, ed.model.utility_tab));
        log.push(action.clone());
        // Engine side.
        let res = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            ed.animate();
            for c in ed.take_commands() {
                ncmd += 1;
                match c {
                    Command::Rebuild { doc, samples } => {
                        nrebuild += 1;
                        let s = samples.unwrap_or_else(|| std::path::PathBuf::from("."));
                        let mut g = dreist_engine::Dreist::from_document(&doc, &s, sr, dreist_engine::RenderOptions::default(), &mut cache).ok_or("rebuild: document did not build")?;
                        g.adopt(&mut engine);
                        engine = g;
                    }
                    mut c => c.apply(&mut engine),
                }
            }
            engine.process(&mut l, &mut r);
            if l.iter().chain(r.iter()).any(|v| !v.is_finite()) {
                return Err("non-finite audio".into());
            }
            if l.iter().chain(r.iter()).any(|v| v.abs() > 64.0) {
                return Err(format!("audio blowup {:.1}", l.iter().chain(r.iter()).fold(0.0f32, |a, v| a.max(v.abs()))));
            }
            if step % 25 == 0 {
                let _ = ed.render(1.0);
            }
            if std::env::var("FUZZ_DIVERGE").is_ok() {
                if let Some(d) = first_diff(&comparable(&ed.model.doc), &comparable(engine.document()), "") {
                    return Err(format!("editor/engine documents differ: {d}"));
                }
            }
            Ok(())
        }));
        let bad = match res {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(e) => Some(format!("engine panic: {}", e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())),
        };
        if let Some(b) = bad {
            problems += 1;
            println!("PROBLEM step {step}: {b}\n  last actions:");
            for a in log.iter().rev().take(6).rev() {
                println!("    {a}");
            }
            if problems > 3 {
                return;
            }
            // Fresh engine from the editor's document.
            let s = ed.model.samples.clone().unwrap_or_else(|| std::path::PathBuf::from("."));
            if let Some(g) = dreist_engine::Dreist::from_document(&ed.model.doc, &s, sr, dreist_engine::RenderOptions::default(), &mut cache) {
                engine = g;
            }
        }
    }
    println!("ok: {steps} steps, seed {seed}, {problems} problems; {ncmd} commands, {nrebuild} rebuilds, {nmenu} menu picks, tabs {tabs:?}");
}
