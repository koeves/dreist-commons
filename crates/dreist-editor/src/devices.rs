//! Standalone audio and MIDI settings ([re:b5d239], ev_c012d36c…,
//! ev_bc97f83f…; [re:fd47b6], same evidence), on the original
//! [re:d1c9a9] / `MIDIOptions.skin`.
//!
//! Audio (`SetupAndCreateAllControls` [re:2d4553]): SampleRate, BufferSize,
//! InDevice and OutDevice popups (the Windows panel with the ASIO controls
//! is removed on the Mac). `UpdateSampleRateComboBox` [re:b8e440] lists the
//! device's rates; `UpdateBlocksizeComboBox` [re:fd4b4a] 32, 64, 128, 256,
//! 512, 1024; the device popups list the delegate's devices. Any popup
//! change hands {rate, block, input, output} to the delegate and refreshes
//! everything ($_0 [re:43218b]). The IOGroup tabs (INPUTS / OUTPUTS) fill
//! IOTable (`UpdateIOTable` [re:ae7160]): columns "Enabled" (50) and
//! "Description", one row per stereo pair, outputs ending with "All
//! outputs". Refresh re-reads the devices ($_1), Close closes ($_2).
//!
//! MIDI (`SetupAndCreateAllControls` [re:d4cd64], `UpdateInputDevices`
//! [re:cce3c4]): InputTable with "Enable", "Clock" and "Description",
//! OutputTable with "Enable" and "Description"; Refresh, Sync to clock,
//! Close.
//!
//! Port: the standalone (nih-plug) opens one input device, one output
//! device, one MIDI input and one MIDI output, chosen when audio starts.
//! The settings are kept in the options (`Standalone*`); closing a panel
//! after a change restarts the app's audio (the app relaunches with the
//! current document). The channel table shows the pair in use; MIDI clock
//! sync is not available.

use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Skin};

use crate::EditorError;
use crate::options::Options;

/// `UpdateBlocksizeComboBox` [re:fd4b4a].
pub const BUFFER_SIZES: [u32; 6] = [32, 64, 128, 256, 512, 1024];
pub const DEFAULT_BUFFER: u32 = 512;
/// The input popup's "no input" item (nih-plug connects no input then).
pub const NO_INPUT: &str = "None";

/// Devices the standalone can open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioSetup {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: String,
    /// Rates the chosen output supports, and its own rate.
    pub rates: Vec<u32>,
    pub default_rate: u32,
    pub out_channels: u16,
    pub in_channels: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiSetup {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// The standalone app's side ([re:16f787],
/// `IMIDIOptionsDelegate`).
pub trait DeviceHost: Send {
    /// Devices, and the rates / channels of `output` and `input` ("" =
    /// the default output, no input).
    fn audio(&self, output: &str, input: &str) -> AudioSetup;
    fn midi(&self) -> MidiSetup;
    /// Restart audio with the stored settings, keeping the document.
    fn restart(&self, doc: &dreist_formats::Node, samples: Option<&std::path::Path>);
}

/// The stored choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub output: String,
    pub input: String,
    pub rate: u32,
    pub buffer: u32,
    pub midi_in: String,
    pub midi_out: String,
}

impl Settings {
    pub fn load(o: &Options) -> Settings {
        Settings {
            output: o.string("StandaloneAudioOutput", ""),
            input: o.string("StandaloneAudioInput", ""),
            rate: o.int("StandaloneSampleRate", 0).max(0) as u32,
            buffer: o.int("StandaloneBufferSize", DEFAULT_BUFFER as i64).max(0) as u32,
            midi_in: o.string("StandaloneMidiInput", ""),
            midi_out: o.string("StandaloneMidiOutput", ""),
        }
    }

