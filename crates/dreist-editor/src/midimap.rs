//! MIDI map tab ([re:0a1cc7], `Automation/AutoMapPanel.skin`): the
//! project's bindings as a table.
//!
//! - [re:cd3bfc]: Lock (option `BindingsLocked`),
//!   Clear All, Auto select (`SelectIncomingBindings`, default on), Send
//!   Note / Send CC toggles; AddButton hidden.
//! - [re:5a7a2b]: columns Clear (40), Locale (170,
//!   min 90), Path, Name (170, min 90), Min, Max, Type (90), Focus (40), rows
//!   20 px. Locale reads "MIDI Ch %d CC %d" or "Host param %d"; Clear is the
//!   icon-font glyph U+E922; Type is a popup of the binding types; Min/Max are
//!   value steppers for continuous bindings only; Focus (only for bindings
//!   with a target mode) shows ☑/☐ and is disabled for messages
//!   0x1b2–0x1bf. Table styles `BrowserListViewStyle` /
//!   `BrowserListViewHeaderStyle`.
//!
//! Edits change the document's `BindingMap` and rebuild the engine.

use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{FontSpec, HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Skin};

use crate::command::Command;
use crate::model::Model;
use crate::{EditorError, err};

/// [re:54266d] (`CreateBindingTypeMap`).
pub const BINDING_TYPES: [&str; 6] = ["Continuous", "Momentary", "Latching", "Trigger", "Gated", "FlipFlop"];
const ROW: f32 = 20.0;
const HEADER: f32 = 20.0;
const TABLE_STYLE: &str = "BrowserListViewStyle";
/// `intScrollBarDim` of the skin's TableView; `VScrollBarStyle`.
const BAR: f32 = 12.0;
const BAR_STYLE: &str = "ScrollbarStyle";

/// Rows that fit below the header.
fn visible_rows(h: f32) -> usize {
    ((h - HEADER) / ROW).floor().max(1.0) as usize
}

/// The vertical scroll bar (`optAutoHide`, not overlaid: it takes its width
/// from the columns): track y and height, thumb y and height, or None when
/// every row fits.
fn bar_geometry(h: f32, n: usize, scroll: usize) -> Option<(f32, f32, f32, f32)> {
    let vis = visible_rows(h);
    if n <= vis {
        return None;
    }
    let track = (h - HEADER).max(1.0);
    let th = (track * vis as f32 / n as f32).max(8.0);
    let ty = HEADER + (track - th) * scroll.min(n - vis) as f32 / (n - vis) as f32;
    Some((HEADER, track, ty, th))
}
const HEADER_STYLE: &str = "BrowserListViewHeaderStyle";
/// (title, width, min, max) from `UpdateTable` (`AddColumn`: resizable
/// when min ≠ max; u32::MAX = no maximum).
const COLUMNS: [(&str, f32, f32, f32); 8] = [
    ("Clear", 40.0, 40.0, 40.0),
    ("Locale", 170.0, 90.0, f32::MAX),
    ("Path", 170.0, 90.0, f32::MAX),
    ("Name", 170.0, 90.0, f32::MAX),
    ("Min", 90.0, 90.0, f32::MAX),
    ("Max", 90.0, 90.0, f32::MAX),
    ("Type", 90.0, 90.0, f32::MAX),
    ("Focus", 40.0, 40.0, 40.0),
];

/// [re:856d26] (ev_b5b84613…): the last 5 px of a resizable
/// column's header cell resize it.
const RESIZE_GRIP: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Midi(usize, usize),
    Host(usize),
}

impl Key {
    fn count_attr(self) -> String {
        match self {
            Key::Midi(ch, cc) => format!("MIDI{ch}CC{cc}BindingCount"),
            Key::Host(p) => format!("HostBindingCount{p}"),
        }
    }

    fn child_name(self, k: usize) -> String {
        match self {
            Key::Midi(ch, cc) => format!("MIDI{ch}CC{cc}Binding{k}"),
            Key::Host(p) => format!("Auto{p}Binding{k}"),
        }
    }

