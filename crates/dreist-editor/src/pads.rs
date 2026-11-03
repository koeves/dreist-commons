//! Pads panel: [re:1ab7d3] (`PadsPanel.skin`) with one [re:2d30eb]
//! (`Pad.skin`) per visible pad in the `PadsMatrix`.
//!
//! - [re:518b99]: banks A–D show 16 pads in 4×4,
//!   "All" shows 64 in 8×8; the matrix lays out from the bottom
//!   (`optLayoutFromTop="false"`).
//! - [re:ff2d9a] / pad update [re:bc1ce0] /
//!   layer indicators [re:535995]: styles `PadStyle` /
//!   `PadSelectedStyle`, `PadNameLabel[Selected]Style`, layer indicators
//!   `PadLayer{,Selected,Muted,Soloed,MutedSoloed}IndicatorStyle`, name text
//!   per pad mode.
//! - [re:b99f7e]: velocity from the click's
//!   x position (peak at the centre), or with option `PadVelocityBehaviour`
//!   1 − y/height; triggering [re:069c0a] posts `e_playSound` (0x3b).

use dreist_engine::live::PadMode;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::Command;
use crate::model::{LAYERS, Model};
use crate::{EditorError, err};

pub struct Pads {
    /// Path of the `PadsMatrix` control.
    matrix: Vec<usize>,
    /// Path of the `PadBank` button group.
    bank: Option<Vec<usize>>,
    /// Template pad control (`Pad.skin`).
    template: Ctrl,
    /// Which pads are currently instantiated (first, count).
    shown: (usize, usize),
    /// Pad held down by the mouse.
    held: Option<usize>,
    /// Path of the `PadControl` strip (mode, bank, erase, repeat, full level).
    control: Option<Vec<usize>>,
    /// Note repeat base per engine ([re:6e9d2e], not
    /// saved, default 8 = 1/16) for the RepeatCount label and menu.
    repeat_base: [usize; crate::model::ENGINES],
    /// Open menu: 1 repeat timing (RepeatCount), 2 collapsed options.
    menu: u8,
    /// Collapsed options menu actions.
    options: Vec<OptAction>,
    /// Flash overlay per pad slot ([re:2b39ea] pad array).
    flash: [Flash; 64],
    /// Last flash timer tick.
    flash_tick: Option<std::time::Instant>,
    /// Where the held pad was pressed (pad coordinates).
    press: (f32, f32),
}

/// [re:2d30eb] flash overlay.
///
/// - Flash [re:870ce8]: on, alpha 0.65, colour = the
///   style's colAltFill (default Pink).
/// - Flash unless flashing [re:c6c89e]: the same at 0.85 when off.
/// - Flash loop [re:74ec67]: a flash and the loop flag unless
///   looping; ending the loop [re:5b24df] clears it unless the pad is being
///   dragged (set when its drag starts, cleared when the drag ends /
///   mouse up).
/// - Timer tick [re:33a0c6] (every 1/30 s): rising (falling flag clear)
///   alpha += 0.1 up to 0.65, then falling; falling −0.2 down to 0, then
///   off, or rising again while looping. The falling flag is never reset
///   by a flash, so later flashes start falling.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Flash {
    pub on: bool,
    pub looping: bool,
    pub falling: bool,
    pub dragged: bool,
    pub alpha: f32,
}

impl Flash {
    pub fn flash(&mut self) {
        self.on = true;
        self.alpha = 0.65;
    }

    pub fn flash_if_idle(&mut self) {
        if !self.on {
            self.on = true;
            self.alpha = 0.85;
        }
    }

    pub fn flash_loop(&mut self) {
        if !self.looping {
            self.flash();
            self.looping = true;
        }
    }

    pub fn end_loop(&mut self) {
        if !self.dragged {
            self.looping = false;
        }
    }

    /// One timer tick; whether the overlay changed.
    pub fn tick(&mut self) -> bool {
        if !self.on {
            return false;
        }
        if !self.falling {
            self.alpha = (self.alpha + 0.1).min(0.65);
            if (self.alpha - 0.65).abs() <= 1e-6 {
                self.falling = true;
            }
        } else {
            self.alpha = (self.alpha - 0.2).max(0.0);
            if self.alpha.abs() <= 1e-6 {
                if self.looping {
                    self.falling = false;
                } else {
                    self.on = false;
                }
            }
        }
        true
    }
}