    pub fn store(&self, o: &mut Options) {
        o.set("StandaloneAudioOutput", self.output.clone());
        o.set("StandaloneAudioInput", self.input.clone());
        o.set("StandaloneSampleRate", self.rate as i64);
        o.set("StandaloneBufferSize", self.buffer as i64);
        o.set("StandaloneMidiInput", self.midi_in.clone());
        o.set("StandaloneMidiOutput", self.midi_out.clone());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Audio,
    Midi,
}

const ROW: f32 = 18.0;
const HEADER: f32 = 20.0;
const LIST_STYLE: &str = "DefaultTableViewStyle";
const HEADER_STYLE: &str = "DefaultTableViewHeaderStyle";
const CHECK_W: f32 = 50.0;

pub struct Devices {
    audio_overlay: Option<Vec<usize>>,
    midi_overlay: Option<Vec<usize>>,
    audio_slide: crate::modal::Slide,
    midi_slide: crate::modal::Slide,
    pub open: Option<Panel>,
    pub host: Option<Box<dyn DeviceHost>>,
    settings: Settings,
    /// Settings when the panel opened (a change restarts audio on Close).
    opened_with: Settings,
    audio: AudioSetup,
    midi: MidiSetup,
    /// IOGroup: 0 inputs, 1 outputs.
    io_tab: usize,
    /// Close after a change: the editor asks the host to restart.
    pub restart_requested: bool,
}

impl Devices {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Devices, EditorError> {
        let prep = |root: &mut Ctrl, op: &Option<Vec<usize>>, tables: &[&str]| {
            let Some(op) = op else { return };
            let mut o = &mut *root;
            for &i in op {
                o = &mut o.children[i];
            }
            // `SetupAndCreateAllControls`: the Windows panel and the
            // template CheckBox are removed.
            for n in ["WinSettingPanel", "CheckBox"] {
                if let Some(c) = o.find_mut(n) {
                    c.visible = false;
                    c.enabled = false;
                }
            }
            for t in tables {
                if let Some(c) = o.find_mut(t) {
                    c.overload = format!("DeviceTable:{t}");
                    c.children.clear();
                }
            }
        };
        let audio_overlay = crate::modal::overlay(skin, root, "AudioOptions.skin", "AudioOptionsOverlay")?;
        prep(root, &audio_overlay, &["IOTable"]);
        let midi_overlay = crate::modal::overlay(skin, root, "MIDIOptions.skin", "MIDIOptionsOverlay")?;
        prep(root, &midi_overlay, &["InputTable", "OutputTable"]);
        let settings = Settings::load(&Options::load());
        Ok(Devices {
            audio_overlay,
            midi_overlay,
            audio_slide: Default::default(),
            midi_slide: Default::default(),
            open: None,
            host: None,
            opened_with: settings.clone(),
            settings,
            audio: AudioSetup::default(),
            midi: MidiSetup::default(),
            io_tab: 0,
            restart_requested: false,
        })
    }

    /// Open a panel (the standalone's Audio / MIDI settings menu items).
    pub fn show(&mut self, p: Panel) {
        self.settings = Settings::load(&Options::load());
        self.opened_with = self.settings.clone();
        self.open = Some(p);
        self.refresh();
    }

    /// Refresh: re-read the devices.
    fn refresh(&mut self) {
        let Some(h) = &self.host else { return };
        let input = if self.settings.input == NO_INPUT { "" } else { &self.settings.input };
        self.audio = h.audio(&self.settings.output, input);
        self.midi = h.midi();
    }