    fn of(name: &str) -> Option<(Key, usize)> {
        if let Some(r) = name.strip_prefix("MIDI") {
            let (ch, r) = r.split_once("CC")?;
            let (cc, k) = r.split_once("Binding")?;
            return Some((Key::Midi(ch.parse().ok()?, cc.parse().ok()?), k.parse().ok()?));
        }
        let r = name.strip_prefix("Auto")?;
        let (p, k) = r.split_once("Binding")?;
        Some((Key::Host(p.parse().ok()?), k.parse().ok()?))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: Key,
    pub index: usize,
    pub path: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub kind: usize,
    pub target_mode: bool,
    pub enter_target: bool,
    pub message: Option<u32>,
    /// Parameter index of a pad (0x4e value 1), engine (0x90 value 1) or
    /// layer (0x66 value 0) parameter binding.
    pub param: i32,
    /// The whole message prototype.
    pub proto: Option<dreist_engine::binding::Message>,
}

impl Row {
    pub fn locale(&self) -> String {
        match self.key {
            Key::Midi(ch, cc) => format!("MIDI Ch {} CC {}", ch + 1, cc),
            Key::Host(p) => format!("Host param {}", p + 1),
        }
    }
}

fn bindings(doc: &Node) -> Option<&Node> {
    doc.children.iter().find(|c| c.class == "BindingMap")
}

fn bindings_mut(doc: &mut Node) -> &mut Node {
    if let Some(i) = doc.children.iter().position(|c| c.class == "BindingMap") {
        return &mut doc.children[i];
    }
    let mut n = Node::new("BindingMap", Some("Bindings"));
    n.version = Some(2);
    doc.children.push(n);
    doc.children.last_mut().unwrap()
}

fn string(n: &Node, name: &str) -> String {
    n.children.iter().find(|c| c.name.as_deref() == Some(name)).and_then(|c| c.attr("string")).unwrap_or("").to_string()
}

/// The bindings in table order (MIDI channel/CC, then host parameters).
pub fn rows(doc: &Node) -> Vec<Row> {
    let Some(m) = bindings(doc) else { return Vec::new() };
    let mut v: Vec<Row> = m
        .children
        .iter()
        .filter(|c| c.class == "Binding")
        .filter_map(|c| {
            let (key, index) = Key::of(c.name.as_deref()?)?;
            let kind = BINDING_TYPES.iter().position(|t| *t == string(c, "Type")).unwrap_or(0);
            let proto = c.children.iter().find(|p| p.name.as_deref() == Some("MessagePrototype")).map(dreist_engine::binding::Message::from_node);
            let message = proto.as_ref().map(|m| m.kind);
            let param = proto.as_ref().map(|m| if m.kind == 0x66 { m.value[0] } else { m.value[1] } as i32).unwrap_or(0);
            Some(Row {
                key,
                index,
                path: string(c, "Path"),
                name: string(c, "Name"),
                min: c.f32("Min").unwrap_or(0.0),
                max: c.f32("Max").unwrap_or(1.0),
                kind,
                target_mode: c.bool("TargetMode").unwrap_or(false),
                enter_target: c.bool("EnterTargetMode").unwrap_or(false),
                message,
                param,
                proto,
            })
        })
        .collect();
    v.sort_by_key(|r| {
        let k = match r.key {
            Key::Midi(ch, cc) => (0, ch * 128 + cc),
            Key::Host(p) => (1, p),
        };
        (k, r.index)
    });
    v
}

fn binding_node_mut<'a>(doc: &'a mut Node, r: &Row) -> Option<&'a mut Node> {
    let name = r.key.child_name(r.index);
    bindings_mut(doc).children.iter_mut().find(|c| c.class == "Binding" && c.name.as_deref() == Some(&name))
}

/// The row right-click menu (`UpdateTable` $_3).
const ROW_MENU: [&str; 6] = ["Remove mapping", "", "Invert range", "Reset min", "Reset max", "Reset min and max"];

/// Remove one binding, renumbering the rest under its key.
pub fn remove(doc: &mut Node, r: &Row) {
    let m = bindings_mut(doc);
    let name = r.key.child_name(r.index);
    m.children.retain(|c| !(c.class == "Binding" && c.name.as_deref() == Some(&name)));
    let mut k = 0;
    for c in m.children.iter_mut().filter(|c| c.class == "Binding") {
        if let Some((key, _)) = c.name.as_deref().and_then(Key::of) {
            if key == r.key {
                c.name = Some(r.key.child_name(k));
                k += 1;
            }
        }
    }
    let attr = r.key.count_attr();
    m.attrs.retain(|(a, _)| *a != attr);
    if k > 0 {
        m.set(&attr, k);
    }
}

