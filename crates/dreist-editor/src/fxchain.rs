//! Effect chain panel (`EffectChain.skin`, [re:e04aee]):
//! one [re:052ee5] per effect slot (header = effect type in upper
//! case, `SetupPanel` with input trim / mix / output) holding the effect's
//! own skin (`Effects/FX/<name>.skin`, table in [re:46bffe]
//! [re:a200d6]), plus `EmptyEffect.skin` to add one.
//!
//! [re:0e72ae] binds `Control<n>` to effect
//! parameter n−1 (up to 12); changes post `e_setFXParameter` 0x59 with index
//! [engine, pad, layer, slot] and values [parameter, source, value]
//! (`DoSendEffectFloatMessageChange` [re:b5d4be]). Trim/mix/output post
//! 0x5e/0x5c/0x5d with values [0, value, source].
//!
//! [re:ea87a2] (EffectTypeCombo): Save
//! preset, Save as default, Revert to default, the type's presets (user
//! folder first, then system `Effects/<Type>/*.fxpreset`), Remove effect,
//! then one submenu per category (`EffectCategoryToString` [re:cf5f13],
//! `GetCategoryForEffect` [re:63da62] table [re:b06dc6]). The empty slot's
//! AddButton offers the category submenus ([re:57c692]). Choosing
//! posts `e_setEffect` (100) [engine, pad, layer] with the slot and a new
//! `Effect` loaded from the type's `Default.fxpreset`
//! ([re:769021]); the port edits the document
//! and rebuilds. Submenus open as a second menu.

use dreist_engine::binding::id;
use dreist_formats::Node;
use dreist_formats::params::{EFFECT_TYPES, effect_option_strings, effect_param_names, effect_type_index};
use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::Model;

pub const CATEGORIES: [&str; 6] = ["Distortion", "Dynamics", "EQ", "Filters", "FX", "Reverbs"];
#[cfg(test)]
use dreist_formats::params::effect_category as category;

/// System and user effect preset folders for a type.
fn preset_dirs(resources: &std::path::Path, kind: usize) -> Vec<std::path::PathBuf> {
    let name = EFFECT_TYPES[kind];
    let mut v = Vec::new();
    // [re:f15e15].
    v.push(crate::options::documents_dir().join("Effects").join(name));
    if let Some(r) = resources.parent() {
        v.push(r.join("Effects").join(name));
    }
    v
}

/// `GetPresetsForEffectType` [re:1598f6]: user presets, then system ones;
/// `ScanFolderForPresetsForEffectType` [re:53368c] descends into subfolders
/// and skips `Default.fxpreset` (the type's default, not a preset).
pub fn presets(resources: &std::path::Path, kind: usize) -> Vec<std::path::PathBuf> {
    fn scan(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let mut v: Vec<std::path::PathBuf> =
            std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                scan(&p, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("fxpreset")) && p.file_stem().is_some_and(|n| n != "Default") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for d in preset_dirs(resources, kind) {
        scan(&d, &mut out);
    }
    out
}

/// An `Effect` node from a preset file, parameters renamed from
/// `Parameter<n>` to the document's names.
fn load_preset(path: &std::path::Path, kind: usize) -> Option<Node> {
    let root = Node::parse(&std::fs::read_to_string(path).ok()?).ok()?;
    let mut n = root.children.into_iter().find(|c| c.class == "Effect")?;
    let names = effect_param_names(kind);
    for (k, _) in n.attrs.iter_mut() {
        if let Some(i) = k.strip_prefix("Parameter").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(pn) = names.get(i) {
                *k = pn.chars().filter(|c| !c.is_whitespace()).collect();
            }
        }
    }
    n.set("EffectType", EFFECT_TYPES[kind]);
    Some(n)
}

/// A new effect of `kind` (its `Default.fxpreset`, else bare).
fn new_effect(resources: &std::path::Path, kind: usize) -> Node {
    preset_dirs(resources, kind).into_iter().find_map(|d| load_preset(&d.join("Default.fxpreset"), kind)).unwrap_or_else(|| {
        let mut n = Node::new("Effect", None);
        n.set("EffectType", EFFECT_TYPES[kind]).set("IsActive", "true");
        n
    })
}

/// What a menu item does.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    None,
    Revert,
    Preset(std::path::PathBuf),
    Remove,
    Category(usize),
    Type(usize),
    /// Option parameter item: (parameter, normalized value).
    Option(usize, f32),
    /// `DoShowEffectTypeMenu` Save preset / Save as default.
    SavePreset,
    SaveDefault,
    /// Chain menu ([re:e04aee] $_3 [re:3687bc]).
    ChainSave,
    ChainClear,
    ChainLoad(std::path::PathBuf),
    /// A preset folder (the tokenised menu's submenu).
    ChainFolder(std::path::PathBuf),
    /// [re:a86ca5] / [re:57c692] $_1.
    EffectCut,
    EffectCopy,
    EffectPaste,
}

/// The effect clipboard ([re:e066f8]).
static EFFECT_CLIPBOARD: std::sync::Mutex<Option<Node>> = std::sync::Mutex::new(None);

fn effect_clipboard() -> Option<Node> {
    EFFECT_CLIPBOARD.lock().ok().and_then(|c| c.clone())
}

/// A file the editor should ask for and write.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveRequest {
    /// [re:7b9693].
    Chain(Target),
    /// An effect preset (`.fxpreset`) of the slot, in the type's user folder.
    Effect(Target, usize, usize),
}

/// [re:3cc799] / `GetSystemEffectsChainPath`
/// [re:e73ad8]: "Effect Chains" under the user documents folder and the
/// resources.
pub fn chain_dirs(resources: &std::path::Path) -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
    let user = Some(crate::options::documents_dir().join("Effect Chains"));
    (user, resources.parent().map(|r| r.join("Effect Chains")))
}

