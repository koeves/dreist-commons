//! MIDI CC / host automation bindings (`BindingMap` [re:608a7c], `Binding`
//! [re:731ceb]): each binding turns a 0..1 controller value into a message
//! ([re:1aa701] prototype + stuffer) posted to the engine. See
//! the dreist-re bindings notes.

use dreist_formats::Node;

/// Message type ids (index into the message name table at [re:b89a3a]).
pub mod id {
    pub const REGISTRY_OPTIONS_CHANGED: u32 = 0x1;
    pub const REGISTRY_FOLLOW_OPTIONS_CHANGED: u32 = 0x2;
    pub const REGISTRY_MIDI_OPTIONS_CHANGED: u32 = 0x3;
    pub const PANIC: u32 = 0x12;
    pub const TOGGLE_RECORD: u32 = 0x14;
    pub const TOGGLE_PLAYBACK: u32 = 0x15;
    pub const SET_PLAYBACK_STATE: u32 = 0x16;
    pub const TOGGLE_QUANTIZE: u32 = 0x19;
    pub const TOGGLE_METRONOME: u32 = 0x18;
    pub const SET_TEMPO: u32 = 0x1a;
    pub const TAP_TEMPO: u32 = 0x1b;
    pub const RETURN_TO_ZERO: u32 = 0x1d;
    pub const SET_NOTE_ERASE_STATE: u32 = 0x1e;
    pub const SET_NOTE_REPEAT_STATE: u32 = 0x1f;
    pub const SET_NOTE_REPEAT_BASE: u32 = 0x20;
    pub const SET_FULL_LEVEL_STATE: u32 = 0x21;
    pub const SET_FULL_LEVEL_LEVEL: u32 = 0x22;
    pub const GRAB_RETRO_RECORDING: u32 = 0x23;
    pub const SET_METRONOME_LEVEL: u32 = 0x24;
    pub const SET_METRONOME_OUTPUT_LOCATION: u32 = 0x25;
    pub const SET_MODIFIERS: u32 = 0x26;
    pub const SET_PREROLL_MODE: u32 = 0x27;
    pub const SET_PREROLL_ONLY_ON_REC: u32 = 0x28;
    pub const SET_REC_WAITS_FOR_MIDI: u32 = 0x29;
    pub const SET_METRO_ONLY_DURING_REC: u32 = 0x2a;
    pub const SET_MODULATION_SOURCE_TRIGGER: u32 = 0x2d;
    pub const SET_MODULATION_SOURCE_TRANSPORT_START: u32 = 0x2e;
    pub const SET_MODULATION_SOURCE_PATTERN_CHANGE: u32 = 0x2f;
    pub const SET_MODULATION_SOURCE_PARAMETER: u32 = 0x30;
    pub const MUTE_PAD: u32 = 0x4c;
    pub const SOLO_PAD: u32 = 0x4d;
    pub const SET_PAD_PARAMETER: u32 = 0x4e;
    pub const SET_PAD_SEND_LEVEL: u32 = 0x55;
    pub const SET_FX_PARAMETER: u32 = 0x59;
    pub const SET_FX_ACTIVE: u32 = 0x5b;
    pub const SET_FX_CHAIN_ACTIVE: u32 = 0x60;
    pub const SET_FX_CHAIN_MIX: u32 = 0x62;
    pub const SET_FX_MIX: u32 = 0x5c;
    pub const SET_FX_LEVEL: u32 = 0x5d;
    pub const SET_FX_TRIM: u32 = 0x5e;
    pub const SET_LAYER_PARAMETER: u32 = 0x66;
    pub const SET_LAYER_CUT_MODE: u32 = 0x69;
    pub const SET_LAYER_PLAY_MODE: u32 = 0x6a;
    pub const SET_LAYER_SYNTHESIS_MODE: u32 = 0x6b;
    pub const SET_LAYER_REVERSE_STATUS: u32 = 0x6e;
    pub const SET_LAYER_PHASE_POSITION: u32 = 0x6f;
    pub const SET_LAYER_ENVELOPE_TRACKING: u32 = 0x70;
    pub const SET_LAYER_MUTE: u32 = 0x74;
    pub const TUNE_LAYER: u32 = 0x88;
    pub const SET_LAYER_SOLO: u32 = 0x75;
    pub const SET_LAYER_SEND_DEPTH: u32 = 0x82;
    pub const MUTE_ENGINE: u32 = 0x8c;
    pub const SOLO_ENGINE: u32 = 0x8d;
    pub const SET_ENGINE_PARAMETER: u32 = 0x90;
    pub const SET_ENGINE_TEMPO_NUM: u32 = 0x93;
    pub const SET_ENGINE_TEMPO_DENOM: u32 = 0x94;
    pub const SET_ENGINE_SWING_TEMPLATE_DEPTH: u32 = 0x95;
    pub const SET_ENGINE_SEND_LEVEL: u32 = 0x98;
    pub const SET_GLOBAL_AUX_GAIN: u32 = 0x9f;
    pub const MUTE_GLOBAL_AUX: u32 = 0xa0;
    pub const SOLO_GLOBAL_AUX: u32 = 0xa1;
    pub const SET_MASTER_CHANNEL_GAIN: u32 = 0xa5;
    pub const SET_MASTER_CHANNEL_TUNE: u32 = 0xa6;
    pub const SET_PATTERN_POWER_STATE: u32 = 0xa8;
    pub const SELECT_PATTERN: u32 = 0xa9;
    pub const SET_PATTERN_STEP_LENGTH: u32 = 0xb2;
    pub const SET_PATTERN_SYNC_MODE: u32 = 0xb4;
    pub const SET_PATTERN_STEP_COUNT: u32 = 0xb5;
    pub const SET_TRACK_MUTE_STATUS: u32 = 0xbb;
    pub const SET_TRACK_SOLO_STATUS: u32 = 0xbc;
    pub const SET_TRACK_LENGTH: u32 = 0xca;
    pub const SET_STEP_ON: u32 = 0xd2;
    pub const SET_STEP_OFF: u32 = 0xd3;
    pub const SET_STEP_VELOCITY: u32 = 0xd5;
    pub const SET_MACRO_VALUE: u32 = 0xea;
    pub const SAMPLER_TOGGLE_ARMED: u32 = 0xec;
    pub const SAMPLER_SET_ARMED: u32 = 0xed;
    pub const SAMPLER_SET_NOT_ARMED: u32 = 0xee;
    pub const SAMPLER_TOGGLE_RECORD: u32 = 0xef;
    pub const SAMPLER_START_RECORD: u32 = 0xf0;
    pub const SAMPLER_STOP_RECORD: u32 = 0xf1;
    pub const SAMPLER_START_MODE: u32 = 0xf2;
    pub const SAMPLER_TIMED_MODE: u32 = 0xf3;
    pub const SAMPLER_INPUT_GAIN: u32 = 0xf4;
    pub const SAMPLER_OUTPUT_GAIN: u32 = 0xf5;
    pub const SAMPLER_START_THRESHOLD: u32 = 0xf6;
    pub const SAMPLER_STOP_THRESHOLD: u32 = 0xf7;
    pub const SAMPLER_RECORD_SECS: u32 = 0xf8;
    pub const SAMPLER_BARS_BEATS: u32 = 0xf9;
    pub const SAMPLER_BEATS_MODE: u32 = 0xfa;
    pub const SAMPLER_CLEAR_TAKE: u32 = 0xfb;
    pub const SAMPLER_CLEAR_TAKES: u32 = 0xfc;
    pub const SAMPLER_RECORD_SOURCE: u32 = 0xfd;
    pub const SAMPLER_MONO: u32 = 0xfe;
    pub const SAMPLER_THRU: u32 = 0xff;
    pub const SAMPLER_AUTO_MODE: u32 = 0x100;
    pub const CLEAR_PATTERN: u32 = 0xaf;
    pub const CLEAR_ALL_PATTERNS: u32 = 0xb0;
    pub const CLEAR_TRACK: u32 = 0xba;
    pub const NUDGE_TRACK_FORWARD: u32 = 0xc2;
    pub const NUDGE_TRACK_BACKWARDS: u32 = 0xc3;
    pub const NUDGE_PATTERN_FORWARD: u32 = 0xc6;
    pub const NUDGE_PATTERN_BACKWARDS: u32 = 0xc7;
    pub const SET_STEP_GRAPH_STATE: u32 = 0xd6;
    pub const SET_INSTO_GRAPH_VALUE: u32 = 0xd8;
    pub const SET_INSTO_MIDI_GRAPH_VALUE: u32 = 0xd9;
    pub const SET_GRAPH_RANDOMISATION: u32 = 0xd1;
    pub const STOP_PREVIEW: u32 = 0x3f;
    pub const SET_PREVIEW_LOOP_MODE: u32 = 0xdd;
    pub const SET_PREVIEW_OUTPUT: u32 = 0xe7;
    pub const SET_PAD_OUTPUT: u32 = 0x4f;
    pub const SET_LAYER_OUTPUT: u32 = 0x6c;
    pub const SET_ENGINE_OUTPUT: u32 = 0x91;
    pub const SET_GLOBAL_AUX_OUTPUT: u32 = 0xa2;
    pub const SET_GLOBAL_AUX_PRE_POST: u32 = 0xa3;
    pub const SET_LAYER_START_SAMPLE: u32 = 0x76;
    pub const SET_LAYER_END_SAMPLE: u32 = 0x77;
    pub const SET_LAYER_LOOP_START_SAMPLE: u32 = 0x78;
    pub const SET_LAYER_LOOP_END_SAMPLE: u32 = 0x79;
    pub const SET_LAYER_START_SAMPLE01: u32 = 0x7a;
    pub const SET_LAYER_END_SAMPLE01: u32 = 0x7b;
    pub const SET_LAYER_LOOP_START_SAMPLE01: u32 = 0x7c;
    pub const SET_LAYER_LOOP_END_SAMPLE01: u32 = 0x7d;
    pub const SET_PREVIEW_VOLUME: u32 = 0xe8;
    pub const SET_PAD_LAYER_MODE: u32 = 0x51;
    pub const SET_PAD_LAYER_MODE01: u32 = 0x52;
    pub const SET_PAD_LINKAGE: u32 = 0x53;
    pub const AUTO_VELOCITY_SPLIT_PAD: u32 = 0x57;
    pub const AUTO_VELOCITY_CROSS: u32 = 0x58;
    pub const SET_LAYER_VELOCITY_SPLIT_MIN: u32 = 0x7e;
    pub const SET_LAYER_VELOCITY_SPLIT_MIN_TURNOVER: u32 = 0x7f;
    pub const SET_LAYER_VELOCITY_SPLIT_MAX: u32 = 0x80;
    pub const SET_LAYER_VELOCITY_SPLIT_MAX_TURNOVER: u32 = 0x81;
    pub const CLEAR_TRACK_AUTOMATION: u32 = 0xbf;
    pub const QUANTIZE_PATTERN_TIMING: u32 = 0xad;
    pub const QUANTIZE_PATTERN_VELOCITY: u32 = 0xae;
    pub const SET_PATTERN_MONO_MODE: u32 = 0xb6;
    pub const CLEAR_ALL_TRACK_AUTOMATION: u32 = 0xc0;
    pub const CLEAR_ALL_PATTERN_AUTOMATION: u32 = 0xc1;
    pub const SCENE_CLEAR: u32 = 0x108;
    pub const SONG_SET_ACTIVE: u32 = 0x117;
    pub const SONG_CLEAR_ALL: u32 = 0x118;
    pub const SONG_ADD_PART: u32 = 0x119;
    pub const SONG_DELETE_PART: u32 = 0x11a;
    pub const SONG_SET_INTERNAL_LOOP_ACTIVE: u32 = 0x120;
    pub const SONG_SET_INTERNAL_LOOP_RANGE: u32 = 0x121;
    pub const SONG_SET_ERASE_STATE: u32 = 0x123;
    pub const SET_CURRENT_TIME: u32 = 0x1c;
    pub const SET_PAD_SELECTIONS: u32 = 0x44;
    pub const SET_LAYER_SELECTIONS: u32 = 0x45;
    pub const SCENE_TRIGGER: u32 = 0x10a;
    pub const SCENE_TRIGGER_NEXT: u32 = 0x10b;
    pub const SCENE_TRIGGER_PREV: u32 = 0x10c;
    pub const SCENE_SNAPSHOT: u32 = 0x10d;
    pub const SCENE_CUT: u32 = 0x111;
    pub const SCENE_COPY: u32 = 0x112;
    pub const SCENE_PASTE: u32 = 0x113;
    pub const SCENE_SET_START_MODE: u32 = 0x10f;
    /// Editor messages: the engine changed the selected engine [e] / the
    /// engine's current pad [e, pad] (MIDI follow, [re:ea55ac]
    /// [re:4d42c4] / `SelectPad` [re:c44355]).
    pub const UPDATE_ENGINE_SELECTION_CHANGED: u32 = 0x181;
    pub const UPDATE_PAD_SELECTION_CHANGED: u32 = 0x182;
}

