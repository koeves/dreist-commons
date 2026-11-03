//! Pattern Convertor ([re:c65c97], pattern key menu › Convert
//! timing): NumStepsBox (1..1024, default 16), RezCombo (resolutions,
//! default 1/16), ApplyToAll, Cancel / Apply ([re:cf7e00]
//! → [re:4ec8dd] per pattern; port:
//! `PatternData::convert_timing`, sent as step count / length / clear /
//! step messages).

use dreist_engine::binding::id;
use dreist_engine::pattern::RESOLUTION_NAMES;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::{Ctrl, Skin};

use crate::EditorError;
use crate::command::{Command, msg};
use crate::model::Model;

pub struct Convertor {
    overlay: Option<Vec<usize>>,
    slide: crate::modal::Slide,
    pub open: bool,
    /// Pattern the menu was opened on.
    pub pattern: usize,
    steps: u32,
    resolution: usize,
    all: bool,
}

impl Convertor {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Convertor, EditorError> {
        let overlay = crate::modal::overlay(skin, root, "PatternConvertor.skin", "ConvertorOverlay")?;
        if let Some(op) = &overlay {
            let mut o = &mut *root;
            for &i in op {
                o = &mut o.children[i];
            }
            if let Some(b) = o.find_mut("NumStepsBox") {
                b.set_attr("realMin", "1".into());
                b.set_attr("realMax", "1024".into());
                b.set_attr("realStep", "1".into());
            }
        }
        Ok(Convertor { overlay, slide: Default::default(), open: false, pattern: 0, steps: 16, resolution: 8, all: false })
    }

    pub fn show(&mut self, pattern: usize) {
        self.open = true;
        self.pattern = pattern;
    }

    pub fn sync(&mut self, app: &mut App) {
        // [re:6abcdc]: slides in
        // from the top (500 ms Exponential); closing slides out.
        let shown = self.overlay.clone().map(|p| self.slide.sync(app, &p, self.open)).unwrap_or(false);
        let (open, steps, res, all) = (shown, self.steps, self.resolution, self.all);
        if let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            o.visible = open;
            if let Some(b) = o.find_mut("NumStepsBox") {
                b.value = steps as f32;
                b.text = steps.to_string();
            }
            if let Some(b) = o.find_mut("RezCombo") {
                b.text = RESOLUTION_NAMES[res.min(13)].into();
            }
            if let Some(b) = o.find_mut("ApplyToAll") {
                b.on = all;
            }
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    /// Returns whether the event was the panel's.
    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if let Some(p) = self.overlay.clone() {
            if self.slide.animated(app, &p, ev) {
                return true;
            }
        }
        if !self.open {
            return false;
        }
        match ev {
            UiEvent::ValueChanged { path, name, value, .. } if self.mine(path) && name == "NumStepsBox" => {
                self.steps = (value.round() as i64).clamp(1, 1024) as u32;
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) && name == "RezCombo" => {
                self.resolution = (*index).min(13);
                true
            }
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                match name.as_str() {
                    "RezCombo" => app.open_menu(path, RESOLUTION_NAMES.iter().map(|s| s.to_string()).collect(), Some(self.resolution)),
                    "ApplyToAll" => self.all = *on,
                    "CancelButton" => self.open = false,
                    "ApplyButton" => {
                        let targets: Vec<usize> = if self.all { (0..24).collect() } else { vec![self.pattern] };
                        for p in targets {
                            convert(model, p, self.steps, self.resolution, out);
                        }
                        self.open = false;
                    }
                    _ => {}
                }
                true
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::Down, .. } if overload == "ModalOverlay" && self.overlay.as_deref() == Some(path.as_slice()) => {
                self.open = false;
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => self.mine(path),
            _ => false,
        }
    }
}

/// Re-time pattern `p` of the selected engine.
pub fn convert(model: &Model, p: usize, steps: u32, resolution: usize, out: &mut Vec<Command>) {
    let e = model.engine;
    let old = model.pattern(e, p);
    let new = old.convert_timing(steps, resolution);
    let pi = p as i32;
    out.push(msg(id::SET_PATTERN_STEP_COUNT, &[e as i32, -1, -1, pi], &[steps as f32]));
    out.push(msg(id::SET_PATTERN_STEP_LENGTH, &[e as i32, -1, -1, pi], &[resolution as f32]));
    out.push(msg(id::CLEAR_PATTERN, &[e as i32, -1, -1, pi], &[0.0]));
    for (t, tr) in new.tracks.iter().enumerate() {
        for (s, st) in tr.steps.iter().enumerate().filter(|(_, st)| st.on) {
            crate::pattern::write_step(out, e, pi, t, s, st);
        }
    }
}