/// [re:f94c3a]: user `.fxchain` files, then
/// system ones (folders scanned recursively, sorted).
pub fn chain_presets(resources: &std::path::Path) -> Vec<std::path::PathBuf> {
    fn scan(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let mut v: Vec<std::path::PathBuf> =
            std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                scan(&p, out);
            } else if p.extension().is_some_and(|x| x == "fxchain") {
                out.push(p);
            }
        }
    }
    let (u, sys) = chain_dirs(resources);
    let mut out = Vec::new();
    for d in [u, sys].into_iter().flatten() {
        scan(&d, &mut out);
    }
    out
}

/// The doc chain for a `.fxchain` file (`DoLoadPreset` [re:8e8a6f]): effects
/// with `Parameter<n>` renamed to the document's names, `ChainPresetName`
/// = the file path.
pub fn load_chain(path: &std::path::Path) -> Option<Node> {
    let root = Node::parse(&std::fs::read_to_string(path).ok()?).ok()?;
    let mut chain = root.children.into_iter().find(|c| c.class == "EffectChain")?;
    for fx in chain.children.iter_mut().filter(|c| c.class == "Effect") {
        let kind = fx.attr("EffectType").and_then(effect_type_index).unwrap_or(0);
        let names = effect_param_names(kind);
        for (k, _) in fx.attrs.iter_mut() {
            if let Some(i) = k.strip_prefix("Parameter").and_then(|i| i.parse::<usize>().ok()) {
                if let Some(pn) = names.get(i) {
                    *k = pn.chars().filter(|c| !c.is_whitespace()).collect();
                }
            }
        }
        if fx.version.is_none() {
            fx.version = Some(2);
        }
    }
    set_chain_name(&mut chain, &path.to_string_lossy());
    chain.name = Some("EffectChain".into());
    Some(chain)
}

/// [re:376498].
pub fn set_chain_preset_name(chain: &mut Node, name: &str) {
    set_chain_name(chain, name);
}

/// An effect's `PresetName`.
pub fn set_effect_preset_name(n: &mut Node, name: &str) {
    n.children.retain(|c| c.name.as_deref() != Some("PresetName"));
    let mut pn = Node::new("FixedString", Some("PresetName"));
    pn.set("string", name);
    n.children.push(pn);
}

fn set_chain_name(chain: &mut Node, name: &str) {
    chain.children.retain(|c| c.name.as_deref() != Some("ChainPresetName"));
    let mut n = Node::new("FixedString", Some("ChainPresetName"));
    n.set("string", name);
    chain.children.push(n);
}

/// [re:103e36].
pub fn chain_name(chain: &Node) -> Option<String> {
    chain.children.iter().find(|c| c.name.as_deref() == Some("ChainPresetName")).and_then(|c| c.attr("string")).map(str::to_string)
}

/// Write a preset file: `<root>` around `node` (the document format, as
/// [re:a43b77] writes the current serializer's).
pub fn save_preset_file(path: &std::path::Path, node: &Node) -> std::io::Result<()> {
    let mut root = Node::new("root", None);
    root.children.push(node.clone());
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    dreist_formats::file::write_atomic(path, root.to_xml())
}

/// Effect type index → skin ([re:46bffe]; the dedicated
/// classes load VariEQ/ChanComp/BusComp/Limiter themselves; the model
/// filters share ModelFilter).
fn skin_for(kind: usize) -> Option<&'static str> {
    Some(match kind {
        1 => "Gain.skin",
        2 => "StereoWidth.skin",
        3 => "ParaEQ.skin",
        4 => "DJEQ.skin",
        5 => "VariEQ.skin",
        6 => "HILO.skin",
        7 => "Enhancer.skin",
        8 => "ChanComp.skin",
        9 => "BusComp.skin",
        10 => "VCAComp.skin",
        11 => "NoiseGate.skin",
        12 => "EnvShaper.skin",
        13 => "DrumDynamics.skin",
        14 => "Limiter.skin",
        15 => "Delay.skin",
        16 => "Flanger.skin",
        17 => "Chorus.skin",
        18 => "Phaser.skin",
        19 => "Distortion.skin",
        20 => "SingleMault.skin",
        21 => "Mault.skin",
        22 => "Maul.skin",
        23 => "Crusher.skin",
        24 => "RingSting.skin",
        25 => "Shifter.skin",
        26 => "Amber.skin",
        27 => "PhaseMod.skin",
        28 => "Filter.skin",
        29 => "ModFilter.skin",
        30 => "Comb.skin",
        31..=36 => "ModelFilter.skin",
        37 => "GuruFilter.skin",
        38 => "Freezer.skin",
        39 => "BreverbHall.skin",
        40 => "BreverbPlate.skin",
        41 => "BreverbRoom.skin",
        42 => "BreverbInverse.skin",
        43 => "TinCan.skin",
        44 => "Reverb.skin",
        45 => "Bloom.skin",
        46 => "DirtyDac.skin",
        47 => "AnalogRingMod.skin",
        _ => return None,
    })
}

/// Which chain the panel shows: engine, pad or layer of the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Engine(usize),
    Pad(usize, usize),
    Layer(usize, usize, usize),
    /// Global aux return (`GetEffectChain` engine −4, pad = aux).
    Aux(usize),
    /// Master channel (`GetEffectChain` engine −3).
    Master,
}

impl Target {
    fn index(self) -> [i32; 3] {
        match self {
            Target::Engine(e) => [e as i32, -1, -1],
            Target::Pad(e, p) => [e as i32, p as i32, -1],
            Target::Layer(e, p, l) => [e as i32, p as i32, l as i32],
            Target::Aux(a) => [-4, a as i32, -1],
            Target::Master => [-3, -1, -1],
        }
    }