/// Message id for an `e_*` type name (`MESSAGE_NAMES`).
pub fn message_id(name: &str) -> Option<u32> {
    crate::message_names::MESSAGE_NAMES.iter().position(|n| *n == name).map(|i| i as u32)
}

/// The `e_*` type name of a message id.
pub fn message_name(kind: u32) -> Option<&'static str> {
    crate::message_names::MESSAGE_NAMES.get(kind as usize).copied()
}

/// Binding child name → (MIDI?, major, minor, index):
/// `MIDI%dCC%dBinding%d` or `Auto%dBinding%d`.
pub fn binding_key(name: &str) -> Option<(bool, usize, usize, usize)> {
    if let Some(r) = name.strip_prefix("MIDI") {
        let (ch, r) = r.split_once("CC")?;
        let (cc, k) = r.split_once("Binding")?;
        return Some((true, ch.parse().ok()?, cc.parse().ok()?, k.parse().ok()?));
    }
    let r = name.strip_prefix("Auto")?;
    let (p, k) = r.split_once("Binding")?;
    Some((false, p.parse().ok()?, 0, k.parse().ok()?))
}

/// A message ([re:6b4cf8]): type, indices, values.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub kind: u32,
    /// Unknown type name (kept so bindings round-trip; never dispatched).
    pub name: String,
    pub index: [i32; 6],
    pub value: [f32; 4],
    /// Posted by an `EditorOnly` binding: for the editor, not the engine
    /// ([re:684c4c] posts these to the editor port).
    pub editor: bool,
}