/// Add a binding of `proto` (value slot `stuffer`) to MIDI channel/CC
/// (`midi`) or host parameter `a`; returns the new node.
#[allow(clippy::too_many_arguments)]
pub fn add_binding(doc: &mut Node, midi: bool, a: usize, b: usize, proto: &dreist_engine::binding::Message, stuffer: usize, path: &str, name: &str, kind: &str) -> Node {
    let key = if midi { Key::Midi(a, b) } else { Key::Host(a) };
    let m = bindings_mut(doc);
    let k = m.children.iter().filter(|c| c.class == "Binding" && c.name.as_deref().and_then(Key::of).is_some_and(|(kk, _)| kk == key)).count();
    let mut n = Node::new("Binding", Some(&key.child_name(k)));
    n.set("EnterTargetMode", "false").set("MajorIndex", a).set("Max", 1).set("Min", 0).set("MinorIndex", b);
    if proto.editor {
        // [re:1aeee8] `EditorOnly`: the message goes to the editor.
        n.set("EditorOnly", "true");
    }
    let s = |c: &str, nm: &str, v: &str| {
        let mut x = Node::new(c, Some(nm));
        x.set("string", v);
        x
    };
    n.children.push(s("VFixedString_64", "Path", path));
    n.children.push(s("VFixedString_64", "Name", name));
    n.children.push(s("VString", "MessageStuffer", &format!("Binding::StuffToValue{}", stuffer.min(2))));
    n.children.push(s("VString", "MessagePopper", &format!("Binding::GetValueFromValue{}", stuffer.min(2))));
    n.children.push(proto.to_node("MessagePrototype"));
    n.children.push(s("VString", "Type", kind));
    m.children.push(n.clone());
    let attr = key.count_attr();
    m.attrs.retain(|(x, _)| *x != attr);
    m.set(&attr, k + 1);
    n
}

/// Remove the bindings whose prototype is `kind` with `index`.
pub fn remove_matching(doc: &mut Node, kind: u32, index: &[i32; 6]) -> bool {
    let rs: Vec<Row> = rows(doc).into_iter().collect();
    let mut any = false;
    for r in rs.iter().rev() {
        let Some(node) = bindings(doc).and_then(|m| m.children.iter().find(|c| c.name.as_deref() == Some(&r.key.child_name(r.index)))) else { continue };
        let Some(p) = node.children.iter().find(|c| c.name.as_deref() == Some("MessagePrototype")) else { continue };
        let msg = dreist_engine::binding::Message::from_node(p);
        if msg.kind == kind && msg.index == *index {
            remove(doc, r);
            any = true;
        }
    }
    any
}

/// `Clear All`.
pub fn clear(doc: &mut Node) {
    let m = bindings_mut(doc);
    m.children.retain(|c| c.class != "Binding");
    m.attrs.retain(|(a, _)| !(a.ends_with("BindingCount") || a.starts_with("HostBindingCount")));
}

fn set_string(n: &mut Node, name: &str, v: &str) {
    if let Some(c) = n.children.iter_mut().find(|c| c.name.as_deref() == Some(name)) {
        c.set("string", v);
    } else {
        let mut c = Node::new("VString", Some(name));
        c.set("string", v);
        n.children.push(c);
    }
}

/// [re:66fafd] / `GetBindingMinMax` [re:260d06]:
/// a binding's Min/Max in the target's units — pad, engine and layer
/// parameters through their conversions, gains (metronome 0x24, aux 0x9f,
/// master 0xa5, preview 0x15f) linear −100..6, master tune 0xa6 −12..12,
/// everything else 0..1.
pub fn binding_value(r: &Row, v: f32) -> f32 {
    use dreist_formats::{EngineParam, LayerParam, PadParam};
    let i = r.param.max(0) as usize;
    let lerp = |lo: f32, hi: f32| (hi - lo) * v + lo;
    match r.message {
        Some(0x4e) => PadParam::ALL.get(i).map(|p| p.curve().to_units(v)).unwrap_or(v),
        Some(0x90) => EngineParam::ALL.get(i).map(|p| p.curve().to_units(v)).unwrap_or(v),
        Some(0x66) => LayerParam::ALL.get(i).map(|p| p.curve().to_units(v)).unwrap_or(v),
        Some(0x24 | 0x9f | 0xa5 | 0x15f) => lerp(-100.0, 6.0),
        Some(0xa6) => lerp(-12.0, 12.0),
        _ => v,
    }
}

