//! Tooltip texts ([re:36b15b]s, the panels' tooltip lambdas
//! and per-control text overrides): asked of the control under the
//! pointer, then its parents, after each pointer event
//! ([re:eb2a9b]). Controls without a provider show
//! none (there is no generic fallback); static texts are the skin
//! `displayName`s of the controls given a [re:06bacf].

use dreist_gui::Ctrl;
use dreist_gui::tooltip::Pos;

use crate::Editor;
use crate::conv::{Conv, bool_text, db15, db15_text, depth_text, long_percent_text, midi_text, transmod_text};

/// Buttons given a [re:38d526] ("N: On" / "N: Off"; the
/// controls connected to a provider).
const BOOL_BUTTONS: [&str; 33] = [
    "Mute", "Solo", "MuteButton", "SoloButton", "QuantizeButton", "PlayButton", "RecButton", "RecordButton", "LearnButton", "MetroButton", "SongButton", "LoopButton", "PrePostButton", "PrePost",
    "Gated", "Restart", "Remix", "EraseButton", "FullLevelButton", "MultiLayersButton", "LoopPreview", "ArmButton", "ThruButton", "AllLayers", "AllPads", "ShowSplits", "AutoAllocateButton",
    "SliceModeButton", "ActiveButton", "PatternPower", "PowerButton", "RepeatButton", "View",
];

/// Fixed help texts of the pattern tool bar and
/// sampler tooltip lambdas.
const HELP: [(&str, &str); 7] = [
    ("StepLengthCombo", "Length of time each step lasts"),
    ("LengthBox", "Number of steps in the pattern"),
    ("PatternNameBox", "Name of the pattern"),
    ("DragEdit", "When the mouse is dragged up and down over a step, which graph is changed"),
    ("Follow", "Set the follow action to occur once a sample has been captured"),
    ("TriggerMode", "Set the mode that is used to determine sampling start and stop conditions"),
    ("SourceList", "Set where the sampler takes its input from"),
];

impl Editor {
    /// Show the tooltip for the control at `path` (or the nearest parent
    /// that has one), else hide it.
    pub(crate) fn update_tooltip(&mut self, path: &[usize]) {
        let found = (0..=path.len()).rev().find_map(|n| self.tooltip_text(&path[..n]).map(|t| (n, t)));
        match found {
            Some((n, (text, pos))) => {
                let anchor = path[..n].to_vec();
                // [re:17719e]: the
                // path buttons' tooltips are the delayed kind.
                if self.app.ctrl(&anchor).is_some_and(|c| c.overload == "BrowserPathNavigator") {
                    let mut r = self.app.abs_rect(&anchor);
                    if let Some(b) = self.browser.as_ref().and_then(|b| b.nav_button_at(self.app.mouse_pos().0 - r.x as f32)) {
                        r = dreist_gui::ctrl::Rect::new(r.x + b.x as i32, r.y, b.w as i32, r.h);
                    }
                    self.app.set_tooltip_delayed(r, &text, pos);
                    return;
                }
                match self.tooltip_rect(&anchor) {
                    Some(r) => self.app.set_tooltip_at(r, &text, pos),
                    None => self.app.set_tooltip(&anchor, &text, pos),
                }
            }
            None => self.app.hide_tooltip(),
        }
    }

    /// A tooltip placed against something other than its control: the
    /// pattern graph's at the pointer's column, above the graph.
    fn tooltip_rect(&self, path: &[usize]) -> Option<dreist_gui::ctrl::Rect> {
        let c = self.app.ctrl(path)?;
        if c.overload == "PatternGraph" {
            let oy = self.app.origin(path).1;
            return Some(dreist_gui::ctrl::Rect::new(self.app.mouse_pos().0 as i32, oy - 2, 0, 0));
        }
        // [re:45e9a3]: the out rect's y = 10 pushes it down.
        if c.name == "PlusButton" {
            let mut r = self.app.abs_rect(path);
            r.y += 10;
            return Some(r);
        }
        None
    }

    /// The shown transmod source's label and index.
    fn tooltip_source(&self) -> Option<(String, usize)> {
        let s = self.model.source;
        if s == 0 {
            return None;
        }
        let p = self.app.root.path_of(&format!("Src{s}"))?;
        let c = self.app.ctrl(&p)?;
        let label = if c.text.is_empty() { c.display_name.clone() } else { c.text.clone() };
        Some((label, s))
    }

