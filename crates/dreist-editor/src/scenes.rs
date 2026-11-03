//! Scenes tab (`ScenePanel.skin`, [re:07e3d5]): 6 banks of
//! 16 [re:cfb303]s (`Scene.skin`) in the `ScenesPanel` matrix.
//!
//! - [re:4756be]: `SceneSelectedStyle` (selected),
//!   `SceneUsedStyle` (used) or `SceneButtonStyle`, with matching icon and
//!   name-label styles.
//! - [re:1b7c5f]: the select button shows the scene's MIDI
//!   key name ([re:8ad0c7], C-2 = scene 0).
//! - `OnSceneTrigger` [re:3a4349] posts `e_sceneTrigger` 0x10a [scene, on],
//!   `OnSceneSnapshot` [re:d39e7a] `e_sceneSnapshot` 0x10d, `OnSceneClear`
//!   [re:10dbd2] `e_sceneClear` 0x108.

use dreist_engine::binding::id;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::Model;
use crate::{EditorError, err};

const NOTES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// [re:8ad0c7]: note name and octave (C-2 = 0).
pub fn key_name(n: usize) -> String {
    format!("{}{}", NOTES[n % 12], n as i32 / 12 - 2)
}

pub struct Scenes {
    page: Vec<usize>,
    matrix: Option<Vec<usize>>,
    bank: usize,
    selected: usize,
    held: Option<usize>,
    /// Scene clipboard (the engine keeps its own copy, messages 0x111..0x113).
    clipboard: Option<dreist_formats::Node>,
    /// Scene the context menu was opened on.
    menu: Option<usize>,
    /// Activity label per scene: (looping cue, alpha) — [re:8a7ff8]
    ///  [re:eeec50] ("...", repeats) / `StartActivated
    /// [re:62afd2]` [re:9f501c] ("▶", once); `ATC_Tick` [re:cc15e0] lowers the alpha
    /// by 0.2 per tick.
    anim: Vec<Option<(bool, f32)>>,
    last_tick: Option<std::time::Instant>,
}

/// [re:62afd2] tick period: [re:cfb303] registers [re:5fc9f5]
/// (every tick of [re:6556de], whose event-loop timer runs at 1/30 s,
/// [re:6ae388]).
const ANIM_TICK: f32 = 1.0 / 30.0;
const MENU: [&str; 3] = ["Cut", "Copy", "Paste"];