impl Message {
    /// Whether `o` addresses the same parameter (kind, indices and the
    /// value slots that select the parameter / transmod source), so a
    /// newer one can replace it.
    pub fn same_target(&self, o: &Message) -> bool {
        let sel: &[usize] = match self.kind {
            id::SET_LAYER_PARAMETER | id::SET_LAYER_SEND_DEPTH | id::SET_PAD_SEND_LEVEL | id::SET_ENGINE_SEND_LEVEL => &[0, 2],
            id::SET_FX_PARAMETER => &[0, 1],
            id::SET_PAD_PARAMETER | id::SET_ENGINE_PARAMETER | id::SET_MODULATION_SOURCE_PARAMETER => &[1, 2],
            id::SET_FX_MIX | id::SET_FX_LEVEL | id::SET_FX_TRIM => &[2],
            _ => &[],
        };
        self.kind == o.kind && self.index == o.index && sel.iter().all(|&k| self.value[k] == o.value[k])
    }

    pub fn new(kind: u32, index: &[i32], value: &[f32]) -> Message {
        let mut m = Message { kind, name: String::new(), index: [-1; 6], value: [0.0; 4], editor: false };
        m.index[..index.len()].copy_from_slice(index);
        m.value[..value.len()].copy_from_slice(value);
        m
    }

