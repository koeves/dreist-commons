//! Pad right-click menu ([re:e45225]):
//! cut / copy / paste (with the pad's track), clear pad / all pads / a
//! layer, lock pad / unlock all / lock track and pad, swap / copy / move
//! to another pad, clone up / down / centred, the layer sample paths, and
//! the colour label.
//!
//! The original posts synth messages (0x4a `e_lockPad`, 0x4b
//! `e_unlockAllPads`, 0xb7 `e_lockTrack`, 0xe1/0xe3/0xe4 deallocate, 0x54
//! `e_swapPads`, 0x56 `e_setPad`, 0x49 `e_setPadColour`, 0x99
//! `e_loadEngine` for clones). The port edits the document and rebuilds
//! the engine.

use std::path::PathBuf;

use dreist_formats::{LayerParam, Node};
use dreist_gui::app::App;

use crate::command::Command;
use crate::model::Model;

pub const LAYERS: usize = 8;
pub const PADS: usize = 64;

/// [re:c1ed56]: transparent, then 63
/// colours (8 × 8 grid in the original).
pub const COLOURS: [&str; 63] = [
    "#242424", "#585858", "#8a8a8a", "#b1b1b1", "#cfcfcf", "#e7e7e7", "#f5f5f5", "#0080ff", "#2576c6", "#3d71a4", "#4b6c8d", "#5a7188", "#475767", "#174f87", "#063563", "#9600ff", "#8018c9",
    "#daa6ff", "#7d33b0", "#7d4d9e", "#694a7f", "#4a2c5f", "#3f1959", "#ea0707", "#ee4141", "#ef9d9d", "#b13b3b", "#931616", "#823535", "#4e0c0c", "#451616", "#0edaec", "#55d1dc", "#88d7de",
    "#bcebef", "#7eb9be", "#39a3ad", "#0f95a1", "#08575e", "#12f06b", "#47d47f", "#81dea6", "#abe8c3", "#5aa176", "#328f57", "#12803e", "#06672d", "#f2df0f", "#e2d547", "#e2d971", "#e5e0a3",
    "#ccc03c", "#a09b5f", "#998e13", "#645d09", "#f48111", "#e0954c", "#eab683", "#bc8d5f", "#c9792b", "#bd630b", "#9f5813", "#79430e",
];

/// The colour menu's single grid item ([re:51f1e7]
/// `(colours, 8, 8, ..)`): transparent then [`COLOURS`];
/// cell i picks colour index i.
pub fn colour_grid_item() -> String {
    let mut v = vec![None];
    v.extend(COLOURS.iter().map(|c| u32::from_str_radix(&c[1..], 16).ok().map(|v| dreist_gui::Colour::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255))));
    dreist_gui::menu::colour_grid(&v, 8)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Clone {
    Up,
    Down,
    Centred,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadAction {
    Cut { track: bool },
    Copy { track: bool },
    Paste { track: bool },
    ClearPad,
    ClearAll,
    ClearLayerMenu,
    ClearLayer(usize),
    Lock,
    UnlockAll,
    LockTrackAndPad,
    Rename,
    SwapMenu,
    Swap(usize),
    CopyMenu,
    CopyTo(usize),
    MoveMenu,
    MoveTo(usize),
    CloneMenu,
    Clone(Clone),
    PathMenu,
    CopyPath(usize),
    ParentMenu,
    OpenParent(usize),
    RevealMenu,
    Reveal(usize),
    SaveMenu,
    SaveSample(usize),
    ColourMenu,
    Colour(usize),
    None,
}

/// What the editor does after an action.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub changed: bool,
    /// Open this folder in the browser.
    pub browse: Option<PathBuf>,
    /// Save a copy of this sample (Save layer sample...).
    /// Layer to save ("Save layer sample...").
    pub save: Option<usize>,
}

/// The editor clipboard (`Clipboard`: pad, track, pattern).
#[derive(Default)]
pub struct Clipboard {
    pub pad: Option<Node>,
    /// An engine ([re:fa3f32]).
    pub engine: Option<Node>,
    /// A pad bank's 16 pads ([re:453892]).
    pub bank: Option<Vec<Node>>,
    pub track: Option<Node>,
    pub pattern: Option<Node>,
    /// A layer ([re:ed58fc]).
    pub layer: Option<Node>,
}

#[derive(Default)]
pub struct PadMenu {
    /// The pad the open menu is for, its item actions and anchor.
    open: Option<(usize, Vec<PadAction>, Vec<usize>)>,
}

