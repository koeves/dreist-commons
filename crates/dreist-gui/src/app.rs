//! Windowless GUI state: the control tree, hit testing and the stock
//! controls' mouse behaviour ([re:8638cc]
//! [re:0a434b]/[re:c53c76], knob/slider dragging). Editor panels observe the
//! returned [`UiEvent`]s.

use crate::ctrl::{Ctrl, Rect};
use crate::skin::Skin;
use crate::text::Fonts;

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    /// A button was clicked (`on` is the new state for latching buttons).
    Clicked { path: Vec<usize>, name: String, id: i32, on: bool },
    /// A knob or slider moved to `value` (0..1).
    ValueChanged { path: Vec<usize>, name: String, id: i32, value: f32 },
    /// A text box edit was committed (Enter or focus loss).
    TextCommitted { path: Vec<usize>, name: String, text: String },
    /// Mouse wheel over a custom control (`dy` in lines, positive = up).
    Wheel { path: Vec<usize>, overload: String, dy: f32 },
    /// A popup menu item was chosen.
    MenuSelected { anchor: Vec<usize>, name: String, index: usize },
    /// A keyboard shortcut while no text box is being edited: "c", "v",
    /// "x", "a", "d" with cmd, or "delete", "left", "right", "up", "down".
    Shortcut { key: String, cmd: bool, shift: bool, alt: bool, ctrl: bool },
    /// Right click on a stock interactive control.
    RightClicked { path: Vec<usize>, name: String },
    /// Left button went down on a stock interactive control (before any
    /// change it causes).
    Pressed { path: Vec<usize>, name: String },
    /// Mouse on a custom control (non-empty `overload`): position local to it.
    Mouse { path: Vec<usize>, overload: String, kind: MouseKind, x: f32, y: f32 },
    /// A second left press on the same control within the double-click
    /// time (event 8 for stock controls), after
    /// that press's own events.
    DoubleClicked { path: Vec<usize>, name: String },
    /// A left drag that started on control `from` (custom or stock) ended
    /// over a different control: `to` is the full hit path under the
    /// pointer (deepest last), `x`/`y` in root coordinates
    /// ([re:4dff70] → the target's drop handler).
    Dropped { from: Vec<usize>, to: Vec<usize>, x: f32, y: f32 },
    /// A drag from `from` (past the threshold) is over `to` (full hit path),
    /// for drop-target feedback and drag cursors.
    DragOver { from: Vec<usize>, to: Vec<usize>, x: f32, y: f32 },
    /// The pointer moved over a custom control with no button held
    /// (a mouse move without a press), position local to it.
    Hover { path: Vec<usize>, overload: String, x: f32, y: f32 },
    /// The pointer entered another interactive control (or left them all):
    /// the [re:944095] 0x80 / 0x100 enter / leave listeners.
    HoverChanged { path: Option<Vec<usize>> },
    /// After each pointer move, press or wheel: the owner shows the
    /// tooltip of the control at `path` (or a parent) with
    /// [`App::set_tooltip`], or hides it ([re:038e5e]).
    ToolTip { path: Vec<usize> },
    /// A menu slider item ([re:2b6ca2]) was set to `value`;
    /// the menu stays open.
    MenuSlider { anchor: Vec<usize>, index: usize, value: f32 },
    /// A [re:6b287b] starts (`end` false) or finishes folding
    /// (`iconic`) or unfolding: the collapse / expand start and end
    /// notifications.
    Collapse { path: Vec<usize>, iconic: bool, end: bool },
    /// A rect animation started with [`App::animate_rect`] finished.
    Animated { path: Vec<usize>, tag: u32 },
    /// Files dragged in from outside (Finder) over `to`; `dropped` on
    /// release ([re:f27b3f] `draggingUpdated:` /
    /// `performDragOperation:` → [re:deab1e]).
    Files { to: Vec<usize>, files: Vec<std::path::PathBuf>, x: f32, y: f32, dropped: bool },
    /// A drag from `from` left the window ([re:be4a8a]):
    /// the editor may put files in [`App::drag_out`] to hand the drag to
    /// the OS.
    DragOut { from: Vec<usize> },
    /// A [re:e58107]'s bar moved to `pos` (dragged or animated);
    /// `end` when the drag or animation finished (the original then writes
    /// the position under `strAutoSaveName`).
    Split { path: Vec<usize>, pos: i32, end: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    RightDown,
    Drag,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

/// Paths of named controls for panels that look the same names up on every
/// sync: the first control called `name` under a scope (as
/// [`Ctrl::path_of`] finds it), remembered and found again when the
/// control at the remembered path is gone or renamed.
#[derive(Debug, Default)]
pub struct PathCache(std::cell::RefCell<std::collections::HashMap<&'static str, Vec<usize>>>);

impl PathCache {
    /// The path of `name` under the control at `scope`.
    pub fn path_in(&self, app: &App, scope: &[usize], name: &'static str) -> Option<Vec<usize>> {
        let mut paths = self.0.borrow_mut();
        if let Some(p) = paths.get(name).filter(|p| p.starts_with(scope) && app.ctrl(p).is_some_and(|c| c.name == name)) {
            return Some(p.clone());
        }
        let p = [scope, &app.ctrl(scope)?.path_of(name)?].concat();
        paths.insert(name, p.clone());
        Some(p)
    }

    /// The first control called `name` in the whole tree.
    pub fn ctrl_mut<'a>(&self, app: &'a mut App, name: &'static str) -> Option<&'a mut Ctrl> {
        let p = self.path_in(app, &[], name)?;
        app.ctrl_mut(&p)
    }
}

pub struct App {
    pub skin: Skin,
    pub root: Ctrl,
    pub fonts: Fonts,
    /// The whole window needs drawing.
    pub dirty: bool,
    /// Rectangles (root coordinates) that need drawing when `dirty` is
    /// not set: continuous updates (meters) repaint only their controls.
    damage: Vec<crate::ctrl::Rect>,
    /// The last frame and its scale, updated in place for damage.
    frame: Option<(f32, tiny_skia::Pixmap)>,
    /// Where the last full frame's custom drawing went (root coordinates).
    custom_rects: Vec<crate::ctrl::Rect>,
    /// The custom-drawn controls of the last full frame (paths) with the
    /// fingerprint of their drawing as the frame shows it
    /// ([`crate::render::replay_custom`]; `None`: unknown).
    custom_marks: Vec<(Vec<usize>, Option<u64>)>,
    /// Controls in the last [`App::marks`] (its vector's size next time).
    marks_len: std::cell::Cell<usize>,
    hover: Option<Vec<usize>>,
    pressed: Option<(Vec<usize>, f32, f32, f32)>,
    custom_pressed: Option<Vec<usize>>,
    /// Custom control under a right-button press (right drags erase in the
    /// pattern grid): moves send `Drag`, the release `Up`.
    custom_right: Option<Vec<usize>>,
    pub menu: Option<crate::menu::Menu>,
    /// Text box being edited: path, original text.
    editing: Option<(Vec<usize>, String)>,
    mouse: (f32, f32),
    /// Keyboard modifiers held during the last input event.
    pub mods: Mods,
    /// UI scale (`LastEditorScaleFactor` / 100): window size, drawing and
    /// mouse coordinates are multiplied by it.
    pub ui_scale: f32,
    /// Last left press (control, time) for double clicks.
    last_press: Option<(Vec<usize>, std::time::Instant)>,
    /// Where the current left press started (for drag and drop).
    press_at: (f32, f32),
    /// The current press travelled past the drag threshold.
    pub dragging: bool,
    /// Files for an OS drag out of the window ([re:6367bf]),
    /// taken by the window.
    pub drag_out: Option<Vec<std::path::PathBuf>>,
    /// The current drag already left the window.
    drag_left: bool,
    /// Menu slider item being dragged.
    menu_slider: Option<usize>,
    /// The UI scale changed: the window should resize to size × scale.
    pub resize_request: bool,
    /// Shift alone turns knobs ten times slower while dragging (the
    /// original v1's fine mode: 2000 px a turn, [re:427435]). Off by default.
    pub fine_drag: bool,
    /// Ctrl + left button acts as the right button (the original v1's
    /// one-button-mouse remap, [re:ac4dad]). Off by default.
    pub ctrl_click_right: bool,
    /// The current press is a remapped ctrl-click.
    ctrl_press: bool,
    /// Running collapse panel folds.
    folds: Vec<crate::collapse::Anim>,
    /// The cursor the window should show ([re:752b0e]); reset to
    /// the arrow on each move without a press, handlers set others.
    pub cursor: crate::cursor::Cursor,
    /// Splitter bar drag: panel, position at the press.
    split_drag: Option<(Vec<usize>, i32)>,
    /// Running splitter animations.
    splits: Vec<crate::splitter::Anim>,
    /// Window sizer drag: the anchor point in window pixels.
    sizer_drag: Option<(f32, f32)>,
    /// The shown tooltip ([re:403a0f]).
    pub tooltip: Option<crate::tooltip::Tip>,
    /// The menu the last frame shows: a change (opened, closed, hovered)
    /// repaints only the menu's rectangles.
    menu_drawn: Option<crate::menu::Menu>,
    /// The last frame's pixels under the open menu without it (scale,
    /// root-coordinate rectangle, pixels): a hover redraws the menu over
    /// them instead of the controls beneath.
    menu_under: Option<(f32, crate::ctrl::Rect, tiny_skia::Pixmap)>,
}

/// Editor open → [re:680872].
pub const MIN_WINDOW: (i32, i32) = (1150, 650);

/// Pixels the pointer must travel before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 4.0;

/// Double-click interval.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

/// Modifier keys ([re:4a79c9] event modifier flags).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub cmd: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Pixels of vertical drag for a full knob turn (`realMacroValueScalar`
/// default 200).
const DRAG_RANGE: f32 = 200.0;

impl App {
    pub fn new(skin: Skin, root: Ctrl) -> App {
        let fonts = Fonts::new(&skin.font_dir());
        App {
            skin,
            root,
            fonts,
            dirty: true,
            damage: Vec::new(),
            frame: None,
            custom_rects: Vec::new(),
            custom_marks: Vec::new(),
            marks_len: std::cell::Cell::new(0),
            hover: None,
            pressed: None,
            custom_pressed: None,
            custom_right: None,
            menu: None,
            editing: None,
            mouse: (0.0, 0.0),
            mods: Mods::default(),
            ui_scale: 1.0,
            last_press: None,
            press_at: (0.0, 0.0),
            dragging: false,
            drag_out: None,
            drag_left: false,
            menu_slider: None,
            folds: Vec::new(),
            resize_request: false,
            fine_drag: false,
            ctrl_click_right: false,
            ctrl_press: false,
            cursor: crate::cursor::Cursor::Arrow,
            split_drag: None,
            splits: Vec::new(),
            sizer_drag: None,
            tooltip: None,
            menu_drawn: None,
            menu_under: None,
        }
    }

    /// [re:b82852]: a 15×15
    /// [re:12632f] "WindowSizer" in the bottom right corner (z-order
    /// 999), kept there as the root resizes.
    pub fn set_has_window_sizer(&mut self) {
        if self.root.children.iter().any(|c| c.class == "CtrlWindowSizer") {
            return;
        }
        use crate::ctrl::Anchor;
        let (w, h) = (self.root.rect.w, self.root.rect.h);
        let mut c = Ctrl { class: "CtrlWindowSizer".into(), name: "WindowSizer".into(), rect: Rect::new(w - 15, h - 15, 15, 15), zorder: 999, visible: true, enabled: true, ..Ctrl::default() };
        c.anchors = [(Anchor::FixedFar, 15.0), (Anchor::FixedFar, 15.0), (Anchor::FixedFar, 15.0), (Anchor::FixedFar, 15.0)];
        self.root.children.push(c);
    }

    pub fn size(&self) -> (i32, i32) {
        (self.root.rect.w, self.root.rect.h)
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        let r = self.root.rect;
        if (r.w, r.h) != (w, h) {
            self.root.set_rect(Rect::new(r.x, r.y, w, h));
            self.dirty = true;
        }
    }

    pub fn ctrl(&self, path: &[usize]) -> Option<&Ctrl> {
        let mut c = &self.root;
        for &i in path {
            c = c.children.get(i)?;
        }
        Some(c)
    }

    /// Switch the style theme ([re:cac947]): the skin's
    /// stylesheets reload and everything repaints. False when it fails to
    /// load (the current theme stays).
    pub fn set_theme(&mut self, theme: &str) -> bool {
        match Skin::load(&self.skin.dir, theme) {
            Ok(s) => {
                self.skin = s;
                self.frame = None;
                self.dirty = true;
                true
            }
            Err(_) => false,
        }
    }

    /// The control and all its parents shown.
    pub fn shown(&self, path: &[usize]) -> bool {
        let mut c = &self.root;
        for &i in path {
            match c.children.get(i) {
                Some(k) if k.visible => c = k,
                _ => return false,
            }
        }
        c.visible
    }

    pub fn ctrl_mut(&mut self, path: &[usize]) -> Option<&mut Ctrl> {
        let mut c = &mut self.root;
        for &i in path {
            c = c.children.get_mut(i)?;
        }
        Some(c)
    }

    /// The control a tooltip is asked of: the pressed (captured) control
    /// while a button is held, else the deepest one under the pointer.
    pub fn tooltip_target(&self) -> Vec<usize> {
        if let Some(p) = self.custom_pressed.clone().or_else(|| self.pressed.as_ref().map(|p| p.0.clone())) {
            return p;
        }
        let (x, y) = self.mouse;
        self.hit(x, y)
    }

    /// Show `text` against the control at `anchor` ([re:c62b0b]
    /// , immediate); empty or blank text hides it.
    pub fn set_tooltip(&mut self, anchor: &[usize], text: &str, pos: crate::tooltip::Pos) {
        let a = self.abs_rect(anchor);
        self.set_tooltip_at(a, text, pos);
    }

    /// The delayed form (not immediate, e.g. the browser
    /// path buttons on mouse enter): shown after 50 ticks of the pointer
    /// staying within 4 px.
    pub fn set_tooltip_delayed(&mut self, a: crate::ctrl::Rect, text: &str, pos: crate::tooltip::Pos) {
        let now = std::time::Instant::now();
        let mouse = self.mouse;
        if let Some(t) = self.tooltip.as_mut().filter(|t| t.text == text) {
            if let Some((at, m)) = &mut t.pending {
                if ((mouse.0 - m.0).powi(2) + (mouse.1 - m.1).powi(2)).sqrt() > 4.0 {
                    *at = now + crate::tooltip::SHOW_DELAY;
                    *m = mouse;
                    t.hide_at = *at + crate::tooltip::AUTO_HIDE;
                }
            }
            return;
        }
        self.set_tooltip_at(a, text, pos);
        if let Some(t) = self.tooltip.as_mut().filter(|t| t.text == text) {
            t.pending = Some((now + crate::tooltip::SHOW_DELAY, mouse));
            t.hide_at = now + crate::tooltip::SHOW_DELAY + crate::tooltip::AUTO_HIDE;
        }
    }

    /// As [`App::set_tooltip`] against rectangle `a` (root coordinates;
    /// the providers' rect offsets).
    pub fn set_tooltip_at(&mut self, a: crate::ctrl::Rect, text: &str, pos: crate::tooltip::Pos) {
        if text.trim().is_empty() || self.menu.is_some() {
            self.hide_tooltip();
            return;
        }
        if self.tooltip.as_ref().is_some_and(|t| t.text == text) {
            return;
        }
        let rect = crate::tooltip::layout(&mut self.fonts, &self.skin.styles, text, a, pos, (self.root.rect.w, self.root.rect.h));
        let tip = crate::tooltip::Tip { text: text.to_string(), rect, pos, hide_at: std::time::Instant::now() + crate::tooltip::AUTO_HIDE, pending: None };
        self.hide_tooltip();
        self.invalidate_rect(tip.damage());
        self.tooltip = Some(tip);
    }

    /// [re:053951].
    pub fn hide_tooltip(&mut self) {
        if let Some(t) = self.tooltip.take() {
            self.invalidate_rect(t.damage());
        }
    }

    /// [re:c33e1a] (left button up).
    pub fn hide_tooltip_delayed(&mut self) {
        match &mut self.tooltip {
            Some(t) if t.pos == crate::tooltip::Pos::Middle => self.hide_tooltip(),
            Some(t) => t.hide_at = std::time::Instant::now() + crate::tooltip::DELAYED_HIDE,
            None => {}
        }
    }

    /// The pointer left the window.
    pub fn mouse_left(&mut self) {
        self.hide_tooltip();
    }

    /// Deepest visible control under (x, y) in root coordinates, topmost
    /// (highest zorder, last drawn) first.
    pub fn hit(&self, x: f32, y: f32) -> Vec<usize> {
        fn rec(c: &Ctrl, x: i32, y: i32, path: &mut Vec<usize>) -> bool {
            let mut order: Vec<usize> = (0..c.children.len()).collect();
            order.sort_by_key(|&i| c.children[i].zorder);
            for &i in order.iter().rev() {
                let ch = &c.children[i];
                if ch.visible && !ch.passive && ch.rect.contains(x, y) {
                    path.push(i);
                    rec(ch, x - ch.rect.x, y - ch.rect.y, path);
                    return true;
                }
            }
            false
        }
        let mut path = Vec::new();
        let (rx, ry) = (x as i32 - self.root.rect.x, y as i32 - self.root.rect.y);
        rec(&self.root, rx, ry, &mut path);
        path
    }

    /// The nearest interactive control on `path` (innermost first).
    fn interactive(&self, path: &[usize]) -> Option<Vec<usize>> {
        (0..=path.len())
            .rev()
            .map(|n| &path[..n])
            .find(|p| {
                self.ctrl(p).is_some_and(|c| {
                    matches!(
                        c.class.as_str(),
                        "CtrlButton"
                            | "CtrlPopupButton"
                            | "CtrlKnob"
                            | "CtrlTransmodKnob"
                            | "CtrlSlider"
                            | "CtrlTransmodSlider"
                            | "CtrlPanSlider"
                            | "CtrlRangeSlider"
                            | "CtrlDisclosureButton"
                            | "CtrlScrollBar"
                    ) && c.enabled
                })
            })
            .map(<[usize]>::to_vec)
    }

    /// Absolute origin of the control at `path` (a stale path stops at
    /// its last existing control).
    pub fn origin(&self, path: &[usize]) -> (i32, i32) {
        let mut c = &self.root;
        let (mut x, mut y) = (c.rect.x, c.rect.y);
        for &i in path {
            let Some(k) = c.children.get(i) else { break };
            c = k;
            x += c.rect.x;
            y += c.rect.y;
        }
        (x, y)
    }

    /// The nearest control on `path` with an `overload` class.
    fn custom(&self, path: &[usize]) -> Option<Vec<usize>> {
        (0..=path.len()).rev().map(|n| &path[..n]).find(|p| self.ctrl(p).is_some_and(|c| !c.overload.is_empty() && c.enabled)).map(<[usize]>::to_vec)
    }

    fn custom_event(&self, path: Vec<usize>, kind: MouseKind) -> UiEvent {
        let (ox, oy) = self.origin(&path);
        let overload = self.ctrl(&path).map(|c| c.overload.clone()).unwrap_or_default();
        UiEvent::Mouse { path, overload, kind, x: self.mouse.0 - ox as f32, y: self.mouse.1 - oy as f32 }
    }

    /// Open a popup menu under the control at `anchor`.
    pub fn open_menu(&mut self, anchor: &[usize], items: Vec<String>, checked: Option<usize>) {
        self.hide_tooltip();
        let (ax, ay) = self.origin(anchor);
        let (aw, ah) = self.ctrl(anchor).map(|c| (c.rect.w, c.rect.h)).unwrap_or((100, 20));
        let w = crate::menu::menu_width(&self.skin.styles, &mut self.fonts, &items, aw as f32);
        let attached = self.ctrl(anchor).is_some_and(|c| c.class == "CtrlPopupButton");
        let mut m = crate::menu::Menu { anchor: anchor.to_vec(), items, checked, x: ax as f32, y: (ay + ah) as f32, w, hover: None, attached };
        let (rw, rh) = (self.root.rect.w as f32, self.root.rect.h as f32);
        // [re:0e12f0].
        if m.y + m.height() > rh {
            m.y = (rh - m.height()).max(0.0);
        }
        if m.x + m.w > rw {
            m.x = (rw - m.w - 2.0).max(0.0);
        }
        self.menu = Some(m);
        self.pressed = None;
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> Vec<UiEvent> {
        // Drags change controls: only those repaint.
        if self.pressed.is_some() || self.split_drag.is_some() {
            return self.damage_only(|a| {
                let out = a.mouse_move_events(x, y);
                a.scroll_bar_drags(out)
            });
        }
        self.mouse_move_events(x, y)
    }

    /// A dragged scroll container bar scrolls its container (the editor
    /// does not see the bar's value).
    fn scroll_bar_drags(&mut self, out: Vec<UiEvent>) -> Vec<UiEvent> {
        out.into_iter()
            .filter(|e| {
                let UiEvent::ValueChanged { path, value, .. } = e else { return true };
                let Some(axis) = self.ctrl(path).and_then(|c| c.attr(crate::scroll::BAR)).map(str::to_string) else { return true };
                if let Some(c) = path.split_last().and_then(|(_, p)| self.ctrl_mut(p)) {
                    crate::scroll::from_bar(c, &axis, *value);
                }
                self.dirty = true;
                false
            })
            .collect()
    }

    /// Run an input handler; when it asks for a whole repaint, repaint only
    /// the controls it changed instead (where they were and are). Events
    /// for custom-drawn controls keep the whole repaint: the application
    /// draws them from its own state.
    fn damage_only(&mut self, f: impl FnOnce(&mut App) -> Vec<UiEvent>) -> Vec<UiEvent> {
        if self.dirty {
            return f(self);
        }
        let before = self.marks();
        let out = f(self);
        let custom = out.iter().any(|e| matches!(e, UiEvent::Mouse { .. } | UiEvent::Wheel { .. }));
        if self.dirty && !custom {
            self.dirty = false;
            self.invalidate_tree_changes(&before);
        }
        out
    }

    fn mouse_move_events(&mut self, x: f32, y: f32) -> Vec<UiEvent> {
        self.mouse = (x, y);
        let mut out = Vec::new();
        if let (Some(i), Some(m)) = (self.menu_slider, self.menu.as_mut()) {
            if let Some(v) = m.slider_at(i, x) {
                m.items[i] = crate::menu::slider_item(v);
                out.push(UiEvent::MenuSlider { anchor: m.anchor.clone(), index: i, value: v });
            }
            return out;
        }
        if let Some(m) = &mut self.menu {
            let h = m.item_at(x, y);
            // Only the menu repaints (see `render_frame_with`).
            m.hover = h;
            return out;
        }
        if let Some((path, start)) = self.split_drag.clone() {
            return self.split_move(&path, start, x, y, false);
        }
        if self.sizer_drag.is_some() {
            self.sizer_move(x, y);
            return out;
        }
        if let Some(from) = self.custom_pressed.clone().or_else(|| self.pressed.as_ref().map(|p| p.0.clone())) {
            let (px, py) = self.press_at;
            if (x - px).hypot(y - py) >= DRAG_THRESHOLD {
                self.dragging = true;
            }
            if self.dragging {
                let (w, h) = (self.root.rect.w as f32, self.root.rect.h as f32);
                let outside = x < 0.0 || y < 0.0 || x >= w || y >= h;
                if outside && !self.drag_left {
                    self.drag_left = true;
                    out.push(UiEvent::DragOut { from: from.clone() });
                }
                let to = self.hit(x, y);
                if !to.starts_with(&from) {
                    out.push(UiEvent::DragOver { from, to, x, y });
                }
            }
        }
        if let Some(p) = self.custom_pressed.clone().or_else(|| self.custom_right.clone()) {
            out.insert(0, self.custom_event(p, MouseKind::Drag));
            return out;
        }
        if let Some((path, x0, y0, v0)) = self.pressed.clone() {
            // Opt-in slider options (control attribute, else style).
            let (flip, detent) = self
                .ctrl(&path)
                .map(|c| {
                    let st = if self.skin.styles.styles.contains_key(&c.style) { c.style.as_str() } else { "ApplicationStyle" };
                    let flip = crate::render::opt_bool(&self.skin.styles, st, c, "optFlipDirection", false);
                    (flip, crate::render::opt_f32(&self.skin.styles, st, c, "realCentreDetent", 0.03))
                })
                .unwrap_or((false, 0.0));
            let m = self.mods;
            let self_fine = self.fine_drag && m.shift && !m.cmd && !m.alt && !m.ctrl;
            let Some(c) = self.ctrl_mut(&path) else { return out };
            let dir = if flip { -1.0 } else { 1.0 };
            let mut range_moved = false;
            let value = match c.class.as_str() {
                // [re:4ee9e7]: up raises value01 by 1 per 200 px (2000 px
                // with shift).
                "CtrlValueBox" => Some((v0 + (y0 - y) / if m.shift { 2000.0 } else { 200.0 }).clamp(0.0, 1.0)),
                "CtrlKnob" | "CtrlTransmodKnob" => {
                    let fine = self_fine;
                    Some((v0 + (y0 - y) / if fine { DRAG_RANGE * 10.0 } else { DRAG_RANGE }).clamp(0.0, 1.0))
                }
                "CtrlSlider" | "CtrlTransmodSlider" if !flip => Some(if c.rect.w < c.rect.h {
                    (v0 + (y0 - y) / (c.rect.h - c.rect.w).max(1) as f32).clamp(0.0, 1.0)
                } else {
                    (v0 + (x - x0) / (c.rect.w - c.rect.h).max(1) as f32).clamp(0.0, 1.0)
                }),
                // `optFlipDirection` sliders, and pan sliders (snapping to
                // the centre within `realCentreDetent`, default 0.03).
                "CtrlSlider" | "CtrlTransmodSlider" | "CtrlPanSlider" => {
                    let v = if c.rect.w < c.rect.h {
                        (v0 + dir * (y0 - y) / (c.rect.h - c.rect.w).max(1) as f32).clamp(0.0, 1.0)
                    } else {
                        (v0 + dir * (x - x0) / (c.rect.w - c.rect.h).max(1) as f32).clamp(0.0, 1.0)
                    };
                    Some(if c.class == "CtrlPanSlider" && (v - 0.5).abs() < detent { 0.5 } else { v })
                }
                // Range sliders move the handle the press picked
                // (`dragPart` low / high), kept on its side of the other.
                "CtrlRangeSlider" => {
                    let v =
                        if c.rect.w < c.rect.h { (v0 + (y0 - y) / (c.rect.h - c.rect.w).max(1) as f32).clamp(0.0, 1.0) } else { (v0 + (x - x0) / (c.rect.w - c.rect.h).max(1) as f32).clamp(0.0, 1.0) };
                    let (lo, hi) = crate::render::range_values(c);
                    let (key, v) = if c.attr("dragPart") == Some("low") { ("valueLow", v.min(hi)) } else { ("valueHigh", v.max(lo)) };
                    if (v - c.attr_f32(key, 0.0)).abs() > 1e-6 {
                        c.set_attr(key, v.to_string());
                        c.value = v;
                        out.push(UiEvent::ValueChanged { path: path.clone(), name: c.name.clone(), id: c.id, value: v });
                        range_moved = true;
                    }
                    None
                }
                // Scroll position 0..1 over the track minus the thumb
                // (`realPage`, the visible fraction).
                "CtrlScrollBar" => {
                    let page = c.attr_f32("realPage", 1.0).clamp(0.0, 1.0);
                    let (d, len) = if c.rect.w < c.rect.h { (y - y0, c.rect.h) } else { (x - x0, c.rect.w) };
                    Some((v0 + d / (len as f32 * (1.0 - page)).max(1.0)).clamp(0.0, 1.0))
                }
                _ => None,
            };
            if let Some(v) = value {
                // A primary drag on a modulated knob moves the base.
                if c.attr("dragPart") == Some("primary") {
                    if (v - c.attr_f32("baseValue", 0.0)).abs() > 1e-6 {
                        c.set_attr("baseValue", v.to_string());
                        out.push(UiEvent::ValueChanged { path: path.clone(), name: c.name.clone(), id: c.id, value: v });
                        self.dirty = true;
                    }
                } else if (v - c.value).abs() > 1e-6 {
                    c.value = v;
                    out.push(UiEvent::ValueChanged { path: path.clone(), name: c.name.clone(), id: c.id, value: v });
                    self.dirty = true;
                }
            }
            if range_moved {
                self.dirty = true;
            }
            return out;
        }
        let hit = self.hit(x, y);
        self.cursor = crate::cursor::Cursor::Arrow;
        if let Some(c) = self.ctrl(&hit).filter(|c| c.enabled) {
            let (ox, oy) = self.origin(&hit);
            let mut cur = crate::cursor::transmod(c, x - ox as f32, y - oy as f32, self.mods.alt);
            // [re:18130b]: the NW-SE resize cursor.
            if c.class == "CtrlWindowSizer" {
                cur = Some(crate::cursor::Cursor::ResizeNwse);
            }
            // Splitter bar enter: resize cursor unless locked.
            if crate::splitter::on_bar(c, x as i32 - ox, y as i32 - oy) && !crate::splitter::locked(c) && crate::splitter::draggable(c) {
                cur = Some(if crate::splitter::vertical(c) { crate::cursor::Cursor::UpDown } else { crate::cursor::Cursor::LeftRight });
            }
            if let Some(cur) = cur {
                self.cursor = cur;
            }
        }
        if let Some(p) = self.custom(&hit) {
            let (ox, oy) = self.origin(&p);
            let overload = self.ctrl(&p).map(|c| c.overload.clone()).unwrap_or_default();
            out.push(UiEvent::Hover { path: p, overload, x: x - ox as f32, y: y - oy as f32 });
        }
        let target = self.interactive(&hit);
        if target != self.hover {
            // Only the controls entered and left repaint.
            if let Some(old) = self.hover.take() {
                if let Some(c) = self.ctrl_mut(&old) {
                    c.hover = false;
                }
                let r = self.abs_rect(&old);
                self.invalidate_rect(r);
            }
            if let Some(new) = &target {
                if let Some(c) = self.ctrl_mut(new) {
                    c.hover = true;
                }
                let r = self.abs_rect(new);
                self.invalidate_rect(r);
            }
            out.push(UiEvent::HoverChanged { path: target.clone() });
            self.hover = target;
        }
        out
    }

    /// [re:9d0c14]: step the value (attrs `realMin`, `realMax`,
    /// `realStep`; defaults 0, 100, 1) and show it as text.
    fn step(&mut self, path: &[usize], dir: f32) -> Vec<UiEvent> {
        let Some(c) = self.ctrl_mut(path) else { return Vec::new() };
        let (lo, hi, st) = (c.attr_f32("realMin", 0.0), c.attr_f32("realMax", 100.0), c.attr_f32("realStep", 1.0));
        let v = (c.value + dir * st).clamp(lo, hi);
        if v == c.value {
            return Vec::new();
        }
        c.value = v;
        c.text = stepper_text(v, st);
        self.dirty = true;
        let c = self.ctrl(path).unwrap();
        vec![UiEvent::ValueChanged { path: path.to_vec(), name: c.name.clone(), id: c.id, value: v }]
    }

    fn stepper_at(&self, hit: &[usize]) -> Option<Vec<usize>> {
        (0..=hit.len()).rev().map(|n| hit[..n].to_vec()).find(|p| self.ctrl(p).is_some_and(|c| c.class == "CtrlValueStepper" && c.enabled && c.overload.is_empty()))
    }

    pub fn mouse_wheel(&mut self, dy: f32) -> Vec<UiEvent> {
        self.damage_only(|a| a.mouse_wheel_events(dy))
    }

    /// A wheel or trackpad scroll: `dx` / `dy` as the platform reports
    /// them (positive = right / up), in lines or (`lines` false) pixels.
    /// The innermost scroll container under the mouse (see
    /// [`crate::scroll`]) takes it; else steppers and custom controls get
    /// the dominant axis in lines, as [`App::mouse_wheel`].
    pub fn mouse_scroll(&mut self, dx: f32, dy: f32, lines: bool) -> Vec<UiEvent> {
        let hit = self.hit(self.mouse.0, self.mouse.1);
        let container = self.stepper_at(&hit).is_none().then(|| (0..=hit.len()).rev().map(|n| hit[..n].to_vec()).find(|p| self.ctrl(p).is_some_and(crate::scroll::scrollable))).flatten();
        if let Some(p) = container {
            let shift = self.mods.shift;
            return self.damage_only(|a| {
                if let Some(c) = a.ctrl_mut(&p) {
                    if crate::scroll::wheel(c, -dx, -dy, lines, shift) {
                        a.dirty = true;
                    }
                }
                Vec::new()
            });
        }
        let d = if dy.abs() >= dx.abs() { dy } else { dx };
        self.mouse_wheel(if lines { d } else { d / 18.0 })
    }

    fn mouse_wheel_events(&mut self, dy: f32) -> Vec<UiEvent> {
        let hit = self.hit(self.mouse.0, self.mouse.1);
        if let Some(p) = self.stepper_at(&hit) {
            return self.step(&p, dy.signum());
        }
        let Some(p) = self.custom(&hit) else { return Vec::new() };
        let overload = self.ctrl(&p).map(|c| c.overload.clone()).unwrap_or_default();
        self.dirty = true;
        vec![UiEvent::Wheel { path: p, overload, dy }]
    }

    /// A key with no text box editing (see [`UiEvent::Shortcut`]).
    pub fn shortcut(&mut self, key: &str) -> Vec<UiEvent> {
        if self.editing.is_some() {
            return Vec::new();
        }
        vec![UiEvent::Shortcut { key: key.to_string(), cmd: self.mods.cmd, shift: self.mods.shift, alt: self.mods.alt, ctrl: self.mods.ctrl }]
    }

    /// Start editing a text box (as if it was clicked).
    pub fn begin_edit(&mut self, path: &[usize]) {
        let _ = self.commit_edit();
        if let Some(c) = self.ctrl_mut(path) {
            let orig = c.text.clone();
            c.set_attr("editing", "true".into());
            self.editing = Some((path.to_vec(), orig));
            self.dirty = true;
        }
    }

    /// An inline edit box ([re:c8b896] and the rename
    /// boxes): a [re:ba0ee3] named `name` at `rect` under `parent` holding
    /// `text` (replacing an earlier one), editing at once.
    pub fn open_edit_box(&mut self, parent: &[usize], name: &str, rect: crate::Rect, text: String) {
        let Some(host) = self.ctrl_mut(parent) else { return };
        host.children.retain(|k| k.name != name);
        let mut edit = Ctrl { class: "CtrlLabel".into(), name: name.into(), visible: true, enabled: true, ..Ctrl::default() };
        edit.set_rect(rect);
        edit.text = text;
        host.children.push(edit);
        let path = [parent, &[host.children.len() - 1]].concat();
        self.begin_edit(&path);
    }

    /// Mouse position (root coordinates).
    pub fn mouse_pos(&self) -> (f32, f32) {
        self.mouse
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    fn commit_edit(&mut self) -> Vec<UiEvent> {
        let Some((path, _)) = self.editing.take() else { return Vec::new() };
        self.dirty = true;
        let Some(c) = self.ctrl_mut(&path) else { return Vec::new() };
        c.remove_attr("editing");
        vec![UiEvent::TextCommitted { name: c.name.clone(), text: c.text.clone(), path }]
    }

    fn cancel_edit(&mut self) {
        if let Some((path, orig)) = self.editing.take() {
            if let Some(c) = self.ctrl_mut(&path) {
                c.text = orig;
                c.remove_attr("editing");
            }
            self.dirty = true;
        }
    }

    /// Typed text for the focused text box; returns false when nothing has
    /// the keyboard (so the host gets the key).
    pub fn key_text(&mut self, text: &str) -> bool {
        let Some((path, _)) = self.editing.clone() else { return false };
        if let Some(c) = self.ctrl_mut(&path) {
            c.text.push_str(text);
            self.dirty = true;
        }
        true
    }

    /// Editing keys: "Enter", "Escape", "Backspace". Returns events and
    /// whether the key was used.
    pub fn key_named(&mut self, key: &str) -> (Vec<UiEvent>, bool) {
        let Some((path, _)) = self.editing.clone() else { return (Vec::new(), false) };
        match key {
            "Enter" | "Tab" => (self.commit_edit(), true),
            "Escape" => {
                self.cancel_edit();
                (Vec::new(), true)
            }
            "Backspace" => {
                if let Some(c) = self.ctrl_mut(&path) {
                    c.text.pop();
                    self.dirty = true;
                }
                (Vec::new(), true)
            }
            _ => (Vec::new(), true),
        }
    }

    pub fn mouse_down(&mut self, button: MouseButton) -> Vec<UiEvent> {
        self.damage_only(|a| a.mouse_down_events(button))
    }

    fn mouse_down_events(&mut self, button: MouseButton) -> Vec<UiEvent> {
        self.ctrl_press = self.ctrl_click_right && button == MouseButton::Left && self.mods.ctrl;
        let button = if self.ctrl_press { MouseButton::Right } else { button };
        let (x, y) = self.mouse;
        // A click anywhere commits a pending text edit.
        let mut committed = self.commit_edit();
        let hit = self.hit(x, y);
        let text_box = (0..=hit.len())
            .rev()
            .map(|n| hit[..n].to_vec())
            .find(|p| self.ctrl(p).is_some_and(|c| matches!(c.class.as_str(), "CtrlTextBox" | "CtrlValueBox" | "CtrlTempoBox" | "CtrlSearchBox") && c.enabled && !c.passive));
        // `optSingleClickEdit="false"`: a double click edits; a single
        // press goes to the control underneath (e.g. a mixer strip).
        let candidate = text_box.clone();
        let text_box = text_box.filter(|p| button != MouseButton::Left || self.ctrl(p).is_some_and(|c| c.attr_bool("optSingleClickEdit", true)) || self.double_click(p).is_some());
        // The press passed through: keep it as the text box's first click.
        let deferred = if text_box.is_none() { candidate } else { None };
        if let Some(p) = text_box {
            if button == MouseButton::Left {
                // [re:d6bcf8]: the h×h
                // search icon at the left opens the search menu (reported
                // as `Pressed`), the clear icon at the right empties the
                // text and commits it.
                if let Some((ox, _)) = self.ctrl(&p).filter(|c| c.class == "CtrlSearchBox").map(|_| self.origin(&p)) {
                    let (w, h, name) = self.ctrl(&p).map(|c| (c.rect.w as f32, c.rect.h as f32, c.name.clone())).unwrap_or_default();
                    let lx = x - ox as f32;
                    if lx < h {
                        committed.push(UiEvent::Pressed { path: p, name });
                        return committed;
                    }
                    if lx >= w - h && self.ctrl(&p).is_some_and(|c| !c.text.is_empty()) {
                        if let Some(c) = self.ctrl_mut(&p) {
                            c.text.clear();
                        }
                        self.dirty = true;
                        committed.push(UiEvent::TextCommitted { path: p, name, text: String::new() });
                        return committed;
                    }
                }
                if let Some(c) = self.ctrl_mut(&p) {
                    let orig = c.text.clone();
                    c.set_attr("editing", "true".into());
                    self.editing = Some((p, orig));
                    self.dirty = true;
                }
                return committed;
            }
        }
        // `dragable` value boxes (control attribute, else style; [re:4ad8c8]):
        // a single press starts a drag that changes the value.
        if button == MouseButton::Left {
            if let Some((p, v0)) = deferred.as_ref().and_then(|p| self.ctrl(p).filter(|c| c.class == "CtrlValueBox" && self.opt_bool(c, "dragable")).map(|c| (p.clone(), c.value))) {
                self.press_at = (x, y);
                self.dragging = false;
                self.drag_left = false;
                self.pressed = Some((p.clone(), x, y, v0));
                self.last_press = Some((p, std::time::Instant::now()));
                self.dirty = true;
                return committed;
            }
        }
        let more = self.mouse_down_inner(button);
        committed.extend(more);
        if let Some(p) = deferred {
            self.last_press = Some((p, std::time::Instant::now()));
        }
        committed
    }

    /// A control option: the control's attribute, else its style's.
    fn opt_bool(&self, c: &Ctrl, k: &str) -> bool {
        let st = if self.skin.styles.styles.contains_key(&c.style) { c.style.as_str() } else { "ApplicationStyle" };
        crate::render::opt_bool(&self.skin.styles, st, c, k, false)
    }

    fn mouse_down_inner(&mut self, button: MouseButton) -> Vec<UiEvent> {
        let (x, y) = self.mouse;
        if button == MouseButton::Left {
            self.press_at = (x, y);
            self.dragging = false;
            self.drag_left = false;
        }
        if let Some(mut m) = self.menu.take() {
            if let Some((i, v)) = m.item_at(x, y).and_then(|i| m.slider_at(i, x).map(|v| (i, v))) {
                m.items[i] = crate::menu::slider_item(v);
                let anchor = m.anchor.clone();
                self.menu = Some(m);
                self.menu_slider = Some(i);
                return vec![UiEvent::MenuSlider { anchor, index: i, value: v }];
            }
            if let Some(i) = m.item_at(x, y) {
                let name = self.ctrl(&m.anchor).map(|c| c.name.clone()).unwrap_or_default();
                return vec![UiEvent::MenuSelected { anchor: m.anchor, name, index: i }];
            }
            if m.contains(x, y) {
                self.menu = Some(m);
            }
            return Vec::new();
        }
        let hit = self.hit(x, y);
        // A press on a scroll container bar's track pages; the thumb drags.
        if button == MouseButton::Left {
            if let Some(axis) = self.ctrl(&hit).and_then(|c| c.attr(crate::scroll::BAR)).map(str::to_string) {
                let (ox, oy) = self.origin(&hit);
                let at = if axis == "h" { x as i32 - ox } else { y as i32 - oy };
                let paged = hit.split_last().and_then(|(_, p)| self.ctrl_mut(p)).is_some_and(|c| crate::scroll::page(c, &axis, at));
                if paged {
                    self.dirty = true;
                    return Vec::new();
                }
            }
        }
        if let Some(p) = self.stepper_at(&hit).filter(|_| button == MouseButton::Left) {
            // Upper half steps up, lower half down.
            let (_, oy) = self.origin(&p);
            let h = self.ctrl(&p).map(|c| c.rect.h as f32).unwrap_or(0.0);
            let dir = if y - (oy as f32) < h / 2.0 { 1.0 } else { -1.0 };
            return self.step(&p, dir);
        }
        // [re:d45123]: a click clears the clip latches.
        if button == MouseButton::Left {
            if let Some(n) = (0..=hit.len()).rev().find(|&n| self.ctrl(&hit[..n]).is_some_and(|c| c.class == "CtrlMultiChannelDBMeter")) {
                let p = hit[..n].to_vec();
                if let Some(c) = self.ctrl_mut(&p) {
                    for m in &mut c.meters {
                        m.clip = false;
                    }
                }
                self.dirty = true;
                return Vec::new();
            }
        }
        // [re:8582ad]: capture, resize
        // cursor, remember the pointer.
        if button == MouseButton::Left && self.ctrl(&hit).is_some_and(|c| c.class == "CtrlWindowSizer") {
            let ui = self.ui_scale.max(0.25);
            self.sizer_drag = Some((x * ui, y * ui));
            self.cursor = crate::cursor::Cursor::ResizeNwse;
            return Vec::new();
        }
        // [re:e58107] bar: a press starts a drag, a double click
        // inverts the collapse state.
        if button == MouseButton::Left {
            if let Some(c) = self.ctrl(&hit) {
                let (ox, oy) = self.origin(&hit);
                if crate::splitter::on_bar(c, x as i32 - ox, y as i32 - oy) {
                    if crate::splitter::locked(c) {
                        return Vec::new();
                    }
                    let start = crate::splitter::position(c);
                    if let Some(UiEvent::DoubleClicked { .. }) = self.double_click(&hit) {
                        self.split_drag = None;
                        let Some(c) = self.ctrl_mut(&hit) else { return Vec::new() };
                        let to = crate::splitter::invert_collapse(c);
                        self.splits.retain(|a| a.path != hit);
                        self.splits.push(crate::splitter::Anim { path: hit, from: start, to, start: std::time::Instant::now() });
                        return Vec::new();
                    }
                    self.press_at = (x, y);
                    self.split_drag = Some((hit, start));
                    return Vec::new();
                }
            }
        }
        // [re:cd7d95]: the panel itself
        // under the pointer, inside its header.
        if button == MouseButton::Left && self.ctrl(&hit).is_some_and(|c| c.class == "CtrlCollapsePanel") {
            if let Some(UiEvent::DoubleClicked { .. }) = self.double_click(&hit) {
                let (ox, oy) = self.origin(&hit);
                let c = self.ctrl(&hit).expect("hit");
                let h = crate::collapse::header(c);
                let (lx, ly) = (x as i32 - ox, y as i32 - oy);
                if lx >= h.x && lx < h.x + h.w && ly >= h.y && ly < h.y + h.h {
                    let iconic = !crate::collapse::is_iconic(c);
                    return self.fold(&hit, iconic);
                }
            }
            return Vec::new();
        }
        let Some(path) = self.interactive(&hit) else {
            if let Some(p) = self.custom(&hit) {
                let kind = if button == MouseButton::Left { MouseKind::Down } else { MouseKind::RightDown };
                let mut out = vec![self.custom_event(p.clone(), kind)];
                if button == MouseButton::Right {
                    self.custom_right = Some(p.clone());
                }
                if button == MouseButton::Left {
                    self.custom_pressed = Some(p.clone());
                    if let Some(d) = self.double_click(&p) {
                        out.push(d);
                    }
                }
                return out;
            }
            // A right press on a plain panel (the right mouse down of the
            // panel under the pointer, e.g. an effect panel's background).
            if button == MouseButton::Right && !hit.is_empty() {
                let name = self.ctrl(&hit).map(|c| c.name.clone()).unwrap_or_default();
                return vec![UiEvent::RightClicked { path: hit, name }];
            }
            return Vec::new();
        };
        let double = if button == MouseButton::Left { self.double_click(&path) } else { None };
        let mut out = self.press_control(path, button);
        out.extend(double);
        out
    }

    /// Start folding (`iconic`) or unfolding the
    /// collapse panel at `path`; [`App::animate`] runs it.
    pub fn fold(&mut self, path: &[usize], iconic: bool) -> Vec<UiEvent> {
        let Some(c) = self.ctrl_mut(path) else { return Vec::new() };
        if iconic {
            crate::collapse::store_full(c);
        }
        let (from, to) = (c.rect, crate::collapse::target_rect(c, iconic));
        self.folds.retain(|a| a.path != path);
        self.folds.push(crate::collapse::Anim {
            path: path.to_vec(),
            from,
            to,
            ms: crate::collapse::ANIM_MS,
            curve: crate::collapse::curve,
            end: crate::collapse::AnimEnd::Fold(iconic),
            start: std::time::Instant::now(),
        });
        vec![UiEvent::Collapse { path: path.to_vec(), iconic, end: false }]
    }

    /// Fold / unfold at once (start and end
    /// notifications).
    pub fn set_iconic(&mut self, path: &[usize], iconic: bool) -> Vec<UiEvent> {
        let Some(c) = self.ctrl_mut(path) else { return Vec::new() };
        if crate::collapse::is_iconic(c) == iconic {
            return Vec::new();
        }
        if iconic {
            crate::collapse::store_full(c);
        }
        crate::collapse::set_iconic(c, iconic);
        self.restack_parent(path);
        self.dirty = true;
        vec![UiEvent::Collapse { path: path.to_vec(), iconic, end: false }, UiEvent::Collapse { path: path.to_vec(), iconic, end: true }]
    }

    /// Animated rect change (to, ms, curve, end): move the control at
    /// `path` from `from`; [`App::animate`] reports `UiEvent::Animated` with
    /// `tag` at the end.
    pub fn animate_rect(&mut self, path: &[usize], from: crate::ctrl::Rect, to: crate::ctrl::Rect, ms: f32, curve: fn(f32) -> f32, tag: u32) {
        if let Some(c) = self.ctrl_mut(path) {
            c.set_rect(from);
        }
        self.folds.retain(|a| a.path != path);
        self.folds.push(crate::collapse::Anim { path: path.to_vec(), from, to, ms, curve, end: crate::collapse::AnimEnd::Tag(tag), start: std::time::Instant::now() });
        self.dirty = true;
    }

    fn restack_parent(&mut self, path: &[usize]) {
        if let Some(p) = path.split_last().and_then(|(_, p)| self.ctrl_mut(p)) {
            crate::collapse::restack(p);
        }
    }

    /// Advance running folds ([re:f41060]); finished
    /// ones set the state and report their end.
    pub fn animate(&mut self) -> Vec<UiEvent> {
        let now = std::time::Instant::now();
        if self.tooltip.as_ref().is_some_and(|t| now >= t.hide_at) {
            self.hide_tooltip();
        }
        if let Some(t) = self.tooltip.as_mut().filter(|t| t.pending.is_some_and(|(at, _)| now >= at)) {
            t.pending = None;
            let d = t.damage();
            self.invalidate_rect(d);
        }
        let mut out = self.animate_splits();
        if self.folds.is_empty() {
            return out;
        }
        let now = std::time::Instant::now();
        for a in std::mem::take(&mut self.folds) {
            let t = now.duration_since(a.start).as_secs_f32() * 1000.0 / a.ms;
            let done = t >= 1.0;
            let r = crate::collapse::lerp_rect(a.from, a.to, (a.curve)(t.min(1.0)));
            let Some(c) = self.ctrl_mut(&a.path) else { continue };
            c.set_rect(r);
            match a.end {
                crate::collapse::AnimEnd::Fold(iconic) => {
                    if done {
                        c.set_attr("iconic", if iconic { "1" } else { "0" }.into());
                        out.push(UiEvent::Collapse { path: a.path.clone(), iconic, end: true });
                    }
                    self.restack_parent(&a.path);
                }
                crate::collapse::AnimEnd::Tag(tag) => {
                    if done {
                        out.push(UiEvent::Animated { path: a.path.clone(), tag });
                    }
                }
            }
            if !done {
                self.folds.push(a);
            }
        }
        self.dirty = true;
        out
    }

    /// [re:d703b9]: the pointer's travel in
    /// screen pixels is added to the root's (unscaled) size; an axis below
    /// the minimum keeps its size and its anchor. The editor's resize handler then
    /// asks the host for the size × scale (at least 1150×650); the window
    /// follows through [`App::resize_request`].
    fn sizer_move(&mut self, x: f32, y: f32) {
        let Some((ax, ay)) = self.sizer_drag else { return };
        self.cursor = crate::cursor::Cursor::ResizeNwse;
        let ui = self.ui_scale.max(0.25);
        let (sx, sy) = (x * ui, y * ui);
        let (w, h) = self.size();
        let (mut nw, mut nh) = (w + (sx - ax).round() as i32, h + (sy - ay).round() as i32);
        let (mut nax, mut nay) = (sx, sy);
        if nw < MIN_WINDOW.0 {
            nw = w;
            nax = ax;
        }
        if nh < MIN_WINDOW.1 {
            nh = h;
            nay = ay;
        }
        if self.sizer_drag.is_some() {
            self.sizer_drag = Some((nax, nay));
        }
        if (nw, nh) != (w, h) {
            self.resize(nw, nh);
            self.resize_request = true;
        }
    }

    /// Whether the window sizer is being dragged (live resizing).
    pub fn live_resizing(&self) -> bool {
        self.sizer_drag.is_some()
    }

    /// [re:9d17dc] / mouse up: the bar
    /// follows the pointer's travel since the press, clamped (draggable
    /// splitters only).
    fn split_move(&mut self, path: &[usize], start: i32, x: f32, y: f32, end: bool) -> Vec<UiEvent> {
        let (px, py) = self.press_at;
        let Some(c) = self.ctrl_mut(path) else { return Vec::new() };
        if !crate::splitter::draggable(c) {
            return Vec::new();
        }
        let delta = if crate::splitter::vertical(c) { y - py } else { x - px };
        let pos = crate::splitter::clamp(c, start + delta.round() as i32);
        crate::splitter::set_position(c, pos);
        self.dirty = true;
        vec![UiEvent::Split { path: path.to_vec(), pos, end }]
    }

    /// Splitter position animations (250 ms Linear, floored steps).
    fn animate_splits(&mut self) -> Vec<UiEvent> {
        let now = std::time::Instant::now();
        let mut out = Vec::new();
        for a in std::mem::take(&mut self.splits) {
            let t = (now.duration_since(a.start).as_secs_f32() * 1000.0 / crate::splitter::ANIM_MS).min(1.0);
            let pos = a.from + ((a.to - a.from) as f32 * t).floor() as i32;
            let Some(c) = self.ctrl_mut(&a.path) else { continue };
            crate::splitter::set_position(c, pos);
            self.dirty = true;
            let done = t >= 1.0;
            out.push(UiEvent::Split { path: a.path.clone(), pos, end: done });
            if !done {
                self.splits.push(a);
            }
        }
        out
    }

    /// Files from outside the window over (`x`, `y`), or dropped there.
    pub fn files(&mut self, x: f32, y: f32, files: Vec<std::path::PathBuf>, dropped: bool) -> Vec<UiEvent> {
        self.mouse = (x, y);
        let to = self.hit(x, y);
        self.dirty = true;
        vec![UiEvent::Files { to, files, x, y, dropped }]
    }

    /// A `DoubleClicked` when `path` was pressed within [`DOUBLE_CLICK`].
    fn double_click(&mut self, path: &[usize]) -> Option<UiEvent> {
        let now = std::time::Instant::now();
        let dbl = self.last_press.as_ref().is_some_and(|(p, t)| p == path && now.duration_since(*t) < DOUBLE_CLICK);
        if dbl {
            self.last_press = None;
            let name = self.ctrl(path).map(|c| c.name.clone()).unwrap_or_default();
            return Some(UiEvent::DoubleClicked { path: path.to_vec(), name });
        }
        self.last_press = Some((path.to_vec(), now));
        None
    }

    fn press_control(&mut self, path: Vec<usize>, button: MouseButton) -> Vec<UiEvent> {
        let (x, y) = self.mouse;
        if button != MouseButton::Left {
            let name = self.ctrl(&path).map(|c| c.name.clone()).unwrap_or_default();
            return vec![UiEvent::RightClicked { path, name }];
        }
        let mut out = Vec::new();
        // The drag keeps the press's transmod cursor (as on mouse down);
        // with a source selected a press on the centre / main track edits
        // the primary (base) value, the ring / strip the depth.
        let mut primary = false;
        if let Some(c) = self.ctrl(&path) {
            let (ox, oy) = self.origin(&path);
            if let Some(cur) = crate::cursor::transmod(c, x - ox as f32, y - oy as f32, self.mods.alt) {
                primary = c.attr("editingDepth") == Some("true") && cur != crate::cursor::Cursor::image("TransmodSecondary");
                self.cursor = cur;
            }
        }
        let local = {
            let (ox, oy) = self.origin(&path);
            (x - ox as f32, y - oy as f32)
        };
        let Some(c) = self.ctrl_mut(&path) else { return out };
        out.push(UiEvent::Pressed { path: path.clone(), name: c.name.clone() });
        let v0 = if primary {
            c.set_attr("dragPart", "primary".into());
            c.attr_f32("baseValue", c.value)
        } else if c.class == "CtrlRangeSlider" {
            // The handle nearer the press (the high one when they meet and
            // the press is above them).
            let (lo, hi) = crate::render::range_values(c);
            let (w, h) = (c.rect.w as f32, c.rect.h as f32);
            let at = if w < h { 1.0 - (local.1 - w / 2.0) / (h - w).max(1.0) } else { (local.0 - h / 2.0) / (w - h).max(1.0) };
            let low = (at - lo).abs() < (at - hi).abs() || ((at - lo).abs() == (at - hi).abs() && at < lo);
            c.set_attr("dragPart", if low { "low" } else { "high" }.into());
            if low { lo } else { hi }
        } else {
            c.value
        };
        if c.class == "CtrlButton" && c.attr_bool("optSendOnMouseDown", false) {
            let momentary = c.attr_bool("optIsMomentary", false);
            if !momentary {
                c.on = !c.on;
            }
            out.push(UiEvent::Clicked { path: path.clone(), name: c.name.clone(), id: c.id, on: c.on });
        }
        if c.attr_bool("optIsMomentary", false) {
            c.on = true;
        }
        self.pressed = Some((path, x, y, v0));
        self.dirty = true;
        out
    }

    pub fn mouse_up(&mut self, button: MouseButton) -> Vec<UiEvent> {
        self.damage_only(|a| a.mouse_up_events(button))
    }

    fn mouse_up_events(&mut self, button: MouseButton) -> Vec<UiEvent> {
        let button = if std::mem::take(&mut self.ctrl_press) && button == MouseButton::Left { MouseButton::Right } else { button };
        if self.menu_slider.take().is_some() {
            return Vec::new();
        }
        if button != MouseButton::Left {
            return self.custom_right.take().map(|p| vec![self.custom_event(p, MouseKind::Up)]).unwrap_or_default();
        }
        self.hide_tooltip_delayed();
        let (x, y) = self.mouse;
        if self.sizer_drag.take().is_some() {
            self.sizer_move(x, y);
            return Vec::new();
        }
        if let Some((path, start)) = self.split_drag.take() {
            self.cursor = crate::cursor::Cursor::Arrow;
            return self.split_move(&path, start, x, y, true);
        }
        if std::mem::take(&mut self.dragging) {
            self.cursor = crate::cursor::Cursor::Arrow;
        }
        if let Some(p) = self.custom_pressed.take() {
            // The drop first: the source still knows what it was dragging.
            let mut out: Vec<UiEvent> = self.drop_event(&p).into_iter().collect();
            out.push(self.custom_event(p, MouseKind::Up));
            return out;
        }
        let Some((path, ..)) = self.pressed.take() else { return Vec::new() };
        if let Some(c) = self.ctrl_mut(&path) {
            c.remove_attr("dragPart");
        }
        let inside = self.hit(x, y).starts_with(&path);
        let mut out = Vec::new();
        if !inside {
            out.extend(self.drop_event(&path));
        }
        let parent_path = path[..path.len().saturating_sub(1)].to_vec();
        let Some(c) = self.ctrl_mut(&path) else { return out };
        let momentary = c.attr_bool("optIsMomentary", false);
        if matches!(c.class.as_str(), "CtrlButton" | "CtrlPopupButton" | "CtrlDisclosureButton") {
            if momentary {
                c.on = false;
            }
            if inside && !c.attr_bool("optSendOnMouseDown", false) {
                if !momentary && c.class != "CtrlPopupButton" {
                    c.on = !c.on;
                }
                let ev = UiEvent::Clicked { path: path.clone(), name: c.name.clone(), id: c.id, on: c.on };
                out.push(ev);
                self.radio_select(&parent_path, &path);
            }
        }
        self.dirty = true;
        out
    }

    /// The OS took the drag over: forget the press (no release follows).
    pub fn end_press(&mut self) {
        self.dragging = false;
        self.custom_pressed = None;
        self.pressed = None;
        self.cursor = crate::cursor::Cursor::Arrow;
        self.dirty = true;
    }

    /// A `Dropped` when the press on `from` travelled past the drag
    /// threshold and was released outside it.
    fn drop_event(&self, from: &[usize]) -> Option<UiEvent> {
        let (x, y) = self.mouse;
        let (x0, y0) = self.press_at;
        if (x - x0).hypot(y - y0) < DRAG_THRESHOLD {
            return None;
        }
        let to = self.hit(x, y);
        if to.starts_with(from) {
            return None;
        }
        Some(UiEvent::Dropped { from: from.to_vec(), to, x, y })
    }

    /// [re:a4268c]: selecting one child button deselects the others.
    fn radio_select(&mut self, parent: &[usize], chosen: &[usize]) {
        let Some(p) = self.ctrl_mut(parent) else { return };
        if p.class != "CtrlButtonGroup" {
            return;
        }
        let idx = *chosen.last().unwrap();
        for (i, ch) in p.children.iter_mut().enumerate() {
            if ch.class == "CtrlButton" {
                ch.on = i == idx;
            }
        }
    }

    /// Render the current tree at `scale` (physical pixels per point).
    pub fn render(&mut self, scale: f32) -> tiny_skia::Pixmap {
        self.render_with(scale, None)
    }

    /// A complete fresh frame.
    pub fn render_with(&mut self, scale: f32, custom: Option<&dyn crate::render::CustomDraw>) -> tiny_skia::Pixmap {
        self.dirty = true;
        self.render_frame_with(scale, custom).clone()
    }

    /// The control at `path` in root coordinates.
    pub fn abs_rect(&self, path: &[usize]) -> crate::ctrl::Rect {
        let (x, y) = self.origin(path);
        let (w, h) = self.ctrl(path).map(|c| (c.rect.w, c.rect.h)).unwrap_or((0, 0));
        crate::ctrl::Rect::new(x, y, w, h)
    }

    /// Repaint `r` (root coordinates) on the next frame.
    pub fn invalidate_rect(&mut self, r: crate::ctrl::Rect) {
        if r.w > 0 && r.h > 0 {
            self.damage.push(r);
        }
    }

    /// The tree's drawable state, to repaint only what changes until
    /// [`App::invalidate_changes`].
    pub fn marks(&self) -> Marks {
        fn walk(c: &Ctrl, x: i32, y: i32, out: &mut Vec<(u64, crate::ctrl::Rect)>) {
            out.push((c.draw_hash(), crate::ctrl::Rect::new(x, y, c.rect.w, c.rect.h)));
            for k in &c.children {
                walk(k, x + k.rect.x, y + k.rect.y, out);
            }
        }
        let mut v = Vec::with_capacity(self.marks_len.get());
        walk(&self.root, self.root.rect.x, self.root.rect.y, &mut v);
        self.marks_len.set(v.len());
        Marks(v)
    }

    /// Repaint the controls whose drawable state changed since `before`
    /// (where they were and where they are), and the custom-drawn areas,
    /// which draw from the application's model. A changed tree shape
    /// repaints everything.
    pub fn invalidate_changes(&mut self, before: &Marks) {
        self.invalidate_changes_with(before, None);
    }

    /// [`App::invalidate_changes`], the custom-drawn controls repainted
    /// only when `custom` (the drawer frames are drawn with) now draws them
    /// differently from what the frame shows.
    pub fn invalidate_changes_with(&mut self, before: &Marks, custom: Option<&dyn crate::render::CustomDraw>) {
        if !self.invalidate_tree_changes(before) {
            return;
        }
        for r in self.custom_rects.clone() {
            self.invalidate_rect(r);
        }
        let scale = self.frame.as_ref().map(|(s, _)| *s);
        for i in 0..self.custom_marks.len() {
            let changed = match (custom, scale, self.custom_marks[i].1) {
                (Some(cd), Some(s), Some(k)) => crate::render::replay_custom(&self.skin.styles, &mut self.fonts, &self.root, s, cd, &self.custom_marks[i].0) != Some(k),
                _ => true,
            };
            if changed {
                let r = self.abs_rect(&self.custom_marks[i].0.clone());
                self.invalidate_rect(r);
            }
        }
    }

    /// Fingerprint again the custom-drawn controls a repaint of `rects`
    /// (all: `None`) drew, as the frame now shows them.
    fn refresh_custom_marks(&mut self, scale: f32, custom: Option<&dyn crate::render::CustomDraw>, rects: Option<&[crate::ctrl::Rect]>) {
        for i in 0..self.custom_marks.len() {
            let path = self.custom_marks[i].0.clone();
            if rects.is_some_and(|rs| {
                let a = self.abs_rect(&path);
                !rs.iter().any(|r| r.intersects(&a))
            }) {
                continue;
            }
            self.custom_marks[i].1 = custom.and_then(|cd| crate::render::replay_custom(&self.skin.styles, &mut self.fonts, &self.root, scale, cd, &path));
        }
    }

    /// The controls part of [`App::invalidate_changes`]; false when the
    /// tree's shape changed (everything repaints).
    fn invalidate_tree_changes(&mut self, before: &Marks) -> bool {
        let after = self.marks();
        if after.0.len() != before.0.len() {
            self.dirty = true;
            return false;
        }
        for (a, b) in after.0.iter().zip(&before.0) {
            if a.0 != b.0 {
                self.invalidate_rect(b.1);
                self.invalidate_rect(a.1);
            }
        }
        true
    }

    /// The rectangles waiting to be repainted (root coordinates).
    pub fn pending_damage(&self) -> &[crate::ctrl::Rect] {
        &self.damage
    }

    /// Something needs drawing.
    pub fn needs_redraw(&self) -> bool {
        self.dirty || !self.damage.is_empty() || self.menu != self.menu_drawn
    }

    /// The frame for the window: redrawn completely when `dirty` (or the
    /// size, scale or an open menu requires it), else only the damaged
    /// rectangles are repainted into the last frame.
    pub fn render_frame_with(&mut self, scale: f32, custom: Option<&dyn crate::render::CustomDraw>) -> &tiny_skia::Pixmap {
        let w = ((self.root.rect.w.max(1)) as f32 * scale).ceil() as u32;
        let h = ((self.root.rect.h.max(1)) as f32 * scale).ceil() as u32;
        let reusable = matches!(&self.frame, Some((s, pm)) if *s == scale && pm.width() == w && pm.height() == h);
        let mut damage = std::mem::take(&mut self.damage);
        let tip_shown = self.tooltip.as_ref().is_some_and(|t| t.visible());
        // Where the open menu goes, when it can be drawn over the pixels
        // beneath it (no tooltip on top, whole device pixels).
        let menu_at = self.menu.as_ref().filter(|_| !tip_shown).and_then(|m| {
            let r = menu_damage_rect(m);
            crate::device_rect(&self.root, scale, r).map(|d| (r, d))
        });
        // Where a partial repaint drew.
        let mut repainted = Vec::new();
        if self.dirty || !reusable {
            let drawn = std::cell::RefCell::new(Vec::new());
            let paths = std::cell::RefCell::new(Vec::new());
            let tip = self.tooltip.as_ref().filter(|t| t.visible());
            // The menu is drawn after the pixels beneath it are kept.
            let menu = if menu_at.is_some() { None } else { self.menu.as_ref() };
            // The last frame's pixmap is redrawn when its size still fits.
            if !reusable {
                self.frame = tiny_skia::Pixmap::new(w.max(1), h.max(1)).map(|pm| (scale, pm));
            }
            if let Some((_, pm)) = self.frame.as_mut() {
                crate::render_full_into(&self.skin, &mut self.fonts, &self.root, scale, custom, menu, tip, Some(&drawn), Some(&paths), pm);
            }
            let (ox, oy) = (self.root.rect.x, self.root.rect.y);
            self.custom_rects = drawn.into_inner().into_iter().map(|r| crate::ctrl::Rect::new(r.x + ox, r.y + oy, r.w, r.h)).collect();
            self.custom_marks = paths.into_inner().into_iter().map(|p| (p, None)).collect();
            self.refresh_custom_marks(scale, custom, None);
            self.menu_under = None;
            if let (Some((r, d)), Some(m), Some((_, pm))) = (menu_at, self.menu.as_ref(), self.frame.as_mut()) {
                if let Some(mut under) = tiny_skia::Pixmap::new(d.2, d.3) {
                    crate::copy_device_rect(pm, &mut under, d, false);
                    crate::render_menu_over(&self.skin, &mut self.fonts, scale, m, &under, pm, r, d);
                    self.menu_under = Some((scale, r, under));
                }
            }
        } else if let Some((_, pm)) = self.frame.as_mut() {
            let menu_changed = self.menu != self.menu_drawn;
            let menu_rect = self.menu.as_ref().map(menu_damage_rect);
            // A menu that cannot go over kept pixels repaints as damage.
            if let (true, Some(r), None) = (menu_changed, menu_rect, menu_at) {
                damage.push(r);
            }
            // The menu the frame shows went away or moved: its rectangle
            // gets back the kept pixels beneath it, or repaints.
            if let Some(old) = self.menu_drawn.as_ref().filter(|_| menu_changed).map(menu_damage_rect) {
                if menu_rect != Some(old) {
                    let kept = self.menu_under.take().filter(|(s, u, _)| *s == scale && *u == old);
                    let d = crate::device_rect(&self.root, scale, old);
                    match (kept, d) {
                        (Some((_, _, mut under)), Some(d)) if menu_rect.is_none_or(|r| !r.intersects(&old)) && !damage.iter().any(|r| r.intersects(&old)) => {
                            crate::copy_device_rect(pm, &mut under, d, true);
                        }
                        _ => damage.push(old),
                    }
                }
            }
            let damage_rects = merge_rects(std::mem::take(&mut damage));
            // Anything else repainted under the menu makes the kept pixels
            // stale.
            if self.menu_under.as_ref().is_some_and(|(_, u, _)| damage_rects.iter().any(|r| r.intersects(u))) {
                self.menu_under = None;
            }
            // The frame already shows what is beneath a new menu: nothing
            // repaints there and no menu was drawn in that place.
            let clean = menu_at.is_some_and(|(r, _)| !damage_rects.iter().any(|d| d.intersects(&r)) && self.menu_drawn.as_ref().is_none_or(|o| menu_damage_rect(o) != r));
            for &r in &damage_rects {
                crate::render_region(&self.skin, &mut self.fonts, &self.root, scale, custom, self.menu.as_ref(), self.tooltip.as_ref().filter(|t| t.visible()), pm, r);
            }
            repainted = damage_rects;
            if let (true, Some((r, d)), Some(m)) = (menu_changed, menu_at, self.menu.as_ref()) {
                let kept = self.menu_under.take().filter(|(s, u, _)| *s == scale && *u == r);
                let under = match kept {
                    Some((_, _, under)) => Some(under),
                    None if clean => tiny_skia::Pixmap::new(d.2, d.3).map(|mut u| {
                        crate::copy_device_rect(pm, &mut u, d, false);
                        u
                    }),
                    // The pixels beneath, drawn without the menu.
                    None => {
                        crate::render_region(&self.skin, &mut self.fonts, &self.root, scale, custom, None, None, pm, r);
                        repainted.push(r);
                        tiny_skia::Pixmap::new(d.2, d.3).map(|mut u| {
                            crate::copy_device_rect(pm, &mut u, d, false);
                            u
                        })
                    }
                };
                if let Some(under) = under {
                    crate::render_menu_over(&self.skin, &mut self.fonts, scale, m, &under, pm, r, d);
                    self.menu_under = Some((scale, r, under));
                }
            }
        }
        self.menu_drawn = self.menu.clone();
        if !repainted.is_empty() {
            self.refresh_custom_marks(scale, custom, Some(&repainted));
        }
        self.dirty = false;
        &self.frame.as_ref().expect("frame").1
    }
}

/// What an open menu covers (its shadow included), root coordinates.
fn menu_damage_rect(m: &crate::menu::Menu) -> crate::ctrl::Rect {
    crate::ctrl::Rect::new(m.x.floor() as i32 - 8, m.y.floor() as i32 - 8, m.w.ceil() as i32 + 16, m.height().ceil() as i32 + 16)
}

/// The drawable state of a control tree ([`App::marks`]).
pub struct Marks(Vec<(u64, crate::ctrl::Rect)>);

/// Overlapping or touching damage rectangles merged where that costs little
/// extra area (fewer tree walks).
fn merge_rects(mut v: Vec<crate::ctrl::Rect>) -> Vec<crate::ctrl::Rect> {
    let mut out: Vec<crate::ctrl::Rect> = Vec::new();
    while let Some(mut r) = v.pop() {
        let mut grew = true;
        while grew {
            grew = false;
            let mut i = 0;
            while i < v.len() {
                let o = v[i];
                let (x0, y0) = (r.x.min(o.x), r.y.min(o.y));
                let (x1, y1) = ((r.x + r.w).max(o.x + o.w), (r.y + r.h).max(o.y + o.h));
                let area = |w: i32, h: i32| w as i64 * h as i64;
                let touch = o.x <= r.x + r.w && r.x <= o.x + o.w && o.y <= r.y + r.h && r.y <= o.y + o.h;
                // Only when the bounding box adds little: scattered panels
                // would otherwise chain into one box over the window
                // (overlaps repainted twice give the same pixels).
                if touch && area(x1 - x0, y1 - y0) * 4 <= (area(r.w, r.h) + area(o.w, o.h)) * 5 {
                    r = crate::ctrl::Rect::new(x0, y0, x1 - x0, y1 - y0);
                    v.swap_remove(i);
                    grew = true;
                } else {
                    i += 1;
                }
            }
        }
        out.push(r);
    }
    out
}

/// Text of a stepper value: whole numbers for whole steps.
pub fn stepper_text(v: f32, step: f32) -> String {
    if step.fract() == 0.0 { format!("{}", v.round() as i64) } else { format!("{v:.2}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press picks the nearer handle of a range slider; the drag moves
    /// only that one and keeps it on its side of the other.
    #[test]
    fn range_sliders_drag_the_nearer_handle() {
        let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: crate::style::StyleSheet::new() };
        let mut slider = Ctrl { class: "CtrlRangeSlider".into(), name: "Cut".into(), rect: crate::ctrl::Rect::new(10, 10, 119, 19), visible: true, enabled: true, ..Ctrl::default() };
        slider.set_attr("valueLow", "0.2".into());
        slider.set_attr("valueHigh", "0.8".into());
        let root = Ctrl { class: "root".into(), rect: crate::ctrl::Rect::new(0, 0, 200, 100), visible: true, enabled: true, children: vec![slider], ..Ctrl::default() };
        let mut app = App::new(skin, root);
        let get = |app: &App, k: &str| app.ctrl(&[0]).unwrap().attr_f32(k, -1.0);
        // Near the low handle (x 10 + 9.5 + 0.2 × 100).
        app.mouse_move(40.0, 19.0);
        app.mouse_down(MouseButton::Left);
        let ev = app.mouse_move(60.0, 19.0);
        assert!(ev.iter().any(|e| matches!(e, UiEvent::ValueChanged { name, .. } if name == "Cut")));
        assert!((get(&app, "valueLow") - 0.4).abs() < 1e-4);
        assert!((get(&app, "valueHigh") - 0.8).abs() < 1e-4);
        // Past the high handle: stops there.
        app.mouse_move(190.0, 19.0);
        assert!((get(&app, "valueLow") - 0.8).abs() < 1e-4);
        app.mouse_up(MouseButton::Left);
        // Near the high handle.
        app.mouse_move(100.0, 19.0);
        app.mouse_down(MouseButton::Left);
        app.mouse_move(110.0, 19.0);
        assert!((get(&app, "valueHigh") - 0.9).abs() < 1e-4);
        app.mouse_up(MouseButton::Left);
    }

    /// A menu's hover repaints the menu over the kept pixels beneath it:
    /// the same frame as a full render, also after other repaints under
    /// and beside the menu.
    #[test]
    fn menu_hover_frames_match_full_renders() {
        let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: crate::style::StyleSheet::new() };
        let panel = |name: &str, r: crate::ctrl::Rect, col: &str| {
            let mut c = Ctrl { class: "CtrlPanel".into(), name: name.into(), rect: r, visible: true, enabled: true, ..Ctrl::default() };
            c.set_attr("fillColour", col.into());
            c
        };
        let button = Ctrl { class: "CtrlPanel".into(), name: "Opener".into(), rect: crate::ctrl::Rect::new(20, 10, 40, 16), visible: true, enabled: true, ..Ctrl::default() };
        let kids = vec![panel("Under", crate::ctrl::Rect::new(10, 30, 120, 90), "#336699"), panel("Beside", crate::ctrl::Rect::new(160, 30, 30, 30), "#996633"), button];
        let root = Ctrl { class: "root".into(), rect: crate::ctrl::Rect::new(0, 0, 200, 160), visible: true, enabled: true, children: kids, ..Ctrl::default() };
        for scale in [1.0f32, 2.0] {
            let mut app = App::new(skin.clone(), root.clone());
            let _ = app.render_frame_with(scale, None);
            app.open_menu(&[2], vec!["One".into(), "Two".into(), String::new(), "Three".into()], None);
            let m = app.menu.clone().unwrap();
            let check = |app: &mut App, what: &str| {
                let partial = app.render_frame_with(scale, None).clone();
                let full = crate::render_full(&app.skin, &mut app.fonts, &app.root, scale, None, app.menu.as_ref(), None, None);
                assert!(partial.data() == full.data(), "scale {scale}: {what}");
            };
            check(&mut app, "opened");
            for (k, i) in [0, 1, 3, 0, 1].into_iter().enumerate() {
                let _ = app.mouse_move(m.x + 5.0, m.y + 10.0 + i as f32 * 20.0);
                assert!(!app.dirty && app.needs_redraw());
                check(&mut app, &format!("hover {k}"));
                // Something beneath the menu and something beside it change.
                let (path, col) = if k % 2 == 0 { (0, "#00ff00") } else { (1, "#ff00ff") };
                app.ctrl_mut(&[path]).unwrap().set_attr("fillColour", col.into());
                let r = app.abs_rect(&[path]);
                app.invalidate_rect(r);
                check(&mut app, &format!("repaint {k}"));
            }
            // Off the items: no highlight.
            let _ = app.mouse_move(m.x - 5.0, m.y - 5.0);
            check(&mut app, "left");
        }
    }

    /// Presses, releases and menus opening and closing repaint only what
    /// they change: no whole-window repaint, the same frame as a full
    /// render.
    #[test]
    fn clicks_repaint_only_what_they_change() {
        let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: crate::style::StyleSheet::new() };
        let mut back = Ctrl { class: "CtrlPanel".into(), name: "Back".into(), rect: crate::ctrl::Rect::new(0, 30, 200, 130), visible: true, enabled: true, ..Ctrl::default() };
        back.set_attr("fillColour", "#335577".into());
        let button = |name: &str, x: i32, momentary: bool| {
            let mut b = Ctrl { class: "CtrlButton".into(), name: name.into(), rect: crate::ctrl::Rect::new(x, 6, 40, 18), visible: true, enabled: true, ..Ctrl::default() };
            b.set_attr("optIsMomentary", momentary.to_string());
            b
        };
        let root = Ctrl {
            class: "root".into(),
            rect: crate::ctrl::Rect::new(0, 0, 200, 160),
            visible: true,
            enabled: true,
            children: vec![back, button("Toggle", 10, false), button("Opener", 60, true)],
            ..Ctrl::default()
        };
        for scale in [1.0f32, 2.0] {
            let mut app = App::new(skin.clone(), root.clone());
            let _ = app.render_frame_with(scale, None);
            let check = |app: &mut App, what: &str| {
                assert!(!app.dirty, "scale {scale}: {what} repaints the whole window");
                let partial = app.render_frame_with(scale, None).clone();
                let full = crate::render_full(&app.skin, &mut app.fonts, &app.root, scale, None, app.menu.as_ref(), None, None);
                let off: Vec<usize> = (0..partial.data().len() / 4).filter(|i| partial.data()[i * 4..i * 4 + 4] != full.data()[i * 4..i * 4 + 4]).collect();
                assert!(
                    off.is_empty(),
                    "scale {scale}: {what}: {} differ, first {:?}",
                    off.len(),
                    off.iter().take(5).map(|i| (i % partial.width() as usize, i / partial.width() as usize, &partial.data()[i * 4..i * 4 + 4], &full.data()[i * 4..i * 4 + 4])).collect::<Vec<_>>()
                );
            };
            let click = |app: &mut App, x: f32, y: f32, what: &str| {
                let _ = app.mouse_move(x, y);
                let mut ev = app.mouse_down(MouseButton::Left);
                check(app, &format!("{what} press"));
                ev.extend(app.mouse_up(MouseButton::Left));
                check(app, &format!("{what} release"));
                ev
            };
            for k in 0..2 {
                let ev = click(&mut app, 30.0, 15.0, "toggle");
                assert!(ev.iter().any(|e| matches!(e, UiEvent::Clicked { name, on, .. } if name == "Toggle" && *on == (k == 0))));
            }
            // Opened from the click, closed by a click beside it, then
            // opened again and an item chosen.
            for close_at in [(190.0, 150.0), (0.0, 0.0)] {
                let ev = click(&mut app, 80.0, 15.0, "opener");
                assert!(ev.iter().any(|e| matches!(e, UiEvent::Clicked { name, .. } if name == "Opener")));
                app.open_menu(&[2], vec!["One".into(), "Two".into()], None);
                check(&mut app, "menu opened");
                let m = app.menu.clone().unwrap();
                let at = if close_at.0 > 0.0 { close_at } else { (m.x + 5.0, m.y + 30.0) };
                let _ = app.mouse_move(at.0, at.1);
                check(&mut app, "menu hovered");
                let ev = app.mouse_down(MouseButton::Left);
                assert!(app.menu.is_none());
                check(&mut app, "menu closed");
                assert_eq!(close_at.0 == 0.0, ev.iter().any(|e| matches!(e, UiEvent::MenuSelected { index: 1, .. })));
                let _ = app.mouse_up(MouseButton::Left);
                check(&mut app, "release after the menu");
            }
        }
    }

    /// After a change, a code-drawn control repaints only when its drawer
    /// now draws it differently; the frame stays a full render's.
    #[test]
    fn custom_controls_repaint_when_their_drawing_changes() {
        struct Bars<'a>(&'a std::cell::Cell<[u8; 2]>);
        impl crate::render::CustomDraw for Bars<'_> {
            fn draw(&self, _r: &crate::render::Renderer, p: &mut crate::paint::Painter, c: &Ctrl) -> bool {
                let k = if c.overload == "BarA" {
                    0
                } else if c.overload == "BarB" {
                    1
                } else {
                    return false;
                };
                let v = self.0.get()[k];
                p.fill_rect(crate::paint::RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32), crate::Colour::rgba(20, 20, 20, 255));
                p.fill_rect(crate::paint::RectF::new(2.0, 2.0, v as f32, 6.0), crate::Colour::rgba(200, 120, 40, 255));
                true
            }
        }
        let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: crate::style::StyleSheet::new() };
        let bar = |o: &str, y: i32| Ctrl { class: "CtrlPanel".into(), overload: o.into(), rect: crate::ctrl::Rect::new(10, y, 100, 10), visible: true, enabled: true, ..Ctrl::default() };
        let mut label = Ctrl { class: "CtrlPanel".into(), name: "Plain".into(), rect: crate::ctrl::Rect::new(10, 60, 40, 10), visible: true, enabled: true, ..Ctrl::default() };
        label.set_attr("fillColour", "#204060".into());
        let root = Ctrl { class: "root".into(), rect: crate::ctrl::Rect::new(0, 0, 120, 80), visible: true, enabled: true, children: vec![bar("BarA", 10), bar("BarB", 30), label], ..Ctrl::default() };
        let state = std::cell::Cell::new([30u8, 50]);
        for scale in [1.0f32, 2.0] {
            let mut app = App::new(skin.clone(), root.clone());
            let d = Bars(&state);
            let _ = app.render_frame_with(scale, Some(&d));
            let step = |app: &mut App, s: [u8; 2], colour: &str, want: &[crate::ctrl::Rect]| {
                let before = app.marks();
                state.set(s);
                app.ctrl_mut(&[2]).unwrap().set_attr("fillColour", colour.into());
                app.invalidate_changes_with(&before, Some(&d));
                let mut got = app.pending_damage().to_vec();
                got.sort_by_key(|r| (r.y, r.x));
                assert_eq!(got, want, "scale {scale}: {s:?}");
                let partial = app.render_frame_with(scale, Some(&d)).clone();
                let full = crate::render_full(&app.skin, &mut app.fonts, &app.root, scale, Some(&d), None, None, None);
                assert!(partial.data() == full.data(), "scale {scale}: {s:?}");
            };
            let (a, b, plain) = (crate::ctrl::Rect::new(10, 10, 100, 10), crate::ctrl::Rect::new(10, 30, 100, 10), crate::ctrl::Rect::new(10, 60, 40, 10));
            // Nothing changed: nothing repaints.
            step(&mut app, [30, 50], "#204060", &[]);
            // One bar's state: that bar.
            step(&mut app, [30, 70], "#204060", &[b]);
            // The other bar and a plain control.
            step(&mut app, [10, 70], "#406080", &[a, plain, plain]);
            // Back to an earlier state: still repainted.
            step(&mut app, [30, 50], "#406080", &[a, b]);
            state.set([30, 50]);
        }
    }

    #[test]
    fn dragable_value_boxes_drag_their_value() {
        let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: crate::style::StyleSheet::new() };
        let mut vb = Ctrl { class: "CtrlValueBox".into(), name: "Num".into(), rect: crate::ctrl::Rect::new(10, 50, 30, 17), visible: true, enabled: true, ..Ctrl::default() };
        vb.set_attr("optSingleClickEdit", "false".into());
        let mut plain = vb.clone();
        plain.name = "Plain".into();
        plain.rect = crate::ctrl::Rect::new(60, 50, 30, 17);
        vb.set_attr("dragable", "true".into());
        let root = Ctrl { class: "root".into(), rect: crate::ctrl::Rect::new(0, 0, 200, 300), visible: true, enabled: true, children: vec![vb, plain], ..Ctrl::default() };
        let mut app = App::new(skin, root);
        app.mouse_move(20.0, 58.0);
        app.mouse_down(MouseButton::Left);
        let ev = app.mouse_move(20.0, 8.0);
        assert!(ev.iter().any(|e| matches!(e, UiEvent::ValueChanged { name, value, .. } if name == "Num" && (*value - 0.25).abs() < 1e-4)));
        app.mouse_up(MouseButton::Left);
        // Without `dragable` the press passes through: no value change.
        app.mouse_move(70.0, 58.0);
        app.mouse_down(MouseButton::Left);
        let ev = app.mouse_move(70.0, 8.0);
        assert!(!ev.iter().any(|e| matches!(e, UiEvent::ValueChanged { .. })));
        app.mouse_up(MouseButton::Left);
    }
}