/// What is dragged over the pads ([re:2a11ae]).
pub(crate) enum DragFlash {
    /// A pad (or alt held): the pad under the pointer.
    AtPosition,
    /// `n` files from the browser or Finder: pads from the one under the
    /// pointer on.
    Files(usize),
    /// Kit files: every pad.
    All,
}

/// Flash timer period ([re:6556de], 1/30 s).
const FLASH_TICK: f32 = 1.0 / 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum OptAction {
    Erase,
    Repeat,
    RepeatBase(usize),
    RepeatSync,
    FullLevel,
    /// The Full Level slider row ([re:2b6ca2]).
    FullLevelSlider,
    None,
}

/// [re:853d6d] layout of `n` cells (cols × rows) inside `r`, bottom-up.
pub(crate) fn matrix_cells(m: &Ctrl, cols: i32, rows: i32) -> Vec<Rect> {
    let sp = m.attr_i32("intIntraSpacing", 0);
    let (l, rgt, t, b) = (m.attr_i32("intLeftSpacing", 0), m.attr_i32("intRightSpacing", 0), m.attr_i32("intTopSpacing", 0), m.attr_i32("intBottomSpacing", 0));
    let from_top = m.attr_bool("optLayoutFromTop", true);
    let w = (m.rect.w - l - rgt - (cols - 1) * sp) as f32 / cols as f32;
    let h = (m.rect.h - t - b - (rows - 1) * sp) as f32 / rows as f32;
    let mut out = Vec::new();
    for i in 0..cols * rows {
        let (c, r) = (i % cols, i / cols);
        let row = if from_top { r } else { rows - 1 - r };
        let x0 = (l as f32 + c as f32 * (w + sp as f32)).round() as i32;
        let y0 = (t as f32 + row as f32 * (h + sp as f32)).round() as i32;
        let x1 = (l as f32 + c as f32 * (w + sp as f32) + w).round() as i32;
        let y1 = (t as f32 + row as f32 * (h + sp as f32) + h).round() as i32;
        out.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
    }
    out
}

/// [re:114e61]: the bounds filled in the flash
/// colour (style colAltFill, default Pink) at the flash alpha, rounded by
/// the style's corner rounding. Drawn over the pad's children.
pub(crate) fn draw_flash(r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) {
    let Some(a) = c.attr("flashAlpha").and_then(|v| v.parse::<f32>().ok()) else { return };
    let st = r.style(c);
    let col = r.styles.colour(st, "colAltFill", dreist_gui::Colour::rgba(255, 200, 200, 255));
    let radius = r.styles.float(st, "realCornerRounding", 0.0).max(0.0);
    let rect = dreist_gui::paint::RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32);
    p.fill_rounded(rect, radius, [true; 4], col.with_alpha(a));
}

fn velocity_from_click(x: f32, w: f32) -> f32 {
    let half = (w as i32 / 2) as f32;
    if half <= 0.0 {
        return 1.0;
    }
    if x <= half { (x / half).clamp(0.0, 1.0) } else { 1.0 - ((x - half) / half).clamp(0.0, 1.0) }
}