/// `BindingsLocked`: the loaded project keeps the current bindings
/// ([re:92292a] skips restoring "Bindings" while
/// `BindingMap` +0xce48 is set).
pub fn carry_bindings(from: &Node, to: &mut Node) {
    to.children.retain(|c| c.class != "BindingMap");
    if let Some(b) = bindings(from) {
        to.children.push(b.clone());
    }
}

pub struct MidiMap {
    page: Vec<usize>,
    table: Option<Vec<usize>>,
    /// The option buttons `sync` sets.
    named: dreist_gui::app::PathCache,
    scroll: usize,
    selected: Option<usize>,
    /// Row whose Type menu is open.
    menu_row: Option<usize>,
    /// A row's right-click menu is open (the row).
    row_menu: Option<usize>,
    /// Min/max drag: (row, column 4/5, start y, start value).
    drag: Option<(usize, usize, f32, f32)>,
    locked: bool,
    auto_select: bool,
    send_notes: bool,
    send_cc: bool,
    /// The Clear All confirmation menu is open.
    confirm: bool,
    /// Scroll thumb drag: (start y, start scroll).
    bar_drag: Option<(f32, usize)>,
    /// Column widths (user-resizable ones change by header drags).
    widths: [f32; 8],
    /// Header grip drag: (column, last x).
    col_drag: Option<(usize, f32)>,
}

/// Column x positions: the columns side by side at their widths (header
/// cells laid out in order; the table does not stretch them).
fn layout(widths: &[f32; 8]) -> [(f32, f32); 8] {
    let mut out = [(0.0, 0.0); 8];
    let mut x = 0.0;
    for (i, w) in widths.iter().enumerate() {
        out[i] = (x, *w);
        x += w;
    }
    out
}

/// The resizable column whose header grip is at (`x`, `y`).
fn grip_at(widths: &[f32; 8], x: f32, y: f32) -> Option<usize> {
    if !(0.0..HEADER).contains(&y) {
        return None;
    }
    layout(widths).iter().position(|(cx, cw)| x < cx + cw && x >= cx + cw - RESIZE_GRIP).filter(|&i| COLUMNS[i].2 != COLUMNS[i].3)
}

