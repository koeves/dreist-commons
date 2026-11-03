//! The screens golden and repaint checks under the reference profile.

use super::*;
use crate::screens::{click, editor};

#[test]
fn screens_match_fingerprints() {
    crate::screens::check_screens("screens");
}

/// Edits repaint only what they change (`App::invalidate_changes`): after
/// each, the incrementally repainted frame equals a full render.
#[test]
fn partial_repaints_match_full_renders() {
    let Some(mut ed) = editor() else { return };
    // Repainted regions match a full render up to anti-aliasing rounding
    // (paths are translated by the region's origin, a level or two, and
    // the odd edge pixel of a knob arc a little more); a missed repaint
    // leaves stale content, far off over many pixels.
    let check = |ed: &mut Editor, what: &str| {
        let partial = ed.render_frame(1.0).clone();
        let full = ed.render(1.0);
        let w = full.width() as usize;
        let diffs: Vec<(u8, usize)> =
            partial.data().chunks(4).zip(full.data().chunks(4)).enumerate().map(|(i, (a, b))| (a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0), i)).filter(|d| d.0 > 4).collect();
        let worst = diffs.iter().copied().max_by_key(|d| d.0).unwrap_or((0, 0));
        assert!(diffs.len() <= 4 && worst.0 <= 16, "{what}: stale pixels after a partial repaint ({} off, worst by {} at {}, {})", diffs.len(), worst.0, worst.1 % w, worst.1 / w);
    };
    let _ = ed.render_frame(1.0);
    check(&mut ed, "nothing changed");
    // Region repaints of an unchanged window.
    for r in [
        dreist_gui::Rect::new(0, 0, 1400, 850),
        dreist_gui::Rect::new(100, 200, 300, 150),
        dreist_gui::Rect::new(390, 300, 200, 100),
        dreist_gui::Rect::new(5, 600, 500, 200),
        dreist_gui::Rect::new(700, 150, 600, 300),
    ] {
        ed.app.invalidate_rect(r);
        check(&mut ed, &format!("repainting {r:?} unchanged"));
    }
    // A layer knob moved by host automation.
    let e = ed.model.engine;
    let cutoff = dreist_engine::binding::Message::new(dreist_engine::binding::id::SET_LAYER_PARAMETER, &[e as i32, 0, 0], &[0.0, 0.2, 0.0]);
    ed.automation(&[cutoff]);
    check(&mut ed, "automation");
    // A header toggle clicked.
    click(&mut ed, "AllPads", dreist_gui::app::MouseButton::Left);
    check(&mut ed, "click");
    // Recorded steps from the engine.
    let w =
        dreist_engine::StepWrite { engine: e, pattern: ed.model.current_pattern(e), track: 0, step: 3, data: dreist_engine::pattern::StepData { on: true, ..dreist_engine::pattern::StepData::off() } };
    ed.step_writes(&[w]);
    check(&mut ed, "step writes");
    // Another editor tab.
    ed.model.editor_tab = 5;
    ed.sync();
    check(&mut ed, "tab");
}