    /// [re:2a670a]: the IdentifierLabel text.
    pub fn identity(self) -> String {
        match self {
            Target::Layer(e, p, l) => format!("E {}\nP {}\nL {}", e + 1, p + 1, l + 1),
            Target::Pad(e, p) => format!("E {}\nP {}", e + 1, p + 1),
            Target::Engine(e) => format!("E {}", e + 1),
            Target::Aux(a) => format!("A {}", a + 1),
            Target::Master => "Mst".into(),
        }
    }

    fn node(self, model: &Model) -> Option<&Node> {
        match self {
            Target::Engine(e) => model.engine_node(e),
            Target::Pad(e, p) => model.pad_node(e, p),
            Target::Layer(e, p, l) => model.layer_node(e, p, l),
            Target::Aux(a) => model.doc.children.iter().find(|c| c.class == "GlobalAux" && c.index() == Some(a as u32)),
            Target::Master => model.doc.children.iter().find(|c| c.class == "MasterChannel"),
        }
    }
}

/// The chain's current preset path.
fn chain_current(model: &Model, t: Target) -> Option<String> {
    t.node(model).and_then(|n| n.children.iter().find(|c| c.class == "EffectChain")).and_then(chain_name)
}

/// The document chain node of `t` (for saving).
pub fn chain_node(model: &Model, t: Target) -> Option<Node> {
    t.node(model).and_then(|n| n.children.iter().find(|c| c.class == "EffectChain")).cloned()
}

/// The effect node in `slot` of `t`.
pub fn effect_node(model: &Model, t: Target, slot: usize) -> Option<Node> {
    effects(model, t).into_iter().find(|(s, _, _)| *s == slot).map(|(_, _, n)| n)
}

/// The effects of a chain: (slot, type, node).
pub(crate) fn effects(model: &Model, t: Target) -> Vec<(usize, usize, Node)> {
    let Some(chain) = t.node(model).and_then(|n| n.children.iter().find(|c| c.class == "EffectChain")) else { return Vec::new() };
    let mut v: Vec<(usize, usize, Node)> = chain
        .children
        .iter()
        .filter(|c| c.class == "Effect")
        .filter_map(|c| {
            let slot = c.name.as_deref()?.strip_prefix("Effect")?.parse().ok()?;
            let kind = c.attr("EffectType").and_then(effect_type_index).unwrap_or(0);
            (kind != 0).then(|| (slot, kind, c.clone()))
        })
        .collect();
    v.sort_by_key(|x| x.0);
    v
}

pub struct FxPanel {
    host: Vec<usize>,
    chain_tpl: Option<Ctrl>,
    panel_tpl: Option<Ctrl>,
    empty_tpl: Option<Ctrl>,
    fx_skins: Vec<(usize, Ctrl)>,
    /// What is built: target and (slot, type) list.
    built: Option<(Target, Vec<(usize, usize)>)>,
    /// Open menu: the slot it acts on (None = add) and its item actions.
    menu: Option<(Option<usize>, Vec<Action>)>,
    resources: std::path::PathBuf,
    /// A save the editor should run (file dialog + write).
    pub save_request: Option<SaveRequest>,
    /// Chain presets found by the last folder scan and when (the prev /
    /// next buttons' state; panels resync on every edit).
    chain_presets: std::cell::Cell<Option<(usize, std::time::Instant)>>,
}

impl FxPanel {
    pub fn new(skin: &mut Skin, host: Vec<usize>) -> FxPanel {
        let load = |skin: &mut Skin, name: &str| -> Option<Ctrl> {
            let rel = skin.find(name)?;
            skin.load_skin(&rel).ok()?.children.into_iter().next()
        };
        let mut fx_skins = Vec::new();
        for k in 1..EFFECT_TYPES.len() {
            if let Some(c) = skin_for(k).and_then(|s| load(skin, s)) {
                fx_skins.push((k, c));
            }
        }
        FxPanel {
            host,
            chain_tpl: load(skin, "EffectChain.skin"),
            panel_tpl: load(skin, "EffectPanel.skin"),
            empty_tpl: load(skin, "EmptyEffect.skin"),
            fx_skins,
            built: None,
            menu: None,
            resources: dreist_formats::paths::skin_dir(),
            save_request: None,
            chain_presets: Default::default(),
        }
    }

    fn rebuild(&mut self, app: &mut App, t: Target, fx: &[(usize, usize, Node)]) {
        let sig: Vec<(usize, usize)> = fx.iter().map(|(s, k, _)| (*s, *k)).collect();
        if self.built.as_ref() == Some(&(t, sig.clone())) {
            return;
        }
        self.built = Some((t, sig));
        let Some(host) = app.ctrl_mut(&self.host) else { return };
        host.children.clear();
        let Some(mut chain) = self.chain_tpl.clone() else { return };
        chain.set_rect(Rect::new(0, 0, host.rect.w, host.rect.h));
        if let Some(sc) = chain.find_mut("FXScroller") {
            let h = sc.rect.h;
            let mut x = 0;
            // [re:6929fb] re-stacks its container when a panel folds.
            sc.set_attr("stackAxis", "h".into());
            for (slot, kind, node) in fx {
                let Some(mut panel) = self.panel_tpl.clone() else { continue };
                panel.display_name = dreist_formats::params::effect_label(*kind).to_uppercase();
                panel.set_attr("fxSlot", slot.to_string());
                panel.set_attr("fxKind", kind.to_string());
                let w = panel.rect.w;
                panel.set_rect(Rect::new(x, 0, w, h));
                // `UpdateToShowCurrentChain`: Effect `IsFolded` → `SetIconicState(true)`.
                if node.bool("IsFolded").unwrap_or(false) {
                    dreist_gui::collapse::set_iconic(&mut panel, true);
                    fold_controls(&mut panel, false);
                }
                let w = panel.rect.w;
                if let (Some(inner), Some((_, fxc))) = (panel.find_mut("EffectPanel"), self.fx_skins.iter().find(|(k, _)| k == kind)) {
                    let mut c = fxc.clone();
                    let (iw, ih) = (inner.rect.w, inner.rect.h);
                    c.set_rect(Rect::new(0, 0, iw, ih));
                    inner.children.push(c);
                }
                x += w;
                sc.children.push(panel);
            }
            if let Some(mut empty) = self.empty_tpl.clone() {
                let w = empty.rect.w;
                empty.set_rect(Rect::new(x, 0, w, h));
                sc.children.push(empty);
            }
        }
        host.children.push(chain);
    }

