//! Knob editing: the right-click menu, the value edit box and the
//! double-click reset.

use crate::*;

/// What a knob menu item does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum KnobAction {
    Reset,
    Edit,
    RemoveModulation,
    TuneToTempo,
    PreDelayMsec,
    Rename,
    None,
}

/// The typed knob edit box ([re:c8b896]).
pub(crate) const KNOB_EDIT: &str = "KnobValueEdit";

/// The unit a typed edit shows for a conversion.
fn curve_suffix(c: dreist_formats::params::Curve) -> &'static str {
    use dreist_formats::params::Curve;
    match c {
        Curve::Decibels => " dB",
        Curve::Cutoff => " Hz",
        Curve::Stretch => " %",
        Curve::Linear(lo, hi) if lo == 0.0 && hi == 100.0 => " %",
        _ => "",
    }
}

impl Editor {
    /// A knob's reset value from the panel that owns it.
    pub(crate) fn knob_reset_value(&self, path: &[usize], name: &str) -> Option<f32> {
        self.inspector
            .reset_value(path, name)
            .or_else(|| self.mixers.iter().find_map(|m| m.reset_value(&self.app, path)))
            .or_else(|| self.transmod.reset_value(&self.app, &self.model, path, name))
            .or_else(|| self.pattern.as_ref().and_then(|p| p.reset_value(path, name)))
    }

    /// Set a knob as if moved to `v` (base value: transmod source 0).
    pub(crate) fn knob_set(&mut self, path: &[usize], name: &str, id: i32, v: f32) {
        if let Some(c) = self.app.ctrl_mut(path) {
            c.value = v;
        }
        let source = std::mem::replace(&mut self.model.source, 0);
        self.handle(&UiEvent::ValueChanged { path: path.to_vec(), name: name.to_string(), id, value: v });
        self.model.source = source;
        self.sync();
        self.app.dirty = true;
    }