impl MidiMap {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize]) -> Result<MidiMap, EditorError> {
        let mut host = &mut *root;
        for &i in page {
            host = &mut host.children[i];
        }
        skin.mount(host, "AutoMapPanel.skin").map_err(err)?;
        if let Some(t) = host.find_mut("TableView") {
            t.overload = "MIDIMapTable".into();
        }
        if let Some(a) = host.find_mut("AddButton") {
            a.visible = false;
        }
        let table = root.path_of("TableView").filter(|p| p.starts_with(page));
        let o = crate::options::Options::load();
        Ok(MidiMap {
            page: page.to_vec(),
            table,
            named: Default::default(),
            scroll: 0,
            selected: None,
            menu_row: None,
            row_menu: None,
            drag: None,
            locked: o.bool("BindingsLocked", false),
            auto_select: o.bool("SelectIncomingBindings", true),
            send_notes: o.bool("SendMIDINotes", false),
            send_cc: o.bool("SendMIDICCs", false),
            confirm: false,
            bar_drag: None,
            widths: COLUMNS.map(|c| c.1),
            col_drag: None,
        })
    }

    /// 0x3 `e_registryMIDIOptionsChanged` [send notes, send CCs].
    pub fn midi_options_message(&self) -> Command {
        crate::command::msg(dreist_engine::binding::id::REGISTRY_MIDI_OPTIONS_CHANGED, &[], &[self.send_notes as i32 as f32, self.send_cc as i32 as f32])
    }

    pub fn sync(&mut self, app: &mut App, _model: &Model) {
        let states = [("Lock", self.locked), ("Select", self.auto_select), ("SendNotesButton", self.send_notes), ("SendCCButton", self.send_cc)];
        for (name, on) in states {
            if let Some(c) = self.named.path_in(app, &self.page, name).and_then(|p| app.ctrl_mut(&p)) {
                c.on = on;
            }
        }
    }

    fn rebuild(model: &Model, out: &mut Vec<Command>) {
        out.push(model.rebuild_command());
    }

    fn max_scroll(&self, app: &App, n: usize) -> usize {
        let h = self.table.as_ref().and_then(|t| app.ctrl(t)).map(|c| c.rect.h as f32).unwrap_or(0.0);
        n.saturating_sub(visible_rows(h))
    }

    fn cell(&self, app: &App, _n: usize, x: f32, y: f32) -> Option<(usize, usize)> {
        app.ctrl(self.table.as_ref()?)?;
        if y < HEADER {
            return None;
        }
        let row = ((y - HEADER) / ROW) as usize + self.scroll;
        let col = layout(&self.widths).iter().position(|(cx, cw)| x >= *cx && x < cx + cw)?;
        Some((row, col))
    }

    /// Hover over a header grip: the left / right cursor.
    pub fn hover_cursor(&self, overload: &str, x: f32, y: f32) -> Option<dreist_gui::cursor::Cursor> {
        (overload == "MIDIMapTable" && grip_at(&self.widths, x, y).is_some()).then_some(dreist_gui::cursor::Cursor::LeftRight)
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        match ev {
            UiEvent::Clicked { path, name, on, .. } if path.starts_with(&self.page) => {
                match name.as_str() {
                    // `HandleControlEvent` [re:99090c]: confirm, then 0x10.
                    "Clear" if app.ctrl(path).is_some_and(|c| c.class == "CtrlButton") => {
                        let items = vec!["Are you sure you want to remove all bindings? This cannot be undone!".into(), String::new(), "Yes".into(), "No".into()];
                        app.open_menu(path, items, None);
                        self.confirm = true;
                        return false;
                    }
                    "Lock" | "Select" | "SendNotesButton" | "SendCCButton" => {
                        let key = match name.as_str() {
                            "Lock" => {
                                self.locked = *on;
                                "BindingsLocked"
                            }
                            "Select" => {
                                self.auto_select = *on;
                                "SelectIncomingBindings"
                            }
                            "SendNotesButton" => {
                                self.send_notes = *on;
                                "SendMIDINotes"
                            }
                            _ => {
                                self.send_cc = *on;
                                "SendMIDICCs"
                            }
                        };
                        let mut o = crate::options::Options::load();
                        o.set(key, *on);
                        o.save();
                        if key.starts_with("SendMIDI") {
                            out.push(self.midi_options_message());
                        }
                    }
                    _ => return false,
                }
                true
            }
            UiEvent::Wheel { overload, dy, .. } if overload == "MIDIMapTable" => {
                let n = rows(&model.doc).len();
                self.scroll = (self.scroll as i32 - dy.signum() as i32).clamp(0, self.max_scroll(app, n) as i32) as usize;
                true
            }
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "MIDIMapTable" => {
                // [re:ba68d6] / `OnMouseMove`:
                // a press on a grip resizes that column by the pointer's
                // travel within its min / max.
                if let Some((col, last)) = self.col_drag {
                    match kind {
                        MouseKind::Drag => {
                            let (_, min, max) = (COLUMNS[col].1, COLUMNS[col].2, COLUMNS[col].3);
                            self.widths[col] = (self.widths[col] + x - last).clamp(min, max);
                            self.col_drag = Some((col, *x));
                            app.cursor = dreist_gui::cursor::Cursor::LeftRight;
                        }
                        MouseKind::Up => self.col_drag = None,
                        _ => {}
                    }
                    return true;
                }
                if *kind == MouseKind::Down {
                    if let Some(col) = grip_at(&self.widths, *x, *y) {
                        self.col_drag = Some((col, *x));
                        app.cursor = dreist_gui::cursor::Cursor::LeftRight;
                        return true;
                    }
                }
                let rs = rows(&model.doc);
                // The scroll bar: drag the thumb, click the track to page.
                let geo = self.table.as_ref().and_then(|t| app.ctrl(t)).and_then(|c| bar_geometry(c.rect.h as f32, rs.len(), self.scroll).map(|g| (c.rect.w as f32, g)));
                if let Some((w, (_, track, ty, th))) = geo {
                    let max = self.max_scroll(app, rs.len());
                    match kind {
                        MouseKind::Down if *x >= w - BAR && *y >= HEADER => {
                            if *y >= ty && *y < ty + th {
                                self.bar_drag = Some((*y, self.scroll));
                            } else {
                                let page = visible_rows(app.ctrl(self.table.as_ref().unwrap()).unwrap().rect.h as f32);
                                self.scroll = if *y < ty { self.scroll.saturating_sub(page) } else { (self.scroll + page).min(max) };
                            }
                            return true;
                        }
                        MouseKind::Drag if self.bar_drag.is_some() => {
                            let (y0, s0) = self.bar_drag.unwrap();
                            let per = max as f32 / (track - th).max(1.0);
                            self.scroll = (s0 as f32 + (y - y0) * per).round().clamp(0.0, max as f32) as usize;
                            return true;
                        }
                        _ => {}
                    }
                }
                if *kind == MouseKind::Up && self.bar_drag.take().is_some() {
                    return true;
                }
                match kind {
                    // [re:fb19a6] $_3 [re:1cc864]: a row's
                    // right-click menu (the row gets selected).
                    MouseKind::RightDown => {
                        let Some((row, _)) = self.cell(app, rs.len(), *x, *y) else { return false };
                        if row >= rs.len() {
                            return false;
                        }
                        self.selected = Some(row);
                        self.row_menu = Some(row);
                        app.open_menu(path, ROW_MENU.iter().map(|s| s.to_string()).collect(), None);
                        true
                    }
                    MouseKind::Down => {
                        let Some((row, col)) = self.cell(app, rs.len(), *x, *y) else { return false };
                        let Some(r) = rs.get(row).cloned() else { return false };
                        self.selected = Some(row);
                        match col {
                            0 => {
                                remove(&mut model.doc, &r);
                                self.selected = None;
                                Self::rebuild(model, out);
                            }
                            4 | 5 if r.kind == 0 => self.drag = Some((row, col, *y, if col == 4 { r.min } else { r.max })),
                            6 => {
                                self.menu_row = Some(row);
                                app.open_menu(path, BINDING_TYPES.iter().map(|s| s.to_string()).collect(), Some(r.kind));
                            }
                            7 if r.target_mode && !r.message.is_some_and(|m| (0x1b2..=0x1bf).contains(&m)) => {
                                if let Some(n) = binding_node_mut(&mut model.doc, &r) {
                                    n.set("EnterTargetMode", !r.enter_target);
                                }
                                Self::rebuild(model, out);
                            }
                            _ => {}
                        }
                        true
                    }
                    MouseKind::Drag => {
                        let Some((row, col, y0, v0)) = self.drag else { return false };
                        let Some(r) = rs.get(row).cloned() else { return false };
                        let v = (v0 + (y0 - y) * 0.005).clamp(0.0, 1.0);
                        let v = (v * 100.0).round() / 100.0;
                        if let Some(n) = binding_node_mut(&mut model.doc, &r) {
                            n.set(if col == 4 { "Min" } else { "Max" }, v);
                        }
                        true
                    }
                    MouseKind::Up => {
                        if self.drag.take().is_some() {
                            Self::rebuild(model, out);
                            return true;
                        }
                        false
                    }
                }
            }
            UiEvent::MenuSelected { name, index, .. } if self.confirm && name == "Clear" => {
                self.confirm = false;
                if *index == 2 {
                    clear(&mut model.doc);
                    self.selected = None;
                    Self::rebuild(model, out);
                    return true;
                }
                false
            }
            // Remove mapping (0xf), Invert range / Reset min / max / both
            // (0x3a after editing the binding).
            UiEvent::MenuSelected { name, index, .. } if name == "TableView" && self.row_menu.is_some() => {
                let Some(row) = self.row_menu.take() else { return false };
                let Some(r) = rows(&model.doc).get(row).cloned() else { return false };
                if *index == 0 {
                    remove(&mut model.doc, &r);
                    self.selected = None;
                } else if let Some(n) = binding_node_mut(&mut model.doc, &r) {
                    let (lo, hi) = (n.f32("Min").unwrap_or(0.0), n.f32("Max").unwrap_or(1.0));
                    let (lo, hi) = match index {
                        2 => (hi, lo),
                        3 => (0.0, hi),
                        4 => (lo, 1.0),
                        5 => (0.0, 1.0),
                        _ => return false,
                    };
                    n.set("Min", dreist_formats::node::format_g16(lo as f64)).set("Max", dreist_formats::node::format_g16(hi as f64));
                }
                Self::rebuild(model, out);
                true
            }
            UiEvent::MenuSelected { name, index, .. } if name == "TableView" => {
                let Some(row) = self.menu_row.take() else { return false };
                let Some(r) = rows(&model.doc).get(row).cloned() else { return false };
                if let Some(n) = binding_node_mut(&mut model.doc, &r) {
                    set_string(n, "Type", BINDING_TYPES[(*index).min(5)]);
                }
                Self::rebuild(model, out);
                true
            }
            _ => false,
        }
    }

    /// `ATC_Tick` [re:451f3a]: with Auto select on, the binding of the last
    /// incoming MIDI channel/CC gets selected and scrolled into view.
    pub fn incoming(&mut self, app: &App, model: &Model, ch: usize, cc: usize) {
        if !self.auto_select {
            return;
        }
        let Some(row) = rows(&model.doc).iter().position(|r| r.key == Key::Midi(ch, cc)) else { return };
        self.selected = Some(row);
        let visible = self.table.as_ref().and_then(|t| app.ctrl(t)).map(|c| ((c.rect.h as f32 - HEADER) / ROW).floor().max(1.0) as usize).unwrap_or(1);
        if row < self.scroll || row >= self.scroll + visible {
            self.scroll = row.min(self.max_scroll(app, rows(&model.doc).len()));
        }
    }

    pub fn draw(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != "MIDIMapTable" {
            return false;
        }
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let rs = rows(&model.doc);
        let bar = bar_geometry(h, rs.len(), self.scroll);
        let cols = layout(&self.widths);
        let font = s.font(TABLE_STYLE);
        let text = s.colour(TABLE_STYLE, "colText", Colour::BLACK);
        let alt = s.colour(TABLE_STYLE, "colAltFill", Colour::WHITE);
        let fill = s.colour(TABLE_STYLE, "colFill", Colour::TRANSPARENT);
        let divider = s.colour(TABLE_STYLE, "colColDivider", Colour::BLACK);
        let selection = s.colour(TABLE_STYLE, "colRowSelection", Colour::BLACK);
        let icons = FontSpec { family: dreist_gui::text::icon_family().into(), size: 16.0, bold: false };
        let big = s.fonts.get("ExLarge").cloned().unwrap_or_else(|| font.clone());
        p.fill_rect(RectF::new(0.0, 0.0, w, h), fill);
        // Header.
        let hfill = s.colour(HEADER_STYLE, "colFill", Colour::WHITE);
        let htext = s.colour(HEADER_STYLE, "colText", Colour::BLACK);
        let hdiv = s.colour(HEADER_STYLE, "colAltBorder", divider);
        let hfont = s.font(HEADER_STYLE);
        p.fill_rect(RectF::new(0.0, 0.0, w, HEADER), hfill);
        for (i, (x, cw)) in cols.iter().enumerate() {
            p.text(RectF::new(x + 4.0, 0.0, cw - 8.0, HEADER), COLUMNS[i].0, htext, &hfont, HAlign::Left, VAlign::Center);
            p.fill_rect(RectF::new(x + cw - 1.0, 0.0, 1.0, HEADER), hdiv);
        }
        let visible = ((h - HEADER) / ROW).ceil() as usize;
        for (k, row) in rs.iter().enumerate().skip(self.scroll).take(visible) {
            let y = HEADER + (k - self.scroll) as f32 * ROW;
            let bg = if self.selected == Some(k) {
                selection
            } else if k % 2 == 1 {
                alt
            } else {
                Colour::TRANSPARENT
            };
            p.fill_rect(RectF::new(0.0, y, w, ROW), bg);
            let cell = |i: usize| RectF::new(cols[i].0 + 4.0, y, cols[i].1 - 8.0, ROW);
            p.text(RectF::new(cols[0].0, y, cols[0].1, ROW), "\u{e922}", text, &icons, HAlign::Center, VAlign::Center);
            for (i, t) in [(1, row.locale()), (2, row.path.clone()), (3, row.name.clone())] {
                let shown = p.fonts.shorten(&font, &t, cols[i].1 - 8.0, dreist_gui::style::Shorten::End);
                p.text(cell(i), &shown, text, &font, HAlign::Left, VAlign::Center);
            }
            if row.kind == 0 {
                p.text(cell(4), &format!("{:.2}", binding_value(row, row.min)), text, &font, HAlign::Left, VAlign::Center);
                p.text(cell(5), &format!("{:.2}", binding_value(row, row.max)), text, &font, HAlign::Left, VAlign::Center);
            }
            p.text(cell(6), &format!("{} ▾", BINDING_TYPES[row.kind]), text, &font, HAlign::Left, VAlign::Center);
            if row.target_mode {
                let disabled = row.message.is_some_and(|m| (0x1b2..=0x1bf).contains(&m));
                let glyph = if row.enter_target || disabled { "\u{2611}" } else { "\u{2610}" };
                let col = if disabled { text.with_alpha(0.4) } else { text };
                p.text(RectF::new(cols[7].0, y, cols[7].1, ROW), glyph, col, &big, HAlign::Center, VAlign::Center);
            }
        }
        for (x, cw) in cols.iter() {
            p.fill_rect(RectF::new(x + cw - 1.0, HEADER, 1.0, h - HEADER), divider);
        }
        if let Some((y0, track, ty, th)) = bar {
            let bg = s.colour(BAR_STYLE, "colFill", Colour::TRANSPARENT);
            let thumb = s.colour(BAR_STYLE, "colAltFill", divider);
            p.fill_rect(RectF::new(w - BAR, y0, BAR, track), bg);
            p.fill_rounded(RectF::new(w - BAR + 1.0, ty, BAR - 2.0, th), 3.0, [true; 4], thumb);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Node {
        Node::parse(
            r#"<root><BindingMap name="Bindings" version="2" MIDI0CC10BindingCount="2">
            <Binding name="MIDI0CC10Binding0" MajorIndex="0" MinorIndex="10" Min="0" Max="1"><VFixedString_64 name="Path" string="Global" /><VFixedString_64 name="Name" string="Macro 1" /><VString name="Type" string="Continuous" /></Binding>
            <Binding name="MIDI0CC10Binding1" MajorIndex="0" MinorIndex="10" Min="0" Max="1"><VFixedString_64 name="Path" string="Global" /><VFixedString_64 name="Name" string="Macro 2" /><VString name="Type" string="Latching" /></Binding>
            <Binding name="Auto3Binding0" Min="0" Max="1"><VFixedString_64 name="Name" string="Tempo" /></Binding>
            </BindingMap></root>"#,
        )
        .unwrap()
    }

    #[test]
    fn scroll_bar_geometry() {
        // 100 px: 4 rows fit below the header.
        assert!(bar_geometry(100.0, 4, 0).is_none());
        let (y0, track, ty, th) = bar_geometry(100.0, 8, 0).unwrap();
        assert_eq!((y0, track, ty, th), (HEADER, 80.0, HEADER, 40.0));
        let (_, _, ty, _) = bar_geometry(100.0, 8, 4).unwrap();
        assert_eq!(ty, HEADER + 40.0, "thumb at the bottom at the last scroll");
    }

    #[test]
    fn rows_and_remove() {
        let mut d = doc();
        let rs = rows(&d);
        assert_eq!(rs.len(), 3);
        assert_eq!(rs[0].locale(), "MIDI Ch 1 CC 10");
        assert_eq!(rs[1].kind, 2);
        assert_eq!(rs[2].locale(), "Host param 4");
        remove(&mut d, &rs[0]);
        let rs = rows(&d);
        assert_eq!(rs.len(), 2);
        assert_eq!((rs[0].name.as_str(), rs[0].index), ("Macro 2", 0));
        assert_eq!(bindings(&d).unwrap().attr("MIDI0CC10BindingCount"), Some("1"));
        clear(&mut d);
        assert!(rows(&d).is_empty());
    }

    #[test]
    fn binding_units_and_carry() {
        let mut r = rows(&doc())[0].clone();
        r.message = Some(0xa5);
        assert!((binding_value(&r, 0.0) + 100.0).abs() < 1e-4 && (binding_value(&r, 1.0) - 6.0).abs() < 1e-4);
        r.message = Some(0x4e);
        r.param = dreist_formats::PadParam::Pan as i32;
        assert!((binding_value(&r, 0.0) + 1.0).abs() < 1e-4);
        r.message = Some(0x12);
        assert_eq!(binding_value(&r, 0.3), 0.3);
        let from = doc();
        let mut to = Node::new("root", None);
        to.children.push(Node::new("BindingMap", Some("Bindings")));
        carry_bindings(&from, &mut to);
        assert_eq!(rows(&to).len(), 3);
    }

    #[test]
    fn columns_sit_side_by_side_and_only_ranged_ones_resize() {
        let w = COLUMNS.map(|c| c.1);
        let c = layout(&w);
        assert_eq!(c[0], (0.0, 40.0));
        assert_eq!(c[7], (820.0, 40.0));
        assert_eq!(grip_at(&w, 38.0, 5.0), None);
        assert_eq!(grip_at(&w, 208.0, 5.0), Some(1));
        assert_eq!(grip_at(&w, 208.0, HEADER + 1.0), None);
    }
}