    /// How many chain presets exist (folders rescanned at most every two
    /// seconds, or after [`FxPanel::forget_presets`]).
    fn chain_preset_count(&self) -> usize {
        match self.chain_presets.get() {
            Some((n, at)) if at.elapsed() < std::time::Duration::from_secs(2) => n,
            _ => {
                let n = chain_presets(&self.resources).len();
                self.chain_presets.set(Some((n, std::time::Instant::now())));
                n
            }
        }
    }

    /// A preset was saved: count again.
    pub fn forget_presets(&self) {
        self.chain_presets.set(None);
    }

    pub fn sync(&mut self, app: &mut App, model: &Model, t: Target) {
        let fx = effects(model, t);
        self.rebuild(app, t, &fx);
        // Chain header: Active / Mix, and `UpdateNextPreviousButtons`
        // [re:100883] — prev / next enabled with two or more presets while the
        // chain is active.
        let chain = chain_node(model, t);
        let chain_active = chain.as_ref().and_then(|c| c.attr("IsActive")).map(str::trim) != Some("false");
        let chain_mix = chain.as_ref().and_then(|c| c.f32("ChainMix")).unwrap_or(1.0).clamp(0.0, 1.0);
        let stepping = chain_active && self.chain_preset_count() >= 2;
        let Some(host) = app.ctrl_mut(&self.host) else { return };
        fn header(c: &mut Ctrl, f: &mut dyn FnMut(&mut Ctrl)) {
            for k in c.children.iter_mut() {
                if k.attr("fxSlot").is_some() {
                    continue;
                }
                f(k);
                header(k, f);
            }
        }
        let identity = t.identity();
        header(host, &mut |k: &mut Ctrl| match k.name.as_str() {
            "IdentifierLabel" => {
                k.text = identity.clone();
                k.passive = true;
            }
            "ActiveButton" => k.on = chain_active,
            "MixKnob" => k.value = chain_mix,
            "PresetPrev" | "PresetNext" => k.enabled = stepping,
            _ => {}
        });
        host.walk_mut(&mut |c: &mut Ctrl| {
            let (Some(slot), Some(kind)) = (c.attr("fxSlot").and_then(|v| v.parse::<usize>().ok()), c.attr("fxKind").and_then(|v| v.parse::<usize>().ok())) else { return };
            let Some((_, _, node)) = fx.iter().find(|(s, _, _)| *s == slot) else { return };
            let names = effect_param_names(kind);
            let get = |k: &str, d: f32| node.f32(k).unwrap_or(d).clamp(0.0, 1.0);
            let active = node.attr("IsActive").map(str::trim) != Some("false");
            c.walk_mut(&mut |k: &mut Ctrl| {
                if let Some(n) = k.name.strip_prefix("Control").and_then(|n| n.parse::<usize>().ok()) {
                    if let Some(pn) = names.get(n.wrapping_sub(1)) {
                        let key: String = pn.chars().filter(|ch| !ch.is_whitespace()).collect();
                        let v = get(&key, 0.5);
                        if k.class == "CtrlPopupButton" {
                            // `SelectItemWithNormalizedFloat`: round(v·(n − 1)).
                            if let Some(items) = effect_option_strings(kind, n - 1) {
                                let i = ((v * (items.len() - 1) as f32).round() as usize).min(items.len() - 1);
                                k.text = items[i].to_string();
                            }
                        } else if k.class == "CtrlButton" {
                            k.on = v >= 0.5;
                        } else {
                            k.value = v;
                        }
                    }
                    return;
                }
                match k.name.as_str() {
                    "TrimKnob" => k.value = get("Trim", 1.0),
                    "MixKnob" => k.value = get("Mix", 1.0),
                    "OutputKnob" => k.value = get("Level", 1.0),
                    "ActiveButton" => k.on = active,
                    _ => {}
                }
            });
        });
    }

    /// The (slot, kind) of the effect panel containing `path`.
    /// The control at `path` is inside one of this panel's effects.
    pub fn contains(&self, app: &App, path: &[usize]) -> bool {
        path.starts_with(&self.host) && self.slot_of(app, path).is_some()
    }