    /// Knob right clicks (the panels' [re:944095] 0x10 listeners, e.g.
    /// [re:efb170] [re:2f6dee], [re:042ac9] [re:aaaa12],
    /// [re:f61ec0] [re:b9a170], [re:e5c3c3]
    /// [re:22e1ae], [re:2f9e16] [re:382eda], [re:4196af],
    /// [re:f5b367], [re:223b19],
    /// [re:284b3d], [re:bc7b1d]):
    /// Reset, Edit, and for modulatable knobs "Remove all modulation from
    /// this control".
    pub(crate) fn knob_right_click(&mut self, ev: &UiEvent) -> bool {
        match ev {
            UiEvent::RightClicked { path, name } => {
                let Some(c) = self.app.ctrl(path) else { return false };
                let class = c.class.clone();
                if !matches!(class.as_str(), "CtrlKnob" | "CtrlTransmodKnob" | "CtrlSlider" | "CtrlTransmodSlider") {
                    return false;
                }
                // The graph controls' menu is on Variance and Rand only;
                // swing and v2a have their own menus (pattern.rs, velsplit.rs).
                if matches!(name.as_str(), "ScaleKnob" | "CompKnob" | "SwingKnob" | "v2a") {
                    return false;
                }
                let header_gain = name == "GlobalGain";
                if !header_gain && self.knob_reset_value(path, name).is_none() {
                    return false;
                }
                let mut items = vec![("Reset".to_string(), KnobAction::Reset), ("Edit".to_string(), KnobAction::Edit)];
                // Macro knobs add "Rename" ([re:4632d9]).
                if name.starts_with("Macro") && self.transmod.reset_value(&self.app, &self.model, path, name).is_some() {
                    items.push(("Rename".into(), KnobAction::Rename));
                }
                // [re:38b63d]: the coarse tune knob (param 5) adds "Tune
                // loop to tempo", the pre-delay knob (param 7) the
                // "Set pre delay in msec" check.
                if self.inspector.reset_value(path, name).is_some() && class == "CtrlTransmodKnob" {
                    if name == "CoarseTune" {
                        items.push((String::new(), KnobAction::None));
                        items.push(("Tune loop to tempo".into(), KnobAction::TuneToTempo));
                    } else if name == "PreDelayTime" {
                        let on = options::Options::load().bool("PreDelayInMsec", true);
                        items.push((String::new(), KnobAction::None));
                        items.push((format!("{}Set pre delay in msec", if on { dreist_gui::menu::CHECK } else { "" }), KnobAction::PreDelayMsec));
                    }
                }
                if !self.remove_modulation_commands(path).is_empty() {
                    // The layer and effect menus put a divider first.
                    if self.inspector.reset_value(path, name).is_some() || self.mixers.iter().any(|m| m.is_effect_control(&self.app, path)) {
                        items.push((String::new(), KnobAction::None));
                    }
                    items.push(("Remove all modulation from this control".into(), KnobAction::RemoveModulation));
                }
                let (labels, acts): (Vec<String>, Vec<KnobAction>) = items.into_iter().unzip();
                self.knob_menu = Some((path.clone(), name.clone(), acts));
                self.app.open_menu(path, labels, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.knob_menu.as_ref().is_some_and(|(p, _, _)| p == anchor) => {
                let Some((path, name, acts)) = self.knob_menu.take() else { return false };
                let id = self.app.ctrl(&path).map(|c| c.id).unwrap_or(0);
                match acts.get(*index).copied().unwrap_or(KnobAction::None) {
                    KnobAction::Reset => {
                        let v = if name == "GlobalGain" { Some(dreist_formats::params::UNITY_GAIN) } else { self.knob_reset_value(&path, &name) };
                        if let Some(v) = v {
                            self.knob_set(&path, &name, id, v);
                        }
                    }
                    KnobAction::Edit => self.start_knob_edit(&path, &name, id),
                    KnobAction::RemoveModulation => {
                        let cmds = self.remove_modulation_commands(&path);
                        for c in &cmds {
                            self.model.apply(c);
                        }
                        self.outbox.extend(cmds);
                        self.sync();
                        self.app.dirty = true;
                    }
                    // [re:bcf3d5]: e_tuneLayer for the current layer.
                    KnobAction::TuneToTempo => {
                        let e = self.model.engine;
                        let (p, l) = (self.model.pad[e], self.model.layer[e]);
                        self.outbox.push(command::msg(dreist_engine::binding::id::TUNE_LAYER, &[e as i32, p as i32, l as i32], &[0.0, 0.0, 0.0]));
                    }
                    // [re:264898]: toggles the option (the engine reads it when built).
                    KnobAction::PreDelayMsec => {
                        let mut o = options::Options::load();
                        let on = !o.bool("PreDelayInMsec", true);
                        o.set("PreDelayInMsec", on);
                        o.save();
                        self.outbox.push(self.model.rebuild_command());
                    }
                    KnobAction::Rename => {
                        if let Some(k) = name.strip_prefix("Macro").and_then(|n| n.parse::<usize>().ok()) {
                            self.transmod.start_rename(&mut self.app, &self.model, &path, Ok(k));
                        }
                    }
                    KnobAction::None => {}
                }
                true
            }
            UiEvent::TextCommitted { path, name, text } if name == KNOB_EDIT => {
                let parent = path[..path.len() - 1].to_vec();
                if let Some(c) = self.app.ctrl_mut(&parent) {
                    c.children.retain(|k| k.name != KNOB_EDIT);
                }
                if let Some((target, tname, id, curve)) = self.knob_edit.take() {
                    let t = text.trim().to_ascii_lowercase();
                    let num: String = t.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+')).collect();
                    let units = if t.starts_with("-inf") { Some(-1000.0) } else { crate::conv::number(&num) };
                    // A value outside the curve's domain (a negative
                    // frequency) converts to NaN: ignored.
                    if let Some(v) = units.map(|u| curve.from_units(u)).filter(|v| !v.is_nan()) {
                        self.knob_set(&target, &tname, id, v.clamp(0.0, 1.0));
                    }
                }
                self.app.dirty = true;
                true
            }
            _ => false,
        }
    }

    /// The messages removing every transmod depth (sources 1..=28) from
    /// the knob at `path` (empty when it is not modulatable).
    pub(crate) fn remove_modulation_commands(&self, path: &[usize]) -> Vec<Command> {
        let mut v = self.inspector.remove_modulation(&self.app, &self.model, path);
        if v.is_empty() {
            v = self.mixers.iter().map(|m| m.remove_modulation(&self.app, &self.model, path)).find(|c| !c.is_empty()).unwrap_or_default();
        }
        if v.is_empty() {
            v = self.transmod.remove_modulation(&self.app, path);
        }
        v
    }

    /// [re:c8b896]: a text box over the control with
    /// its value in its units; the typed value sets it.
    pub(crate) fn start_knob_edit(&mut self, path: &[usize], name: &str, id: i32) {
        use dreist_formats::params::Curve;
        let Some(c) = self.app.ctrl(path).cloned() else { return };
        let curve = self.inspector.edit_curve(path, name).or_else(|| self.mixers.iter().find_map(|m| m.edit_curve(&self.app, path))).unwrap_or(if name == "GlobalGain" {
            Curve::Decibels
        } else {
            Curve::Linear(0.0, 100.0)
        });
        let u = curve.to_units(c.value);
        let text = match curve {
            Curve::Decibels => format!("{:.1} dB", u.max(-100.0)),
            Curve::Cutoff => format!("{u:.0} Hz"),
            _ => format!("{}{}", (u * 100.0).round() / 100.0, curve_suffix(curve)),
        };
        let rect = dreist_gui::Rect::new(c.rect.x + c.rect.w / 2 - 35, c.rect.y + c.rect.h / 2 - 9, 70, 18);
        self.app.open_edit_box(&path[..path.len() - 1], KNOB_EDIT, rect, text);
        self.knob_edit = Some((path.to_vec(), name.to_string(), id, curve));
        self.app.dirty = true;
    }

    /// Knob / slider double clicks with `DoubleClickResets` (default on):
    /// the control goes to its default and reports it as a move (the
    /// original sets the value and notifies the listeners, or posts the default
    /// directly), always the base value (transmod source 0). Off, the
    /// original opens a typed edit ([re:c8b896]), as
    /// the knob menu's "Edit" does.
    pub(crate) fn double_click_reset(&mut self, ev: &UiEvent) -> bool {
        let UiEvent::DoubleClicked { path, name } = ev else { return false };
        let Some(c) = self.app.ctrl(path) else { return false };
        if !matches!(c.class.as_str(), "CtrlKnob" | "CtrlTransmodKnob" | "CtrlSlider" | "CtrlTransmodSlider") || name == "GlobalGain" {
            return false;
        }
        let id = c.id;
        let Some(v) = self.knob_reset_value(path, name) else { return false };
        if !options::Options::load().bool("DoubleClickResets", true) {
            self.start_knob_edit(path, name, id);
            return true;
        }
        self.knob_set(path, name, id, v);
        true
    }
}