    /// `MessagePrototype` node (`TMessage_I6_V3`): indices, type name and
    /// three untyped values.
    pub fn to_node(&self, name: &str) -> Node {
        let mut n = Node::new("TMessage_I6_V3", Some(name));
        for (i, x) in self.index.iter().enumerate() {
            n.set(&format!("Index{i}"), x);
        }
        let mut t = Node::new("VString", Some("Type"));
        t.set("string", message_name(self.kind).unwrap_or(&self.name));
        n.children.push(t);
        for i in 0..3 {
            let mut v = Node::new("VValue", Some(&format!("MessageValue{i}")));
            v.set("type", "unknown");
            n.children.push(v);
        }
        n
    }

    /// [re:fbf9fd]: `Index0..5`, child [re:47cf32], values
    /// [re:abf62b]  (`type`, `value`).
    pub fn from_node(n: &Node) -> Message {
        let ty = n.child("VString", "Type").and_then(|c| c.attr("string")).unwrap_or("");
        // The name is kept only for unknown types: known ones serialize
        // from `kind`, and copies of the message stay allocation-free.
        let (kind, name) = match message_id(ty) {
            Some(k) => (k, String::new()),
            None => (u32::MAX, ty.to_string()),
        };
        let mut m = Message { kind, name, index: [-1; 6], value: [0.0; 4], editor: false };
        for (i, x) in m.index.iter_mut().enumerate() {
            *x = n.attr(&format!("Index{i}")).and_then(|v| v.parse().ok()).unwrap_or(-1);
        }
        for (i, x) in m.value.iter_mut().enumerate() {
            if let Some(c) = n.child("VValue", &format!("MessageValue{i}")) {
                *x = match c.attr("value") {
                    Some("true") => 1.0,
                    Some("false") => 0.0,
                    Some(v) => v.parse().unwrap_or(0.0),
                    None => 0.0,
                };
            }
        }
        m
    }
}

/// Binding type ([re:eaa2d1]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BindingType {
    #[default]
    Continuous,
    Momentary,
    Latching,
    Trigger,
    Gated,
    FlipFlop,
}

impl BindingType {
    pub fn from_name(s: &str) -> BindingType {
        match s {
            "Momentary" => BindingType::Momentary,
            "Latching" => BindingType::Latching,
            "Trigger" => BindingType::Trigger,
            "Gated" => BindingType::Gated,
            "FlipFlop" => BindingType::FlipFlop,
            _ => BindingType::Continuous,
        }
    }
}

/// Which message value the controller value goes into (`StuffToValue0/1/2`,
/// `StuffToValue0AsBool`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stuffer {
    #[default]
    Value0,
    Value1,
    Value2,
    Value0AsBool,
}

/// [re:097286]: steps 1/32 of the way per update.
#[derive(Debug, Clone, Copy, Default)]
pub struct Interpolator {
    cur: f32,
    target: f32,
    idle: bool,
    smooth: bool,
}

impl Interpolator {
    fn new(smooth: bool) -> Interpolator {
        Interpolator { cur: 0.0, target: 0.0, idle: true, smooth }
    }

    fn reset(&mut self, v: f32) {
        self.cur = v;
        self.target = v;
        self.idle = false;
    }