    /// The tooltip of the control at `path` itself.
    pub(crate) fn tooltip_text(&self, path: &[usize]) -> Option<(String, Pos)> {
        let c = self.app.ctrl(path)?;
        if !c.visible || !c.enabled {
            return None;
        }
        let below = |t: String| Some((t, Pos::Below));
        let v = c.value;
        let name = c.name.as_str();
        let src = self.tooltip_source();
        // Layer panel ([re:7eb9d2]).
        if let Some(par) = self.inspector.layer_param_at(path, name) {
            let e = self.model.engine;
            let label = crate::transmod::camel_words(par.enum_name());
            if c.class == "CtrlButton" || c.class == "CtrlPopupButton" {
                if c.class == "CtrlPopupButton" {
                    return None;
                }
                // Conversion handler [re:fbe4f3].
                if name == "TimestretchMode" {
                    return below(format!("{}: {}", c.display_name, if c.on { "Low CPU Mode (Fast)" } else { "High Precision Mode" }));
                }
                return below(Conv::OnOff.named(&label, if c.on { 1.0 } else { 0.0 }));
            }
            let predelay_ms = crate::options::Options::load().bool("PreDelayInMsec", true);
            let node = self.model.layer_node(e, self.model.pad[e], self.model.layer[e]);
            let s = src
                .as_ref()
                .filter(|_| c.class == "CtrlTransmodKnob")
                .map(|(l, s)| (l.as_str(), node.and_then(|n| dreist_engine::engine::doc::attr_name(par.name(), *s).and_then(|a| n.f32(&a))).unwrap_or(0.0)));
            return below(transmod_text(Conv::layer(par, predelay_ms), &label, v, s));
        }
        // [re:fe83c5] [re:9ac3aa].
        if self.inspector.is_v2a(path, name) {
            let r = v * 100.0;
            return below(format!("Velocity To Amplitude: {}%", (r + if r < 0.0 { -0.5 } else { 0.5 }) as i64));
        }
        if let Some(t) = self.transmod.knob_tooltip(&self.app, &self.model, path, src.as_ref().map(|(l, s)| (l.as_str(), *s))) {
            return Some((t, Pos::Above));
        }
        if let Some(t) = self.mixers.iter().find_map(|m| m.tooltip(&self.app, &self.model, path, src.as_ref().map(|(l, s)| (l.as_str(), *s)))) {
            return below(t);
        }
        if let Some((t, pos)) = HELP.iter().find(|(n, _)| *n == name).map(|(_, t)| (t.to_string(), Pos::Below)) {
            return Some((t, pos));
        }
        match name {
            // [re:4139f8] [re:f8b6cc].
            "GlobalGain" => return below(Conv::Decibels.named("Master Level", v)),
            // [re:45e9a3], [re:bc7b1d].
            "SwingKnob" => return below(Conv::Percent.named("Swing Depth", v)),
            "VarianceKnob" => return below(Conv::Depth.named("Variance", v)),
            "ScaleKnob" | "CompKnob" | "RandKnob" => return below(depth_text(&c.display_name, v)),
            "InstoGraph" | "Sensitivity" => return below(long_percent_text(&c.display_name, v)),
            "FullLevelKnob" => return below(midi_text(&c.display_name, v)),
            // [re:b3e82a].
            "MixKnob" => return below(Conv::Percent.named("Mix", v)),
            "OutputKnob" => return below(Conv::Percent.named("Level", v)),
            "TrimKnob" => return below(Conv::Percent.named("Trim", v)),
            // [re:9fbf48] (with the start one above).
            "StartThresholdSlider" => return Some((format!("Start threshold: {}", db15(v)), Pos::Above)),
            "StopThresholdSlider" => return below(format!("Stop threshold: {}", db15(v))),
            "PreviewVolume" if !c.display_name.is_empty() => return below(db15_text(&c.display_name, v)),
            // [re:948d61].
            "SliceMode" => return below("If enabled, transients are detected and used to slice the file. If disabled the audio file is divided in to equal sections".into()),
            // [re:6278e9].
            "PrevLocnButton" | "NextLocnButton" => {
                let b = self.browser.as_ref()?;
                let (prefix, loc) = if name == "PrevLocnButton" { ("Return to ", b.previous_location()) } else { ("Move to ", b.next_location()) };
                return below(loc.map(|l| format!("{prefix}{}", l.display())).unwrap_or_default());
            }
            _ => {}
        }
        // [re:284b3d] (macros; sources above).
        if let Some(k) = name.strip_prefix("Macro").and_then(|k| k.parse::<usize>().ok()).filter(|_| c.class.contains("Knob")) {
            return below(Conv::Percent.named(&format!("Macro {k}"), v));
        }
        if name.strip_prefix("Src").is_some_and(|k| k.parse::<usize>().is_ok()) {
            let t = if c.display_name.is_empty() { c.text.clone() } else { c.display_name.clone() };
            return Some((t, Pos::Above));
        }
        if let Some(t) = self.inspector.vsplit_tooltip(&self.model, c) {
            return below(t);
        }
        if c.overload == "TransmodSequencerGrid" {
            let ox = self.app.origin(path).0 as f32;
            return crate::transmod::seq_tooltip(&self.model, c, self.app.mouse_pos().0 - ox).map(|t| (t, Pos::Below));
        }
        if c.overload == "LayerWaveDisplay" {
            let (ox, oy) = self.app.origin(path);
            let (mx, my) = self.app.mouse_pos();
            return self.inspector.wave_tooltip(&self.app, &self.model, path, mx - ox as f32, my - oy as f32).map(|t| (t, Pos::Below));
        }
        if c.overload == "PatternGraph" {
            let ox = self.app.origin(path).0 as f32;
            return self.pattern.as_ref()?.graph_tooltip(&self.app, &self.model, self.app.mouse_pos().0 - ox).map(|t| (t, Pos::Above));
        }
        if c.overload == "PatternSelector" {
            let n: usize = name.strip_prefix("Key")?.parse().ok()?;
            return below(self.pattern_selector_tooltip(n));
        }
        if c.overload == "BrowserPathNavigator" {
            let (x, _) = self.app.mouse_pos();
            let ox = self.app.origin(path).0 as f32;
            return self.browser.as_ref()?.nav_button_at(x - ox).map(|b| (b.display, Pos::Below));
        }
        generic(c)
    }
}