    /// "Remove all modulation from this control" ([re:5d3162],
    /// [re:de6d9f]): the value 0 for sources 1..=28.
    pub fn remove_modulation(&self, app: &App, model: &Model, path: &[usize]) -> Vec<Command> {
        let Some(c) = app.ctrl(path) else { return Vec::new() };
        if c.class.ends_with("Button") || !self.contains(app, path) {
            return Vec::new();
        }
        let Some((slot, _)) = self.slot_of(app, path) else { return Vec::new() };
        let Some((t, _)) = self.built.as_ref() else { return Vec::new() };
        let _ = model;
        let [e, p, l] = t.index();
        let index = [e, p, l, slot as i32];
        let name = c.name.clone();
        (1..=28)
            .filter_map(|s| {
                let s = s as f32;
                if let Some(n) = name.strip_prefix("Control").and_then(|n| n.parse::<usize>().ok()) {
                    return Some(msg(id::SET_FX_PARAMETER, &index, &[(n - 1) as f32, s, 0.0]));
                }
                let kind = match name.as_str() {
                    "TrimKnob" => id::SET_FX_TRIM,
                    "MixKnob" => id::SET_FX_MIX,
                    "OutputKnob" => id::SET_FX_LEVEL,
                    _ => return None,
                };
                Some(msg(kind, &index, &[0.0, 0.0, s]))
            })
            .collect()
    }

    /// Sources modulating the effect parameter knob at `path`
    /// ([re:2f9e16] $_5, [re:a50db4]).
    pub fn modulation_mask(&self, app: &App, model: &Model, path: &[usize]) -> Option<u32> {
        if !path.starts_with(&self.host) {
            return None;
        }
        let (slot, kind) = self.slot_of(app, path)?;
        let n = app.ctrl(path)?.name.strip_prefix("Control")?.parse::<usize>().ok()?;
        let key: String = effect_param_names(kind).get(n.checked_sub(1)?)?.chars().filter(|c| !c.is_whitespace()).collect();
        let (t, _) = self.built.as_ref()?;
        let node = effect_node(model, *t, slot);
        Some(crate::model::modulation_mask(node.as_ref(), &key))
    }

    /// An effect parameter knob's tooltip ([re:947faf]
    ///  [re:9c30fa]): "<param>: <Effect::GetStringParameter>"
    /// and, with a transmod source shown and a non-zero depth, " -> " and
    /// the modulated value.
    pub fn tooltip(&self, app: &App, model: &Model, path: &[usize], source: Option<usize>) -> Option<String> {
        let (slot, kind) = self.slot_of(app, path)?;
        let c = app.ctrl(path)?;
        if !c.class.contains("Knob") && !c.class.contains("Slider") {
            return None;
        }
        let n = c.name.strip_prefix("Control")?.parse::<usize>().ok()?.checked_sub(1)?;
        let name = effect_param_names(kind).get(n)?;
        let key: String = name.chars().filter(|c| !c.is_whitespace()).collect();
        let (t, _) = self.built.as_ref()?;
        let node = effect_node(model, *t, slot);
        let sync = node.as_ref().and_then(|e| e.f32("Sync")).is_none_or(|v| v >= 0.5);
        let mut text = format!("{name}: {}", crate::fxtext::text(kind, n, c.value, sync)?);
        if let Some(s) = source {
            let d = node.as_ref().and_then(|e| dreist_engine::engine::doc::attr_name(&key, s).and_then(|a| e.f32(&a))).unwrap_or(0.0);
            if d.abs() >= 1e-6 {
                text.push_str(" -> ");
                text.push_str(&crate::fxtext::text(kind, n, (c.value + d).clamp(0.0, 1.0), sync)?);
            }
        }
        Some(text)
    }

    /// A knob double click's reset value: [re:375f19] $_9
    /// [re:759587] (Mix 0.5, Trim / Output 1.0) and [re:2f9e16]
    /// $_3 [re:6a5c33] (the value the type's default preset gave the
    /// parameter, [re:1cda27] → +0x744).
    pub fn reset_value(&self, app: &App, path: &[usize], name: &str) -> Option<f32> {
        if !path.starts_with(&self.host) {
            return None;
        }
        let (_, kind) = self.slot_of(app, path)?;
        match name {
            "MixKnob" => Some(0.5),
            "TrimKnob" | "OutputKnob" => Some(1.0),
            _ => {
                let n = name.strip_prefix("Control")?.parse::<usize>().ok()?;
                let key: String = effect_param_names(kind).get(n.checked_sub(1)?)?.chars().filter(|c| !c.is_whitespace()).collect();
                Some(new_effect(&self.resources, kind).f32(&key).unwrap_or(0.5).clamp(0.0, 1.0))
            }
        }
    }

    fn slot_of(&self, app: &App, path: &[usize]) -> Option<(usize, usize)> {
        if !path.starts_with(&self.host) {
            return None;
        }
        (self.host.len()..=path.len()).rev().find_map(|n| {
            let c = app.ctrl(&path[..n])?;
            Some((c.attr("fxSlot")?.parse().ok()?, c.attr("fxKind")?.parse().ok()?))
        })
    }

    pub(crate) fn chain_mut(model: &mut Model, t: Target) -> &mut Node {
        let n = match t {
            Target::Engine(e) => model.engine_mut(e),
            Target::Pad(e, p) => model.pad_mut(e, p),
            Target::Layer(e, p, l) => model.layer_mut(e, p, l),
            Target::Aux(a) => model.doc.child_mut_or_insert("GlobalAux", &format!("GlobalAux{a}")),
            Target::Master => model.doc.child_mut_or_insert("MasterChannel", "MasterChannel"),
        };
        if !n.children.iter().any(|c| c.class == "EffectChain") {
            let mut c = Node::new("EffectChain", Some("EffectChain"));
            c.version = Some(2100);
            n.children.push(c);
        }
        n.child_mut_or_insert("EffectChain", "EffectChain")
    }

    fn open_menu(&mut self, app: &mut App, path: &[usize], slot: Option<usize>, items: Vec<(String, Action)>) {
        let (labels, actions): (Vec<String>, Vec<Action>) = items.into_iter().unzip();
        app.open_menu(path, labels, None);
        self.menu = Some((slot, actions));
    }

