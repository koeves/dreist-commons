//! Pattern selector and track right-click menus.
//!
//! - [re:6abcdc] 0x5c60: Rename pattern; Cut,
//!   Copy, Paste; Clear pattern (0xaf), Clear all patterns (0xb0); Lock /
//!   Unlock pattern (0xac `e_lockPattern`); Reset pattern velocity (0xae),
//!   Quantize pattern timing (0xad), Extract groove from pattern (editor
//!   message 0x136), Convert timing (Pattern Convertor panel); Swap with /
//!   Copy to / Move to ▸ patterns ([re:c23a58] /
//!   `CopyPattern` / `MovePattern`); Export as audio (current pattern).
//! - [re:ca3223] $_7 [re:3c8b75] (track name area):
//!   Cut / Cut with pad, Copy / Copy with pad, Paste / Paste with pad;
//!   Clear track, Clear all tracks, Clear pad, Select tracks' pad; Lock all
//!   tracks (0xb8), Unlock all tracks (0xb9), Lock / Unlock track (0xb7),
//!   the track-and-pad lock pair; Insert ▸ (insertable patterns); Set
//!   colour label ▸ (the pad colour).

use dreist_engine::binding::id;
use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};

use crate::command::{Command, msg};
use crate::model::Model;
use crate::padmenu::{COLOURS, Clipboard, clear_samples, pad_locked, pad_or_empty, put_pad, set_lock, track_mut, track_node};

const PATTERNS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    // Pattern selector.
    Rename,
    CutPattern,
    CopyPattern,
    PastePattern,
    ClearPattern,
    ClearAllPatterns,
    LockPattern,
    ResetVelocity,
    Quantize,
    ExtractGroove,
    ConvertTiming,
    ExportAudio,
    SwapMenu,
    CopyMenu,
    MoveMenu,
    SwapWith(usize),
    CopyTo(usize),
    MoveTo(usize),
    // Track.
    CutTrack { pad: bool },
    CopyTrack { pad: bool },
    PasteTrack { pad: bool },
    ClearTrack,
    ClearAllTracks,
    ClearPad,
    SelectPad,
    LockAllTracks(bool),
    LockTrack,
    LockTrackAndPad,
    ColourMenu,
    Colour(usize),
    InsertMenu,
    Insert(usize),
    None,
}

/// [re:8a256e]: the `.patterntemplate`
/// files of `<Resources>/Pattern Templates` as (name, steps on/off).
pub fn pattern_templates(resources: &std::path::Path) -> Vec<(String, Vec<bool>)> {
    let dir = resources.parent().unwrap_or(resources).join("Pattern Templates");
    let mut files: Vec<std::path::PathBuf> =
        std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("patterntemplate"))).collect()).unwrap_or_default();
    files.sort();
    files
        .iter()
        .filter_map(|f| {
            let text = std::fs::read_to_string(f).ok()?;
            let root = Node::parse(&text).ok()?;
            let n = if root.class == "InsertablePattern" { root } else { root.children.into_iter().find(|c| c.class == "InsertablePattern")? };
            let bits: Vec<bool> = (0..).map_while(|i| n.attr(&format!("Element{i}")).map(|v| v.trim() == "true")).collect();
            if bits.is_empty() {
                return None;
            }
            // `<VString name="PatternName">text</VString>` (text content).
            let name = text
                .split_once("name=\"PatternName\">")
                .and_then(|(_, r)| r.split_once('<'))
                .map(|(t, _)| t.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            Some((name, bits))
        })
        .collect()
}

#[derive(Default)]
pub struct PatMenu {
    /// Pattern or track index, actions, anchor.
    open: Option<(usize, Vec<Action>, Vec<usize>)>,
    /// "Extract groove from pattern" asked for this pattern (the editor
    /// runs the save dialog).
    pub groove: Option<usize>,
    /// "Convert timing" asked for this pattern (the editor opens the panel).
    pub convert: Option<usize>,
    /// Export as audio was chosen (the editor renders the pattern).
    pub export: bool,
    /// The pattern editor's step selection (track, step), set by the
    /// editor before events reach the menu.
    pub selection: std::collections::BTreeSet<(usize, usize)>,
    /// The selection was cut: the editor clears it in the pattern panel.
    pub selection_cut: bool,
}

fn pattern_locked(model: &Model, e: usize, p: usize) -> bool {
    model.pattern_node(e, p).is_some_and(|n| n.locked())
}