fn find_path(root: &Ctrl, base: &[usize], name: &str) -> Option<Vec<usize>> {
    fn rec(c: &Ctrl, name: &str, path: &mut Vec<usize>) -> bool {
        for (i, ch) in c.children.iter().enumerate() {
            path.push(i);
            if ch.name == name || rec(ch, name, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut c = root;
    for &i in base {
        c = &c.children[i];
    }
    let mut p = base.to_vec();
    rec(c, name, &mut p).then_some(p)
}

impl Scenes {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize]) -> Result<Scenes, EditorError> {
        {
            let mut host = &mut *root;
            for &i in page {
                host = &mut host.children[i];
            }
            skin.mount(host, "ScenePanel.skin").map_err(err)?;
            if let Some(s) = host.find_mut("Selectors") {
                if s.children.is_empty() {
                    skin.mount(s, "PatternSelectors.skin").map_err(err)?;
                }
            }
            let rel = skin.find("Scene.skin").unwrap_or_else(|| "Scene/Scene.skin".into());
            let tpl = skin.load_skin(&rel).ok().and_then(|r| r.children.into_iter().next());
            if let (Some(m), Some(mut t)) = (host.find_mut("ScenesPanel"), tpl) {
                t.overload = "EditorScene".into();
                for c in t.children.iter_mut() {
                    if matches!(c.name.as_str(), "NameLabel" | "ActivityLabel") {
                        c.passive = true;
                    }
                }
                let (cols, rows) = (m.attr_i32("intNumberOfCols", 4), m.attr_i32("intNumberOfRows", 4));
                let sp = m.attr_i32("intIntraSpacing", 5);
                let top = m.attr_i32("intTopSpacing", 0);
                let w = (m.rect.w - (cols - 1) * sp) / cols;
                let h = (m.rect.h - top - (rows - 1) * sp) / rows;
                for i in 0..cols * rows {
                    let mut s = t.clone();
                    s.set_attr("cell", i.to_string());
                    s.set_rect(Rect::new((i % cols) * (w + sp), top + (i / cols) * (h + sp), w, h));
                    m.children.push(s);
                }
            }
        }
        let matrix = find_path(root, page, "ScenesPanel");
        if let Some(t) = find_path(root, page, "DetailsTable") {
            let mut c = &mut *root;
            for &i in &t {
                c = &mut c.children[i];
            }
            c.overload = "SceneDetailsTable".into();
            c.children.clear();
        }
        Ok(Scenes { page: page.to_vec(), matrix, bank: 0, selected: 0, held: None, clipboard: None, menu: None, anim: vec![None; 96], last_tick: None })
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        // The scene panel's pattern selectors (`ScenePanel.skin` Selectors,
        // a [re:6f2709]): same keys as the pattern panel's.
        if let Some(sp) = find_path(&app.root, &self.page, "Selectors") {
            crate::pattern::sync_selectors(app, model, &sp);
        }
        let scenes = dreist_engine::scene::SceneManager::from_node(model.doc.children.iter().find(|c| c.class == "SceneManager").unwrap_or(&dreist_formats::Node::new("SceneManager", None)));
        let _current = scenes.current;
        if let Some(bp) = find_path(&app.root, &self.page, "BankButtons") {
            if let Some(g) = app.ctrl_mut(&bp) {
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = i == self.bank;
                }
            }
        }
        let sel = self.selected;
        if let Some(mp) = &self.matrix {
            if let Some(m) = app.ctrl_mut(mp) {
                for c in &mut m.children {
                    let Some(cell) = c.attr("cell").and_then(|v| v.parse::<usize>().ok()) else { continue };
                    let n = self.bank * 16 + cell;
                    let sc = scenes.scenes.get(n).copied().unwrap_or_default();
                    let is_sel = n == sel;
                    c.set_attr("scene", n.to_string());
                    c.style = if is_sel {
                        "SceneSelectedStyle"
                    } else if sc.used {
                        "SceneUsedStyle"
                    } else {
                        "SceneButtonStyle"
                    }
                    .into();
                    for k in &mut c.children {
                        match k.name.as_str() {
                            "SelectButton" => k.text = key_name(n),
                            "NameLabel" => {
                                k.text = model.scene_name(n);
                                k.style = if is_sel { "SceneSelectedNameLabelStyle" } else { "SceneNameLabelStyle" }.into();
                            }
                            "ActivityLabel" => match self.anim.get(n).copied().flatten() {
                                Some((cue, a)) => {
                                    k.text = if cue { "..." } else { "\u{25b6}" }.into();
                                    k.set_attr("textAlpha", a.to_string());
                                }
                                None => k.text = String::new(),
                            },
                            "SnapshotButton" | "ClearButton" => k.style = if is_sel { "SceneSelectedIconButtonStyle" } else { "SceneIconButtonStyle" }.into(),
                            _ => {}
                        }
                    }
                }
            }
        }
        let selected = scenes.scenes.get(sel).copied().unwrap_or_default();
        if let Some(p) = find_path(&app.root, &self.page, "NameTextBox") {
            if let Some(c) = app.ctrl_mut(&p) {
                c.text = model.scene_name(sel);
            }
        }
        if let Some(p) = find_path(&app.root, &self.page, "StartModeButtonGroup") {
            if let Some(g) = app.ctrl_mut(&p) {
                let mode = selected.start as usize;
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = i == mode;
                }
            }
        }
    }

    /// `e_sceneChangeCued` / `Activated` / `ChangesCleared` from the engine.
    pub fn event(&mut self, ev: dreist_engine::scene::SceneEvent) {
        use dreist_engine::scene::SceneEvent;
        match ev {
            SceneEvent::Cued(n) => {
                if let Some(a) = self.anim.get_mut(n) {
                    *a = Some((true, 1.0));
                }
            }
            SceneEvent::Activated(n) => {
                if let Some(a) = self.anim.get_mut(n) {
                    *a = Some((false, 1.0));
                }
            }
            SceneEvent::Selected(n) => self.selected = n.min(95),
            SceneEvent::Cleared => self.anim.iter_mut().for_each(|a| *a = None),
        }
    }

    /// Advance the activity animations; returns whether anything changed.
    pub fn animate(&mut self) -> bool {
        let now = std::time::Instant::now();
        let dt = self.last_tick.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(0.0);
        if !self.anim.iter().any(Option::is_some) {
            self.last_tick = None;
            return false;
        }
        let ticks = (dt / ANIM_TICK).floor();
        if ticks < 1.0 {
            self.last_tick.get_or_insert(now);
            return false;
        }
        self.last_tick = Some(now);
        for a in &mut self.anim {
            if let Some((cue, alpha)) = a {
                if *alpha <= 0.0 {
                    if *cue {
                        *alpha = 1.0;
                    } else {
                        *a = None;
                    }
                } else {
                    *alpha = (*alpha - 0.2 * ticks).max(0.0);
                }
            }
        }
        true
    }

    /// [re:02f079] into the `DetailsTable`
    /// (columns Engine 40, Pattern 189, Power 50; rows 1..8): for a used
    /// scene "n: name" and On/Off per engine.
    pub fn draw(&self, model: &Model, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) -> bool {
        use dreist_gui::paint::RectF;
        use dreist_gui::style::{HAlign, VAlign};
        if c.overload != "SceneDetailsTable" {
            return false;
        }
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let (st, hs) = ("DefaultTableViewStyle", "DefaultTableViewHeaderStyle");
        p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour(st, "colFill", dreist_gui::Colour::TRANSPARENT));
        let row_h = (h / 9.0).floor().max(12.0);
        let cols = [(0.0, 40.0), (40.0, 189.0), (229.0, 50.0)];
        p.fill_rect(RectF::new(0.0, 0.0, w, row_h), s.colour(hs, "colFill", dreist_gui::Colour::rgba(200, 200, 200, 255)));
        let font = s.font(st);
        let hfont = s.font(hs);
        let htext = s.colour(hs, "colText", dreist_gui::Colour::BLACK);
        let text = s.colour(st, "colText", dreist_gui::Colour::BLACK);
        let alt = s.colour(st, "colAltFill", dreist_gui::Colour::TRANSPARENT);
        let div = s.colour(st, "colColDivider", s.colour(hs, "colAltBorder", dreist_gui::Colour::BLACK));
        for ((x, cw), name) in cols.iter().zip(["Engine", "Pattern", "Power"]) {
            p.text(RectF::new(x + 3.0, 0.0, cw - 6.0, row_h), name, htext, &hfont, HAlign::Left, VAlign::Center);
        }
        let scenes = dreist_engine::scene::SceneManager::from_node(model.doc.children.iter().find(|c| c.class == "SceneManager").unwrap_or(&dreist_formats::Node::new("SceneManager", None)));
        let sc = scenes.scenes.get(self.selected).cloned().unwrap_or_default();
        for e in 0..8 {
            let y = row_h * (e + 1) as f32;
            if e % 2 == 1 {
                p.fill_rect(RectF::new(0.0, y, w, row_h), alt);
            }
            let (pat, pow) = if sc.used {
                let se = sc.engines[e];
                (format!("{}: {}", se.pattern + 1, model.pattern_name(e, se.pattern)), if se.power { "On" } else { "Off" }.to_string())
            } else {
                (String::new(), String::new())
            };
            for ((x, cw), t) in cols.iter().zip([(e + 1).to_string(), pat, pow]) {
                p.text(RectF::new(x + 3.0, y, cw - 6.0, row_h), &t, text, &font, HAlign::Left, VAlign::Center);
            }
        }
        for (x, _) in cols.iter().skip(1) {
            p.line(x + 0.5, 0.0, x + 0.5, h, div, 1.0);
        }
        true
    }

    fn scene_of(app: &App, path: &[usize]) -> Option<usize> {
        (0..=path.len()).rev().find_map(|n| app.ctrl(&path[..n])?.attr("scene")?.parse().ok())
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        // [re:6f2709] key → `e_selectPattern` for the current engine.
        if let UiEvent::Clicked { path, name, .. } = ev {
            if let (Some(n), Some(sp)) = (name.strip_prefix("Key").and_then(|n| n.parse::<i32>().ok()), find_path(&app.root, &self.page, "Selectors")) {
                if path.starts_with(&sp) && (0..24).contains(&n) {
                    let e = model.engine;
                    out.push(crate::command::msg(dreist_engine::binding::id::SELECT_PATTERN, &[e as i32, -1, -1, n], &[0.0]));
                    crate::pattern::set_current_pattern(model, e, n as usize);
                    return true;
                }
            }
        }
        if let UiEvent::MenuSelected { index, .. } = ev {
            if let Some(n) = self.menu.take() {
                // [re:341343] → `OnCut/Copy/PasteScene
                // ...Clipboard` [re:1e957d] / [re:03355d] / [re:6aafb3].
                let node = |model: &Model| model.doc.children.iter().find(|c| c.class == "SceneManager").and_then(|m| m.child("Scene", &format!("Scene{n}"))).cloned();
                match index {
                    0 | 1 => {
                        self.clipboard = Some(node(model).unwrap_or_else(|| dreist_formats::Node::new("Scene", Some(&format!("Scene{n}")))));
                        out.push(msg(if *index == 0 { id::SCENE_CUT } else { id::SCENE_COPY }, &[], &[n as f32]));
                    }
                    _ => {
                        if let Some(mut c) = self.clipboard.clone() {
                            c.name = Some(format!("Scene{n}"));
                            if !model.doc.children.iter().any(|c| c.class == "SceneManager") {
                                model.doc.children.push(dreist_formats::Node::new("SceneManager", Some("Scenes")));
                            }
                            if let Some(mgr) = model.doc.children.iter_mut().find(|c| c.class == "SceneManager") {
                                match mgr.children.iter_mut().find(|k| k.class == "Scene" && k.name.as_deref() == Some(&format!("Scene{n}"))) {
                                    Some(k) => *k = c,
                                    None => mgr.children.push(c),
                                }
                            }
                            out.push(msg(id::SCENE_PASTE, &[], &[n as f32]));
                        }
                    }
                }
                return true;
            }
        }
        match ev {
            UiEvent::Pressed { path, name } if path.starts_with(&self.page) && matches!(name.as_str(), "TriggerNextSceneButton" | "TriggerPrevSceneButton") => {
                let k = if name == "TriggerNextSceneButton" { id::SCENE_TRIGGER_NEXT } else { id::SCENE_TRIGGER_PREV };
                out.push(msg(k, &[], &[1.0]));
                true
            }
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.page) && matches!(name.as_str(), "TriggerNextSceneButton" | "TriggerPrevSceneButton") => {
                let k = if name == "TriggerNextSceneButton" { id::SCENE_TRIGGER_NEXT } else { id::SCENE_TRIGGER_PREV };
                out.push(msg(k, &[], &[0.0]));
                if let Some(c) = app.ctrl_mut(path) {
                    c.on = false;
                }
                true
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::RightDown, .. } if overload == "EditorScene" => {
                let Some(n) = Self::scene_of(app, path) else { return false };
                let mut items: Vec<String> = MENU.iter().map(|s| s.to_string()).collect();
                if self.clipboard.is_none() {
                    items[2] = "(Paste)".into();
                }
                app.open_menu(path, items, None);
                self.menu = Some(n);
                true
            }
            UiEvent::Mouse { path, overload, kind, .. } if overload == "EditorScene" => {
                let Some(n) = Self::scene_of(app, path) else { return false };
                match kind {
                    MouseKind::Down => {
                        self.selected = n;
                        self.held = Some(n);
                        out.push(msg(id::SCENE_TRIGGER, &[], &[n as f32, 1.0]));
                        true
                    }
                    MouseKind::Up => {
                        if let Some(h) = self.held.take() {
                            out.push(msg(id::SCENE_TRIGGER, &[], &[h as f32, 0.0]));
                        }
                        false
                    }
                    _ => false,
                }
            }
            UiEvent::TextCommitted { path, name, text } if name == "NameTextBox" && path.starts_with(&self.page) => {
                model.set_scene_name(self.selected, text.trim());
                true
            }
            UiEvent::Clicked { path, name, .. } => {
                if !path.starts_with(&self.page) {
                    return false;
                }
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                if parent == "BankButtons" {
                    self.bank = *path.last().unwrap();
                    return true;
                }
                if parent == "StartModeButtonGroup" {
                    out.push(msg(id::SCENE_SET_START_MODE, &[], &[self.selected as f32, *path.last().unwrap() as f32]));
                    return true;
                }
                let scene = Self::scene_of(app, path);
                match name.as_str() {
                    "SnapshotButton" => {
                        let n = scene.unwrap_or(self.selected);
                        self.selected = n;
                        out.push(msg(id::SCENE_SNAPSHOT, &[], &[n as f32]));
                        true
                    }
                    "ClearButton" => {
                        let n = scene.unwrap_or(self.selected);
                        self.selected = n;
                        out.push(msg(id::SCENE_CLEAR, &[], &[n as f32]));
                        true
                    }
                    "SelectButton" => {
                        if let Some(n) = scene {
                            self.selected = n;
                        }
                        true
                    }
                    "ClearAllButton" => {
                        for n in 0..96 {
                            out.push(msg(id::SCENE_CLEAR, &[], &[n as f32]));
                        }
                        true
                    }
                    "SnapshotToNextButton" => {
                        let doc = model.doc.children.iter().find(|c| c.class == "SceneManager");
                        let scenes = doc.map(dreist_engine::scene::SceneManager::from_node).unwrap_or_default();
                        if let Some(n) = scenes.scenes.iter().position(|s| !s.used) {
                            self.selected = n;
                            out.push(msg(id::SCENE_SNAPSHOT, &[], &[n as f32]));
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_names() {
        assert_eq!(super::key_name(0), "C-2");
        assert_eq!(super::key_name(8), "G#-2");
        assert_eq!(super::key_name(60), "C3");
    }
}
