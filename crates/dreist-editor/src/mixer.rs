//! Mixer tabs (`Mixer.skin`, [re:faf56f]): channel strips
//! (`MixerChannel.skin`, `AuxChannel.skin`, [re:b3ce4d]) in the
//! `ChannelScroller`, with the Mix / Sends / Effects mode group.
//!
//! - Global mixer ([re:51d49b]): 8 engines, 4 global aux returns, master.
//! - Pad mixer ([re:d84457]): the visible pads of the selected engine.
//! - Layer mixer ([re:de6382]): the 8 layers of the selected pad.
//!
//! Selection and the shown effect chain (ev_37b25110…, ev_06897cf5…,
//! ev_975c5a77…, ev_550d1ba0…, ev_705113971…, ev_e8192a36…):
//! - A press on a strip outside its controls ([re:fcb381]
//!   $_2 [re:42d973], aux $_10 [re:4a3270]) selects it: an
//!   engine selects that engine (the global mixer's $_0 [re:1068c4]), a pad goes
//!   through the pad selection (cmd toggles, shift extends), a layer becomes
//!   the current layer; an aux or the master strip ($_1 [re:070ea3] / $_2
//!   [re:d1e5bd]) shows its own chain (`SetIdentity` 3 / 4) until an engine is
//!   picked again (+0x3b8).
//! - The NameLabel takes `MixerChannelEditTitleSelectedStyle` on the chain's
//!   strip, `…SubSelStyle` on the other selected pads, else
//!   `MixerChannelEditTitleStyle` ([re:ba4a66]
//!   [re:d09042], [re:f5104b]).
//! - EffectViewGroup ("Show FX", [re:faf56f] $_1 →
//!   `UpdateVisibleEffect`): items E1–E8, A1–A4, M (global), 1–64 (pads),
//!   1–8 (layers); picking one selects that channel.
//! - Layout ([re:faf56f] $_2 [re:50c976]): up to 384 px tall the
//!   chain and the strips share the space — Effects (mode 2) shows the
//!   chain right of the mode buttons with the Show FX popup, Mix / Sends
//!   the strips; taller, the chain sits 200 px high on top, the strips
//!   below, Effects is disabled (and left for Mix) and the popup hidden.
//!
//! Messages ([re:b8e0fd]): engine/pad parameter
//! value at v0 with the parameter at v1 and source at v2; layer parameter
//! index at v0 and value at v1; send levels with the aux at v0, level at v1;
//! aux 0x9f and master 0xa5/0xa6 gains at v0.

use dreist_engine::binding::id;
use dreist_formats::{EngineParam, LayerParam, Node, PadParam};
use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::{ENGINES, LAYERS, Model};
use crate::{EditorError, err};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Global,
    Pad,
    Layer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Channel {
    Engine(usize),
    Aux(usize),
    Master,
    Pad(usize),
    Layer(usize),
}

/// OutputCombo items (`ConnectControls` [re:1123ca]: the 38
/// [re:0dccd3] names — subs in
/// the short "S n" form).
pub fn output_items() -> Vec<String> {
    (0..38)
        .map(|i| match i {
            0 => "Parent".to_string(),
            1 => "Master".to_string(),
            2..=33 => format!("S {}", i - 1),
            _ => format!("Aux {}", i - 33),
        })
        .collect()
}

/// Item index of a stored `OutputLocation` name (unknown → Parent).
fn output_index(name: &str) -> usize {
    let num = |p: &str| name.strip_prefix(p).and_then(|n| n.trim().parse::<usize>().ok());
    match name {
        "Master" => 1,
        _ => match (num("Sub output "), num("Aux ")) {
            (Some(k @ 1..=32), _) => k + 1,
            (_, Some(k @ 1..=4)) => k + 33,
            _ => 0,
        },
    }
}