pub(crate) fn pattern_or_empty(model: &Model, e: usize, p: usize) -> Node {
    model.pattern_node(e, p).cloned().unwrap_or_else(|| Node::new("StepPattern", Some(&format!("StepPattern{p}"))))
}

pub(crate) fn put_pattern(model: &mut Model, e: usize, p: usize, mut node: Node) {
    node.name = Some(format!("StepPattern{p}"));
    let eng = model.engine_mut(e);
    match eng.children.iter_mut().find(|c| c.class == "StepPattern" && c.name.as_deref() == Some(&format!("StepPattern{p}"))) {
        Some(slot) => *slot = node,
        None => eng.children.push(node),
    }
}

/// Steps off, lanes and names kept (`StepPattern::Clear` keeps settings).
pub(crate) fn clear_pattern(n: &mut Node) {
    for t in n.children.iter_mut().filter(|c| c.class == "Track") {
        t.children.retain(|c| c.class != "Step");
    }
}

impl PatMenu {
    fn show(&mut self, app: &mut App, anchor: &[usize], index: usize, items: Vec<(String, Action, bool)>) {
        let labels = items.iter().map(|(s, _, on)| if *on || s.is_empty() { s.clone() } else { format!("({s})") }).collect();
        let actions = items.into_iter().map(|(_, a, on)| if on { a } else { Action::None }).collect();
        self.open = Some((index, actions, anchor.to_vec()));
        app.open_menu(anchor, labels, None);
    }

    fn pattern_menu(&mut self, app: &mut App, anchor: &[usize], model: &Model, clip: &Clipboard, p: usize) {
        let e = model.engine;
        let sep = || (String::new(), Action::None, true);
        let items = vec![
            ("Rename pattern".into(), Action::Rename, true),
            sep(),
            ("Cut".into(), Action::CutPattern, true),
            ("Copy".into(), Action::CopyPattern, true),
            ("Paste".into(), Action::PastePattern, clip.pattern.is_some()),
            sep(),
            ("Clear pattern".into(), Action::ClearPattern, true),
            ("Clear all patterns".into(), Action::ClearAllPatterns, true),
            sep(),
            (if pattern_locked(model, e, p) { "Unlock pattern" } else { "Lock pattern" }.into(), Action::LockPattern, true),
            sep(),
            ("Reset pattern velocity".into(), Action::ResetVelocity, true),
            ("Quantize pattern timing".into(), Action::Quantize, true),
            ("Extract groove from pattern".into(), Action::ExtractGroove, true),
            ("Convert timing".into(), Action::ConvertTiming, true),
            sep(),
            ("Swap with... ▸".into(), Action::SwapMenu, true),
            ("Copy to... ▸".into(), Action::CopyMenu, true),
            ("Move to... ▸".into(), Action::MoveMenu, true),
            sep(),
            ("Export as audio (stops playback)...".into(), Action::ExportAudio, true),
        ];
        self.show(app, anchor, p, items);
    }