    fn set(&mut self, v: f32) {
        self.target = v;
        self.idle = (self.cur - v).abs() < 1e-6;
    }

    pub fn active(&self) -> bool {
        !self.idle
    }

    fn process(&mut self) -> f32 {
        if !self.smooth {
            self.idle = true;
            self.cur = self.target;
            return self.cur;
        }
        if (self.cur - self.target).abs() <= 1e-4 {
            self.idle = true;
            self.cur = self.target;
            return self.cur;
        }
        self.cur += (self.target - self.cur) * 0.03125;
        self.idle = (self.cur - self.target).abs() <= 1e-4;
        if self.idle {
            self.cur = self.target;
        }
        self.cur
    }
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub min: f32,
    pub max: f32,
    pub kind: BindingType,
    pub stuffer: Stuffer,
    pub prototype: Message,
    pub editor_only: bool,
    /// Last controller value.
    last: f32,
    interp: Interpolator,
}

impl Binding {
    /// [re:9aa62f] (`Min Max MIDI MajorIndex MinorIndex
    /// TargetMode EnterTargetMode EditorOnly NeedsIterp`, `Path`, `Name`,
    /// `MessageStuffer`, `MessagePopper`, `MessagePrototype`, `Type`).
    pub fn from_node(n: &Node) -> Binding {
        let stuffer = match n.child("VString", "MessageStuffer").and_then(|c| c.attr("string")) {
            Some("Binding::StuffToValue1") => Stuffer::Value1,
            Some("Binding::StuffToValue2") => Stuffer::Value2,
            Some("Binding::StuffToValue0AsBool") => Stuffer::Value0AsBool,
            _ => Stuffer::Value0,
        };
        let prototype = n.children.iter().find(|c| c.name.as_deref() == Some("MessagePrototype")).map(Message::from_node).unwrap_or_else(|| Message::new(u32::MAX, &[], &[]));
        Binding {
            min: n.f32("Min").unwrap_or(0.0),
            max: n.f32("Max").unwrap_or(1.0),
            kind: n.child("VString", "Type").and_then(|c| c.attr("string")).map(BindingType::from_name).unwrap_or_default(),
            stuffer,
            prototype,
            editor_only: n.bool("EditorOnly").unwrap_or(false),
            last: 0.0,
            interp: Interpolator::new(n.bool("NeedsIterp").unwrap_or(true)),
        }
    }

    /// [re:f4d409]: the message to post, if any.
    fn process(&mut self, v: f32) -> Option<Message> {
        let out = if self.kind == BindingType::Continuous {
            self.last = v;
            (self.max - self.min) * v + self.min
        } else {
            let round = |x: f32| if (x.abs() > 1e-6) && ((x - 1.0).abs() > 1e-6) { x.round().clamp(0.0, 1.0) } else { x };
            self.last = round(self.last);
            let prev = self.last;
            let v = round(v);
            match self.kind {
                BindingType::Momentary => {
                    self.last = v;
                    1.0 - prev
                }
                BindingType::Latching => {
                    if (v - 1.0).abs() > 1e-6 {
                        return None;
                    }
                    self.last = 1.0 - prev;
                    self.last
                }
                BindingType::Trigger => {
                    let nv = if v >= 0.5 { 1.0 } else { 0.0 };
                    self.last = nv;
                    if v < 0.5 || prev >= 0.5 {
                        return None;
                    }
                    nv
                }
                BindingType::Gated => {
                    if v < 0.5 || prev >= 0.5 {
                        let nv = if v >= 0.5 { 1.0 } else { 0.0 };
                        self.last = nv;
                        if v >= 0.5 || prev < 0.5 {
                            return None;
                        }
                        nv
                    } else {
                        self.last = 1.0;
                        1.0
                    }
                }
                _ => {
                    self.last = 1.0 - prev;
                    self.last
                }
            }
        };
        // Built field by field: no copy of an unknown type's name on the
        // audio thread.
        let p = &self.prototype;
        let mut m = Message { kind: p.kind, name: String::new(), index: p.index, value: p.value, editor: self.editor_only };
        match self.stuffer {
            Stuffer::Value0 => m.value[0] = out,
            Stuffer::Value1 => m.value[1] = out,
            Stuffer::Value2 => m.value[2] = out,
            Stuffer::Value0AsBool => m.value[0] = if out >= 0.5 { 1.0 } else { 0.0 },
        }
        Some(m)
    }

    /// Hand a value to the interpolator [re:9e40b6].
    fn push(&mut self, v: f32, out: &mut Vec<Message>) -> bool {
        if self.last - v <= 0.2 {
            self.interp.set(v);
        } else {
            self.interp.reset(v);
        }
        if !self.interp.active() {
            out.extend(self.process(v));
        }
        self.interp.active()
    }
}

