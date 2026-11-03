//! Transmod: the source selector ([re:548e1b] in the root
//! `TransmodSelectPanel`, with the 4 macro knobs) and the modulator slots
//! panel ([re:413943]: 16 slots, `EmptySlot.skin` or the
//! modulator's own skin).
//!
//! Selecting a source (`Src1..28`, `Src0` = the parameters themselves)
//! switches every transmod knob to edit that source's depth.
//! Modulator controls `Control<n>` drive parameter n
//! (`ModulatorType::params`); changes post
//! `e_setModulationSourceParameter` 0x30 [slot] [_, param, source, value].
//!
//! Menus: an empty slot's TypePopup lists the 8 modulator types
//! ([re:88ebb5] lambda [re:ff6599], names via
//! [re:321f37] split from camel case); choosing one inserts it
//! ([re:9c3f18]: message 0x2b [slot, modulator], then
//! selects source slot + 5). A modulator's Options button
//! ([re:adeff9] lambda [re:0cb4ec]) offers Remove
//! modulator, the 8 types to replace it, and the check item "Keep trigger
//! data when replacing modulators" (option
//! `ReplacedModulatorsRetainTriggers`, default off). The port edits the
//! document and rebuilds.

use dreist_engine::binding::id;
use dreist_formats::{ModulatorType, Node, TransmodSource};
use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::{Model, bind_knob};
use crate::{EditorError, err};

pub const SLOTS: usize = 16;

fn skin_for(t: ModulatorType) -> &'static str {
    match t {
        ModulatorType::LFO => "LFO.skin",
        ModulatorType::MorphingLFO => "MorphingLFO.skin",
        ModulatorType::SampleAndHold => "SampleAndHold.skin",
        ModulatorType::ADEnvelope => "ADEnvelope.skin",
        ModulatorType::MiniSequencer => "Sequencer.skin",
        ModulatorType::Maths => "Math.skin",
        ModulatorType::Constant => "Constant.skin",
        ModulatorType::BouncingBall => "BouncingBall.skin",
    }
}

/// A macro's name (`Macro<k>Name`, default "Macro k+1").
pub fn macro_name(model: &Model, k: usize) -> String {
    model.doc.attr(&format!("Macro{k}Name")).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("Macro {}", k + 1))
}

/// [re:e70099].
pub fn short_name(t: ModulatorType) -> &'static str {
    match t {
        ModulatorType::LFO => "LFO",
        ModulatorType::MorphingLFO => "MLFO",
        ModulatorType::SampleAndHold => "S&H",
        ModulatorType::ADEnvelope => "AD Env",
        ModulatorType::MiniSequencer => "Seq",
        ModulatorType::Maths => "Maths",
        ModulatorType::Constant => "Constant",
        ModulatorType::BouncingBall => "Bounce",
    }
}

/// A modulator's name: `modName` (written only when it differs from the
/// short name, [re:07f1d8]), else the short name.
pub fn modulator_name(n: &Node) -> String {
    n.attr("modName").filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| n.attr("Type").and_then(ModulatorType::from_name).map(short_name).unwrap_or("").to_string())
}

/// The rename edit box.
const RENAME_EDIT: &str = "TransmodNameEdit";

fn modulator(model: &Model, slot: usize) -> Option<&Node> {
    model.doc.children.iter().find(|c| c.class == "Modulator" && c.name.as_deref() == Some(&format!("Modulator{slot}")))
}

pub struct Transmod {
    select: Option<Vec<usize>>,
    /// Running source-button flashes (source index, start).
    flashes: Vec<(usize, std::time::Instant)>,
    scroller: Option<Vec<usize>>,
    empty: Option<Ctrl>,
    skins: Vec<(ModulatorType, Ctrl)>,
    built: Vec<Option<ModulatorType>>,
    /// Open menu: slot and item actions.
    menu: Option<(usize, Vec<MenuAction>)>,
    /// Open option popup: slot, parameter, item count.
    popup: Option<(usize, usize, usize)>,
    /// Sequencer grid drag: slot and last edited step.
    seq_drag: Option<(usize, Option<usize>)>,
    keep_triggers: bool,
    /// Remove modulator's Yes/No/Cancel prompt is open for this slot.
    confirm_remove: Option<usize>,
    /// A source button's right-click menu (source, anchor) and its pending
    /// confirmation (None = all sources).
    src_menu: Option<(usize, Vec<usize>)>,
    src_confirm: Option<Option<usize>>,
    /// A rename in progress: macro k (Some(Ok(k))) or modulator slot
    /// (Some(Err(slot))).
    rename: Option<Result<usize, usize>>,
    /// A slot's Trigger button was clicked: slot and button path, for the
    /// editor to open the trigger panel.
    pub trigger_request: Option<(usize, Vec<usize>)>,
}

/// [re:0c608b]: the 16 step cells
/// (x, width) across a `SeqGridRect` `w` pixels wide.
pub fn seq_cells(w: i32) -> [(i32, i32); 16] {
    let r = |v: f32| (v + if v < 0.0 { -0.5 } else { 0.5 }) as i32;
    let f = (w as f32 - 15.0).ceil();
    let (cell, stride) = (r(f * 0.0625), r(f * 0.0625 - 1.0));
    let x0 = w / 2 - r(f * 0.5);
    std::array::from_fn(|i| (x0 + stride * i as i32, cell))
}