pub fn pad_locked(model: &Model, e: usize, p: usize) -> bool {
    model.pad_node(e, p).is_some_and(|n| n.locked())
}

pub(crate) fn track_node(model: &Model, e: usize, p: usize) -> Option<&Node> {
    let pat = model.current_pattern(e);
    model.pattern_node(e, pat)?.child("Track", &format!("Track{p}"))
}

pub(crate) fn track_mut(model: &mut Model, e: usize, p: usize) -> &mut Node {
    let pat = model.current_pattern(e);
    let sp = model.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{pat}"));
    sp.child_mut_or_insert("Track", &format!("Track{p}"))
}

pub(crate) fn set_lock(n: &mut Node, on: bool) {
    if on {
        n.set("LockState", "true");
    } else {
        n.attrs.retain(|(k, _)| k != "LockState");
    }
}

/// `e_deallocatePadSamples`: the pad's layers lose their samples.
pub(crate) fn clear_samples(pad: &mut Node) {
    for l in pad.children.iter_mut().filter(|c| c.class == "Layer") {
        clear_layer(l);
    }
}

fn clear_layer(l: &mut Node) {
    l.attrs.retain(|(k, _)| k != "AudioFileName");
    l.children.retain(|c| c.class != "AudioFileMarkers" && c.name.as_deref() != Some("LayerName"));
}

/// Put `node` at pad `p` (renamed), keeping the target's lock state.
pub(crate) fn put_pad(model: &mut Model, e: usize, p: usize, mut node: Node) {
    node.name = Some(format!("Pad{p}"));
    let eng = model.engine_mut(e);
    match eng.children.iter_mut().find(|c| c.class == "Pad" && c.name.as_deref() == Some(&format!("Pad{p}"))) {
        Some(slot) => *slot = node,
        None => eng.children.push(node),
    }
}

/// "Save layer sample..." (pad menu lambda [re:319fbe]): the
/// layer's audio cropped to its start / end markers, written as a 24-bit
/// WAV with the file's channel count ([re:12499e]).
pub fn save_layer_sample(model: &Model, e: usize, p: usize, l: usize, dst: &std::path::Path) -> Result<(), String> {
    let src = model.layer_sample(e, p, l).ok_or("Could not save, because the audio file is now null")?;
    let mut cache = dreist_engine::audio::SampleCache::default();
    let data = cache.get(&src).ok_or("Could not save, because the audio file doesnt have any buffers")?;
    let frames = data.left.len() as u32;
    let m = crate::inspector::wave_markers(model, e, p, l, frames);
    let (a, b) = (m.start.min(frames) as usize, m.end.min(frames).max(m.start.min(frames)) as usize);
    let mut chans: Vec<&[f32]> = vec![&data.left[a..b]];
    if let Some(r) = &data.right {
        chans.push(&r[a..b.min(r.len())]);
    }
    dreist_engine::export::write_wav24_channels(dst, data.sample_rate as u32, &chans).map_err(|_| format!("Failed to write file {}", dst.display()))
}

pub(crate) fn pad_or_empty(model: &Model, e: usize, p: usize) -> Node {
    model.pad_node(e, p).cloned().unwrap_or_else(|| Node::new("Pad", Some(&format!("Pad{p}"))))
}

/// [re:8d96e9]: copy pad `p` to the pads above (Up), below
/// (Down) or both (Centred), each layer's coarse tune offset by the pad
/// distance in semitones; past ±12 the excess goes to the octave (counted
/// from 0, clamped to ±4) as in the original.
pub fn clone_pad(model: &mut Model, e: usize, p: usize, dir: Clone) {
    let src = pad_or_empty(model, e, p);
    let targets: Vec<usize> = match dir {
        Clone::Up => (p + 1..PADS).collect(),
        Clone::Down => (0..p).collect(),
        Clone::Centred => (0..PADS).filter(|&q| q != p).collect(),
    };
    for q in targets {
        let mut node = src.clone();
        let offset = q as f64 - p as f64;
        for l in node.children.iter_mut().filter(|c| c.class == "Layer") {
            let tune = l.f32("CoarseTune").unwrap_or(LayerParam::CoarseTune.default_value());
            let mut st = dreist_dsp::convert::semitones(tune, false) + offset;
            let mut oct = 0.0f64;
            let mut wrapped = false;
            while st > 12.0 || st < -12.0 {
                let up = st > 12.0;
                let next = oct + if up { 1.0 } else { -1.0 };
                wrapped = true;
                if next.abs() > 4.0 {
                    st = if up { 12.0 } else { -12.0 };
                    break;
                }
                oct = next;
                st += if up { -12.0 } else { 12.0 };
            }
            if wrapped {
                l.set("Octave", ((oct + 4.0) / 8.0) as f32);
            }
            l.set("CoarseTune", ((st + 12.0) / 24.0) as f32);
        }
        put_pad(model, e, q, node);
    }
}