    fn track_menu(&mut self, app: &mut App, anchor: &[usize], model: &Model, clip: &Clipboard, t: usize) {
        let e = model.engine;
        let tl = track_node(model, e, t).is_some_and(|n| n.locked());
        let pl = pad_locked(model, e, t);
        let has_samples = (0..crate::padmenu::LAYERS).any(|l| model.layer(e, t, l).valid);
        let pair = match (tl, pl) {
            (true, true) => "Unlock track and pad",
            (true, false) => "Unlock track and lock pad",
            (false, true) => "Lock track and unlock pad",
            (false, false) => "Lock track and pad",
        };
        let sep = || (String::new(), Action::None, true);
        let items = vec![
            ("Cut".into(), Action::CutTrack { pad: false }, !tl && !self.selection.is_empty()),
            ("Cut with pad".into(), Action::CutTrack { pad: true }, !tl && !pl),
            ("Copy".into(), Action::CopyTrack { pad: false }, true),
            ("Copy with pad".into(), Action::CopyTrack { pad: true }, !tl),
            ("Paste".into(), Action::PasteTrack { pad: false }, clip.track.is_some() && !tl),
            ("Paste with pad".into(), Action::PasteTrack { pad: true }, clip.track.is_some() && clip.pad.is_some() && !pl),
            sep(),
            ("Clear track".into(), Action::ClearTrack, !tl),
            ("Clear all tracks".into(), Action::ClearAllTracks, !tl),
            ("Clear pad".into(), Action::ClearPad, has_samples && !pl),
            ("Select tracks' pad".into(), Action::SelectPad, true),
            sep(),
            ("Lock all tracks".into(), Action::LockAllTracks(true), true),
            ("Unlock all tracks".into(), Action::LockAllTracks(false), true),
            (if tl { "Unlock track" } else { "Lock track" }.into(), Action::LockTrack, true),
            (pair.into(), Action::LockTrackAndPad, true),
            sep(),
            ("Insert ▸".into(), Action::InsertMenu, !tl),
            sep(),
            ("Set colour label... ▸".into(), Action::ColourMenu, true),
        ];
        self.show(app, anchor, t, items);
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, clip: &mut Clipboard, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        match ev {
            UiEvent::RightClicked { path, name } => {
                let Some(n) = name.strip_prefix("Key").and_then(|n| n.parse::<usize>().ok()).filter(|n| *n < PATTERNS) else { return false };
                if !app.root.path_of("Selectors").is_some_and(|s| path.starts_with(&s)) {
                    return false;
                }
                let path = path.clone();
                self.pattern_menu(app, &path, model, clip, n);
                true
            }
            UiEvent::Mouse { path, overload, kind, .. } if overload == "TrackSelectArea" => {
                let Some(t) = app.ctrl(&path[..path.len() - 1]).and_then(|r| r.attr("track")).and_then(|v| v.parse::<usize>().ok()) else { return false };
                let e = model.engine;
                match kind {
                    MouseKind::Down => {
                        model.pad[e] = t;
                        true
                    }
                    MouseKind::RightDown => {
                        let path = path.clone();
                        self.track_menu(app, &path, model, clip, t);
                        true
                    }
                    _ => false,
                }
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.open.as_ref().is_some_and(|o| o.2 == *anchor) => {
                let (n, actions, anchor) = self.open.take().unwrap();
                let a = actions.get(*index).copied().unwrap_or(Action::None);
                self.apply(app, &anchor, model, clip, n, a, out)
            }
            _ => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply(&mut self, app: &mut App, anchor: &[usize], model: &mut Model, clip: &mut Clipboard, n: usize, a: Action, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let rebuild = |model: &Model, out: &mut Vec<Command>| out.push(model.rebuild_command());
        let pattern_items = |model: &Model, f: &dyn Fn(usize) -> Action| -> Vec<(String, Action, bool)> {
            (0..PATTERNS)
                .map(|q| {
                    let name = model.pattern_name(e, q);
                    (if name.is_empty() { format!("Pattern {}", q + 1) } else { name }, f(q), q != n)
                })
                .collect()
        };
        match a {
            Action::None => false,
            Action::Rename => {
                out.push(msg(id::SELECT_PATTERN, &[e as i32, -1, -1, n as i32], &[0.0]));
                set_current(model, e, n);
                if let Some(p) = app.root.path_of("PatternNameBox") {
                    app.begin_edit(&p);
                }
                true
            }
            Action::CutPattern | Action::CopyPattern => {
                clip.pattern = Some(pattern_or_empty(model, e, n));
                if a == Action::CutPattern {
                    out.push(msg(id::CLEAR_PATTERN, &[e as i32, -1, -1, n as i32], &[0.0]));
                    return true;
                }
                false
            }
            Action::PastePattern => {
                let Some(p) = clip.pattern.clone() else { return false };
                put_pattern(model, e, n, p);
                rebuild(model, out);
                true
            }
            Action::ClearPattern => {
                out.push(msg(id::CLEAR_PATTERN, &[e as i32, -1, -1, n as i32], &[0.0]));
                true
            }
            Action::ClearAllPatterns => {
                out.push(msg(id::CLEAR_ALL_PATTERNS, &[e as i32], &[0.0]));
                true
            }
            Action::LockPattern => {
                let on = !pattern_locked(model, e, n);
                let sp = model.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{n}"));
                set_lock(sp, on);
                true
            }
            Action::ResetVelocity => {
                out.push(msg(id::QUANTIZE_PATTERN_VELOCITY, &[e as i32, -1, -1, n as i32, -1, -1], &[0.0]));
                true
            }
            Action::Quantize => {
                out.push(msg(id::QUANTIZE_PATTERN_TIMING, &[e as i32, -1, -1, n as i32, -1, -1], &[0.0]));
                true
            }
            // e_extractGrooveFromPattern (0x136) → `DoCreateGrooveForPattern`.
            Action::ExtractGroove => {
                self.groove = Some(n);
                false
            }
            Action::ConvertTiming => {
                self.convert = Some(n);
                true
            }
            Action::ExportAudio => {
                self.export = true;
                true
            }
            Action::SwapMenu | Action::CopyMenu | Action::MoveMenu => {
                let f: &dyn Fn(usize) -> Action = match a {
                    Action::SwapMenu => &Action::SwapWith,
                    Action::CopyMenu => &Action::CopyTo,
                    _ => &Action::MoveTo,
                };
                let items = pattern_items(model, f);
                self.show(app, anchor, n, items);
                false
            }
            Action::SwapWith(q) => {
                let (a_node, b_node) = (pattern_or_empty(model, e, n), pattern_or_empty(model, e, q));
                put_pattern(model, e, q, a_node);
                put_pattern(model, e, n, b_node);
                rebuild(model, out);
                true
            }
            Action::CopyTo(q) | Action::MoveTo(q) => {
                let node = pattern_or_empty(model, e, n);
                put_pattern(model, e, q, node);
                if matches!(a, Action::MoveTo(_)) {
                    let sp = model.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{n}"));
                    clear_pattern(sp);
                }
                rebuild(model, out);
                true
            }
            // $_7 lambda #1 [re:4436e5]: Cut copies the pattern
            // ([re:aa54c8]), keeps only the editor-selected
            // steps in the copy (`DeleteAllNonEditorSelectedSteps`) and posts
            // 0xb1 `e_clearSelectedStepsInPattern` [e, −1, −1, pattern].
            Action::CutTrack { pad: false } => {
                let cur = model.current_pattern(e);
                let mut copy = model.pattern_node(e, cur).cloned().unwrap_or_else(|| Node::new("StepPattern", Some(&format!("StepPattern{cur}"))));
                let index = |n: &Node, prefix: &str| n.name.as_deref().and_then(|x| x.strip_prefix(prefix)).and_then(|x| x.parse::<usize>().ok());
                for tr in copy.children.iter_mut().filter(|c| c.class == "Track") {
                    let Some(t) = index(tr, "Track") else { continue };
                    tr.children.retain(|st| st.class != "Step" || index(st, "S").is_some_and(|k| self.selection.contains(&(t, k))));
                }
                clip.pattern = Some(copy);
                let sp = model.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{cur}"));
                for tr in sp.children.iter_mut().filter(|c| c.class == "Track") {
                    let Some(t) = index(tr, "Track") else { continue };
                    tr.children.retain(|st| st.class != "Step" || !index(st, "S").is_some_and(|k| self.selection.contains(&(t, k))));
                }
                self.selection_cut = true;
                rebuild(model, out);
                true
            }
            Action::CopyTrack { pad } | Action::CutTrack { pad } => {
                clip.track = Some(track_node(model, e, n).cloned().unwrap_or_else(|| Node::new("Track", Some(&format!("Track{n}")))));
                clip.pad = pad.then(|| pad_or_empty(model, e, n));
                if matches!(a, Action::CutTrack { .. }) {
                    let cur = model.current_pattern(e) as i32;
                    out.push(msg(id::CLEAR_TRACK, &[e as i32, -1, -1, cur, n as i32, -1], &[0.0]));
                    if pad {
                        clear_samples(model.pad_mut(e, n));
                        rebuild(model, out);
                    }
                    return true;
                }
                false
            }
            Action::PasteTrack { pad } => {
                let Some(mut t) = clip.track.clone() else { return false };
                t.name = Some(format!("Track{n}"));
                *track_mut(model, e, n) = t;
                if let (true, Some(p)) = (pad, clip.pad.clone()) {
                    put_pad(model, e, n, p);
                }
                rebuild(model, out);
                true
            }
            Action::ClearTrack => {
                let cur = model.current_pattern(e) as i32;
                out.push(msg(id::CLEAR_TRACK, &[e as i32, -1, -1, cur, n as i32, -1], &[0.0]));
                true
            }
            Action::ClearAllTracks => {
                let cur = model.current_pattern(e) as i32;
                out.push(msg(id::CLEAR_PATTERN, &[e as i32, -1, -1, cur], &[0.0]));
                true
            }
            Action::ClearPad => {
                clear_samples(model.pad_mut(e, n));
                rebuild(model, out);
                true
            }
            Action::SelectPad => {
                model.pad[e] = n;
                true
            }
            // 0xb8 `e_lockAllTracks` / 0xb9 `e_unlockAllTracks`.
            Action::LockAllTracks(on) => {
                for t in 0..crate::padmenu::PADS {
                    if on || track_node(model, e, t).is_some() {
                        set_lock(track_mut(model, e, t), on);
                    }
                }
                true
            }
            Action::LockTrack => {
                let on = !track_node(model, e, n).is_some_and(|n| n.locked());
                set_lock(track_mut(model, e, n), on);
                true
            }
            // Each flips: the label names the resulting states.
            Action::LockTrackAndPad => {
                let tl = track_node(model, e, n).is_some_and(|n| n.locked());
                let pl = pad_locked(model, e, n);
                set_lock(track_mut(model, e, n), !tl);
                set_lock(model.pad_mut(e, n), !pl);
                true
            }
            Action::InsertMenu => {
                let items = pattern_templates(&dreist_formats::paths::skin_dir()).into_iter().enumerate().map(|(i, (name, _))| (name, Action::Insert(i), true)).collect();
                self.show(app, anchor, n, items);
                false
            }
            // Lambda #15 [re:182e3a]: the template repeats over the track
            // length; 0xd2 step on (1, 1) / 0xd3 step off per step.
            Action::Insert(k) => {
                let Some((_, bits)) = pattern_templates(&dreist_formats::paths::skin_dir()).into_iter().nth(k) else { return false };
                let cur = model.current_pattern(e);
                let pat = model.pattern(e, cur);
                let len = pat.tracks.get(n).map(|t| t.length).unwrap_or(pat.steps).max(1) as usize;
                for st in 0..len {
                    let index = [e as i32, n as i32, -1, cur as i32, n as i32, st as i32];
                    if bits[st % bits.len()] {
                        out.push(msg(id::SET_STEP_ON, &index, &[1.0, 1.0, 0.0]));
                    } else {
                        out.push(msg(id::SET_STEP_OFF, &index, &[0.0]));
                    }
                }
                true
            }
            Action::ColourMenu => {
                self.open = Some((n, (0..=COLOURS.len()).map(Action::Colour).collect(), anchor.to_vec()));
                app.open_menu(anchor, vec![crate::padmenu::colour_grid_item()], None);
                false
            }
            Action::Colour(i) => {
                let pad = model.pad_mut(e, n);
                pad.attrs.retain(|(k, _)| k != "PadColour");
                if let Some(c) = i.checked_sub(1).and_then(|k| COLOURS.get(k)) {
                    let v = u32::from_str_radix(&c[1..], 16).unwrap_or(0);
                    pad.set("PadColour", format!("{}, {}, {}, 255", v >> 16, (v >> 8) & 255, v & 255));
                }
                true
            }
        }
    }
}

fn set_current(model: &mut Model, e: usize, p: usize) {
    let st = model.engine_mut(e).child_mut_or_insert("EngineState", "EngineState");
    st.set("CurrentPattern", p);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_clears_the_source_pattern() {
        let mut doc = Node::new(dreist_formats::legacy::ROOT_ELEMENT, None);
        let mut eng = Node::new("Engine", Some("Engine0"));
        let mut sp = Node::new("StepPattern", Some("StepPattern0"));
        let mut t = Node::new("Track", Some("Track0"));
        let mut st = Node::new("Step", Some("S0"));
        st.set("State", "true");
        t.children.push(st);
        sp.children.push(t);
        eng.children.push(sp);
        doc.children.push(eng);
        let mut m = Model::new(doc);
        let node = pattern_or_empty(&m, 0, 0);
        put_pattern(&mut m, 0, 3, node);
        clear_pattern(m.engine_mut(0).child_mut_or_insert("StepPattern", "StepPattern0"));
        assert!(m.pattern_used(0, 3));
        assert!(!m.pattern_used(0, 0));
    }

    #[test]
    fn factory_pattern_templates() {
        let res = dreist_formats::paths::skin_dir();
        let t = pattern_templates(&res);
        let Some((name, bits)) = t.iter().find(|(n, _)| n.starts_with("E(3,10)")) else { return };
        assert!(name.contains('X'));
        assert_eq!(bits.len(), 10);
        assert_eq!(bits.iter().filter(|b| **b).count(), 3);
    }
}