/// [re:06bacf] / [re:38d526] on buttons that carry a
/// display name.
fn generic(c: &Ctrl) -> Option<(String, Pos)> {
    if !matches!(c.class.as_str(), "CtrlButton" | "CtrlPopupButton") || c.display_name.is_empty() {
        return None;
    }
    let bool_button = BOOL_BUTTONS.contains(&c.name.as_str()) || c.name.strip_prefix("Control").is_some_and(|k| k.parse::<usize>().is_ok());
    if bool_button && c.class == "CtrlButton" {
        return Some((bool_text(&c.display_name, c.on), Pos::Below));
    }
    Some((c.display_name.clone(), Pos::Below))
}

impl Editor {
    /// [re:83aa80]: "<name> | Steps: n |
    /// Res: r | MIDI: <key> (Note: n)" (an unused slot: "Pattern n", 16,
    /// 1/16); the note is [re:49f15a]
    /// [re:f6d33e] = 12 + pattern.
    fn pattern_selector_tooltip(&self, n: usize) -> String {
        let e = self.model.engine;
        let node = self.model.pattern_node(e, n);
        let name = Some(self.model.pattern_name(e, n)).filter(|s| node.is_some() && !s.is_empty()).unwrap_or_else(|| format!("Pattern {}", n + 1));
        let steps = node.and_then(|p| p.attr("PatternSteps")).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(16);
        let res = node.and_then(|p| p.attr("Resolution")).unwrap_or("1/16").to_string();
        let note = (12 + n).min(127) as u8;
        let mode = crate::options::Options::load().int("MIDINumbering", 1).clamp(0, 2) as u8;
        format!("{name} | Steps: {steps} | Res: {res} | MIDI: {} (Note: {note})", crate::midiconsole::key_name(note, mode, false))
    }
}