/// Locked pads and patterns survive loading another project
/// ([re:69fc26] skips locked pads; the pattern
/// copy goes through [re:f12986]). Sample paths of carried
/// pads are made absolute against the old project's samples folder.
pub fn carry_locked(old: &Node, old_samples: Option<&std::path::Path>, new: &mut Node) {
    let locked = |c: &Node| matches!(c.class.as_str(), "Pad" | "StepPattern") && c.locked();
    for eng in old.children.iter().filter(|c| c.class == "Engine") {
        let keep: Vec<Node> = eng.children.iter().filter(|c| locked(c)).cloned().collect();
        // Locked tracks of unlocked patterns (`e_lockTrack`).
        let tracks: Vec<(String, Node)> = eng
            .children
            .iter()
            .filter(|c| c.class == "StepPattern" && !locked(c))
            .flat_map(|sp| sp.children.iter().filter(|t| t.class == "Track" && t.locked()).map(move |t| (sp.name.clone().unwrap_or_default(), t.clone())))
            .collect();
        if keep.is_empty() && tracks.is_empty() {
            continue;
        }
        let Some(name) = eng.name.clone() else { continue };
        let target = new.child_mut_or_insert("Engine", &name);
        for (sp, t) in tracks {
            let sp = target.child_mut_or_insert("StepPattern", &sp);
            match sp.children.iter_mut().find(|c| c.class == "Track" && c.name == t.name) {
                Some(slot) => *slot = t,
                None => sp.children.push(t),
            }
        }
        for mut k in keep {
            if let Some(dir) = old_samples {
                crate::browser::absolutise(&mut k, dir);
            }
            match target.children.iter_mut().find(|c| c.class == k.class && c.name == k.name) {
                Some(slot) => *slot = k,
                None => target.children.push(k),
            }
        }
    }
}

impl PadMenu {
    /// The open menu's anchor.
    pub fn anchor(&self) -> Option<&[usize]> {
        self.open.as_ref().map(|o| o.2.as_slice())
    }

    fn show(&mut self, app: &mut App, anchor: &[usize], pad: usize, items: Vec<(String, PadAction, bool)>) {
        let labels = items.iter().map(|(s, _, on)| if *on || s.is_empty() { s.clone() } else { format!("({s})") }).collect();
        let actions = items.into_iter().map(|(_, a, on)| if on { a } else { PadAction::None }).collect();
        self.open = Some((pad, actions, anchor.to_vec()));
        app.open_menu(anchor, labels, None);
    }