    fn overlay(&self, p: Panel) -> Option<&Vec<usize>> {
        match p {
            Panel::Audio => self.audio_overlay.as_ref(),
            Panel::Midi => self.midi_overlay.as_ref(),
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open.and_then(|p| self.overlay(p)).is_some_and(|o| path.starts_with(o))
    }

    fn output_name(&self) -> String {
        if self.settings.output.is_empty() { self.audio.default_output.clone() } else { self.settings.output.clone() }
    }

    fn rate(&self) -> u32 {
        if self.settings.rate == 0 { self.audio.default_rate } else { self.settings.rate }
    }

    pub fn sync(&mut self, app: &mut App) {
        let audio = self.audio_overlay.clone().map(|p| self.audio_slide.sync(app, &p, self.open == Some(Panel::Audio))).unwrap_or(false);
        let midi = self.midi_overlay.clone().map(|p| self.midi_slide.sync(app, &p, self.open == Some(Panel::Midi))).unwrap_or(false);
        let (out, rate, s) = (self.output_name(), self.rate(), self.settings.clone());
        let tab = self.io_tab;
        if let Some(o) = self.audio_overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            o.visible = audio;
            for (name, text) in [
                ("SampleRate", if rate > 0 { rate.to_string() } else { String::new() }),
                ("BufferSize", s.buffer.to_string()),
                ("InDevice", if s.input.is_empty() { NO_INPUT.to_string() } else { s.input.clone() }),
                ("OutDevice", out),
            ] {
                if let Some(c) = o.find_mut(name) {
                    c.text = text;
                }
            }
            if let Some(g) = o.find_mut("IOGroup") {
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = i == tab;
                }
            }
        }
        if let Some(o) = self.midi_overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            o.visible = midi;
            // No MIDI clock sync in the port.
            if let Some(b) = o.find_mut("Sync") {
                b.enabled = false;
            }
        }
    }

    fn popup_items(&self, name: &str) -> Vec<String> {
        match name {
            "SampleRate" => self.audio.rates.iter().map(u32::to_string).collect(),
            "BufferSize" => BUFFER_SIZES.iter().map(u32::to_string).collect(),
            "InDevice" => std::iter::once(NO_INPUT.to_string()).chain(self.audio.inputs.iter().cloned()).collect(),
            "OutDevice" => self.audio.outputs.clone(),
            _ => Vec::new(),
        }
    }

    fn close(&mut self) {
        self.open = None;
        if self.settings != self.opened_with {
            self.restart_requested = true;
        }
    }

    fn changed(&mut self) {
        let mut o = Options::load();
        self.settings.store(&mut o);
        if !cfg!(test) {
            o.save();
        }
    }

    /// The table rows: (checked, clock, description).
    fn rows(&self, table: &str) -> Vec<(bool, Option<bool>, String)> {
        match table {
            "IOTable" => {
                let n = if self.io_tab == 0 { if self.settings.input.is_empty() || self.settings.input == NO_INPUT { 0 } else { self.audio.in_channels } } else { self.audio.out_channels };
                let kind = if self.io_tab == 0 { "Input" } else { "Output" };
                let mut v: Vec<(bool, Option<bool>, String)> = (0..n / 2).map(|k| (k == 0, None, format!("{kind} {} / {}", 2 * k + 1, 2 * k + 2))).collect();
                if self.io_tab == 1 && !v.is_empty() {
                    v.push((false, None, "All outputs".into()));
                }
                v
            }
            "InputTable" => self.midi.inputs.iter().map(|d| (*d == self.settings.midi_in, Some(false), d.clone())).collect(),
            "OutputTable" => self.midi.outputs.iter().map(|d| (*d == self.settings.midi_out, None, d.clone())).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether the event was the panel's (the panels are modal).
    pub fn handle(&mut self, app: &mut App, ev: &UiEvent) -> bool {
        for p in [Panel::Audio, Panel::Midi] {
            let Some(o) = self.overlay(p).cloned() else { continue };
            let slide = if p == Panel::Audio { &mut self.audio_slide } else { &mut self.midi_slide };
            if slide.animated(app, &o, ev) {
                return true;
            }
        }
        if self.open.is_none() {
            return false;
        }
        match ev {
            UiEvent::Clicked { path, name, .. } if self.mine(path) => {
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                match (parent.as_str(), name.as_str()) {
                    ("IOGroup", _) => self.io_tab = *path.last().unwrap_or(&0),
                    (_, "Close") => self.close(),
                    (_, "Refresh") => self.refresh(),
                    (_, n @ ("SampleRate" | "BufferSize" | "InDevice" | "OutDevice")) => {
                        let items = self.popup_items(n);
                        if !items.is_empty() {
                            app.open_menu(path, items, None);
                        }
                    }
                    _ => {}
                }
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) => {
                let item = self.popup_items(name).get(*index).cloned().unwrap_or_default();
                match name.as_str() {
                    "SampleRate" => self.settings.rate = item.parse().unwrap_or(0),
                    "BufferSize" => self.settings.buffer = item.parse().unwrap_or(DEFAULT_BUFFER),
                    "InDevice" => self.settings.input = if item == NO_INPUT { String::new() } else { item },
                    "OutDevice" => {
                        self.settings.output = item;
                        // Another device: its own rate unless it offers the chosen one.
                        self.refresh();
                        if !self.audio.rates.contains(&self.settings.rate) {
                            self.settings.rate = 0;
                        }
                    }
                    _ => return true,
                }
                self.refresh();
                self.changed();
                true
            }
            UiEvent::Mouse { overload, kind: MouseKind::Down, x, y, .. } if overload.starts_with("DeviceTable:") => {
                let table = &overload["DeviceTable:".len()..];
                if *y >= HEADER && *x < CHECK_W {
                    let i = ((*y - HEADER) / ROW) as usize;
                    // One MIDI device each way: a click picks it, a click on
                    // the checked one turns it off.
                    let pick = |cur: &mut String, list: &[String]| {
                        if let Some(d) = list.get(i) {
                            *cur = if cur == d { String::new() } else { d.clone() };
                        }
                    };
                    match table {
                        "InputTable" => pick(&mut self.settings.midi_in, &self.midi.inputs.clone()),
                        "OutputTable" => pick(&mut self.settings.midi_out, &self.midi.outputs.clone()),
                        _ => return true,
                    }
                    self.changed();
                }
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::ValueChanged { path, .. } | UiEvent::Pressed { path, .. } | UiEvent::RightClicked { path, .. } => self.mine(path),
            _ => false,
        }
    }

    pub fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        let Some(table) = c.overload.strip_prefix("DeviceTable:") else { return false };
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let font = s.font(LIST_STYLE);
        let text = s.colour(LIST_STYLE, "colText", Colour::WHITE);
        let alt = s.colour(LIST_STYLE, "colAltFill", Colour::TRANSPARENT);
        p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour(LIST_STYLE, "colFill", Colour::rgba(30, 30, 30, 255)));
        let cols: Vec<(&str, f32, f32)> = match table {
            "InputTable" => vec![("Enable", 0.0, CHECK_W), ("Clock", CHECK_W, CHECK_W), ("Description", 2.0 * CHECK_W, w - 2.0 * CHECK_W)],
            "IOTable" => vec![("Enabled", 0.0, CHECK_W), ("Description", CHECK_W, w - CHECK_W)],
            _ => vec![("Enable", 0.0, CHECK_W), ("Description", CHECK_W, w - CHECK_W)],
        };
        let hfill = s.colour(HEADER_STYLE, "colFill", Colour::WHITE);
        let htext = s.colour(HEADER_STYLE, "colText", Colour::BLACK);
        let hdiv = s.colour(HEADER_STYLE, "colAltBorder", Colour::BLACK);
        let hfont = s.font(HEADER_STYLE);
        p.fill_rect(RectF::new(0.0, 0.0, w, HEADER), hfill);
        for (title, x, cw) in &cols {
            p.text(RectF::new(x + 4.0, 0.0, cw - 8.0, HEADER), title, htext, &hfont, HAlign::Left, VAlign::Center);
            p.fill_rect(RectF::new(x + cw - 1.0, 0.0, 1.0, HEADER), hdiv);
        }
        // [re:a817ff] boxes, 16 × 16 at (17, 2) in the cell.
        let check = |p: &mut Painter, x: f32, y: f32, on: bool, enabled: bool| {
            let a = if enabled { 255 } else { 90 };
            let r = RectF::new(x + 17.0, y + 1.0, 16.0, 16.0);
            p.fill_rect(r, Colour::rgba(20, 20, 20, a));
            p.stroke_rect(r, Colour::rgba(150, 150, 150, a), 1.0);
            if on {
                p.fill_rect(RectF::new(r.x + 4.0, r.y + 4.0, 8.0, 8.0), Colour::rgba(230, 230, 230, a));
            }
        };
        for (k, (on, clock, desc)) in self.rows(table).iter().enumerate() {
            let y = HEADER + k as f32 * ROW;
            if y > h {
                break;
            }
            if k % 2 == 1 {
                p.fill_rect(RectF::new(0.0, y, w, ROW), alt);
            }
            check(p, 0.0, y, *on, table != "IOTable");
            if let Some(cl) = clock {
                check(p, CHECK_W, y, *cl, false);
            }
            let (_, x, cw) = cols[cols.len() - 1];
            p.text(RectF::new(x + 4.0, y, cw - 8.0, ROW), desc, text, &font, HAlign::Left, VAlign::Center);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl DeviceHost for Fake {
        fn audio(&self, output: &str, _input: &str) -> AudioSetup {
            let rates = if output == "B" { vec![48000, 96000] } else { vec![44100, 48000] };
            AudioSetup { outputs: vec!["A".into(), "B".into()], inputs: vec!["Mic".into()], default_output: "A".into(), default_rate: rates[0], rates, out_channels: 4, in_channels: 2 }
        }
        fn midi(&self) -> MidiSetup {
            MidiSetup { inputs: vec!["Pads".into(), "Keys".into()], outputs: vec!["Synth".into()] }
        }
        fn restart(&self, _: &dreist_formats::Node, _: Option<&std::path::Path>) {}
    }

    #[test]
    fn rows_and_choices() {
        let mut d = Devices {
            audio_overlay: None,
            midi_overlay: None,
            audio_slide: Default::default(),
            midi_slide: Default::default(),
            open: None,
            host: Some(Box::new(Fake)),
            settings: Settings::load(&Options::default()),
            opened_with: Settings::load(&Options::default()),
            audio: Default::default(),
            midi: Default::default(),
            io_tab: 1,
            restart_requested: false,
        };
        d.open = Some(Panel::Audio);
        d.refresh();
        assert_eq!(d.output_name(), "A");
        assert_eq!(d.rate(), 44100);
        assert_eq!(d.rows("IOTable").iter().map(|r| r.2.as_str()).collect::<Vec<_>>(), ["Output 1 / 2", "Output 3 / 4", "All outputs"]);
        d.settings.midi_in = "Keys".into();
        assert_eq!(d.rows("InputTable").iter().map(|r| r.0).collect::<Vec<_>>(), [false, true]);
        assert_eq!(d.popup_items("InDevice"), [NO_INPUT, "Mic"]);
        // A change asks for a restart on Close.
        d.close();
        assert!(d.restart_requested);
    }
}