/// Sequencer OptionMenu shapes ([re:da0f3d] ctor lambda
/// [re:c6e1b9] lists [re:35a610] 0..14; item lambda
/// [re:34d9e7] writes the 16 steps with the pattern graph's shapes).
pub fn seq_shape(shape: usize, r: &mut dyn FnMut() -> f32) -> [f32; 16] {
    let v = crate::pattern::insert_shape(shape, 16, r);
    std::array::from_fn(|i| v[i])
}

/// [re:6e1816] order ([re:4dbdd1]).
pub const LFO_SHAPES: [&str; 9] = ["Sin", "Cos", "Triangle", "SawUp", "SawDown", "Square", "ArcSin", "ArcCos", "Noise"];

/// [re:15b805] order ([re:d9e70a]).
pub const MATH_OPS: [&str; 35] = [
    "Sum", "Subtract", "Multiply", "Scale", "Divide", "SumDiv2", "ADiv2PlusB", "ADiv4PlusBDiv2", "APlus2BDiv3", "MinusAMultB", "AMulti10PowB", "AbsAPlusB", "AbsAMinusB", "Minimum", "Maximum",
    "Quantize", "BDivOneMinusA", "APlusBPower2", "SinAPlusB", "SinAMinusB", "SinAMultiplyB", "SinADivideB", "CosAPlusB", "CosAMinusB", "CosAMultiplyB", "CosADivideB", "AAndB", "AOrB", "AXOrB",
    "FlipFlop", "Invert", "SampleAOnB", "SampleAOnNotB", "TrackAWhileB", "TrackAWhileNotB",
];

/// Items of a modulator's popup parameter ([re:a02600] ctor
/// [re:6dc4e0], [re:62a206] ctor [re:de5717]: names camel-split; the
/// Math inputs list [re:56009e] 3..28).
pub fn popup_items(t: ModulatorType, param: usize) -> Option<Vec<String>> {
    match (t, param) {
        (ModulatorType::LFO, 2) => Some(LFO_SHAPES.iter().map(|s| camel_words(s)).collect()),
        (ModulatorType::Maths, 0) => Some(MATH_OPS.iter().map(|s| camel_words(s)).collect()),
        (ModulatorType::Maths, 1 | 2) => Some(TransmodSource::ALL[3..].iter().map(|s| camel_words(s.name())).collect()),
        _ => None,
    }
}