    /// `ConstructPadMenu` for pad `p`. With other pads selected too the
    /// original drops cut/copy/paste, the layer, rename, move and sample
    /// items, and "Clear pad" stays disabled (its enable flag is only set by
    /// the layer loop it skips).
    pub fn open(&mut self, app: &mut App, anchor: &[usize], model: &Model, clip: &Clipboard, p: usize) {
        let e = model.engine;
        let locked = pad_locked(model, e, p);
        let track_locked = track_node(model, e, p).is_some_and(|n| n.locked());
        let has_samples = (0..LAYERS).any(|l| model.layer(e, p, l).valid);
        let sep = || (String::new(), PadAction::None, true);
        if crate::multipad::selection(model, e).1 & !(1u64 << p.min(63)) != 0 {
            let items = vec![
                ("Clear pad".into(), PadAction::ClearPad, false),
                ("Clear all pads".into(), PadAction::ClearAll, true),
                sep(),
                (format!("{}Lock pad", if locked { "✓ " } else { "" }), PadAction::Lock, true),
                ("Unlock all pads".into(), PadAction::UnlockAll, true),
                (if locked || track_locked { "Unlock track and pad" } else { "Lock track and pad" }.into(), PadAction::LockTrackAndPad, true),
                sep(),
                ("Set colour label... ▸".into(), PadAction::ColourMenu, true),
            ];
            self.show(app, anchor, p, items);
            return;
        }
        let items = vec![
            ("Cut".into(), PadAction::Cut { track: false }, !locked),
            ("Cut with track".into(), PadAction::Cut { track: true }, !locked),
            ("Copy".into(), PadAction::Copy { track: false }, true),
            ("Copy with track".into(), PadAction::Copy { track: true }, true),
            ("Paste".into(), PadAction::Paste { track: false }, clip.pad.is_some() && !locked),
            ("Paste with track".into(), PadAction::Paste { track: true }, clip.pad.is_some() && clip.track.is_some() && !locked),
            sep(),
            ("Clear pad".into(), PadAction::ClearPad, !locked && has_samples),
            ("Clear all pads".into(), PadAction::ClearAll, true),
            ("Clear pad layer... ▸".into(), PadAction::ClearLayerMenu, !locked && has_samples),
            sep(),
            (format!("{}Lock pad", if locked { "✓ " } else { "" }), PadAction::Lock, true),
            ("Unlock all pads".into(), PadAction::UnlockAll, true),
            (if locked || track_locked { "Unlock track and pad" } else { "Lock track and pad" }.into(), PadAction::LockTrackAndPad, true),
            sep(),
            ("Rename pad".into(), PadAction::Rename, true),
            sep(),
            ("Swap pad to... ▸".into(), PadAction::SwapMenu, true),
            ("Copy pad to... ▸".into(), PadAction::CopyMenu, true),
            ("Move pad to... ▸".into(), PadAction::MoveMenu, true),
            ("Clone pad... ▸".into(), PadAction::CloneMenu, true),
            sep(),
            ("Copy sample path... ▸".into(), PadAction::PathMenu, has_samples),
            ("Open parent folder in browser... ▸".into(), PadAction::ParentMenu, has_samples),
            ("Reveal in Finder ▸".into(), PadAction::RevealMenu, has_samples),
            ("Save layer sample... ▸".into(), PadAction::SaveMenu, has_samples),
            sep(),
            ("Set colour label... ▸".into(), PadAction::ColourMenu, true),
        ];
        self.show(app, anchor, p, items);
    }

