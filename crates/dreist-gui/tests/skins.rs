//! Every skin loads, lays out and renders.

use std::path::Path;

use dreist_gui::{Rect, Skin, text::Fonts};

fn skins(dir: &Path, out: &mut Vec<String>, base: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            skins(&p, out, base);
        } else if p.extension().is_some_and(|x| x == "skin") {
            out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
        }
    }
}

#[test]
fn all_skins_render() {
    let dir = Skin::resources_dir();
    let mut skin = Skin::load(&dir, dreist_gui::skin::default_theme()).expect("skin");
    let mut list = Vec::new();
    skins(&dir, &mut list, &dir);
    list.sort();
    assert_eq!(list.len(), 105, "{} skins", list.len());
    let mut fonts = Fonts::new(&skin.font_dir());
    for rel in &list {
        let mut root = skin.load_skin(rel).unwrap_or_else(|e| panic!("{rel}: {e}"));
        // Resizing exercises the anchoring.
        let r = root.rect;
        root.set_rect(Rect::new(r.x, r.y, r.w + 40, r.h + 20));
        let pm = dreist_gui::render_ctrl(&skin, &mut fonts, &root, 1.0);
        assert!(pm.width() > 0, "{rel}");
    }
}

#[test]
fn styles_resolve_through_parents() {
    let skin = Skin::load(&Skin::resources_dir(), dreist_gui::skin::default_theme()).expect("skin");
    let s = &skin.styles;
    // MuteButtonStyle → DefaultButtonStyle → CornerRoundingStyle (4).
    assert_eq!(s.float("MuteButtonStyle", "realCornerRounding", 0.0), 4.0);
    assert_eq!(s.colour("MuteButtonStyle", "colAltFill", dreist_gui::Colour::BLACK), s.colours["MuteSpark"]);
    assert_eq!(s.font("DefaultButtonStyle").size, 12.0);
    // PanelDarkestAlpha075 = Dreist's ink (#252A2C) at alpha 0.75.
    assert_eq!(s.colours["PanelDarkestAlpha075"], dreist_gui::Colour::rgba(0x25, 0x2a, 0x2c, 191));
}

/// `PathCache` finds a control like `Ctrl::path_of`, and again after the
/// tree changes under it.
#[test]
fn path_cache_follows_tree_changes() {
    let skin = Skin::load(&Skin::resources_dir(), dreist_gui::skin::default_theme()).expect("skin");
    let b = dreist_gui::Ctrl { name: "B".into(), ..Default::default() };
    let a = dreist_gui::Ctrl { name: "A".into(), children: vec![b], ..Default::default() };
    let root = dreist_gui::Ctrl { children: vec![a], ..Default::default() };
    let mut app = dreist_gui::app::App::new(skin, root);
    let cache = dreist_gui::app::PathCache::default();
    assert_eq!(cache.path_in(&app, &[], "B"), Some(vec![0, 0]));
    assert_eq!(cache.path_in(&app, &[0], "B"), Some(vec![0, 0]));
    assert_eq!(cache.path_in(&app, &[1], "B"), None);
    // Moved: a sibling inserted in front.
    app.root.children.insert(0, dreist_gui::Ctrl::default());
    assert_eq!(cache.path_in(&app, &[], "B"), Some(vec![1, 0]));
    cache.ctrl_mut(&mut app, "B").unwrap().name = "C".into();
    assert_eq!(cache.path_in(&app, &[], "B"), None);
}