/// [re:f2e743]: popup selection is
/// floor(value · (count − 1)).
pub fn popup_index(value: f32, count: usize) -> usize {
    ((value.clamp(0.0, 1.0) * (count.max(1) - 1) as f32).floor() as usize).min(count.saturating_sub(1))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuAction {
    None,
    Remove,
    /// Remove and also clear the modulator's depths (0x32 then 0x2c).
    RemoveAndClear,
    Set(ModulatorType),
    ToggleKeep,
    SeqShape(usize),
}

/// The Remove modulator prompt (`RemoveModBehavour` = Prompt).
const REMOVE_PROMPT: &str = "Removing the modulation source does not remove any modulation depths that have been applied to parameters. Do you also wish to remove any modulation depths that have been applied? This action cannot be undone (You can hit cancel to prevent any changes)";

/// Drop every attribute ending in `suffix` (a source's depths).
fn clear_depths(n: &mut Node, suffix: &str) {
    n.attrs.retain(|(k, _)| !k.ends_with(suffix));
    for c in &mut n.children {
        clear_depths(c, suffix);
    }
}

/// [re:57af69]: "[re:ea9ee5]" → "AD Envelope".
pub fn camel_words(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &ch) in c.iter().enumerate() {
        if i > 0 && ch.is_uppercase() {
            let prev = c[i - 1];
            let next_lower = c.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || (prev.is_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(ch);
    }
    out
}

impl Transmod {
    /// [re:b3390e], 0x169
    /// `e_updateModulationSourceTriggered` [source]: FlashSrc<source> to
    /// alpha 1, then a 500 ms linear [re:e325b5] to 0. The engine
    /// posts it from [re:927da0] for each slot whose
    /// modulator retriggers on the pad (source = slot + 5).
    pub fn flash(&mut self, source: usize) {
        self.flashes.retain(|(s, _)| *s != source);
        self.flashes.push((source, std::time::Instant::now()));
    }

    /// Advance the flashes; whether anything changed.
    pub fn animate_flashes(&mut self, app: &mut App) -> bool {
        if self.flashes.is_empty() {
            return false;
        }
        let Some(sel) = self.select.clone() else {
            self.flashes.clear();
            return false;
        };
        let now = std::time::Instant::now();
        let fl = self.flashes.clone();
        self.flashes.retain(|(_, t)| now.duration_since(*t).as_secs_f32() < 0.5);
        let Some(panel) = app.ctrl_mut(&sel) else { return false };
        for (s, t) in fl {
            let a = (1.0 - now.duration_since(t).as_secs_f32() / 0.5).max(0.0);
            if let Some(c) = panel.find_mut(&format!("FlashSrc{s}")) {
                c.set_attr("alpha", format!("{a:.4}"));
            }
        }
        true
    }

    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: Option<&[usize]>) -> Result<Transmod, EditorError> {
        if let Some(h) = root.find_mut("TransmodSelectPanel") {
            if h.children.is_empty() {
                skin.mount(h, "TransmodButtonPanel.skin").map_err(err)?;
            }
            // [re:0ed385]: the FlashSrc%d
            // overlays are disabled and transparent.
            fn flash_init(c: &mut Ctrl) {
                for k in &mut c.children {
                    if k.name.starts_with("FlashSrc") {
                        k.enabled = false;
                        k.passive = true;
                        k.set_attr("alpha", "0".into());
                    }
                    flash_init(k);
                }
            }
            flash_init(h);
        }
        if let Some(page) = page {
            let mut host = &mut *root;
            for &i in page {
                host = &mut host.children[i];
            }
            skin.mount(host, "TransmodPanel.skin").map_err(err)?;
        }
        let load = |skin: &mut Skin, name: &str| -> Option<Ctrl> {
            let rel = skin.find(name)?;
            skin.load_skin(&rel).ok()?.children.into_iter().next()
        };
        let empty = load(skin, "EmptySlot.skin");
        let skins = ModulatorType::ALL.iter().filter_map(|t| load(skin, skin_for(*t)).map(|c| (*t, c))).collect();
        let scroller = page.and_then(|p| {
            fn rec(c: &Ctrl, path: &mut Vec<usize>) -> bool {
                for (i, ch) in c.children.iter().enumerate() {
                    path.push(i);
                    if ch.name == "ScrollView" || rec(ch, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
            let mut c = &*root;
            for &i in p {
                c = &c.children[i];
            }
            let mut path = p.to_vec();
            rec(c, &mut path).then_some(path)
        });
        Ok(Transmod {
            select: root.path_of("TransmodSelectPanel"),
            flashes: Vec::new(),
            scroller,
            empty,
            skins,
            built: Vec::new(),
            menu: None,
            popup: None,
            seq_drag: None,
            keep_triggers: crate::options::Options::load().bool("ReplacedModulatorsRetainTriggers", false),
            confirm_remove: None,
            src_menu: None,
            src_confirm: None,
            rename: None,
            trigger_request: None,
        })
    }

    fn rebuild(&mut self, app: &mut App, model: &Model) {
        let types: Vec<Option<ModulatorType>> = (0..SLOTS).map(|s| modulator(model, s).and_then(|n| n.attr("Type")).and_then(ModulatorType::from_name)).collect();
        if types == self.built {
            return;
        }
        self.built = types.clone();
        let Some(sp) = &self.scroller else { return };
        let Some(sc) = app.ctrl_mut(sp) else { return };
        sc.children.clear();
        // [re:8b43ec] re-stacks when a panel folds.
        sc.set_attr("stackAxis", "v".into());
        sc.set_attr("stackGap", "2".into());
        let w = sc.rect.w;
        let mut y = 0;
        for (slot, t) in types.into_iter().enumerate() {
            let tpl = match t {
                Some(t) => self.skins.iter().find(|(k, _)| *k == t).map(|(_, c)| c),
                None => self.empty.as_ref(),
            };
            let Some(tpl) = tpl else { continue };
            let mut c = tpl.clone();
            let h = c.rect.h;
            c.set_rect(Rect::new(0, y, w.min(c.rect.w.max(w)), h));
            c.set_attr("modSlot", slot.to_string());
            // The modulator graph knows its slot.
            fn tag(c: &mut Ctrl, slot: usize) {
                for k in &mut c.children {
                    if k.name == "SeqGridRect" {
                        k.overload = "TransmodSequencerGrid".into();
                    }
                    if matches!(k.overload.as_str(), "TransmodDisplayPanel" | "TransmodSequencerGrid") {
                        k.set_attr("modSlot", slot.to_string());
                    }
                    tag(k, slot);
                }
            }
            tag(&mut c, slot);
            if t.is_none() {
                if let Some(l) = c.find_mut("SlotID") {
                    l.text = format!("Slot {}", slot + 1);
                }
            } else {
                c.display_name = format!("{} {}", slot + 1, c.display_name);
            }
            y += h + 2;
            sc.children.push(c);
        }
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        self.rebuild(app, model);
        let source = model.source;
        if let Some(sp) = &self.select {
            let macros: Vec<f32> = (0..4).map(|k| model.doc.f32(&format!("Macro{k}")).unwrap_or(0.0)).collect();
            // [re:b34727]: macro knobs show
            // the macro names; Src5..20 the modulator names ("S n" for an
            // empty slot); Src21..24 the macro names ("M n" for the default
            // "Macro n").
            let macro_names: Vec<String> = (0..4).map(|k| macro_name(model, k)).collect();
            let mod_names: Vec<Option<String>> = (0..SLOTS).map(|s| modulator(model, s).map(modulator_name)).collect();
            if let Some(s) = app.ctrl_mut(sp) {
                s.walk_mut(&mut |k: &mut Ctrl| {
                    if let Some(n) = k.name.strip_prefix("Src").and_then(|n| n.parse::<usize>().ok()) {
                        k.on = n == source;
                        if (5..21).contains(&n) {
                            k.text = mod_names.get(n - 5).cloned().flatten().unwrap_or_else(|| format!("S {}", n - 4));
                        } else if (21..25).contains(&n) {
                            let m = n - 21;
                            k.text = if macro_names[m] == format!("Macro {}", m + 1) { format!("M {}", m + 1) } else { macro_names[m].clone() };
                        }
                    } else if let Some(n) = k.name.strip_prefix("Macro").and_then(|n| n.parse::<usize>().ok()) {
                        k.value = macros.get(n).copied().unwrap_or(0.0);
                        if let Some(name) = macro_names.get(n) {
                            k.display_name = name.clone();
                        }
                    }
                });
            }
        }
        let Some(sp) = &self.scroller else { return };
        let Some(sc) = app.ctrl_mut(sp) else { return };
        for c in &mut sc.children {
            let Some(slot) = c.attr("modSlot").and_then(|v| v.parse::<usize>().ok()) else { continue };
            let Some(n) = modulator(model, slot) else { continue };
            let Some(t) = n.attr("Type").and_then(ModulatorType::from_name) else { continue };
            let src = TransmodSource::ALL.get(source).copied().unwrap_or(TransmodSource::ActualParameter);
            c.walk_mut(&mut |k: &mut Ctrl| {
                let Some(i) = k.name.strip_prefix("Control").and_then(|v| v.parse::<usize>().ok()) else { return };
                let Some(pn) = t.params().get(i) else { return };
                let base = n.f32(&format!("{pn}_ActualParameter")).unwrap_or(t.default_value(i)).clamp(0.0, 1.0);
                let depth = if source == 0 { 0.0 } else { n.f32(&format!("{pn}_{}", src.name())).unwrap_or(0.0) };
                if k.class == "CtrlPopupButton" {
                    if let Some(items) = popup_items(t, i) {
                        k.text = items[popup_index(base, items.len())].clone();
                    }
                } else if k.class == "CtrlButton" {
                    k.on = base >= 0.5;
                } else if k.class == "CtrlTransmodKnob" {
                    bind_knob(k, source, base, depth);
                } else {
                    k.value = base;
                }
            });
        }
    }

    /// Outline the source buttons in `mask` (bits 1..=28)
    /// (`TransmodButtonStyleHighlight`, else `TransmodButtonStyle`); true
    /// when one changed.
    pub fn highlight_sources(&self, app: &mut App, mask: u32) -> bool {
        let Some(sp) = self.select.clone() else { return false };
        let mut changed = Vec::new();
        if let Some(panel) = app.ctrl_mut(&sp) {
            fn walk(c: &mut Ctrl, mask: u32, path: &mut Vec<usize>, changed: &mut Vec<Vec<usize>>) {
                for (i, k) in c.children.iter_mut().enumerate() {
                    path.push(i);
                    if let Some(s) = k.name.strip_prefix("Src").and_then(|n| n.parse::<u32>().ok()).filter(|s| (1..=28).contains(s)) {
                        let style = if mask & 1 << s != 0 { "TransmodButtonStyleHighlight" } else { "TransmodButtonStyle" };
                        if k.style != style {
                            k.style = style.into();
                            changed.push(path.clone());
                        }
                    }
                    walk(k, mask, path, changed);
                    path.pop();
                }
            }
            walk(panel, mask, &mut Vec::new(), &mut changed);
        }
        for p in &changed {
            let r = app.abs_rect(&[sp.as_slice(), p].concat());
            app.invalidate_rect(r);
        }
        !changed.is_empty()
    }

    /// "Remove all modulation from this control" on a modulator
    /// parameter ([re:8bc6f5]): depth 0 for sources
    /// 1..=28.
    pub fn remove_modulation(&self, app: &App, path: &[usize]) -> Vec<Command> {
        let Some(c) = app.ctrl(path) else { return Vec::new() };
        if c.class != "CtrlTransmodKnob" {
            return Vec::new();
        }
        let (Some(slot), Some(i)) = (Self::slot_of(app, path), c.name.strip_prefix("Control").and_then(|v| v.parse::<usize>().ok())) else { return Vec::new() };
        (1..=28).map(|s| msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, i as f32, s as f32, 0.0])).collect()
    }

    /// A knob double click's reset value: macros 0
    /// ([re:ce19fc] [re:69ec5c]), modulator
    /// parameters their default ([re:e5c3c3] [re:69fc2e],
    /// [re:674117]).
    pub fn reset_value(&self, app: &App, model: &Model, path: &[usize], name: &str) -> Option<f32> {
        if self.select.as_ref().is_some_and(|sp| path.starts_with(sp)) && name.starts_with("Macro") {
            return Some(0.0);
        }
        let slot = Self::slot_of(app, path)?;
        let i = name.strip_prefix("Control")?.parse::<usize>().ok()?;
        let t = modulator(model, slot)?.attr("Type").and_then(ModulatorType::from_name)?;
        Some(t.default_value(i))
    }

    /// A modulator panel knob's tooltip ([re:c397e1]
    ///  [re:3187f8], shown above): its conversion, with the
    /// transmod description / modulated value like the layer knobs.
    pub fn knob_tooltip(&self, app: &App, model: &Model, path: &[usize], source: Option<(&str, usize)>) -> Option<String> {
        let c = app.ctrl(path)?;
        if !c.class.contains("Knob") {
            return None;
        }
        let slot = Self::slot_of(app, path)?;
        let i = c.name.strip_prefix("Control")?.parse::<usize>().ok()?;
        let n = modulator(model, slot)?;
        let t = n.attr("Type").and_then(ModulatorType::from_name)?;
        let pn = *t.params().get(i)?;
        let src = source.map(|(l, s)| (l, dreist_engine::engine::doc::attr_name(pn, s).and_then(|a| n.f32(&a)).unwrap_or(0.0)));
        Some(crate::conv::transmod_text(crate::conv::Conv::modulator(t, pn), &camel_words(pn), c.value, src))
    }

    fn slot_of(app: &App, path: &[usize]) -> Option<usize> {
        (0..=path.len()).rev().find_map(|n| app.ctrl(&path[..n])?.attr("modSlot")?.parse().ok())
    }

    fn set_modulator(&self, model: &mut Model, slot: usize, action: MenuAction, out: &mut Vec<Command>) {
        let name = format!("Modulator{slot}");
        let pos = model.doc.children.iter().position(|c| c.class == "Modulator" && c.name.as_deref() == Some(&name));
        match action {
            MenuAction::Remove | MenuAction::RemoveAndClear => {
                if action == MenuAction::RemoveAndClear {
                    // 0x32 `e_removeModulationSourceDestinations` [slot + 5]:
                    // every depth from this source goes.
                    if let Some(src) = dreist_formats::TransmodSource::ALL.get(slot + 5) {
                        clear_depths(&mut model.doc, &format!("_{}", src.name()));
                    }
                }
                if let Some(i) = model.doc.children.iter().position(|c| c.class == "Modulator" && c.name.as_deref() == Some(&name)) {
                    model.doc.children.remove(i);
                }
            }
            MenuAction::Set(t) => {
                let mut n = Node::new("Modulator", Some(&name));
                n.set("Type", t.name());
                if let Some(i) = pos {
                    if self.keep_triggers {
                        n.children = model.doc.children[i].children.clone();
                    }
                    model.doc.children[i] = n;
                } else {
                    model.doc.children.push(n);
                }
                // As on insert: select the new modulator's source.
                model.source = (slot + 5).min(28);
            }
            _ => return,
        }
        out.push(model.rebuild_command());
    }

    /// [re:2b8699] graph of a slot's modulator (base values).
    pub fn draw(&self, model: &Model, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) -> bool {
        if c.overload == "TransmodSequencerGrid" {
            draw_seq(model, r, p, c);
            return true;
        }
        if c.overload != "TransmodDisplayPanel" {
            return false;
        }
        use dreist_gui::paint::RectF;
        let s = r.styles;
        let style = if c.style.is_empty() { "TransmodPlotStyle" } else { c.style.as_str() };
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let bg = s.colour(style, "colFill", dreist_gui::Colour::BLACK);
        p.fill_rounded(RectF::new(0.0, 0.0, w, h), 5.0, [true; 4], bg);
        let Some(slot) = c.attr("modSlot").and_then(|v| v.parse::<usize>().ok()) else { return true };
        let Some(n) = modulator(model, slot) else { return true };
        let Some(kind) = n.attr("Type").and_then(ModulatorType::from_name) else { return true };
        let params: Vec<f32> = kind.params().iter().enumerate().map(|(i, name)| n.f32(&format!("{name}_ActualParameter")).unwrap_or(kind.default_value(i))).collect();
        let Some(vals) = crate::transmodvis::modulator(kind, &params, w.max(1.0) as usize) else { return true };
        let line = s.colour(style, "colAltBorder", dreist_gui::Colour::WHITE);
        let pts: Vec<(f32, f32)> = vals.iter().enumerate().map(|(x, v)| (x as f32, 3.0 + (h - 6.0) * (1.0 - v.clamp(0.0, 1.0)))).collect();
        for w2 in pts.windows(2) {
            p.line(w2[0].0, w2[0].1, w2[1].0, w2[1].1, line, 2.0);
        }
        true
    }

    /// "Rename" ([re:284b3d] [re:5661c5] for macros,
    /// [re:14a9cd] / [re:4faf1b] for sources): a 150 × 20 edit box over the
    /// control with the current name.
    pub fn start_rename(&mut self, app: &mut App, model: &Model, path: &[usize], target: Result<usize, usize>) {
        let text = match target {
            Ok(k) => macro_name(model, k),
            Err(slot) => modulator(model, slot).map(modulator_name).unwrap_or_default(),
        };
        let Some(c) = app.ctrl(path).cloned() else { return };
        let rect = Rect::new(c.rect.x + c.rect.w / 2 - 75, c.rect.y + c.rect.h / 2 - 10, 150, 20);
        app.open_edit_box(&path[..path.len() - 1], RENAME_EDIT, rect, text);
        self.rename = Some(target);
    }

    /// The rename commit: [re:810291] (an empty name labels the
    /// macro "M n") or the modulator's name (0x33; empty → its short
    /// name).
    fn commit_rename(&mut self, app: &mut App, model: &mut Model, path: &[usize], text: &str) {
        let parent = path[..path.len() - 1].to_vec();
        if let Some(c) = app.ctrl_mut(&parent) {
            c.children.retain(|k| k.name != RENAME_EDIT);
        }
        let name: String = text.trim().chars().take(63).collect();
        match self.rename.take() {
            Some(Ok(k)) => {
                let key = format!("Macro{k}Name");
                if name.is_empty() {
                    model.doc.attrs.retain(|(a, _)| *a != key);
                } else {
                    model.doc.set(&key, name);
                }
            }
            Some(Err(slot)) => {
                let n = model.doc.child_mut_or_insert("Modulator", &format!("Modulator{slot}"));
                let short = n.attr("Type").and_then(ModulatorType::from_name).map(short_name).unwrap_or("");
                if name.is_empty() || name == short {
                    n.attrs.retain(|(a, _)| a != "modName");
                } else {
                    n.set("modName", name);
                }
            }
            None => {}
        }
    }

    /// [re:ce19fc] [re:32cf77]: a source button's
    /// right-click menu — Unipolar (0x31, `Polar_<source>`), Remove all
    /// modulation from this source / from the product (confirmed; 0x32
    /// per source). The port edits the document and rebuilds.
    fn source_menu(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        let rebuild = |model: &Model, out: &mut Vec<Command>| out.push(model.rebuild_command());
        match ev {
            UiEvent::RightClicked { path, name } if self.select.as_ref().is_some_and(|sp| path.starts_with(sp)) => {
                let Some(s) = name.strip_prefix("Src").and_then(|n| n.parse::<usize>().ok()).filter(|s| (1..=28).contains(s)) else { return false };
                let Some(src) = TransmodSource::ALL.get(s) else { return false };
                let unipolar = model.doc.bool(&format!("Polar_{}", src.name())).unwrap_or(true);
                let mut items = vec![
                    if unipolar { format!("{}Unipolar", dreist_gui::menu::CHECK) } else { "Unipolar".into() },
                    "Remove all modulation from this source".into(),
                    format!("Remove all modulation from {}", crate::product()),
                ];
                // Rename: modulator slots holding a modulator, and macros.
                if ((5..21).contains(&s) && modulator(model, s - 5).is_some()) || (21..25).contains(&s) {
                    items.push(String::new());
                    items.push("Rename".into());
                }
                self.src_menu = Some((s, path.clone()));
                app.open_menu(path, items, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.src_confirm.is_some() && self.src_menu.as_ref().is_some_and(|(_, p)| p == anchor) => {
                let which = self.src_confirm.take().unwrap();
                self.src_menu = None;
                if *index != 2 {
                    return true;
                }
                for s in which.map_or(1..=28, |s| s..=s) {
                    if let Some(src) = TransmodSource::ALL.get(s) {
                        clear_depths(&mut model.doc, &format!("_{}", src.name()));
                    }
                }
                rebuild(model, out);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.src_menu.as_ref().is_some_and(|(_, p)| p == anchor) => {
                let (s, path) = self.src_menu.clone().unwrap();
                let Some(src) = TransmodSource::ALL.get(s) else { return true };
                match index {
                    0 => {
                        self.src_menu = None;
                        let key = format!("Polar_{}", src.name());
                        let unipolar = model.doc.bool(&key).unwrap_or(true);
                        model.doc.set(&key, !unipolar);
                        rebuild(model, out);
                    }
                    4 => {
                        self.src_menu = None;
                        let target = if s >= 21 { Ok(s - 21) } else { Err(s - 5) };
                        self.start_rename(app, model, &path, target);
                    }
                    1 | 2 => {
                        let label =
                            if *index == 1 { app.ctrl(&path).map(|c| c.text.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| src.name().to_string()) } else { crate::product().to_string() };
                        let text = format!("Are you sure you want to remove all modulation for {label}? We cannot undo this action");
                        self.src_confirm = Some((*index == 1).then_some(s));
                        app.open_menu(&path, vec![text, String::new(), "Yes".into(), "No".into()], None);
                    }
                    _ => self.src_menu = None,
                }
                true
            }
            _ => false,
        }
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if self.source_menu(app, model, ev, out) {
            return true;
        }
        if let UiEvent::TextCommitted { path, name, text } = ev {
            if name == RENAME_EDIT {
                self.commit_rename(app, model, path, text);
                self.sync(app, model);
                return true;
            }
        }
        if let UiEvent::Mouse { path, overload, kind, x, y } = ev {
            if overload == "TransmodSequencerGrid" {
                use dreist_gui::app::MouseKind;
                let Some(c) = app.ctrl(path) else { return true };
                let Some(slot) = c.attr("modSlot").and_then(|v| v.parse::<usize>().ok()) else { return true };
                let h = c.rect.h.max(1);
                let cells = seq_cells(c.rect.w);
                match kind {
                    MouseKind::Down | MouseKind::Drag => {
                        if *kind == MouseKind::Down {
                            self.seq_drag = Some((slot, None));
                        }
                        let Some((s, last)) = self.seq_drag else { return true };
                        if s != slot {
                            return true;
                        }
                        // Step value from the mouse position [re:1539da].
                        let px = *x as i32;
                        let Some(step) = cells.iter().position(|(cx, cw)| px >= *cx && px < cx + cw) else { return true };
                        let v = (1.0 - *y / h as f32).clamp(0.0, 1.0);
                        let post = |out: &mut Vec<Command>, i: usize| out.push(msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, (6 + i) as f32, 0.0, v]));
                        post(out, step);
                        if let Some(l) = last.filter(|l| *l != step) {
                            for i in l.min(step)..l.max(step) {
                                post(out, i);
                            }
                        }
                        self.seq_drag = Some((slot, Some(step)));
                    }
                    MouseKind::Up => self.seq_drag = None,
                    MouseKind::RightDown => {}
                }
                return true;
            }
        }
        if let UiEvent::MenuSelected { index, .. } = ev {
            if let Some((slot, param, n)) = self.popup.take() {
                // Item index scaled back to 0..1 (inverse of `popup_index`).
                let v = if n > 1 { (*index).min(n - 1) as f32 / (n - 1) as f32 } else { 0.0 };
                out.push(msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, param as f32, 0.0, v]));
                return true;
            }
            if let Some(slot) = self.confirm_remove.take() {
                // DialogYesNoCancel: Yes clears the depths too, No only
                // removes, Cancel does nothing.
                match *index {
                    2 => self.set_modulator(model, slot, MenuAction::RemoveAndClear, out),
                    3 => self.set_modulator(model, slot, MenuAction::Remove, out),
                    _ => {}
                }
                return true;
            }
            if let Some((slot, actions)) = self.menu.take() {
                match actions.get(*index).copied().unwrap_or(MenuAction::None) {
                    // [re:adeff9] lambda #1 [re:edac74]:
                    // option `RemoveModBehavour` (Prompt, AlwaysClear,
                    // NeverClear, ClearOnModifier — option key).
                    MenuAction::Remove => match crate::options::Options::load().string("RemoveModBehavour", "Prompt").as_str() {
                        "AlwaysClear" => self.set_modulator(model, slot, MenuAction::RemoveAndClear, out),
                        "NeverClear" => self.set_modulator(model, slot, MenuAction::Remove, out),
                        "ClearOnModifier" => self.set_modulator(model, slot, if app.mods.alt { MenuAction::RemoveAndClear } else { MenuAction::Remove }, out),
                        _ => {
                            let anchor =
                                app.menu.as_ref().map(|m| m.anchor.clone()).or_else(|| if let UiEvent::MenuSelected { anchor, .. } = ev { Some(anchor.clone()) } else { None }).unwrap_or_default();
                            let items = vec![REMOVE_PROMPT.to_string(), String::new(), "Yes".into(), "No".into(), "Cancel".into()];
                            app.open_menu(&anchor, items, None);
                            self.confirm_remove = Some(slot);
                        }
                    },
                    MenuAction::ToggleKeep => {
                        self.keep_triggers = !self.keep_triggers;
                        let mut o = crate::options::Options::load();
                        o.set("ReplacedModulatorsRetainTriggers", self.keep_triggers);
                        o.save();
                    }
                    MenuAction::SeqShape(k) => {
                        let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1) | 1;
                        // Park–Miller minimal standard, as [re:b24a21].
                        let mut rnd = || {
                            seed = ((seed as u64 * 16807) % 0x7fff_ffff) as u32;
                            seed as f32 / 0x7fff_ffff as f32
                        };
                        for (i, v) in seq_shape(k, &mut rnd).into_iter().enumerate() {
                            out.push(msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, (6 + i) as f32, 0.0, v.clamp(0.0, 1.0)]));
                        }
                    }
                    MenuAction::None => {}
                    a => self.set_modulator(model, slot, a, out),
                }
                return true;
            }
        }
        if let UiEvent::Clicked { path, name, .. } = ev {
            if app.ctrl(path).is_some_and(|c| c.class == "CtrlPopupButton") {
                if let (Some(slot), Some(i)) = (Self::slot_of(app, path), name.strip_prefix("Control").and_then(|v| v.parse::<usize>().ok())) {
                    let Some(n) = modulator(model, slot) else { return true };
                    let Some(t) = n.attr("Type").and_then(ModulatorType::from_name) else { return true };
                    let Some(items) = popup_items(t, i) else { return true };
                    let base = t.params().get(i).and_then(|pn| n.f32(&format!("{pn}_ActualParameter"))).unwrap_or(t.default_value(i));
                    let count = items.len();
                    app.open_menu(path, items, Some(popup_index(base, count)));
                    self.popup = Some((slot, i, count));
                    return true;
                }
            }
            if name == "Trigger" {
                if let Some(slot) = Self::slot_of(app, path) {
                    self.trigger_request = Some((slot, path.clone()));
                    return true;
                }
            }
            if name == "OptionMenu" {
                if let Some(slot) = Self::slot_of(app, path) {
                    let actions = (0..crate::pattern::INSERTABLE.len()).map(MenuAction::SeqShape).collect();
                    app.open_menu(path, crate::pattern::INSERTABLE.iter().map(|s| s.to_string()).collect(), None);
                    self.menu = Some((slot, actions));
                    return true;
                }
            }
            if matches!(name.as_str(), "TypePopup" | "Options") {
                if let Some(slot) = Self::slot_of(app, path) {
                    let types = ModulatorType::ALL.iter().map(|t| (camel_words(t.name()), MenuAction::Set(*t)));
                    let items: Vec<(String, MenuAction)> = if name == "TypePopup" {
                        types.collect()
                    } else {
                        let mut v = vec![("Remove modulator".to_string(), MenuAction::Remove), (String::new(), MenuAction::None)];
                        v.extend(types);
                        v.push((String::new(), MenuAction::None));
                        let mark = if self.keep_triggers { "✓ " } else { "" };
                        v.push((format!("{mark}Keep trigger data when replacing modulators"), MenuAction::ToggleKeep));
                        v
                    };
                    let (labels, actions): (Vec<String>, Vec<MenuAction>) = items.into_iter().unzip();
                    app.open_menu(path, labels, None);
                    self.menu = Some((slot, actions));
                    return true;
                }
            }
        }
        match ev {
            UiEvent::Clicked { path, name, on, .. } => {
                if let Some(sp) = &self.select {
                    if path.starts_with(sp) {
                        if let Some(n) = name.strip_prefix("Src").and_then(|n| n.parse::<usize>().ok()) {
                            model.source = if model.source == n { 0 } else { n.min(28) };
                            return true;
                        }
                    }
                }
                if let Some(slot) = Self::slot_of(app, path) {
                    if let Some(i) = name.strip_prefix("Control").and_then(|v| v.parse::<usize>().ok()) {
                        out.push(msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, i as f32, 0.0, if *on { 1.0 } else { 0.0 }]));
                        return true;
                    }
                }
                false
            }
            UiEvent::ValueChanged { path, name, value, .. } => {
                if let Some(sp) = &self.select {
                    if path.starts_with(sp) {
                        if let Some(k) = name.strip_prefix("Macro").and_then(|n| n.parse::<i32>().ok()) {
                            out.push(msg(id::SET_MACRO_VALUE, &[k], &[*value]));
                            return true;
                        }
                    }
                }
                let Some(slot) = Self::slot_of(app, path) else { return false };
                let Some(i) = name.strip_prefix("Control").and_then(|v| v.parse::<usize>().ok()) else { return false };
                let (src, v) = crate::model::knob_edit(app.ctrl(path), model.source, *value);
                out.push(msg(id::SET_MODULATION_SOURCE_PARAMETER, &[slot as i32], &[0.0, i as f32, src as f32, v]));
                true
            }
            _ => false,
        }
    }
}