    /// A menu item was chosen; submenus open as successive menus.
    pub fn select(&mut self, app: &mut App, anchor: &[usize], model: &mut Model, clip: &mut Clipboard, index: usize, out: &mut Vec<Command>) -> Outcome {
        let Some((p, actions, _)) = self.open.take() else { return Outcome::default() };
        let a = actions.get(index).copied().unwrap_or(PadAction::None);
        let e = model.engine;
        let rebuild = |model: &Model, out: &mut Vec<Command>| out.push(model.rebuild_command());
        let layer_items = |model: &Model, f: &dyn Fn(usize) -> PadAction, need_unlocked: bool| -> Vec<(String, PadAction, bool)> {
            (0..LAYERS)
                .filter(|&l| model.layer(e, p, l).valid)
                .map(|l| {
                    let name = model
                        .layer_sample(e, p, l)
                        .or_else(|| model.layer_node(e, p, l).and_then(|n| n.attr("AudioFileName")).map(PathBuf::from))
                        .and_then(|f| f.file_name().map(|s| s.to_string_lossy().into_owned()))
                        .unwrap_or_default();
                    let locked = model.layer_node(e, p, l).is_some_and(|n| n.locked());
                    (format!("Layer {} ({name})", l + 1), f(l), !(need_unlocked && locked))
                })
                .collect()
        };
        let pad_items = |model: &Model, f: &dyn Fn(usize) -> PadAction, skip_locked: bool| -> Vec<(String, PadAction, bool)> {
            (0..PADS).map(|q| (format!("Pad {}", q + 1), f(q), q != p && !(skip_locked && pad_locked(model, e, q)))).collect()
        };
        let sample = |model: &Model, l: usize| model.layer_sample(e, p, l).or_else(|| model.layer_node(e, p, l).and_then(|n| n.attr("AudioFileName")).map(|f| PathBuf::from(f.replace('\\', "/"))));
        let done = Outcome { changed: true, browse: None, save: None };
        // Lock and colour go through the pad message handler, which applies them
        // to every selected pad.
        let targets: Vec<usize> = {
            let (_, mask) = crate::multipad::selection(model, e);
            if mask & (1u64 << p.min(63)) != 0 { (0..PADS).filter(|&q| mask & (1u64 << q) != 0).collect() } else { vec![p] }
        };
        match a {
            PadAction::None => Outcome::default(),
            PadAction::ClearLayerMenu => {
                let items = layer_items(model, &PadAction::ClearLayer, true);
                self.show(app, anchor, p, items);
                Outcome::default()
            }
            PadAction::PathMenu | PadAction::ParentMenu | PadAction::RevealMenu | PadAction::SaveMenu => {
                let f: &dyn Fn(usize) -> PadAction = match a {
                    PadAction::PathMenu => &PadAction::CopyPath,
                    PadAction::ParentMenu => &PadAction::OpenParent,
                    PadAction::SaveMenu => &PadAction::SaveSample,
                    _ => &PadAction::Reveal,
                };
                let items = layer_items(model, f, false);
                self.show(app, anchor, p, items);
                Outcome::default()
            }
            PadAction::SwapMenu | PadAction::CopyMenu | PadAction::MoveMenu => {
                let (f, skip): (&dyn Fn(usize) -> PadAction, bool) = match a {
                    PadAction::SwapMenu => (&PadAction::Swap, false),
                    PadAction::CopyMenu => (&PadAction::CopyTo, true),
                    _ => (&PadAction::MoveTo, true),
                };
                let items = pad_items(model, f, skip);
                self.show(app, anchor, p, items);
                Outcome::default()
            }
            PadAction::CloneMenu => {
                let items =
                    vec![("Upwards".into(), PadAction::Clone(Clone::Up), true), ("Downwards".into(), PadAction::Clone(Clone::Down), true), ("Centered".into(), PadAction::Clone(Clone::Centred), true)];
                self.show(app, anchor, p, items);
                Outcome::default()
            }
            PadAction::ColourMenu => {
                self.open = Some((p, (0..=COLOURS.len()).map(PadAction::Colour).collect(), anchor.to_vec()));
                app.open_menu(anchor, vec![colour_grid_item()], None);
                Outcome::default()
            }
            PadAction::Copy { track } | PadAction::Cut { track } => {
                clip.track = if track { Some(track_node(model, e, p).cloned().unwrap_or_else(|| Node::new("Track", Some(&format!("Track{p}"))))) } else { None };
                clip.pad = Some(pad_or_empty(model, e, p));
                if matches!(a, PadAction::Cut { .. }) {
                    clear_samples(model.pad_mut(e, p));
                    if track {
                        track_mut(model, e, p).children.retain(|c| c.class != "Step");
                    }
                    rebuild(model, out);
                    return done;
                }
                Outcome::default()
            }
            PadAction::Paste { track } => {
                let Some(pad) = clip.pad.clone() else { return Outcome::default() };
                put_pad(model, e, p, pad);
                if let (true, Some(mut t)) = (track, clip.track.clone()) {
                    t.name = Some(format!("Track{p}"));
                    *track_mut(model, e, p) = t;
                }
                rebuild(model, out);
                done
            }
            PadAction::ClearPad => {
                clear_samples(model.pad_mut(e, p));
                rebuild(model, out);
                done
            }
            PadAction::ClearAll => {
                for q in 0..PADS {
                    if model.pad_node(e, q).is_some() {
                        clear_samples(model.pad_mut(e, q));
                    }
                }
                rebuild(model, out);
                done
            }
            PadAction::ClearLayer(l) => {
                clear_layer(model.layer_mut(e, p, l));
                rebuild(model, out);
                done
            }
            // 0x4a `e_lockPad` [e, p] = !locked.
            PadAction::Lock => {
                let on = !pad_locked(model, e, p);
                for &q in &targets {
                    set_lock(model.pad_mut(e, q), on);
                }
                done
            }
            // 0x4b `e_unlockAllPads`.
            PadAction::UnlockAll => {
                for q in 0..PADS {
                    if model.pad_node(e, q).is_some() {
                        set_lock(model.pad_mut(e, q), false);
                    }
                }
                done
            }
            // 0x4a + 0xb7 `e_lockTrack`: both on unless either was locked.
            PadAction::LockTrackAndPad => {
                let on = !(pad_locked(model, e, p) || track_node(model, e, p).is_some_and(|n| n.locked()));
                for &q in &targets {
                    set_lock(model.pad_mut(e, q), on);
                    set_lock(track_mut(model, e, q), on);
                }
                done
            }
            // The pad inspector's name box (the original edits the pad label).
            PadAction::Rename => {
                model.pad[e] = p;
                if let Some(b) = app.root.path_of("PadName") {
                    app.begin_edit(&b);
                }
                done
            }
            // 0x54 `e_swapPads` [e, p] = q.
            PadAction::Swap(q) => {
                let (a_node, b_node) = (pad_or_empty(model, e, p), pad_or_empty(model, e, q));
                put_pad(model, e, q, a_node);
                put_pad(model, e, p, b_node);
                rebuild(model, out);
                done
            }
            // 0x56 `e_setPad` [e, q] with a copy; Move also clears the source (0xe3).
            PadAction::CopyTo(q) | PadAction::MoveTo(q) => {
                let node = pad_or_empty(model, e, p);
                put_pad(model, e, q, node);
                if matches!(a, PadAction::MoveTo(_)) {
                    clear_samples(model.pad_mut(e, p));
                }
                rebuild(model, out);
                done
            }
            PadAction::Clone(dir) => {
                clone_pad(model, e, p, dir);
                rebuild(model, out);
                done
            }
            PadAction::CopyPath(l) => {
                if let Some(f) = sample(model, l) {
                    if let Ok(mut c) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(s) = c.stdin.as_mut() {
                            let _ = s.write_all(f.to_string_lossy().as_bytes());
                        }
                        let _ = c.wait();
                    }
                }
                Outcome::default()
            }
            PadAction::OpenParent(l) => Outcome { changed: true, browse: sample(model, l).and_then(|f| f.parent().map(PathBuf::from)), save: None },
            PadAction::SaveSample(l) => Outcome { changed: false, browse: None, save: sample(model, l).map(|_| l) },
            PadAction::Reveal(l) => {
                if let Some(f) = sample(model, l) {
                    let _ = std::process::Command::new("open").arg("-R").arg(f).spawn();
                }
                Outcome::default()
            }
            // 0x49 `e_setPadColour`; `PadColour` = "r, g, b, a" (none when transparent).
            PadAction::Colour(i) => {
                for &q in &targets {
                    let pad = model.pad_mut(e, q);
                    pad.attrs.retain(|(k, _)| k != "PadColour");
                    if let Some(c) = i.checked_sub(1).and_then(|k| COLOURS.get(k)) {
                        let v = u32::from_str_radix(&c[1..], 16).unwrap_or(0);
                        pad.set("PadColour", format!("{}, {}, {}, 255", v >> 16, (v >> 8) & 255, v & 255));
                    }
                }
                done
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let mut doc = Node::new(dreist_formats::legacy::ROOT_ELEMENT, None);
        let mut eng = Node::new("Engine", Some("Engine0"));
        let mut pad = Node::new("Pad", Some("Pad4"));
        let mut l = Node::new("Layer", Some("Layer0"));
        l.set("AudioFileName", "/x/kick.wav");
        l.set("CoarseTune", 0.5);
        pad.children.push(l);
        eng.children.push(pad);
        doc.children.push(eng);
        Model::new(doc)
    }

    #[test]
    fn clone_up_shifts_tune_and_wraps_into_octaves() {
        let mut m = model();
        clone_pad(&mut m, 0, 4, Clone::Up);
        let tune = |m: &Model, p| dreist_dsp::convert::semitones(m.layer_param(0, p, 0, LayerParam::CoarseTune), false);
        assert!((tune(&m, 5) - 1.0).abs() < 1e-4);
        assert!((tune(&m, 16) - 12.0).abs() < 1e-4);
        // 13 semitones up: octave 1, tune +1.
        assert!((tune(&m, 17) - 1.0).abs() < 1e-4);
        assert!((m.layer_param(0, 17, 0, LayerParam::Octave) - 5.0 / 8.0).abs() < 1e-6);
        assert!(m.pad_node(0, 3).is_none());
    }

    #[test]
    fn locked_parts_survive_a_new_project() {
        let mut old = model().doc;
        {
            let eng = old.child_mut_or_insert("Engine", "Engine0");
            let pad = eng.child_mut_or_insert("Pad", "Pad4");
            pad.set("LockState", "true");
            pad.children[0].set("AudioFileName", "Samples/kick.wav");
        }
        let mut new = Node::new(dreist_formats::legacy::ROOT_ELEMENT, None);
        let mut eng = Node::new("Engine", Some("Engine0"));
        eng.children.push(Node::new("Pad", Some("Pad4")));
        new.children.push(eng);
        carry_locked(&old, Some(std::path::Path::new("/old")), &mut new);
        let m = Model::new(new);
        assert!(pad_locked(&m, 0, 4));
        let want = std::path::Path::new("/old").join("Samples/kick.wav");
        assert_eq!(m.layer_node(0, 4, 0).and_then(|l| l.attr("AudioFileName")), Some(want.to_string_lossy().as_ref()));
    }

    #[test]
    fn clone_down_reaches_pad_zero() {
        let mut m = model();
        clone_pad(&mut m, 0, 4, Clone::Down);
        let tune = dreist_dsp::convert::semitones(m.layer_param(0, 0, 0, LayerParam::CoarseTune), false);
        assert!((tune + 4.0).abs() < 1e-4);
        assert!(m.pad_node(0, 5).is_none());
    }
}

/// [re:084d85] [re:89a9b0]: the pad bank buttons' right-click
/// menu items (16 pads of bank `b`).
pub const BANK_ITEMS: [&str; 10] = ["Cut", "Copy", "Paste", "", "Mute all", "Solo all", "", "Unmute all", "Unsolo all", "Clear all mute and solo"];

/// A bank menu pick: Cut ([re:835158]: copy, then clear the non-empty pads),
/// Copy ([re:7b5ea8]), Paste ([re:4a3521]: all 16 pads, 0x56), mute / solo all
/// and their clears (0x4c / 0x4d per pad). True when the document changed.
pub fn bank_action(model: &mut Model, clip: &mut Clipboard, b: usize, index: usize, out: &mut Vec<Command>) -> bool {
    use dreist_engine::binding::id;
    let e = model.engine;
    let pads: Vec<usize> = (b * 16..b * 16 + 16).collect();
    let rebuild = |model: &Model, out: &mut Vec<Command>| out.push(model.rebuild_command());
    let set = |model: &mut Model, out: &mut Vec<Command>, kind: u32, on: bool| {
        for &p in &pads {
            let c = crate::command::msg(kind, &[e as i32, p as i32], &[on as i32 as f32]);
            model.apply(&c);
            out.push(c);
        }
    };
    match index {
        0 | 1 => {
            clip.bank = Some(pads.iter().map(|&p| pad_or_empty(model, e, p)).collect());
            if index == 0 {
                for &p in &pads {
                    if model.pad_node(e, p).is_some() {
                        clear_samples(model.pad_mut(e, p));
                    }
                }
                rebuild(model, out);
            }
            index == 0
        }
        2 => {
            let Some(bank) = clip.bank.clone().filter(|v| v.len() == 16) else { return false };
            for (&p, n) in pads.iter().zip(bank) {
                put_pad(model, e, p, n);
            }
            rebuild(model, out);
            true
        }
        4 => {
            set(model, out, id::MUTE_PAD, true);
            true
        }
        5 => {
            set(model, out, id::SOLO_PAD, true);
            true
        }
        7 => {
            set(model, out, id::MUTE_PAD, false);
            true
        }
        8 => {
            set(model, out, id::SOLO_PAD, false);
            true
        }
        9 => {
            set(model, out, id::MUTE_PAD, false);
            set(model, out, id::SOLO_PAD, false);
            true
        }
        _ => false,
    }
}

/// [re:70c4bf] plus the inspector's
/// "Edit meta data" ([re:a49bd9]): the layer row's right-click items.
pub const LAYER_ITEMS: [&str; 13] = ["Cut", "Copy", "Paste", "", "Clear", "Reset", "Reset Envelopes", "", "Lock", "", "Send to slicer", "", "Edit meta data"];

/// Whether layer `l` (or its pad) is locked.
pub fn layer_locked(model: &Model, e: usize, p: usize, l: usize) -> bool {
    pad_locked(model, e, p) || model.layer_node(e, p, l).is_some_and(|n| n.locked())
}

/// The layer menu's items with their enable / check states.
pub fn layer_items(model: &Model, clip: &Clipboard, e: usize, p: usize, l: usize) -> Vec<String> {
    use dreist_gui::menu::{CHECK, item};
    let valid = model.layer(e, p, l).valid;
    let locked = layer_locked(model, e, p, l);
    let own_lock = model.layer_node(e, p, l).is_some_and(|n| n.locked());
    LAYER_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &t)| match i {
            0 => item(t, !locked && valid),
            1 | 10 => item(t, valid),
            2 => item(t, clip.layer.is_some() && !locked),
            4..=6 => item(t, !locked && valid),
            8 => item(if own_lock { format!("{CHECK}{t}") } else { t.to_string() }, valid || own_lock),
            12 => item(t, valid),
            _ => t.to_string(),
        })
        .collect()
}