    /// The chain menu: Save preset, Clear chain, then the user and the
    /// system presets (folders as submenus — the menu is tokenised on '/').
    fn chain_menu(&self, current: Option<&str>) -> Vec<(String, Action)> {
        let mut v = vec![("Save preset".to_string(), Action::ChainSave), ("Clear chain".to_string(), Action::ChainClear)];
        let (u, sys) = chain_dirs(&self.resources);
        for d in [u, sys].into_iter().flatten() {
            let items = self.chain_menu_dir(&d, current);
            if !items.is_empty() {
                v.push((String::new(), Action::None));
                v.extend(items);
            }
        }
        v
    }

    fn chain_menu_dir(&self, dir: &std::path::Path, current: Option<&str>) -> Vec<(String, Action)> {
        let mut v: Vec<std::path::PathBuf> =
            std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        v.sort();
        v.into_iter()
            .filter_map(|p| {
                let name = p.file_stem()?.to_string_lossy().into_owned();
                if p.is_dir() {
                    Some((format!("{} ▸", p.file_name()?.to_string_lossy()), Action::ChainFolder(p)))
                } else if p.extension().is_some_and(|x| x == "fxchain") {
                    let mark = if current == Some(&*p.to_string_lossy()) { "✓ " } else { "" };
                    Some((format!("{mark}{name}"), Action::ChainLoad(p)))
                } else {
                    None
                }
            })
            .collect()
    }

    fn categories() -> Vec<(String, Action)> {
        CATEGORIES.iter().enumerate().map(|(i, c)| (format!("{c} ▸"), Action::Category(i))).collect()
    }

    /// `DoShowEffectTypeMenu` for an existing effect.
    fn type_menu(&self, kind: usize, current: Option<&str>) -> Vec<(String, Action)> {
        let mut v = vec![
            ("Save preset".to_string(), Action::SavePreset),
            ("Save as default".to_string(), Action::SaveDefault),
            ("Revert to default".to_string(), Action::Revert),
            (String::new(), Action::None),
        ];
        let ps = presets(&self.resources, kind);
        for p in &ps {
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mark = if current == Some(name.as_str()) { "✓ " } else { "" };
            v.push((format!("{mark}{name}"), Action::Preset(p.clone())));
        }
        if !ps.is_empty() {
            v.push((String::new(), Action::None));
        }
        v.push(("Remove effect".into(), Action::Remove));
        v.push((String::new(), Action::None));
        v.extend(Self::categories());
        v
    }

    fn apply(&mut self, model: &mut Model, t: Target, slot: Option<usize>, action: &Action, out: &mut Vec<Command>) {
        let res = self.resources.clone();
        let chain = Self::chain_mut(model, t);
        let find = |chain: &Node, s: usize| chain.children.iter().position(|c| c.class == "Effect" && c.name.as_deref() == Some(&format!("Effect{s}")));
        let kind_of = |chain: &Node, s: usize| find(chain, s).and_then(|i| chain.children[i].attr("EffectType")).and_then(effect_type_index).unwrap_or(0);
        let put = |chain: &mut Node, s: usize, mut n: Node| {
            n.name = Some(format!("Effect{s}"));
            n.version = Some(2);
            match find(chain, s) {
                Some(i) => chain.children[i] = n,
                None => chain.children.push(n),
            }
        };
        match (action, slot) {
            (Action::Type(k), Some(s)) => put(chain, s, new_effect(&res, *k)),
            (Action::Type(k), None) => {
                let s = chain.children.iter().filter(|c| c.class == "Effect").filter_map(|c| c.name.as_deref()?.strip_prefix("Effect")?.parse::<usize>().ok()).max().map_or(0, |m| m + 1);
                put(chain, s, new_effect(&res, *k));
            }
            (Action::Revert, Some(s)) => {
                let k = kind_of(chain, s);
                put(chain, s, new_effect(&res, k));
            }
            (Action::Preset(path), Some(s)) => {
                let k = kind_of(chain, s);
                if let Some(n) = load_preset(path, k) {
                    put(chain, s, n);
                }
            }
            (Action::Remove, Some(s)) => {
                if let Some(i) = find(chain, s) {
                    chain.children.remove(i);
                }
            }
            (Action::EffectCopy | Action::EffectCut, Some(s)) => {
                let Some(i) = find(chain, s) else { return };
                if let Ok(mut c) = EFFECT_CLIPBOARD.lock() {
                    *c = Some(chain.children[i].clone());
                }
                if *action == Action::EffectCopy {
                    return;
                }
                chain.children.remove(i);
            }
            (Action::EffectPaste, slot) => {
                let Some(n) = effect_clipboard() else { return };
                let s = slot.unwrap_or_else(|| {
                    chain.children.iter().filter(|c| c.class == "Effect").filter_map(|c| c.name.as_deref()?.strip_prefix("Effect")?.parse::<usize>().ok()).max().map_or(0, |m| m + 1)
                });
                put(chain, s, n);
            }
            (Action::SavePreset, Some(s)) => {
                self.save_request = Some(SaveRequest::Effect(t, s, kind_of(chain, s)));
                return;
            }
            // Save as default: the type's user `Default.fxpreset`.
            (Action::SaveDefault, Some(s)) => {
                let k = kind_of(chain, s);
                if let (Some(i), Some(d)) = (find(chain, s), preset_dirs(&res, k).into_iter().next()) {
                    let mut n = chain.children[i].clone();
                    n.name = Some("Effect".into());
                    n.children.retain(|c| c.name.as_deref() != Some("PresetName"));
                    let mut pn = Node::new("FixedString", Some("PresetName"));
                    pn.set("string", "Default");
                    n.children.push(pn);
                    let _ = save_preset_file(&d.join("Default.fxpreset"), &n);
                }
                return;
            }
            (Action::ChainSave, _) => {
                self.save_request = Some(SaveRequest::Chain(t));
                return;
            }
            // `DoClearChain`: an empty chain.
            (Action::ChainClear, _) => {
                chain.children.retain(|c| c.class != "Effect" && c.name.as_deref() != Some("ChainPresetName"));
            }
            (Action::ChainLoad(path), _) => {
                let Some(n) = load_chain(path) else { return };
                let version = chain.version;
                *chain = n;
                chain.version = version.or(Some(2100));
            }
            _ => return,
        }
        out.push(model.rebuild_command());
    }