impl Pads {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, model: &Model) -> Result<Pads, EditorError> {
        let host = root.find_mut("PadsPanel").ok_or_else(|| EditorError("RootSkin has no PadsPanel".into()))?;
        skin.mount(host, "PadsPanel.skin").map_err(err)?;
        let rel = skin.find("Pad.skin").unwrap_or_else(|| "Pad Editor/Pad.skin".into());
        let pad_skin = skin.load_skin(&rel).map_err(err)?;
        let mut template = pad_skin.children.into_iter().next().ok_or_else(|| EditorError("empty Pad.skin".into()))?;
        template.overload = "EditorPad".into();
        // The indicators are display-only; clicks go to the pad.
        fn passive(c: &mut Ctrl) {
            for ch in &mut c.children {
                ch.passive = true;
                passive(ch);
            }
        }
        passive(&mut template);
        // [re:114692] user-disables
        // the overlay (hidden and disabled); only x0x shading shows it.
        if let Some(o) = template.find_mut("Overlay") {
            o.enabled = false;
            o.visible = false;
        }
        let matrix = root.path_of("PadsMatrix").ok_or_else(|| EditorError("no PadsMatrix".into()))?;
        let bank = root.path_of("PadBank");
        let control = root.path_of("PadControl");
        let mut pads = Pads {
            matrix,
            bank,
            template,
            shown: (0, 0),
            held: None,
            control,
            repeat_base: [8; crate::model::ENGINES],
            menu: 0,
            options: Vec::new(),
            flash: [Flash::default(); 64],
            flash_tick: None,
            press: (0.0, 0.0),
        };
        pads.layout(root, model);
        Ok(pads)
    }

    /// Force the next `sync` to lay the matrix out again.
    pub fn invalidate(&mut self) {
        self.shown = (usize::MAX, 0);
    }

    fn matrix_mut<'a>(&self, root: &'a mut Ctrl) -> Option<&'a mut Ctrl> {
        let mut c = root;
        for &i in &self.matrix {
            c = c.children.get_mut(i)?;
        }
        Some(c)
    }

    /// [re:ca55ac].
    fn layout(&mut self, root: &mut Ctrl, model: &Model) {
        let vis = model.visible_pads();
        if vis == self.shown {
            return;
        }
        self.shown = vis;
        let dim = if vis.1 == 64 { 8 } else { 4 };
        let template = self.template.clone();
        let Some(m) = self.matrix_mut(root) else { return };
        let cells = matrix_cells(m, dim, dim);
        m.children.clear();
        for (i, cell) in cells.into_iter().enumerate() {
            let mut p = template.clone();
            p.set_attr("padIndex", (vis.0 + i).to_string());
            p.set_rect(cell);
            m.children.push(p);
        }
    }

    fn pad_paths(&self, app: &App) -> Vec<(usize, Vec<usize>)> {
        let Some(m) = app.ctrl(&self.matrix) else { return Vec::new() };
        m.children
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let idx = c.attr("padIndex")?.parse().ok()?;
                let mut p = self.matrix.clone();
                p.push(i);
                Some((idx, p))
            })
            .collect()
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        self.layout(&mut app.root, model);
        self.sync_controls(app, model);
        let e = model.engine;
        let mode = model.pad_mode(e);
        let selected = model.pad[e];
        let sel_mask = crate::multipad::selection(model, e).1;
        let (_, count) = model.visible_pads();
        if let Some(bp) = &self.bank {
            if let Some(g) = app.ctrl_mut(bp) {
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = i == model.bank;
                }
            }
        }
        // x0x: the current pad's track in the current pattern.
        let x0x_pat = (mode == PadMode::X0x).then(|| model.pattern(e, model.current_pattern(e)));
        for (idx, path) in self.pad_paths(app) {
            let name = model.pad_name(e, idx);
            let label = match mode {
                PadMode::Chromatic => format!("{:+03} st", (idx as i64 - selected as i64).clamp(-48, 48)),
                PadMode::Velocity => format!("{:03}", ((idx % count) + 1) * 127 / count),
                PadMode::MuteAndSolo => match idx {
                    0..8 => format!("{} Mute", idx + 1),
                    8..16 => format!("{} Solo", idx - 7),
                    _ => String::new(),
                },
                _ => name.unwrap_or_else(|| format!("{}", idx + 1)),
            };
            let sel = idx == selected || sel_mask & (1u64 << idx.min(63)) != 0;
            let x0x = mode == PadMode::X0x;
            let (mute, solo) = (model.pad_mute(e, idx), model.pad_solo(e, idx));
            let layers: Vec<_> = (0..LAYERS).map(|l| model.layer(e, idx, l)).collect();
            let locked = crate::padmenu::pad_locked(model, e, idx);
            let colour = model.pad_node(e, idx).and_then(|n| n.attr("PadColour")).map(str::to_string);
            // [re:39f304] / [re:6a00c8]
            //  [re:274980]: pads past the step count are
            // disabled; an on step (enabled pad) with velocity ≈ 0 shows as
            // off, ≥ 1 as accent, else on; on / accent use the selected
            // label styles.
            let step_state = x0x_pat.as_ref().map(|p| {
                let enabled = idx < p.steps as usize;
                let st = p.tracks.get(selected).map(|t| t.step(idx as u32));
                match st {
                    Some(s) if enabled && s.on && s.lanes[0].abs() > 1e-6 => (enabled, if s.lanes[0] >= 1.0 { 2 } else { 1 }),
                    _ => (enabled, 0),
                }
            });
            let Some(pad) = app.ctrl_mut(&path) else { continue };
            pad.style = if sel { "PadSelectedStyle" } else { "PadStyle" }.into();
            pad.enabled = step_state.is_none_or(|s| s.0);
            if let Some((_, state)) = step_state {
                pad.style = match (state, sel) {
                    (2, true) => "PadAccentSelectedStyle",
                    (2, false) => "PadAccentStyle",
                    (1, true) => "PadOnSelectedStyle",
                    (1, false) => "PadOnStyle",
                    (_, true) => "PadOffSelectedStyle",
                    _ => "PadOffStyle",
                }
                .into();
            }
            let lit = step_state.is_some_and(|s| s.1 > 0);
            // x0x: the overlay marks the steps of odd shading groups
            // (step-state style → `Overlay` shown).
            let shade = x0x_pat.as_ref().map(|p| crate::pattern::shaded(p.resolution, idx));
            for c in pad.children.iter_mut() {
                match c.name.as_str() {
                    "Overlay" => {
                        if let Some(v) = shade {
                            c.visible = v;
                        }
                    }
                    "NameLabel" => {
                        c.text = label.clone();
                        c.style = if (sel && !x0x) || lit { "PadNameLabelSelectedStyle" } else { "PadNameLabelStyle" }.into();
                    }
                    "StepLabel" => {
                        c.visible = x0x;
                        c.text = format!("S {}", idx + 1);
                        c.style = if (sel && !x0x) || lit { "PadStepNameLabelSelectedStyle" } else { "PadStepNameLabelStyle" }.into();
                    }
                    "MuteIndi" => {
                        c.visible = !x0x;
                        c.on = mute;
                    }
                    "SoloIndi" => {
                        c.visible = !x0x;
                        c.on = solo;
                    }
                    "ColourIndi" => {
                        c.visible = !x0x;
                        match &colour {
                            Some(v) => c.set_attr("fillColour", v.clone()),
                            None => c.remove_attr("fillColour"),
                        };
                    }
                    "LockIndi" => c.on = locked,
                    _ => {}
                }
                for li in c.children.iter_mut() {
                    let Some(l) = li.name.strip_prefix("LI").and_then(|n| n.parse::<usize>().ok()) else { continue };
                    let f = &layers[l.min(LAYERS - 1)];
                    li.style = if !f.present {
                        "PadLayerIndicatorStyle"
                    } else if f.mute && f.solo {
                        "PadLayerMutedSoloedIndicatorStyle"
                    } else if f.mute {
                        "PadLayerMutedIndicatorStyle"
                    } else if f.solo {
                        "PadLayerSoloedIndicatorStyle"
                    } else if sel && f.valid {
                        "PadLayerIndicatorSelectedStyle"
                    } else {
                        "PadLayerIndicatorStyle"
                    }
                    .into();
                    li.on = f.present && (f.valid || f.mute || f.solo);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn matrix(&self) -> &[usize] {
        &self.matrix
    }

    /// Whether `path` is inside the pads matrix ([re:1ab7d3] drag target).
    pub fn in_matrix(&self, path: &[usize]) -> bool {
        path.starts_with(&self.matrix)
    }

    /// [re:4288a6]: flash the pads a drop at (`x`, `y`) (window
    /// coordinates) would fill. Nothing changes over a locked pad.
    pub(crate) fn drag_flash(&mut self, app: &App, model: &Model, what: DragFlash, x: f32, y: f32) {
        let (ox, oy) = app.origin(&self.matrix);
        let Some(m) = app.ctrl(&self.matrix) else { return };
        // Pad rects shrunk by 2 px (flashing pads at a position [re:288c64]).
        let inside = |c: &Ctrl| {
            let (px, py) = (x as i32 - ox, y as i32 - oy);
            let r = c.rect;
            c.visible && py >= r.y && py < r.y + (r.h - 2).max(0) && px >= r.x && px < r.x + (r.w - 2).max(0)
        };
        let hit = m.children.iter().position(inside);
        let e = model.engine;
        if let Some(i) = hit {
            let idx = m.children[i].attr("padIndex").and_then(|v| v.parse().ok()).unwrap_or(i);
            if crate::padmenu::pad_locked(model, e, idx) {
                return;
            }
        }
        for (i, c) in m.children.iter().enumerate().take(64) {
            if !c.visible {
                continue;
            }
            let f = &mut self.flash[i];
            let on = match what {
                DragFlash::All => {
                    f.flash_loop();
                    continue;
                }
                DragFlash::AtPosition => hit == Some(i),
                // A miss (−1) still covers [−1, n − 1) as the original does.
                DragFlash::Files(n) => {
                    let h = hit.map_or(-1, |h| h as i64);
                    (i as i64) >= h && (i as i64) < h + n as i64
                }
            };
            if on {
                f.flash_loop();
            } else {
                f.end_loop();
            }
        }
    }

    /// [re:8d9b8a] / drag end: every loop ends.
    pub fn end_drag_flash(&mut self) {
        for f in &mut self.flash {
            f.dragged = false;
            f.end_loop();
        }
    }

    /// Per-frame flash timer ([re:ba3c9b]), and
    /// [re:bd4d75]: in x0x mode, while the pattern
    /// plays with its power on, the current step's pad flashes
    /// (unless already flashing). Returns whether a pad changed.
    pub fn animate(&mut self, app: &mut App, model: &Model) -> bool {
        let e = model.engine;
        if model.pad_mode(e) == PadMode::X0x && model.pattern_power(e) {
            if let Some(pos) = model.steps[e] {
                let pat = model.pattern(e, model.current_pattern(e));
                let len = pat.tracks.get(model.pad[e]).map(|t| t.length.max(1)).unwrap_or(pat.steps).max(1) as usize;
                let step = pos as usize % len;
                if let Some(f) = self.flash.get_mut(step) {
                    f.flash_if_idle();
                }
            }
        }
        let now = std::time::Instant::now();
        if !self.flash.iter().any(|f| f.on) {
            self.flash_tick = None;
            return self.write_flash(app);
        }
        let last = *self.flash_tick.get_or_insert(now);
        let ticks = (now.duration_since(last).as_secs_f32() / FLASH_TICK).floor() as u32;
        if ticks > 0 {
            self.flash_tick = Some(now);
            for f in &mut self.flash {
                for _ in 0..ticks.min(30) {
                    f.tick();
                }
            }
        }
        self.write_flash(app)
    }

    /// Mirror the flash alphas into the pad controls (`flashAlpha`, drawn by
    /// `draw_flash`).
    fn write_flash(&self, app: &mut App) -> bool {
        let Some(m) = self.matrix_mut(&mut app.root) else { return false };
        let mut changed = false;
        for (i, c) in m.children.iter_mut().enumerate() {
            let a = self.flash.get(i).filter(|f| f.on).map(|f| f.alpha);
            let old = c.attr("flashAlpha").and_then(|v| v.parse::<f32>().ok());
            if a != old {
                changed = true;
                match a {
                    Some(a) => c.set_attr("flashAlpha", a.to_string()),
                    None => c.remove_attr("flashAlpha"),
                };
            }
        }
        changed
    }

    /// Returns true when the model changed (needs `sync`).
    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        match ev {
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "EditorPad" => {
                let Some(pad) = app.ctrl(path) else { return false };
                let Some(idx) = pad.attr("padIndex").and_then(|v| v.parse::<usize>().ok()) else { return false };
                let e = model.engine;
                match kind {
                    MouseKind::Down => {
                        // Option `PadVelocityBehaviour` ([re:a95e11]):
                        // the height instead, loudest at the top.
                        let vel = if crate::options::Options::load().bool("PadVelocityBehaviour", false) {
                            1.0 - (*y / pad.rect.h.max(1) as f32).clamp(0.0, 1.0)
                        } else {
                            velocity_from_click(*x, pad.rect.w as f32)
                        };
                        // `DoMouseDown*Mode`: every press flashes the pad.
                        if let Some(f) = path.last().and_then(|&i| self.flash.get_mut(i)) {
                            f.flash();
                        }
                        self.press = (*x, *y);
                        out.push(Command::PadHit { engine: e, pad: idx, velocity: (vel * 127.0) as u32 });
                        self.held = Some(idx);
                        // [re:e066f0]: cmd toggles, shift extends.
                        out.push(crate::multipad::select(model, e, idx, app.mods.cmd, app.mods.shift));
                        true
                    }
                    MouseKind::Drag => {
                        // Mouse move, normal mode [re:10ff5a]: past 10 px (or
                        // outside the pad) the drag starts and the pad loops.
                        let (w, h) = (pad.rect.w as f32, pad.rect.h as f32);
                        let out_of = *x < 0.0 || *y < 0.0 || *x >= w || *y >= h;
                        if self.held.is_some() && ((x - self.press.0).abs() > 10.0 || (y - self.press.1).abs() > 10.0 || out_of) {
                            if let Some(f) = path.last().and_then(|&i| self.flash.get_mut(i)) {
                                f.dragged = true;
                                f.flash_loop();
                            }
                        }
                        false
                    }
                    MouseKind::Up => {
                        if let Some(f) = path.last().and_then(|&i| self.flash.get_mut(i)) {
                            f.dragged = false;
                        }
                        if let Some(p) = self.held.take() {
                            out.push(Command::PadRelease { engine: e, pad: p });
                        }
                        false
                    }
                    _ => false,
                }
            }
            UiEvent::Clicked { path, .. } => {
                let Some(bp) = &self.bank else { return false };
                if path.len() == bp.len() + 1 && path.starts_with(bp) {
                    model.bank = *path.last().unwrap();
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

/// `EngineState` flag from the document.
fn state_flag(model: &Model, e: usize, key: &str, default: bool) -> bool {
    model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "EngineState")).and_then(|s| s.bool(key)).unwrap_or(default)
}

impl Pads {
    fn ctrl_path(&self, app: &App, name: &str) -> Option<Vec<usize>> {
        let base = self.control.clone()?;
        let c = app.ctrl(&base)?;
        let i = c.children.iter().position(|k| k.name == name)?;
        Some([base, vec![i]].concat())
    }

    /// [re:032c62] + the visibility rules:
    /// [re:0c84c6] sets them per pad
    /// mode, then notifies [re:91eddf], whose listener
    /// ([re:d830f2]) sets each of FullLevelKnob + FullLevelButton,
    /// RepeatButton + RepeatCount and EraseButton visible exactly when its
    /// right edge is left of the panel width − 25, and Options when any is
    /// not — so the fit decides.
    fn sync_controls(&mut self, app: &mut App, model: &Model) {
        let e = model.engine;
        let erase = state_flag(model, e, "NoteEraseEnabled", false);
        let repeat = state_flag(model, e, "NoteRepeatEnabled", false);
        let full = state_flag(model, e, "FullLevelEnabled", false);
        let levels =
            model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "EngineState")).and_then(|s| s.attr("FullLevels")).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(127.0);
        let base = self.repeat_base[e.min(crate::model::ENGINES - 1)];
        let Some(cp) = self.control.clone() else { return };
        let panel_w = app.ctrl(&cp[..cp.len() - 1]).map(|c| c.rect.w).unwrap_or(0);
        let Some(c) = app.ctrl_mut(&cp) else { return };
        let limit = panel_w - 0x19;
        let right = |c: &Ctrl, n: &str| c.children.iter().find(|k| k.name == n).map(|k| k.rect.x + k.rect.w).unwrap_or(0);
        let (full_fits, rep_fits, erase_fits) = (right(c, "FullLevelKnob") < limit, right(c, "RepeatCount") < limit, right(c, "EraseButton") < limit);
        for k in &mut c.children {
            let vis = match k.name.as_str() {
                "EraseButton" => {
                    k.on = erase;
                    Some(erase_fits)
                }
                "RepeatButton" => {
                    k.on = repeat;
                    Some(rep_fits)
                }
                "RepeatCount" => {
                    k.text = dreist_engine::pattern::RESOLUTION_NAMES.get(base).copied().unwrap_or("1/16").to_string();
                    Some(rep_fits)
                }
                "FullLevelButton" => {
                    k.on = full;
                    Some(full_fits)
                }
                "FullLevelKnob" => {
                    k.value = levels * 0.007874016;
                    Some(full_fits)
                }
                "Options" => Some(!(full_fits && rep_fits && erase_fits)),
                _ => None,
            };
            if let Some(v) = vis {
                k.visible = v;
                k.enabled = v;
            }
        }
    }

    /// [re:a04671]: the 14 resolution names (current base
    /// checked) then "Sync to next division".
    fn repeat_items(&self, model: &Model) -> Vec<(String, OptAction)> {
        let e = model.engine;
        let base = self.repeat_base[e.min(crate::model::ENGINES - 1)];
        let check = |on: bool, t: &str| if on { format!("✓ {t}") } else { t.to_string() };
        let mut v: Vec<(String, OptAction)> = dreist_engine::pattern::RESOLUTION_NAMES.iter().enumerate().map(|(i, n)| (check(i == base, n), OptAction::RepeatBase(i))).collect();
        v.push((String::new(), OptAction::None));
        v.push((check(state_flag(model, e, "NoteRepeatSynced", true), "Sync to next division"), OptAction::RepeatSync));
        v
    }

    /// The strip's buttons, knob and menus (skin lambdas 0x1e
    /// `e_setNoteEraseState`, 0x1f `e_setNoteRepeatState`, the
    /// timing menu (0x20 `e_setNoteRepeatBase` [e] [i]; sync writes the
    /// engine state directly), 0x22 `e_setFullLevelLevel` [e] [knob],
    /// 0x21 `e_setFullLevelState`, the collapsed options menu).
    pub fn handle_controls(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        use dreist_engine::binding::id;
        let e = model.engine;
        let Some(cp) = self.control.clone() else { return false };
        let msg1 = |kind: u32, v: f32| crate::command::msg(kind, &[e as i32], &[v]);
        let set_state = |model: &mut Model, key: &str, v: String| {
            model.engine_mut(e).child_mut_or_insert("EngineState", "EngineState").set(key, v);
        };
        let run = |me: &mut Self, model: &mut Model, a: OptAction, out: &mut Vec<Command>| match a {
            OptAction::Erase => {
                let on = !state_flag(model, e, "NoteEraseEnabled", false);
                out.push(msg1(id::SET_NOTE_ERASE_STATE, on as i32 as f32));
            }
            OptAction::Repeat => {
                let on = !state_flag(model, e, "NoteRepeatEnabled", false);
                out.push(msg1(id::SET_NOTE_REPEAT_STATE, on as i32 as f32));
            }
            OptAction::FullLevel => {
                let on = !state_flag(model, e, "FullLevelEnabled", false);
                out.push(msg1(id::SET_FULL_LEVEL_STATE, on as i32 as f32));
            }
            OptAction::RepeatBase(i) => {
                me.repeat_base[e.min(crate::model::ENGINES - 1)] = i;
                // The engine reads round(v·13).
                out.push(msg1(id::SET_NOTE_REPEAT_BASE, i as f32 / 13.0));
            }
            OptAction::RepeatSync => {
                let on = !state_flag(model, e, "NoteRepeatSynced", true);
                set_state(model, "NoteRepeatSynced", on.to_string());
                out.push(Command::NoteRepeatSync(e, on));
            }
            OptAction::FullLevelSlider | OptAction::None => {}
        };
        match ev {
            UiEvent::Clicked { path, name, on, .. } if path.starts_with(&cp) => match name.as_str() {
                "EraseButton" => {
                    out.push(msg1(id::SET_NOTE_ERASE_STATE, *on as i32 as f32));
                    true
                }
                "RepeatButton" => {
                    out.push(msg1(id::SET_NOTE_REPEAT_STATE, *on as i32 as f32));
                    true
                }
                "FullLevelButton" => {
                    out.push(msg1(id::SET_FULL_LEVEL_STATE, *on as i32 as f32));
                    true
                }
                "RepeatCount" => {
                    let items = self.repeat_items(model);
                    let (labels, acts): (Vec<String>, Vec<OptAction>) = items.into_iter().unzip();
                    self.options = acts;
                    self.menu = 1;
                    app.open_menu(path, labels, None);
                    false
                }
                // [re:48766b]: the hidden controls as menu items.
                "Options" => {
                    let hidden = |n: &str| self.ctrl_path(app, n).and_then(|p| app.ctrl(&p).map(|c| !c.visible)).unwrap_or(false);
                    let check = |on: bool, t: &str| if on { format!("✓ {t}") } else { t.to_string() };
                    let mut items: Vec<(String, OptAction)> = Vec::new();
                    if hidden("EraseButton") {
                        items.push((check(state_flag(model, e, "NoteEraseEnabled", false), "Erase"), OptAction::Erase));
                    }
                    if hidden("RepeatButton") {
                        if !items.is_empty() {
                            items.push((String::new(), OptAction::None));
                        }
                        items.push((check(state_flag(model, e, "NoteRepeatEnabled", false), "Repeat"), OptAction::Repeat));
                        items.extend(self.repeat_items(model).into_iter().map(|(t, a)| (if t.is_empty() { t } else { format!("    {t}") }, a)));
                    }
                    if hidden("FullLevelKnob") {
                        if !items.is_empty() {
                            items.push((String::new(), OptAction::None));
                        }
                        items.push((check(state_flag(model, e, "FullLevelEnabled", false), "Full Level"), OptAction::FullLevel));
                        // [re:2b6ca2] 120 × 15, SliderStyle,
                        // value = the knob's.
                        let v = self.ctrl_path(app, "FullLevelKnob").and_then(|p| app.ctrl(&p).map(|c| c.value)).unwrap_or(1.0);
                        items.push((dreist_gui::menu::slider_item(v), OptAction::FullLevelSlider));
                    }
                    let (labels, acts): (Vec<String>, Vec<OptAction>) = items.into_iter().unzip();
                    self.options = acts;
                    self.menu = 2;
                    app.open_menu(path, labels, None);
                    false
                }
                _ => false,
            },
            UiEvent::ValueChanged { path, name, value, .. } if name == "FullLevelKnob" && path.starts_with(&cp) => {
                out.push(msg1(id::SET_FULL_LEVEL_LEVEL, *value));
                true
            }
            UiEvent::MenuSlider { anchor, index, value } if self.menu == 2 && anchor.starts_with(&cp) => {
                if self.options.get(*index) == Some(&OptAction::FullLevelSlider) {
                    if let Some(k) = self.ctrl_path(app, "FullLevelKnob").and_then(|p| app.ctrl_mut(&p)) {
                        k.value = *value;
                    }
                    out.push(msg1(id::SET_FULL_LEVEL_LEVEL, *value));
                }
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.menu != 0 && anchor.starts_with(&cp) => {
                self.menu = 0;
                let a = self.options.get(*index).copied().unwrap_or(OptAction::None);
                run(self, model, a, out);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn flash_rises_falls_and_loops() {
        let mut f = Flash::default();
        f.flash();
        assert!(f.tick() && f.falling && (f.alpha - 0.65).abs() < 1e-6);
        for _ in 0..4 {
            f.tick();
        }
        assert!(!f.on);
        f.flash_loop();
        for _ in 0..4 {
            f.tick();
        }
        assert!(f.on && !f.falling && f.alpha.abs() < 1e-6);
        f.tick();
        assert!((f.alpha - 0.1).abs() < 1e-6);
        f.dragged = true;
        f.end_loop();
        assert!(f.looping);
        f.dragged = false;
        f.end_loop();
        assert!(!f.looping);
        let mut g = Flash::default();
        g.flash_if_idle();
        assert!((g.alpha - 0.85).abs() < 1e-6);
    }

    use super::*;

    #[test]
    fn click_velocity_peaks_in_the_middle() {
        assert_eq!(velocity_from_click(0.0, 80.0), 0.0);
        assert_eq!(velocity_from_click(40.0, 80.0), 1.0);
        assert!((velocity_from_click(60.0, 80.0) - 0.5).abs() < 1e-6);
    }
}