/// A learned binding, parsed off the audio thread with room for its
/// key's list ([`BindingMap::add_prepared`]).
#[derive(Debug)]
pub struct NewBinding {
    key: Key,
    binding: Option<Binding>,
    room: Vec<Binding>,
}

/// A copy keeps the room (`Vec::clone` would size it to its contents).
impl Clone for NewBinding {
    fn clone(&self) -> NewBinding {
        let mut room = Vec::with_capacity(self.room.capacity());
        room.extend(self.room.iter().cloned());
        NewBinding { key: self.key, binding: self.binding.clone(), room }
    }
}

impl NewBinding {
    /// A `Binding` node (`MIDI%dCC%dBinding%d` / `Auto%dBinding%d`); its
    /// index sizes the room (the bindings already under the key, plus it).
    pub fn from_node(n: &Node) -> Option<NewBinding> {
        let (midi, a, b, k) = n.name.as_deref().and_then(binding_key)?;
        let key = if midi { Key::Midi(a, b) } else { Key::Host(a) };
        Some(NewBinding { key, binding: Some(Binding::from_node(n)), room: Vec::with_capacity((k + 1).max(8)) })
    }

    /// Whether the binding went in.
    pub fn added(&self) -> bool {
        self.binding.is_none()
    }
}

/// Where a binding lives: MIDI (channel, CC) or a host parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Midi(usize, usize),
    Host(usize),
}

#[derive(Debug, Clone, Default)]
pub struct BindingMap {
    midi: Vec<Vec<Binding>>,
    host: Vec<Vec<Binding>>,
    /// Bindings being interpolated.
    active: Vec<Key>,
}

pub const HOST_PARAMS: usize = 128;

impl BindingMap {
    pub fn new() -> BindingMap {
        BindingMap { midi: vec![Vec::new(); 16 * 128], host: vec![Vec::new(); HOST_PARAMS], active: Vec::with_capacity(64) }
    }

    /// [re:57e91e].
    pub fn from_node(n: &Node) -> BindingMap {
        let mut m = BindingMap::new();
        for p in 0..HOST_PARAMS {
            if n.attr(&format!("HostBindingCount{p}")).is_none() {
                continue;
            }
            // "Auto%dBinding%d" is formatted with the parameter index only.
            for c in n.children.iter().filter(|c| c.name.as_deref().is_some_and(|s| s.starts_with(&format!("Auto{p}Binding")))) {
                if m.host[p].len() < 8 {
                    m.host[p].push(Binding::from_node(c));
                }
            }
        }
        for ch in 0..16 {
            for cc in 0..128 {
                let count: usize = n.attr(&format!("MIDI{ch}CC{cc}BindingCount")).and_then(|v| v.parse().ok()).unwrap_or(0);
                for k in 0..count {
                    if let Some(c) = n.child("Binding", &format!("MIDI{ch}CC{cc}Binding{k}")) {
                        m.midi[ch * 128 + cc].push(Binding::from_node(c));
                    }
                }
            }
        }
        m
    }

    /// Add a binding prepared off the audio thread without allocating:
    /// the key's list moves into the prepared buffer (when it has room)
    /// with the new binding appended, and the old buffer is left in `b`
    /// to be freed by its sender. Host parameters keep at most 8.
    pub fn add_prepared(&mut self, b: &mut NewBinding) {
        let Some(list) = self.bindings_mut(b.key) else { return };
        if matches!(b.key, Key::Host(_)) && list.len() >= 8 || b.room.capacity() < list.len() + 1 {
            return;
        }
        let Some(new) = b.binding.take() else { return };
        b.room.clear();
        b.room.append(list);
        b.room.push(new);
        std::mem::swap(list, &mut b.room);
    }

    pub fn add_midi(&mut self, ch: usize, cc: usize, b: Binding) {
        if let Some(v) = self.midi.get_mut(ch * 128 + cc) {
            v.push(b);
        }
    }

    /// [re:a48fb8]: the MIDI channel / CC of a
    /// binding whose prototype is 0xd9 for (engine, pattern, track) and
    /// whose lane (value 1) is `lane`.
    pub fn graph_binding(&self, engine: usize, pattern: usize, track: usize, lane: usize) -> Option<(u8, u8)> {
        for (k, list) in self.midi.iter().enumerate() {
            for b in list {
                let m = &b.prototype;
                if m.kind == id::SET_INSTO_MIDI_GRAPH_VALUE && m.index[0] == engine as i32 && m.index[3] == pattern as i32 && m.index[4] == track as i32 && m.value[1] as usize == lane {
                    return Some(((k / 128) as u8, (k % 128) as u8));
                }
            }
        }
        None
    }

    /// Whether any MIDI binding targets a graph lane (0xd9).
    pub fn has_graph_bindings(&self) -> bool {
        self.midi.iter().flatten().any(|b| b.prototype.kind == id::SET_INSTO_MIDI_GRAPH_VALUE)
    }