/// [re:d998ec]: step bars in colFill (darker
/// for steps 5–8 and 13–16), cell outlines in colAltFill and steps past
/// the Steps knob under colDisabledOverlay; the playing step's cell is
/// filled with colAltFill first ([re:cf6b89], kept in
/// `Model::seq_steps`).
/// [re:da0f3d] ctor [re:32145b]: "Step n - v%" over a cell.
pub fn seq_tooltip(model: &Model, c: &Ctrl, x: f32) -> Option<String> {
    let slot: usize = c.attr("modSlot")?.parse().ok()?;
    let n = modulator(model, slot)?;
    let t = ModulatorType::MiniSequencer;
    let i = seq_cells(c.rect.w).iter().position(|&(cx, w)| x >= cx as f32 && x < (cx + w) as f32)?;
    let v = t.params().get(6 + i).and_then(|pn| n.f32(&format!("{pn}_ActualParameter"))).unwrap_or(t.default_value(6 + i));
    Some(format!("Step {} - {}%", i + 1, (v * 100.0).floor() as i64))
}

fn draw_seq(model: &Model, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) {
    use dreist_gui::paint::RectF;
    let Some(slot) = c.attr("modSlot").and_then(|v| v.parse::<usize>().ok()) else { return };
    let Some(n) = modulator(model, slot) else { return };
    let t = ModulatorType::MiniSequencer;
    let get = |i: usize| t.params().get(i).and_then(|pn| n.f32(&format!("{pn}_ActualParameter"))).unwrap_or(t.default_value(i));
    let s = r.styles;
    let st = if c.style.is_empty() { "MiniSequencerStyle" } else { c.style.as_str() };
    let fill = s.colour(st, "colFill", dreist_gui::Colour::BLACK);
    let alt = s.colour(st, "colAltFill", dreist_gui::Colour::WHITE);
    let overlay = s.colour(st, "colDisabledOverlay", dreist_gui::Colour::BLACK.with_alpha(0.5));
    let last = (get(1) * 15.0).floor() as usize;
    let h = c.rect.h as f32;
    for (i, (x, w)) in seq_cells(c.rect.w).into_iter().enumerate() {
        let (x, w) = (x as f32, w as f32);
        let col = if i < 12 && i & !3 != 4 { fill } else { fill.darken(0.25) };
        if model.seq_steps.get(slot).is_some_and(|&k| k == i as i32) {
            p.fill_rect(RectF::new(x, 0.0, w, h), alt);
        }
        let v = get(6 + i);
        if v > 0.0 {
            let bh = ((v * h + 0.5).ceil()).max(0.0);
            let top = (h - bh + 0.5).ceil();
            p.fill_rect(RectF::new(x, top, w, bh), col);
        }
        p.stroke_rect(RectF::new(x, 0.0, w, h), alt, 1.0);
        if i > last {
            p.fill_rect(RectF::new(x, 0.0, w, h), overlay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_and_clear_drops_the_sources_depths() {
        let src = dreist_formats::TransmodSource::ALL[5].name();
        let mut doc = Node::new("root", None);
        let mut m = Node::new("Modulator", Some("Modulator0"));
        m.set("Type", "LFO");
        doc.children.push(m);
        let mut eng = Node::new("Engine", Some("Engine0"));
        eng.set(&format!("Gain_{src}"), 0.3).set("Gain", 0.5);
        doc.children.push(eng);
        clear_depths(&mut doc, &format!("_{src}"));
        let e = doc.children.iter().find(|c| c.class == "Engine").unwrap();
        assert!(e.attr(&format!("Gain_{src}")).is_none());
        assert_eq!(e.attr("Gain"), Some("0.5"));
    }

    #[test]
    fn camel_words() {
        assert_eq!(super::camel_words("ADEnvelope"), "AD Envelope");
        assert_eq!(super::camel_words("MorphingLFO"), "Morphing LFO");
        assert_eq!(super::camel_words("SampleAndHold"), "Sample And Hold");
        assert_eq!(super::camel_words("LFO"), "LFO");
    }

    #[test]
    fn seq_grid() {
        let cells = super::seq_cells(240);
        // f = 225: cells 14 wide, 13 apart, centred.
        assert_eq!(cells[0], (7, 14));
        assert_eq!(cells[15].0, 7 + 13 * 15);
        let s = super::seq_shape(13, &mut || 0.0);
        assert_eq!(s[0], 1.0);
        assert!((s[8] - 0.5f32.powf(0.25)).abs() < 1e-6);
        assert_eq!(super::seq_shape(0, &mut || 0.0)[8], 0.5);
    }

    #[test]
    fn popups() {
        use dreist_formats::ModulatorType;
        let shapes = super::popup_items(ModulatorType::LFO, 2).unwrap();
        assert_eq!(shapes[3], "Saw Up");
        let src = super::popup_items(ModulatorType::Maths, 1).unwrap();
        assert_eq!(src.len(), 26);
        assert_eq!(src[0], "Velocity");
        assert_eq!(super::popup_items(ModulatorType::Maths, 0).unwrap().len(), 35);
        // Engine reads round(v·8) for the shape; the GUI index round-trips.
        for i in 0..9 {
            let v = i as f32 / 8.0;
            assert_eq!(super::popup_index(v, 9), i);
            assert_eq!((v * 8.0).round() as usize, i);
        }
    }
}