/// Output locations used anywhere in the document (marked in the menu, the
/// original's `GetBlockImage` for used outputs above Master).
fn used_outputs(doc: &Node) -> std::collections::BTreeSet<usize> {
    fn walk(n: &Node, out: &mut std::collections::BTreeSet<usize>) {
        if let Some(o) = n.attr("OutputLocation") {
            out.insert(output_index(o));
        }
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut s = std::collections::BTreeSet::new();
    walk(doc, &mut s);
    s
}

pub struct Mixer {
    pub(crate) kind: Kind,
    pub(crate) page: Vec<usize>,
    scroller: Option<Vec<usize>>,
    /// Strip templates per size (normal, Mid, Large).
    strip: [Option<Ctrl>; 3],
    aux: [Option<Ctrl>; 3],
    master: [Option<Ctrl>; 3],
    /// Mix (0), Sends (1), Effects (2).
    mode: usize,
    shown: Vec<Channel>,
    /// Horizontal scroll of the channel strips (px).
    scroll_x: i32,
    /// Channel whose OutputCombo menu is open.
    output_menu: Option<Channel>,
    /// The output selector's right-click menu is open (its strip kind).
    route_all_menu: Option<Channel>,
    fx: Option<crate::fxchain::FxPanel>,
    /// Global mixer: an aux or the master shows its chain ([re:51d49b]
    /// +0x3b8).
    bus_view: Option<Channel>,
    /// The Show FX popup is open.
    view_menu: bool,
}

const TITLE: &str = "MixerChannelEditTitleStyle";
const TITLE_SELECTED: &str = "MixerChannelEditTitleSelectedStyle";
const TITLE_SUB: &str = "MixerChannelEditTitleSubSelStyle";

fn attr_f32(n: Option<&dreist_formats::Node>, k: &str, d: f32) -> f32 {
    n.and_then(|n| n.f32(k)).unwrap_or(d).clamp(0.0, 1.0)
}

impl Mixer {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize], kind: Kind) -> Result<Mixer, EditorError> {
        let mut host = &mut *root;
        for &i in page {
            host = &mut host.children[i];
        }
        skin.mount(host, "Mixer.skin").map_err(err)?;
        let load = |skin: &mut Skin, name: &str| -> Option<Ctrl> {
            let rel = skin.find(name)?;
            skin.load_skin(&rel).ok()?.children.into_iter().next()
        };
        let variants = |skin: &mut Skin, base: &str| [load(skin, &format!("{base}.skin")), load(skin, &format!("{base}Mid.skin")), load(skin, &format!("{base}Large.skin"))];
        let strip = variants(skin, "MixerChannel");
        let aux = variants(skin, "AuxChannel");
        let master = variants(skin, "MasterChannel");
        let mut scroller = None;
        let mut fx_host = None;
        {
            fn find(c: &Ctrl, name: &str, path: &mut Vec<usize>) -> bool {
                for (i, ch) in c.children.iter().enumerate() {
                    path.push(i);
                    if ch.name == name || find(ch, name, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
            let mut p = page.to_vec();
            if find(host, "ChannelScroller", &mut p) {
                scroller = Some(p);
            }
            if let Some(sc) = host.find_mut("ChannelScroller") {
                sc.overload = "MixerScroller".into();
            }
            let mut p = page.to_vec();
            if find(host, "EffectPanel", &mut p) {
                fx_host = Some(p);
            }
        }
        let fx = fx_host.map(|h| crate::fxchain::FxPanel::new(skin, h));
        Ok(Mixer { kind, page: page.to_vec(), scroller, strip, aux, master, mode: 0, shown: Vec::new(), scroll_x: 0, output_menu: None, route_all_menu: None, fx, bus_view: None, view_menu: false })
    }

    fn channels(&self, model: &Model) -> Vec<Channel> {
        match self.kind {
            Kind::Global => (0..ENGINES).map(Channel::Engine).chain((0..4).map(Channel::Aux)).chain(std::iter::once(Channel::Master)).collect(),
            Kind::Pad => {
                let (first, count) = model.visible_pads();
                (first..first + count.min(16)).map(Channel::Pad).collect()
            }
            Kind::Layer => (0..LAYERS).map(Channel::Layer).collect(),
        }
    }

    /// [re:6d6a89]: strips under
    /// 275 px tall use the normal layout, under 380 px the Mid skin's, else
    /// the Large skin's.
    fn size_index(h: i32) -> usize {
        if h < 0x113 {
            0
        } else if h < 0x17c {
            1
        } else {
            2
        }
    }

    fn rebuild(&mut self, app: &mut App, model: &Model) {
        let chans = self.channels(model);
        let Some(sp) = self.scroller.clone() else { return };
        let Some(sc) = app.ctrl_mut(&sp) else { return };
        let h = sc.rect.h;
        let size = Self::size_index(h);
        let rebuilt = chans != self.shown || sc.attr("sizeIndex") != Some(&size.to_string());
        if rebuilt {
            self.shown = chans.clone();
            sc.set_attr("sizeIndex", size.to_string());
            sc.children.clear();
            for ch in chans {
                let set = match ch {
                    Channel::Aux(_) => &self.aux,
                    Channel::Master => &self.master,
                    _ => &self.strip,
                };
                let Some(t) = set[size].as_ref().or(set[0].as_ref()) else { continue };
                let mut s = t.clone();
                let w = s.rect.w;
                s.set_rect(Rect::new(0, 0, w, h));
                s.set_attr("channel", format!("{ch:?}"));
                sc.children.push(s);
            }
        }
        // Lay the strips out from the scroll offset.
        let total: i32 = sc.children.iter().map(|c| c.rect.w + 2).sum();
        self.scroll_x = self.scroll_x.clamp(0, (total - sc.rect.w).max(0));
        let mut x = -self.scroll_x;
        for c in &mut sc.children {
            if c.rect.x != x {
                let r = Rect::new(x, 0, c.rect.w, c.rect.h);
                c.set_rect(r);
            }
            x += c.rect.w + 2;
        }
    }

    /// A preset save the FX chain asked for, with the resources folder.
    /// A chain preset was saved somewhere: the panel counts presets again.
    pub fn forget_presets(&self) {
        if let Some(fx) = &self.fx {
            fx.forget_presets();
        }
    }

    pub fn take_save_request(&mut self) -> Option<crate::fxchain::SaveRequest> {
        self.fx.as_mut()?.save_request.take()
    }

    fn fx_target(&self, model: &Model) -> crate::fxchain::Target {
        let e = model.engine;
        match self.kind {
            Kind::Global => match self.bus_view {
                Some(Channel::Aux(k)) => crate::fxchain::Target::Aux(k),
                Some(Channel::Master) => crate::fxchain::Target::Master,
                _ => crate::fxchain::Target::Engine(e),
            },
            Kind::Pad => crate::fxchain::Target::Pad(e, model.pad[e]),
            Kind::Layer => crate::fxchain::Target::Layer(e, model.pad[e], model.layer[e]),
        }
    }

    /// Show FX items (`ConstructMixerChannels`).
    fn view_items(&self) -> Vec<String> {
        match self.kind {
            Kind::Global => (1..=ENGINES).map(|k| format!("E{k}")).chain((1..=4).map(|k| format!("A{k}"))).chain(std::iter::once("M".to_string())).collect(),
            Kind::Pad => (1..=64).map(|k| k.to_string()).collect(),
            Kind::Layer => (1..=LAYERS).map(|k| k.to_string()).collect(),
        }
    }

    /// The Show FX item of the shown chain.
    fn view_index(&self, model: &Model) -> usize {
        let e = model.engine;
        match (self.kind, self.bus_view) {
            (Kind::Global, Some(Channel::Aux(k))) => 8 + k,
            (Kind::Global, Some(Channel::Master)) => 12,
            (Kind::Global, _) => e,
            (Kind::Pad, _) => model.pad[e],
            (Kind::Layer, _) => model.layer[e],
        }
    }

    /// Whether `ch` shows its chain (primary selection) or is otherwise
    /// selected (other selected pads).
    fn selection_of(&self, model: &Model, ch: Channel) -> (bool, bool) {
        let e = model.engine;
        match ch {
            Channel::Engine(k) => {
                let sel = self.bus_view.is_none() && crate::multipad::engine_selection(model) & (1u8 << k.min(7)) != 0;
                (sel && k == e, sel && k != e)
            }
            Channel::Aux(_) | Channel::Master => (self.bus_view == Some(ch), false),
            Channel::Pad(k) => {
                let (anchor, mask) = crate::multipad::selection(model, e);
                (k == anchor, k != anchor && mask & (1u64 << k.min(63)) != 0)
            }
            Channel::Layer(k) => (k == model.layer[e], false),
        }
    }

    /// [re:faf56f] $_2: the chain / strips split for the
    /// panel height and mode.
    fn layout(&mut self, app: &mut App) {
        let Some(page) = app.ctrl(&self.page) else { return };
        let Some(panel) = page.children.first() else { return };
        let (w, h) = (panel.rect.w, panel.rect.h);
        let compact = h < 0x181;
        if !compact && self.mode == 2 {
            self.mode = 0;
        }
        let mode = self.mode;
        let Some(panel) = app.ctrl_mut(&self.page).and_then(|p| p.children.first_mut()) else { return };
        let scroller_x = panel.find("ChannelScroller").map(|c| c.rect.x).unwrap_or(52);
        let fx_rect = if compact { Rect::new(scroller_x, 0, w - scroller_x, h) } else { Rect::new(0, 0, w, 200) };
        let fx_visible = !compact || mode == 2;
        let strips_rect = if compact { Rect::new(0, 0, w, h) } else { Rect::new(0, 202, w, h - 202) };
        for c in panel.children.iter_mut() {
            if c.name == "EffectPanel" {
                if c.rect != fx_rect {
                    c.set_rect(fx_rect);
                }
                c.visible = fx_visible;
            } else if c.find("ChannelScroller").is_some() {
                if c.rect != strips_rect {
                    c.set_rect(strips_rect);
                }
                if let Some(sc) = c.find_mut("ChannelScroller") {
                    sc.visible = !(compact && mode == 2);
                }
                if let Some(g) = c.find_mut("ModeGroup") {
                    if let Some(b) = g.children.get_mut(2) {
                        b.enabled = compact;
                    }
                }
                if let Some(v) = c.find_mut("EffectViewGroup") {
                    v.visible = compact && mode == 2;
                }
            }
        }
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        if self.kind != Kind::Global {
            self.bus_view = None;
        }
        self.layout(app);
        self.rebuild(app, model);
        let items = self.view_items();
        let vi = self.view_index(model);
        if let Some(v) = app.ctrl_mut(&self.page).and_then(|p| p.find_mut("EffectViewGroup")) {
            v.text = items.get(vi).cloned().unwrap_or_default();
        }
        let t = self.fx_target(model);
        if let Some(fx) = &mut self.fx {
            fx.sync(app, model, t);
        }
        let e = model.engine;
        let p = model.pad[e];
        let doc = &model.doc;
        let mode = self.mode;
        let names: Vec<(Channel, String)> = self.shown.iter().map(|c| (*c, self.label(model, *c))).collect();
        let sel: std::collections::HashMap<Channel, (bool, bool)> = self.shown.iter().map(|c| (*c, self.selection_of(model, *c))).collect();
        let Some(sp) = &self.scroller else { return };
        // Mode buttons.
        if let Some(page) = app.ctrl_mut(&self.page) {
            if let Some(g) = page.find_mut("ModeGroup") {
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = i == mode;
                }
            }
        }
        let Some(sc) = app.ctrl_mut(sp) else { return };
        for (strip, (ch, label)) in sc.children.iter_mut().zip(names) {
            // Node for the channel's parameters.
            let node = match ch {
                Channel::Engine(k) => doc.child("Engine", &format!("Engine{k}")),
                Channel::Aux(k) => doc.child("GlobalAux", &format!("GlobalAux{k}")),
                Channel::Master => doc.child("MasterChannel", "MasterChannel"),
                Channel::Pad(k) => model.pad_node(e, k),
                Channel::Layer(k) => model.layer_node(e, p, k),
            };
            let gain_key = match ch {
                Channel::Layer(_) => LayerParam::Gain.name(),
                _ => "Gain",
            };
            let gain_default = match ch {
                Channel::Layer(_) => LayerParam::Gain.default_value(),
                _ => dreist_formats::params::UNITY_GAIN,
            };
            let gain = attr_f32(node, gain_key, gain_default);
            let pan = attr_f32(node, "Pan", 0.5);
            let pitch = match ch {
                Channel::Layer(_) => attr_f32(node, LayerParam::CoarseTune.name(), 0.5),
                _ => attr_f32(node, "Pitch", 0.5),
            };
            let (mute, solo) = (node.is_some_and(|n| n.truthy("Mute")), node.is_some_and(|n| n.truthy("Solo")));
            // [re:5575fd]: PrePostButton
            // = [re:892cf2] (`IsPreFade`).
            // No attribute: the option `AuxesArePreFade` default.
            let pre_fade = match node.and_then(|n| n.attr("IsPreFade")) {
                Some(_) => node.is_some_and(|n| n.truthy("IsPreFade")),
                None => crate::options::Options::load().bool("AuxesArePreFade", false),
            };
            let sends: Vec<f32> = (0..4).map(|k| attr_f32(node, &dreist_formats::params::aux_send_name(k), 0.0)).collect();
            let output = output_items()[output_index(node.and_then(|n| n.attr("OutputLocation")).unwrap_or("Parent"))].clone();
            // `UpdateEffectIndicators` [re:01fdd4]: FxIndi k lit when slot k
            // holds an effect.
            let fx_on: Vec<bool> = (0..6)
                .map(|k| {
                    node.and_then(|n| n.children.iter().find(|c| c.class == "EffectChain"))
                        .and_then(|c| c.children.iter().find(|f| f.class == "Effect" && f.name.as_deref() == Some(&format!("Effect{k}"))))
                        .and_then(|f| f.attr("EffectType"))
                        .is_some_and(|t| dreist_formats::params::effect_type_index(t).unwrap_or(0) != 0)
                })
                .collect();
            // [re:f14fcf] (only strips with
            // SendKnob0 and MuteButton): Mix shows the gain slider's panel,
            // Sends the send knobs' panel, Effects hides both.
            let has_sends = strip.find("SendKnob0").is_some() && strip.find("MuteButton").is_some();
            let title = match sel.get(&ch).copied().unwrap_or((false, false)) {
                (true, _) => TITLE_SELECTED,
                (false, true) => TITLE_SUB,
                _ => TITLE,
            };
            strip.overload = "MixerStrip".into();
            strip.walk_mut(&mut |k: &mut Ctrl| match k.name.as_str() {
                "NameLabel" => {
                    if k.class != "CtrlLabel" {
                        k.text = label.clone();
                    }
                    if k.class == "CtrlTextBox" {
                        k.style = title.to_string();
                    }
                }
                "GainSlider" => k.value = gain,
                "PanKnob" => k.value = pan,
                // [re:ab799e] hides Pitch
                // (`SetUserEnableState(false)`); the engine ([re:05d928]) and pad
                // ([re:a584ed]) channels show it again, layer and aux keep it
                // hidden. The master strip's knob is the master tune.
                "Pitch" => {
                    k.value = pitch;
                    k.visible = matches!(ch, Channel::Engine(_) | Channel::Pad(_) | Channel::Master);
                }
                "MuteButton" => k.on = mute,
                "PrePostButton" => k.on = pre_fade,
                "SoloButton" => k.on = solo,
                "SendPanel" if has_sends => k.visible = mode == 1,
                "MixPanel" if has_sends => k.visible = mode == 0,
                "OutputCombo" => k.text = output.clone(),
                n if n.starts_with("FxIndi") => {
                    if let Ok(i) = n[6..].parse::<usize>() {
                        k.on = fx_on.get(i).copied().unwrap_or(false);
                        k.enabled = false;
                    }
                }
                n if n.starts_with("SendKnob") => {
                    if let Ok(i) = n[8..].parse::<usize>() {
                        k.value = sends.get(i).copied().unwrap_or(0.0);
                    }
                }
                _ => {}
            });
        }
    }

    fn label(&self, model: &Model, ch: Channel) -> String {
        let e = model.engine;
        match ch {
            Channel::Engine(k) => model.engine_name(k),
            Channel::Aux(k) => model
                .doc
                .children
                .iter()
                .find(|c| c.class == "GlobalAux" && c.name.as_deref() == Some(&format!("GlobalAux{k}")))
                .and_then(|n| n.attr("AuxName"))
                .map(str::to_string)
                .unwrap_or_else(|| format!("Aux {}", k + 1)),
            Channel::Master => "Master".into(),
            Channel::Pad(k) => model.pad_name(e, k).unwrap_or_else(|| format!("Pad {}", k + 1)),
            Channel::Layer(k) => model.layer_name(e, model.pad[e], k).unwrap_or_else(|| format!("Layer {}", k + 1)),
        }
    }

    /// The control at `path` belongs to an effect in this mixer's chain.
    pub fn is_effect_control(&self, app: &App, path: &[usize]) -> bool {
        self.fx.as_ref().is_some_and(|f| f.contains(app, path))
    }

    /// The conversion of a strip knob's typed edit.
    pub fn edit_curve(&self, app: &App, path: &[usize]) -> Option<dreist_formats::params::Curve> {
        use dreist_formats::params::Curve;
        if self.is_effect_control(app, path) {
            return Some(Curve::Linear(0.0, 100.0));
        }
        let (ch, name) = self.hit(app, path)?;
        Some(match (ch, name.as_str()) {
            (_, "GainSlider") => Curve::Decibels,
            (_, "PanKnob") => Curve::Linear(-1.0, 1.0),
            (Channel::Layer(_), "Pitch") => LayerParam::CoarseTune.curve(),
            (_, "Pitch") => PadParam::Pitch.curve(),
            (_, n) if n.starts_with("SendKnob") => Curve::Decibels,
            _ => return None,
        })
    }

    /// "Remove all modulation from this control" for strip knobs
    /// (`RemoveAllTransmodFromControl`) and the strip's effects: depth 0
    /// for sources 1..=28.
    pub fn remove_modulation(&self, app: &App, model: &Model, path: &[usize]) -> Vec<Command> {
        if let Some(f) = &self.fx {
            let v = f.remove_modulation(app, model, path);
            if !v.is_empty() {
                return v;
            }
        }
        if !app.ctrl(path).is_some_and(|c| c.class == "CtrlTransmodKnob" || c.class == "CtrlTransmodSlider") {
            return Vec::new();
        }
        let Some((ch, name)) = self.hit(app, path) else { return Vec::new() };
        let (e, p) = (model.engine as i32, model.pad[model.engine] as i32);
        (1..=28)
            .filter_map(|s| {
                let s = s as f32;
                Some(match (ch, name.as_str()) {
                    (Channel::Engine(k), "GainSlider") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[0.0, EngineParam::Gain as u8 as f32, s]),
                    (Channel::Engine(k), "PanKnob") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[0.0, EngineParam::Pan as u8 as f32, s]),
                    (Channel::Engine(k), "Pitch") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[0.0, EngineParam::Pitch as u8 as f32, s]),
                    (Channel::Pad(k), "GainSlider") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[0.0, PadParam::Gain as u8 as f32, s]),
                    (Channel::Pad(k), "PanKnob") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[0.0, PadParam::Pan as u8 as f32, s]),
                    (Channel::Pad(k), "Pitch") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[0.0, PadParam::Pitch as u8 as f32, s]),
                    (Channel::Layer(k), "GainSlider") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::Gain as u8 as f32, 0.0, s]),
                    (Channel::Layer(k), "PanKnob") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::Pan as u8 as f32, 0.0, s]),
                    (Channel::Layer(k), "Pitch") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::CoarseTune as u8 as f32, 0.0, s]),
                    (ch, n) if n.starts_with("SendKnob") => {
                        let a = n[8..].parse::<i32>().ok()? as f32;
                        match ch {
                            Channel::Engine(k) => msg(id::SET_ENGINE_SEND_LEVEL, &[k as i32], &[a, 0.0, s]),
                            Channel::Pad(k) => msg(id::SET_PAD_SEND_LEVEL, &[e, k as i32], &[a, 0.0, s]),
                            Channel::Layer(k) => msg(id::SET_LAYER_SEND_DEPTH, &[e, p, k as i32], &[a, 0.0, s]),
                            _ => return None,
                        }
                    }
                    _ => return None,
                })
            })
            .collect()
    }

    /// Sources modulating the strip knob at `path` (`EditorMixer…Channel`
    /// enter listeners: engine / pad / layer parameters and aux sends), and
    /// effect parameters in the strip's chain.
    pub fn modulation_mask(&self, app: &App, model: &Model, path: &[usize]) -> Option<u32> {
        if let Some(m) = self.fx.as_ref().and_then(|f| f.modulation_mask(app, model, path)) {
            return Some(m);
        }
        let (ch, name) = self.hit(app, path)?;
        let (e, p) = (model.engine, model.pad[model.engine]);
        let node = match ch {
            Channel::Engine(k) => model.engine_node(k),
            Channel::Pad(k) => model.pad_node(e, k),
            Channel::Layer(k) => model.layer_node(e, p, k),
            _ => return None,
        };
        let key = match (ch, name.as_str()) {
            (Channel::Layer(_), "GainSlider") => LayerParam::Gain.name().to_string(),
            (Channel::Layer(_), "PanKnob") => LayerParam::Pan.name().to_string(),
            (Channel::Layer(_), "Pitch") => LayerParam::CoarseTune.name().to_string(),
            (_, "GainSlider") => "Gain".into(),
            (_, "PanKnob") => "Pan".into(),
            (_, "Pitch") => "Pitch".into(),
            (_, n) => dreist_formats::params::aux_send_name(n.strip_prefix("SendKnob")?.parse().ok()?),
        };
        Some(crate::model::modulation_mask(node, &key))
    }

    /// The strip knob tooltip ([re:fcb381] $_7
    /// [re:dafd34] and the derived strips): Decibels "Gain", Pan "Pan",
    /// Percent "Send n", Semitones "Pitch", with the transmod description
    /// and modulated value when a source is shown; the master gain is a
    /// DB15 provider of its display name, aux strips Decibels "Gain".
    pub fn tooltip(&self, app: &App, model: &Model, path: &[usize], source: Option<(&str, usize)>) -> Option<String> {
        use crate::conv::{Conv, db15_text, transmod_text};
        if let Some(f) = self.fx.as_ref().filter(|f| f.contains(app, path)) {
            return f.tooltip(app, model, path, source.map(|(_, s)| s));
        }
        let (ch, name) = self.hit(app, path)?;
        let c = app.ctrl(path)?;
        let v = c.value;
        let (conv, label) = match name.as_str() {
            "GainSlider" => (Conv::Decibels, "Gain".to_string()),
            "PanKnob" => (Conv::Pan, "Pan".into()),
            "Pitch" => (Conv::Semitones, "Pitch".into()),
            n => (Conv::Percent, format!("Send {}", n.strip_prefix("SendKnob")?.parse::<usize>().ok()?)),
        };
        match ch {
            Channel::Master if name == "GainSlider" => return Some(db15_text(&c.display_name, v)),
            Channel::Master | Channel::Aux(_) => return (name == "GainSlider" || name == "Pitch").then(|| conv.named(&label, v)),
            _ => {}
        }
        let (e, p) = (model.engine, model.pad[model.engine]);
        let node = match ch {
            Channel::Engine(k) => model.engine_node(k),
            Channel::Pad(k) => model.pad_node(e, k),
            Channel::Layer(k) => model.layer_node(e, p, k),
            _ => None,
        };
        let key = match (ch, name.as_str()) {
            (Channel::Layer(_), "GainSlider") => LayerParam::Gain.name().to_string(),
            (Channel::Layer(_), "PanKnob") => LayerParam::Pan.name().to_string(),
            (Channel::Layer(_), "Pitch") => LayerParam::CoarseTune.name().to_string(),
            (_, "GainSlider") => "Gain".into(),
            (_, "PanKnob") => "Pan".into(),
            (_, "Pitch") => "Pitch".into(),
            (_, n) => dreist_formats::params::aux_send_name(n.strip_prefix("SendKnob")?.parse().ok()?),
        };
        let src = source.map(|(l, s)| (l, node.and_then(|n| dreist_engine::engine::doc::attr_name(&key, s).and_then(|a| n.f32(&a))).unwrap_or(0.0)));
        Some(transmod_text(conv, &label, v, src))
    }

    /// A knob double click's reset value: the strip's effects, then
    /// [re:7b12f8] (Gain
    /// ∛0.5, Pan 0.5, Pitch 0.5 in the derived strips, sends 0); aux strips ([re:223b19] $_14 [re:c32b02]) only
    /// reset the gain.
    pub fn reset_value(&self, app: &App, path: &[usize]) -> Option<f32> {
        let name = app.ctrl(path)?.name.clone();
        if let Some(v) = self.fx.as_ref().and_then(|f| f.reset_value(app, path, &name)) {
            return Some(v);
        }
        let (ch, name) = self.hit(app, path)?;
        match (ch, name.as_str()) {
            (_, "GainSlider") => Some(dreist_formats::params::UNITY_GAIN),
            (Channel::Aux(_), _) => None,
            (_, "PanKnob") => Some(0.5),
            // The pad / engine / master strips override the default to 0.5
            // ([re:06731b], [re:0a2673]; master [re:81d585]).
            (_, "Pitch") => Some(0.5),
            (_, n) if n.starts_with("SendKnob") => Some(0.0),
            _ => None,
        }
    }

    /// The channel and control name for an event path inside this mixer.
    fn hit(&self, app: &App, path: &[usize]) -> Option<(Channel, String)> {
        let sp = self.scroller.as_ref()?;
        if path.len() < sp.len() + 2 || !path.starts_with(sp) {
            return None;
        }
        let ch = *self.shown.get(path[sp.len()])?;
        Some((ch, app.ctrl(path)?.name.clone()))
    }

    /// The pad whose strip background (or a passive child: label, panel)
    /// is at `path`: [re:bfddf4] $_17
    /// [re:c455d6] opens the pad menu there (right clicks on the knobs keep
    /// their own menus).
    pub fn pad_strip_at(&self, app: &App, path: &[usize]) -> Option<usize> {
        let sp = self.scroller.as_ref()?;
        if path.len() < sp.len() + 1 || !path.starts_with(sp) {
            return None;
        }
        let Channel::Pad(k) = *self.shown.get(path[sp.len()])? else { return None };
        let interactive = (sp.len() + 1..=path.len()).any(|n| {
            app.ctrl(&path[..n]).is_some_and(|c| {
                matches!(
                    c.class.as_str(),
                    "CtrlButton" | "CtrlPopupButton" | "CtrlKnob" | "CtrlTransmodKnob" | "CtrlSlider" | "CtrlTransmodSlider" | "CtrlDisclosureButton" | "CtrlScrollBar" | "CtrlComboBox"
                )
            })
        });
        (!interactive).then_some(k)
    }

    /// A strip press or a Show FX pick: select the channel and show its
    /// chain.
    fn select(&mut self, model: &mut Model, ch: Channel, cmd: bool, shift: bool, out: &mut Vec<Command>) {
        let e = model.engine;
        match ch {
            Channel::Engine(k) => {
                // The global mixer's $_0 [re:1068c4]: [re:352eb9] (cmd
                // toggles, shift extends) → `e_setEngineSelections` 0x43.
                self.bus_view = None;
                let cur = crate::multipad::select_engine(model, k, cmd, shift);
                model.engine = cur;
                model.doc.set("CurrentEngine", cur);
                out.push(Command::SelectEngine(cur));
            }
            Channel::Aux(_) | Channel::Master => self.bus_view = Some(ch),
            Channel::Pad(k) => out.push(crate::multipad::select(model, e, k, cmd, shift)),
            Channel::Layer(k) => model.layer[e] = k,
        }
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if let UiEvent::Wheel { path, overload, dy } = ev {
            if overload == "MixerScroller" && self.scroller.as_ref().is_some_and(|s| path.starts_with(s)) {
                self.scroll_x -= (dy * 40.0) as i32;
                return true;
            }
        }
        if let Some(fx) = &mut self.fx {
            if fx.handle(app, model, ev, out) {
                return true;
            }
        }
        let e = model.engine as i32;
        let p = model.pad[model.engine] as i32;
        if let UiEvent::MenuSelected { index, .. } = ev {
            if std::mem::take(&mut self.view_menu) {
                // `UpdateVisibleEffect`: the item's channel.
                let ch = match self.kind {
                    Kind::Global if *index < ENGINES => Channel::Engine(*index),
                    Kind::Global if *index < 12 => Channel::Aux(*index - 8),
                    Kind::Global => Channel::Master,
                    Kind::Pad => Channel::Pad((*index).min(63)),
                    Kind::Layer => Channel::Layer((*index).min(LAYERS - 1)),
                };
                self.select(model, ch, false, false, out);
                return true;
            }
            // [re:703e6d] $_12 [re:60ed40], [re:b8ab71]
            // $_11 [re:2788cc], [re:ab1a4e] $_11
            // [re:c9fdd9]: route every pad / layer / engine to parent (0),
            // master (1) or "Sub output n" in order (i + 2).
            if let Some(ch) = self.route_all_menu.take() {
                let engine = matches!(ch, Channel::Engine(_));
                let choice = if engine { *index + 1 } else { *index };
                let v = |i: usize| match choice {
                    0 => 0.0,
                    1 => 1.0,
                    _ => (i + 2) as f32,
                };
                match ch {
                    Channel::Engine(_) => out.extend((0..ENGINES).map(|k| msg(id::SET_ENGINE_OUTPUT, &[k as i32], &[v(k)]))),
                    Channel::Pad(_) => out.extend((0..64).map(|k| msg(id::SET_PAD_OUTPUT, &[e, k as i32], &[v(k)]))),
                    Channel::Layer(_) => out.extend((0..LAYERS).map(|k| msg(id::SET_LAYER_OUTPUT, &[e, p, k as i32], &[v(k)]))),
                    _ => {}
                }
                return true;
            }
            if let Some(ch) = self.output_menu.take() {
                let v = (*index).min(37) as f32;
                let cmd = match ch {
                    Channel::Engine(k) => msg(id::SET_ENGINE_OUTPUT, &[k as i32], &[v]),
                    Channel::Pad(k) => msg(id::SET_PAD_OUTPUT, &[e, k as i32], &[v]),
                    Channel::Layer(k) => msg(id::SET_LAYER_OUTPUT, &[e, p, k as i32], &[v]),
                    Channel::Aux(k) => msg(id::SET_GLOBAL_AUX_OUTPUT, &[k as i32], &[v]),
                    Channel::Master => return true,
                };
                out.push(cmd);
                return true;
            }
        }
        if let UiEvent::Clicked { path, name, .. } = ev {
            if name == "EffectViewGroup" && path.starts_with(&self.page) {
                let items = self.view_items();
                let checked = Some(self.view_index(model));
                app.open_menu(path, items, checked);
                self.view_menu = true;
                return true;
            }
        }
        if let UiEvent::Mouse { path, overload, kind: dreist_gui::app::MouseKind::Down, .. } = ev {
            if overload == "MixerStrip" {
                let Some(sp) = self.scroller.as_ref() else { return false };
                if path.len() != sp.len() + 1 || !path.starts_with(sp) {
                    return false;
                }
                let Some(ch) = self.shown.get(path[sp.len()]).copied() else { return false };
                let (cmd, shift) = (app.mods.cmd, app.mods.shift);
                self.select(model, ch, cmd, shift, out);
                return true;
            }
        }
        if let UiEvent::RightClicked { path, name } = ev {
            if name == "OutputCombo" {
                let Some((ch, _)) = self.hit(app, path) else { return false };
                let items: Vec<String> = match ch {
                    Channel::Engine(_) => vec!["Set all to master".into(), "Output sequentially".into()],
                    Channel::Pad(_) | Channel::Layer(_) => vec!["Set all to parent".into(), "Set all to master".into(), "Output sequentially".into()],
                    _ => return false,
                };
                app.open_menu(path, items, None);
                self.route_all_menu = Some(ch);
                return true;
            }
        }
        if let UiEvent::Clicked { path, name, .. } = ev {
            if name == "OutputCombo" {
                let Some((ch, _)) = self.hit(app, path) else { return false };
                let used = used_outputs(&model.doc);
                let current = app.ctrl(path).map(|c| c.text.clone()).unwrap_or_default();
                let items = output_items();
                let checked = items.iter().position(|t| *t == current);
                let labels = items.into_iter().enumerate().map(|(i, t)| if i > 1 && used.contains(&i) { format!("\u{25a0} {t}") } else { t }).collect();
                app.open_menu(path, labels, checked);
                self.output_menu = Some(ch);
                return true;
            }
        }
        match ev {
            UiEvent::ValueChanged { path, value, .. } => {
                let Some((ch, name)) = self.hit(app, path) else { return false };
                let v = *value;
                let cmd = match (ch, name.as_str()) {
                    (Channel::Engine(k), "GainSlider") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[v, EngineParam::Gain as u8 as f32, 0.0]),
                    (Channel::Engine(k), "PanKnob") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[v, EngineParam::Pan as u8 as f32, 0.0]),
                    (Channel::Engine(k), "Pitch") => msg(id::SET_ENGINE_PARAMETER, &[k as i32], &[v, EngineParam::Pitch as u8 as f32, 0.0]),
                    (Channel::Pad(k), "GainSlider") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[v, PadParam::Gain as u8 as f32, 0.0]),
                    (Channel::Pad(k), "PanKnob") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[v, PadParam::Pan as u8 as f32, 0.0]),
                    (Channel::Pad(k), "Pitch") => msg(id::SET_PAD_PARAMETER, &[e, k as i32], &[v, PadParam::Pitch as u8 as f32, 0.0]),
                    (Channel::Layer(k), "GainSlider") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::Gain as u8 as f32, v, 0.0]),
                    (Channel::Layer(k), "PanKnob") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::Pan as u8 as f32, v, 0.0]),
                    (Channel::Layer(k), "Pitch") => msg(id::SET_LAYER_PARAMETER, &[e, p, k as i32], &[LayerParam::CoarseTune as u8 as f32, v, 0.0]),
                    (Channel::Aux(k), "GainSlider") => msg(id::SET_GLOBAL_AUX_GAIN, &[k as i32], &[v]),
                    (Channel::Master, "GainSlider") => msg(id::SET_MASTER_CHANNEL_GAIN, &[], &[v]),
                    (Channel::Master, "Pitch") => msg(id::SET_MASTER_CHANNEL_TUNE, &[], &[v]),
                    (ch, n) if n.starts_with("SendKnob") => {
                        let a = n[8..].parse::<i32>().unwrap_or(0);
                        match ch {
                            Channel::Engine(k) => msg(id::SET_ENGINE_SEND_LEVEL, &[k as i32], &[a as f32, v, 0.0]),
                            Channel::Pad(k) => msg(id::SET_PAD_SEND_LEVEL, &[e, k as i32], &[a as f32, v, 0.0]),
                            Channel::Layer(k) => msg(id::SET_LAYER_SEND_DEPTH, &[e, p, k as i32], &[a as f32, v, 0.0]),
                            _ => return false,
                        }
                    }
                    _ => return false,
                };
                out.push(cmd);
                true
            }
            UiEvent::Clicked { path, name, on, .. } => {
                // Mode group.
                if let Some(page) = app.ctrl(&self.page) {
                    let _ = page;
                }
                if path.len() > self.page.len() && path.starts_with(&self.page) {
                    if let Some(parent) = app.ctrl(&path[..path.len() - 1]) {
                        if parent.name == "ModeGroup" {
                            self.mode = *path.last().unwrap();
                            return true;
                        }
                    }
                }
                let Some((ch, _)) = self.hit(app, path) else { return false };
                let v = *on as i32 as f32;
                let cmd = match (ch, name.as_str()) {
                    (Channel::Engine(k), "MuteButton") => msg(id::MUTE_ENGINE, &[k as i32], &[v]),
                    (Channel::Engine(k), "SoloButton") => msg(id::SOLO_ENGINE, &[k as i32], &[v]),
                    (Channel::Pad(k), "MuteButton") => msg(id::MUTE_PAD, &[e, k as i32], &[v]),
                    (Channel::Pad(k), "SoloButton") => msg(id::SOLO_PAD, &[e, k as i32], &[v]),
                    (Channel::Layer(k), "MuteButton") => msg(id::SET_LAYER_MUTE, &[e, p, k as i32], &[v]),
                    (Channel::Layer(k), "SoloButton") => msg(id::SET_LAYER_SOLO, &[e, p, k as i32], &[v]),
                    (Channel::Aux(k), "MuteButton") => msg(id::MUTE_GLOBAL_AUX, &[k as i32], &[v]),
                    (Channel::Aux(k), "SoloButton") => msg(id::SOLO_GLOBAL_AUX, &[k as i32], &[v]),
                    // [re:0a9148] $_5 [re:a4e09c]: 0xa3
                    // [aux] [pre].
                    (Channel::Aux(k), "PrePostButton") => msg(id::SET_GLOBAL_AUX_PRE_POST, &[k as i32], &[v]),
                    _ => return false,
                };
                out.push(cmd);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod size_tests {
    #[test]
    fn strip_size_thresholds() {
        assert_eq!(super::Mixer::size_index(200), 0);
        assert_eq!(super::Mixer::size_index(300), 1);
        assert_eq!(super::Mixer::size_index(400), 2);
    }
}