/// What a layer menu pick asks of the editor.
#[derive(Default)]
pub struct LayerOutcome {
    pub changed: bool,
    /// "Send to slicer": the layer's sample.
    pub slicer: Option<PathBuf>,
    /// "Edit meta data": open the [re:823bc8].
    pub meta: bool,
}

/// [re:6c9ec4] on the edited layer (its region, no pitch
/// compensation).
fn reset_hold(model: &mut Model, e: usize, p: usize, l: usize) {
    let Some((frames, sr)) = model.layer_sample(e, p, l).and_then(|f| dreist_engine::audio::info(&f)) else { return };
    let m = crate::inspector::wave_markers(model, e, p, l, frames as u32);
    let n = model.layer_mut(e, p, l);
    let c = n.clone();
    let get = |q: LayerParam| c.f32(q.name()).unwrap_or(q.default_value()).clamp(0.0, 1.0);
    let bound = c.attr("PlaybackMode").map(dreist_dsp::voice::PlaybackMode::canonical) == Some("OneShotLengthBound");
    let h = dreist_engine::hold::reset_hold_time(&get, bound, (m.end - m.start.min(m.end)) as f64, frames as f64, sr, false);
    n.set("AmpEnvHold", h).set("FreeEnvHold", h);
}

/// A layer menu pick (indices into [`LAYER_ITEMS`]):
/// - Cut: copy, then `e_deallocateSample` (0xe1).
/// - Copy / Paste (onto layer `l`, keeping its lock).
/// - Clear (0xe2 `e_clearLayer`): sample and state.
/// - Reset (0x85 [re:0c2c1d]): everything but the
///   sample: parameters, depths, markers (back to the file), velocity
///   split, sends, mix and lock, then the hold.
/// - Reset Envelopes (0x86): the amp and free
///   envelope parameters to their defaults, then the hold time.
/// - Lock (0x73 `e_setLayerLock`).
/// - Send to slicer.
pub fn layer_action(model: &mut Model, clip: &mut Clipboard, e: usize, p: usize, l: usize, index: usize, out: &mut Vec<Command>) -> LayerOutcome {
    let mut o = LayerOutcome::default();
    let name = format!("Layer{l}");
    let keep = |k: &str| matches!(k, "AudioFileName");
    match index {
        0 | 1 => {
            let Some(n) = model.layer_node(e, p, l).cloned() else { return o };
            clip.layer = Some(n);
            if index == 0 {
                clear_layer(model.layer_mut(e, p, l));
                o.changed = true;
            }
        }
        2 => {
            let Some(mut n) = clip.layer.clone() else { return o };
            n.name = Some(name);
            let lock = model.layer_node(e, p, l).and_then(|c| c.attr("LockState")).map(str::to_string);
            n.attrs.retain(|(k, _)| k != "LockState");
            if let Some(v) = lock {
                n.set("LockState", v);
            }
            *model.layer_mut(e, p, l) = n;
            o.changed = true;
        }
        4 => {
            *model.layer_mut(e, p, l) = Node::new("Layer", Some(&name));
            o.changed = true;
        }
        5 => {
            let n = model.layer_mut(e, p, l);
            n.attrs.retain(|(k, _)| keep(k));
            n.children.retain(|c| c.class == "FixedString" && c.name.as_deref() == Some("LayerName"));
            reset_hold(model, e, p, l);
            o.changed = true;
        }
        6 => {
            let n = model.layer_mut(e, p, l);
            // The values only; their modulation depths (`_Slot<n>`) stay.
            n.attrs.retain(|(k, _)| !(LayerParam::from_name(k).is_some() && (k.starts_with("AmpEnv") || k.starts_with("FreeEnv"))));
            reset_hold(model, e, p, l);
            o.changed = true;
        }
        8 => {
            let on = !model.layer_node(e, p, l).is_some_and(|n| n.locked());
            set_lock(model.layer_mut(e, p, l), on);
            o.changed = true;
        }
        10 => o.slicer = model.layer_sample(e, p, l),
        12 => o.meta = true,
        _ => {}
    }
    if o.changed {
        out.push(model.rebuild_command());
    }
    o
}