    /// Step through the type's presets (`DoLoadNextPreset` /
    /// `DoLoadPreviousPreset`).
    fn step_preset(&mut self, model: &mut Model, t: Target, slot: usize, kind: usize, dir: i32, out: &mut Vec<Command>) {
        let ps = presets(&self.resources, kind);
        if ps.is_empty() {
            return;
        }
        let current = effects(model, t)
            .into_iter()
            .find(|(s, _, _)| *s == slot)
            .and_then(|(_, _, n)| n.children.iter().find(|c| c.name.as_deref() == Some("PresetName")).and_then(|c| c.attr("string")).map(str::to_string));
        let i = current.and_then(|c| ps.iter().position(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == c)));
        let next = match i {
            Some(i) => (i as i32 + dir).rem_euclid(ps.len() as i32) as usize,
            None => 0,
        };
        self.apply(model, t, Some(slot), &Action::Preset(ps[next].clone()), out);
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        let Some((t, _)) = self.built.clone() else { return false };
        let [e, p, l] = t.index();
        match ev {
            UiEvent::MenuSelected { anchor, index, .. } if anchor.starts_with(&self.host) => {
                let Some((slot, actions)) = self.menu.take() else { return false };
                let Some(action) = actions.get(*index).cloned() else { return true };
                if let Action::Option(param, value) = action {
                    let index = [e, p, l, slot.unwrap_or(0) as i32];
                    out.push(msg(id::SET_FX_PARAMETER, &index, &[param as f32, 0.0, value]));
                    return true;
                }
                if let Action::ChainFolder(dir) = &action {
                    let items = self.chain_menu_dir(dir, chain_current(model, t).as_deref());
                    self.open_menu(app, anchor, slot, items);
                    return true;
                }
                if let Action::Category(c) = action {
                    let items = (dreist_formats::profile::get().effect_menu)(c).into_iter().map(|k| (dreist_formats::params::effect_label(k).to_string(), Action::Type(k))).collect();
                    self.open_menu(app, anchor, slot, items);
                } else {
                    self.apply(model, t, slot, &action, out);
                }
                return true;
            }
            // Right press on an effect panel's background: Cut / Copy /
            // Paste ([re:a86ca5]; not over
            // its controls); on an empty slot's Add button: Paste
            // ([re:57c692] $_1 [re:4bab0b]).
            UiEvent::RightClicked { path, name } if path.starts_with(&self.host) => {
                let Some(c) = app.ctrl(path) else { return false };
                let paste = dreist_gui::menu::item("Paste", effect_clipboard().is_some());
                if name == "AddButton" && c.class == "CtrlButton" {
                    self.open_menu(app, path, None, vec![(paste, Action::EffectPaste)]);
                    return true;
                }
                let interactive = matches!(c.class.as_str(), "CtrlButton" | "CtrlKnob" | "CtrlPopupButton" | "CtrlTransmodKnob" | "CtrlPolyPlot");
                let Some((slot, _)) = self.slot_of(app, path).filter(|_| !interactive) else { return false };
                self.open_menu(app, path, Some(slot), vec![("Cut".into(), Action::EffectCut), ("Copy".into(), Action::EffectCopy), (paste, Action::EffectPaste)]);
                return true;
            }
            // Option parameters ([re:0e72ae]:
            // popup items from `GetParameterOptionStrings`; lambda $_0
            // [re:f8185c] sends `FloatValue` = item/(count − 1)).
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.host) && name.starts_with("Control") && app.ctrl(path).is_some_and(|c| c.class == "CtrlPopupButton") => {
                let (Some((slot, kind)), Some(n)) = (self.slot_of(app, path), name.strip_prefix("Control").and_then(|n| n.parse::<usize>().ok())) else { return false };
                let Some(items) = effect_option_strings(kind, n - 1) else { return true };
                let last = (items.len() - 1).max(1) as f32;
                let list = items.iter().enumerate().map(|(i, s)| (s.to_string(), Action::Option(n - 1, i as f32 / last))).collect();
                self.open_menu(app, path, Some(slot), list);
                return true;
            }
            // Chain preset menu and stepping ([re:e04aee]
            // $_3 / $_4 / $_5).
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.host) && matches!(name.as_str(), "PresetCombo" | "PresetPrev" | "PresetNext") => {
                let current = chain_current(model, t);
                if name == "PresetCombo" {
                    let items = self.chain_menu(current.as_deref());
                    self.open_menu(app, path, None, items);
                    return true;
                }
                let ps = chain_presets(&self.resources);
                if ps.is_empty() {
                    return true;
                }
                let i = current.and_then(|c| ps.iter().position(|p| p.to_string_lossy() == c));
                let next = match (i, name == "PresetNext") {
                    (Some(i), true) => (i + 1) % ps.len(),
                    (Some(i), false) => (i + ps.len() - 1) % ps.len(),
                    (None, _) => 0,
                };
                self.apply(model, t, None, &Action::ChainLoad(ps[next].clone()), out);
                return true;
            }
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.host) && name == "AddButton" => {
                self.open_menu(app, path, None, Self::categories());
                return true;
            }
            UiEvent::Clicked { path, name, .. } if matches!(name.as_str(), "EffectTypeCombo" | "LastPresetButton" | "NextPresetButton") => {
                let Some((slot, kind)) = self.slot_of(app, path) else { return false };
                match name.as_str() {
                    "EffectTypeCombo" => {
                        let current = effects(model, t)
                            .into_iter()
                            .find(|(s, _, _)| *s == slot)
                            .and_then(|(_, _, n)| n.children.iter().find(|c| c.name.as_deref() == Some("PresetName")).and_then(|c| c.attr("string")).map(str::to_string));
                        let items = self.type_menu(kind, current.as_deref());
                        self.open_menu(app, path, Some(slot), items);
                    }
                    "LastPresetButton" => self.step_preset(model, t, slot, kind, -1, out),
                    _ => self.step_preset(model, t, slot, kind, 1, out),
                }
                return true;
            }
            // The chain header's own Active button / Mix knob ([re:6929fb]
            // $_6 / $_7): 0x60 / 0x62 [e, p, l] [value].
            UiEvent::Clicked { path, name, on, .. } if name == "ActiveButton" && path.starts_with(&self.host) && self.slot_of(app, path).is_none() => {
                out.push(msg(id::SET_FX_CHAIN_ACTIVE, &[e, p, l], &[*on as i32 as f32]));
                return true;
            }
            UiEvent::ValueChanged { path, name, value, .. } if name == "MixKnob" && path.starts_with(&self.host) && self.slot_of(app, path).is_none() => {
                out.push(msg(id::SET_FX_CHAIN_MIX, &[e, p, l], &[value.clamp(0.0, 1.0)]));
                return true;
            }
            _ => {}
        }
        match ev {
            UiEvent::ValueChanged { path, name, value, .. } => {
                let Some((slot, _)) = self.slot_of(app, path) else { return false };
                let index = [e, p, l, slot as i32];
                if let Some(n) = name.strip_prefix("Control").and_then(|n| n.parse::<usize>().ok()) {
                    out.push(msg(id::SET_FX_PARAMETER, &index, &[(n - 1) as f32, 0.0, *value]));
                    return true;
                }
                let kind = match name.as_str() {
                    "TrimKnob" => id::SET_FX_TRIM,
                    "MixKnob" => id::SET_FX_MIX,
                    "OutputKnob" => id::SET_FX_LEVEL,
                    _ => return false,
                };
                out.push(msg(kind, &index, &[0.0, *value, 0.0]));
                true
            }
            UiEvent::Clicked { path, name, on, .. } => {
                let Some((slot, _)) = self.slot_of(app, path) else { return false };
                let index = [e, p, l, slot as i32];
                if let Some(n) = name.strip_prefix("Control").and_then(|n| n.parse::<usize>().ok()) {
                    out.push(msg(id::SET_FX_PARAMETER, &index, &[(n - 1) as f32, 0.0, if *on { 1.0 } else { 0.0 }]));
                    return true;
                }
                if name == "ActiveButton" {
                    out.push(msg(id::SET_FX_ACTIVE, &index, &[0.0, *on as i32 as f32]));
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

/// Show / hide the controls an effect panel folds away ([re:375f19]
/// $_19 / $_20): Active, Next / Last preset and the knob labeller.
pub fn fold_controls(panel: &mut Ctrl, show: bool) {
    fn walk(c: &mut Ctrl, show: bool) {
        for k in &mut c.children {
            if matches!(k.name.as_str(), "ActiveButton" | "LastPresetButton" | "NextPresetButton") || k.children.iter().any(|g| g.name == "OutputKnob") {
                k.visible = show;
                k.enabled = show;
            }
            if k.attr("fxSlot").is_none() && k.name != "EffectPanel" {
                walk(k, show);
            }
        }
    }
    walk(panel, show);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_presets() {
        assert_eq!(CATEGORIES[category(effect_type_index("Bit Crusher").unwrap())], "Distortion");
        assert_eq!(CATEGORIES[category(effect_type_index("Breverb Hall").unwrap())], "Reverbs");
        assert_eq!(CATEGORIES[category(effect_type_index("Comb Filter").unwrap())], "Filters");
        let res = dreist_formats::paths::skin_dir();
        let k = effect_type_index("Bit Crusher").unwrap();
        let ps = presets(&res, k);
        if ps.is_empty() {
            return;
        }
        let n = load_preset(&ps[0], k).unwrap();
        assert_eq!(n.attr("EffectType"), Some("Bit Crusher"));
        assert!(n.attrs.iter().all(|(a, _)| !a.starts_with("Parameter")), "{:?}", n.attrs);
    }

    #[test]
    fn chain_presets_load_and_save() {
        let res = dreist_formats::paths::skin_dir();
        let ps = chain_presets(&res);
        let Some(p) = ps.iter().find(|p| p.ends_with("Auxiliary Sends/Delays/Giant Delayed Hall.fxchain")) else { return };
        let c = load_chain(p).unwrap();
        let fx: Vec<&Node> = c.children.iter().filter(|n| n.class == "Effect").collect();
        assert_eq!(fx.len(), 3);
        // Delay has six named parameters (Gain has fewer: its unused
        // Parameter<n> stay, harmless).
        assert!(fx[1].attrs.iter().all(|(a, _)| !a.starts_with("Parameter")), "{:?}", fx[1].attrs);
        assert_eq!(chain_name(&c).as_deref(), Some(&*p.to_string_lossy()));
        let dir = std::env::temp_dir().join(format!("dreist-chain-{}", std::process::id()));
        let out = dir.join("x.fxchain");
        save_preset_file(&out, &c).unwrap();
        let back = load_chain(&out).unwrap();
        assert_eq!(back.children.iter().filter(|n| n.class == "Effect").count(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }
}
