//! Pixel regression: fingerprints of the editor rendered in fixed states
//! (every editor and utility tab, a transmod source, the sampler pad mode,
//! the dialogs, a menu) so refactors are checked pixel for pixel
//! (`tests/golden/screens.txt`; `UPDATE_GOLDEN=1` rewrites it). Skipped
//! without the factory content. Apps run [`check_screens`] with their own
//! profile installed.

use crate::{Editor, Model};
use dreist_testkit::{Golden, fnv_bytes};

/// The demo project's editor (`None` without the factory content).
pub fn editor() -> Option<Editor> {
    let p = std::path::PathBuf::from(std::env::var("HOME").ok()?).join(dreist_formats::legacy::CONTENT_DIR).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
    let doc = dreist_formats::Project::load(&p).ok()?.raw;
    let mut model = Model::new(doc);
    model.samples = Some(dreist_formats::model::samples_dir(&p));
    model.name = Some("Hollow".into());
    let mut ed = Editor::new(dreist_gui::skin::default_theme(), model).ok()?;
    ed.sync();
    Some(ed)
}

/// Click the first control named `name` (a `/`-separated path of names).
pub fn click(ed: &mut Editor, spec: &str, button: dreist_gui::app::MouseButton) {
    let mut path = Vec::new();
    for part in spec.split('/') {
        let base = ed.app.ctrl(&path).expect("control").clone();
        path.extend(base.path_of(part).unwrap_or_else(|| panic!("no control {part}")));
    }
    let (x, y) = ed.app.origin(&path);
    let c = ed.app.ctrl(&path).unwrap();
    let (cx, cy) = (x as f32 + c.rect.w as f32 / 2.0, y as f32 + c.rect.h as f32 / 2.0);
    for ev in ed.app.mouse_move(cx, cy) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_down(button) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_up(button) {
        ed.handle(&ev);
    }
}

pub fn shot(ed: &mut Editor) -> u64 {
    fnv_bytes(ed.render(1.0).data())
}

/// Render the fixed states and check them against the golden set `name`
/// (in the calling crate's `tests/golden`; skipped without the content).
pub fn check_screens(name: &str) {
    let Some(mut ed) = editor() else { return };
    let mut golden = Golden::open(name);
    // Deterministic rendering first: the same state twice.
    let a = shot(&mut ed);
    assert_eq!(a, shot(&mut ed), "rendering is not deterministic");
    golden.check("main", a, &[]);
    for tab in 0..7 {
        let mut ed = editor().unwrap();
        ed.model.editor_tab = tab;
        ed.sync();
        golden.check(&format!("editor-tab-{tab}"), shot(&mut ed), &[]);
    }
    let mut ed = editor().unwrap();
    ed.model.utility_tab = 1;
    ed.sync();
    golden.check("transmod", shot(&mut ed), &[]);
    ed.model.source = 3;
    ed.sync();
    golden.check("transmod-source", shot(&mut ed), &[]);
    let mut ed = editor().unwrap();
    let e = ed.model.engine;
    ed.model.engine_mut(e).set("EnginePadMode", "Sampler");
    ed.sync();
    golden.check("sampler-pads", shot(&mut ed), &[]);
    for modal in ["about", "preferences", "export", "filefind", "metadata"] {
        let mut ed = editor().unwrap();
        ed.show_modal(modal);
        // Let the dialog slide in.
        std::thread::sleep(std::time::Duration::from_millis(600));
        ed.animate();
        golden.check(&format!("modal-{modal}"), shot(&mut ed), &[]);
    }
    let mut ed = editor().unwrap();
    click(&mut ed, "Header/OptionsButton", dreist_gui::app::MouseButton::Left);
    golden.check("options-menu", shot(&mut ed), &[]);
    golden.finish();
}