    pub fn midi_bindings(&self, ch: usize, cc: usize) -> &[Binding] {
        self.midi.get(ch * 128 + cc).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn bindings_mut(&mut self, k: Key) -> Option<&mut Vec<Binding>> {
        match k {
            Key::Midi(ch, cc) => self.midi.get_mut(ch * 128 + cc),
            Key::Host(p) => self.host.get_mut(p),
        }
    }

    fn push(&mut self, k: Key, v: f32, out: &mut Vec<Message>) {
        let Some(list) = self.bindings_mut(k) else { return };
        let mut any = false;
        for b in list.iter_mut() {
            any |= b.push(v, out);
        }
        if any && !self.active.contains(&k) {
            self.active.push(k);
        }
    }

    /// [re:fd54d1] (`v` = value/127).
    pub fn process_midi_cc(&mut self, ch: usize, cc: usize, v: f32, out: &mut Vec<Message>) {
        self.push(Key::Midi(ch, cc), v, out);
    }

    /// [re:2fcbbb].
    pub fn process_host(&mut self, param: usize, v: f32, out: &mut Vec<Message>) {
        self.push(Key::Host(param), v, out);
    }

    /// Interpolated parameter update [re:4ea5a7] (block start and every sub-block).
    /// Keys whose interpolation finished drop out (in place: this runs on
    /// the audio thread every sub-block).
    /// Continuous bindings replace their own message queued since `from`
    /// (the queue is applied at the next block start, where only the last
    /// value counts), so long host blocks do not grow it.
    pub fn update(&mut self, out: &mut Vec<Message>, from: usize) {
        let mut i = 0;
        while i < self.active.len() {
            let k = self.active[i];
            let mut alive = false;
            if let Some(list) = self.bindings_mut(k) {
                for b in list.iter_mut() {
                    if b.interp.active() {
                        let v = b.interp.process();
                        if let Some(m) = b.process(v) {
                            let start = from.min(out.len());
                            let queued = out[start..].iter_mut().find(|o| b.kind == BindingType::Continuous && o.same_target(&m));
                            match queued {
                                Some(o) => *o = m,
                                None => out.push(m),
                            }
                        }
                    }
                    alive |= b.interp.active();
                }
            }
            if alive {
                i += 1;
            } else {
                self.active.remove(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macro_binding(kind: BindingType, min: f32, max: f32) -> Binding {
        Binding { min, max, kind, stuffer: Stuffer::Value0, prototype: Message::new(id::SET_MACRO_VALUE, &[0], &[]), editor_only: false, last: 0.0, interp: Interpolator::new(true) }
    }

    #[test]
    fn continuous_binding_smooths_small_moves_and_jumps_down() {
        let mut m = BindingMap::new();
        m.add_midi(0, 10, macro_binding(BindingType::Continuous, 0.0, 1.0));
        let mut out = Vec::new();
        m.process_midi_cc(0, 10, 0.1, &mut out);
        assert!(out.is_empty(), "upward move is interpolated");
        m.update(&mut out, 0);
        assert!((out[0].value[0] - 0.1 * 0.03125).abs() < 1e-6);
        // Within one block a binding's queued value is replaced, not repeated.
        m.update(&mut out, 0);
        assert_eq!(out.len(), 1);
        assert!((out[0].value[0] - 0.1 * 0.03125 * 2.0).abs() < 1e-3, "{}", out[0].value[0]);
        // A downward jump > 0.2 resets the interpolator and fires at once.
        let mut out = Vec::new();
        for _ in 0..400 {
            m.update(&mut out, 0);
        }
        out.clear();
        m.midi[10].iter_mut().for_each(|b| b.last = 1.0);
        m.process_midi_cc(0, 10, 0.5, &mut out);
        assert_eq!(out.len(), 0); // Reset makes the interpolator active.
        m.update(&mut out, 0);
        assert!((out[0].value[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn inverted_range_and_trigger() {
        let mut b = macro_binding(BindingType::Continuous, 1.0, 0.0);
        assert!((b.process(0.25).unwrap().value[0] - 0.75).abs() < 1e-6);
        let mut t = macro_binding(BindingType::Trigger, 0.0, 1.0);
        assert_eq!(t.process(1.0).unwrap().value[0], 1.0);
        assert!(t.process(1.0).is_none());
        assert!(t.process(0.0).is_none());
        assert!(t.process(1.0).is_some());
    }

    #[test]
    fn parses_saved_binding() {
        let xml = r#"<BindingMap name="Bindings" version="2" MIDI0CC10BindingCount="1">
        <Binding name="MIDI0CC10Binding0" EnterTargetMode="false" MajorIndex="0" Max="1" Min="0" MinorIndex="10">
            <VFixedString_64 name="Path" string="Global" />
            <VFixedString_64 name="Name" string="Macro 1" />
            <VString name="MessageStuffer" string="Binding::StuffToValue0" />
            <VString name="MessagePopper" string="Binding::GetValueFromValue0" />
            <TMessage_I6_V3 name="MessagePrototype" Index0="0" Index1="-1" Index2="-1" Index3="-1" Index4="-1" Index5="-1">
                <VString name="Type" string="e_setMacroValue" />
                <VValue name="MessageValue0" type="unknown" />
            </TMessage_I6_V3>
            <VString name="Type" string="Continuous" />
        </Binding></BindingMap>"#;
        let n = Node::parse(xml).unwrap();
        let m = BindingMap::from_node(&n);
        let b = &m.midi_bindings(0, 10)[0];
        assert_eq!(b.prototype.kind, id::SET_MACRO_VALUE);
        assert_eq!(b.prototype.index[0], 0);
        assert_eq!(b.kind, BindingType::Continuous);
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn ids_match_the_name_table() {
        let old: &[(&str, u32)] = &[
            ("e_panic", id::PANIC),
            ("e_setModifiers", id::SET_MODIFIERS),
            ("e_setGlobalAuxPrePost", id::SET_GLOBAL_AUX_PRE_POST),
            ("e_toggleRecord", id::TOGGLE_RECORD),
            ("e_togglePlayback", id::TOGGLE_PLAYBACK),
            ("e_tapTempo", id::TAP_TEMPO),
            ("e_returnToZero", id::RETURN_TO_ZERO),
            ("e_setPlaybackState", id::SET_PLAYBACK_STATE),
            ("e_toggleQuantize", id::TOGGLE_QUANTIZE),
            ("e_toggleMetronome", id::TOGGLE_METRONOME),
            ("e_setTempo", id::SET_TEMPO),
            ("e_setNoteEraseState", id::SET_NOTE_ERASE_STATE),
            ("e_setNoteRepeatState", id::SET_NOTE_REPEAT_STATE),
            ("e_setNoteRepeatBase", id::SET_NOTE_REPEAT_BASE),
            ("e_setFullLevelState", id::SET_FULL_LEVEL_STATE),
            ("e_setFullLevelLevel", id::SET_FULL_LEVEL_LEVEL),
            ("e_setMetronomeLevel", id::SET_METRONOME_LEVEL),
            ("e_setModulationSourceTrigger", id::SET_MODULATION_SOURCE_TRIGGER),
            ("e_setModulationSourceTransportStart", id::SET_MODULATION_SOURCE_TRANSPORT_START),
            ("e_setModulationSourcePatternChange", id::SET_MODULATION_SOURCE_PATTERN_CHANGE),
            ("e_setModulationSourceParameter", id::SET_MODULATION_SOURCE_PARAMETER),
            ("e_mutePad", id::MUTE_PAD),
            ("e_soloPad", id::SOLO_PAD),
            ("e_setPadParameter", id::SET_PAD_PARAMETER),
            ("e_setPadSendLevel", id::SET_PAD_SEND_LEVEL),
            ("e_setFXParameter", id::SET_FX_PARAMETER),
            ("e_setFXActive", id::SET_FX_ACTIVE),
            ("e_setFXMix", id::SET_FX_MIX),
            ("e_setFXLevel", id::SET_FX_LEVEL),
            ("e_setFXTrim", id::SET_FX_TRIM),
            ("e_setLayerParameter", id::SET_LAYER_PARAMETER),
            ("e_setLayerMute", id::SET_LAYER_MUTE),
            ("e_setLayerSolo", id::SET_LAYER_SOLO),
            ("e_setLayerSendDepth", id::SET_LAYER_SEND_DEPTH),
            ("e_muteEngine", id::MUTE_ENGINE),
            ("e_soloEngine", id::SOLO_ENGINE),
            ("e_setEngineParameter", id::SET_ENGINE_PARAMETER),
            ("e_setEngineSwingTemplateDepth", id::SET_ENGINE_SWING_TEMPLATE_DEPTH),
            ("e_setEngineSendLevel", id::SET_ENGINE_SEND_LEVEL),
            ("e_setGlobalAuxGain", id::SET_GLOBAL_AUX_GAIN),
            ("e_muteGlobalAux", id::MUTE_GLOBAL_AUX),
            ("e_soloGlobalAux", id::SOLO_GLOBAL_AUX),
            ("e_setMasterChannelGain", id::SET_MASTER_CHANNEL_GAIN),
            ("e_setMasterChannelTune", id::SET_MASTER_CHANNEL_TUNE),
            ("e_setPatternPowerState", id::SET_PATTERN_POWER_STATE),
            ("e_selectPattern", id::SELECT_PATTERN),
            ("e_setMacroValue", id::SET_MACRO_VALUE),
        ];
        for (n, i) in old {
            assert_eq!(message_id(n), Some(*i), "{n}");
            assert_eq!(message_name(*i), Some(*n));
        }
    }
}
