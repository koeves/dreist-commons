//! Live/MIDI note input: the default [re:a0d3a6], engine pad modes and the
//! note-repeat grid. See the dreist-re live-input notes.

/// [re:bcefb7] engine value meaning "the selected engine" (−666).
pub const SELECTED: i32 = -666;
/// A pad's MIDI note [re:1d8fbb]: pad + 36.
pub const PAD_NOTE_BASE: u8 = 36;

/// [re:bcefb7]: type, function, engine,
/// pad, pattern, scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapItem {
    #[default]
    None,
    Pad {
        engine: i32,
        pad: usize,
    },
    Pattern {
        engine: i32,
        pattern: usize,
    },
    Scene {
        scene: usize,
    },
    Function {
        engine: i32,
        function: u32,
    },
}

/// [re:a0d3a6] ([re:5f09c1]): 16 channels × 128 notes.
#[derive(Debug, Clone)]
pub struct MidiMap {
    items: Vec<MapItem>,
}

impl Default for MidiMap {
    /// Default map [re:c3e2ba] + the extra scene channel 15 [re:f1f4ad].
    fn default() -> Self {
        let mut m = MidiMap { items: vec![MapItem::None; 16 * 128] };
        for c in 0..9 {
            let engine = if c == 8 { SELECTED } else { c };
            for n in 12..36 {
                m.set(c as usize, n, MapItem::Pattern { engine, pattern: n - 12 });
            }
            for n in 36..100 {
                m.set(c as usize, n, MapItem::Pad { engine, pad: n - 36 });
            }
            m.set(c as usize, 102, MapItem::Function { engine, function: 3 });
            m.set(c as usize, 104, MapItem::Function { engine, function: 4 });
        }
        for n in 0..128 {
            m.set(9, n, MapItem::Pad { engine: (n / 16) as i32, pad: n % 16 });
        }
        for n in 0..96 {
            m.set(10, n, MapItem::Scene { scene: n });
        }
        for n in 12..36 {
            m.set(15, n, MapItem::Pattern { engine: SELECTED, pattern: n - 12 });
        }
        for n in 36..52 {
            m.set(15, n, MapItem::Pad { engine: SELECTED, pad: n - 36 });
        }
        for n in 52..76 {
            m.set(15, n, MapItem::Scene { scene: n });
        }
        m
    }
}

impl MidiMap {
    pub fn get(&self, channel: usize, note: usize) -> MapItem {
        self.items.get(channel * 128 + note).copied().unwrap_or_default()
    }

    pub fn set(&mut self, channel: usize, note: usize, item: MapItem) {
        if let Some(i) = self.items.get_mut(channel * 128 + note) {
            *i = item;
        }
    }
}

/// Engine `PadMode` ([re:8d4ce4]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadMode {
    #[default]
    Normal,
    Chromatic,
    Velocity,
    MuteAndSolo,
    Sampler,
    X0x,
}

impl PadMode {
    pub fn from_name(s: &str) -> PadMode {
        match s {
            "Chromatic" => PadMode::Chromatic,
            "Velocity" => PadMode::Velocity,
            "MuteAndSolo" | "Engine M+S" => PadMode::MuteAndSolo,
            "Sampler" => PadMode::Sampler,
            "x0x" => PadMode::X0x,
            _ => PadMode::Normal,
        }
    }

    pub fn index(self) -> u32 {
        self as u32
    }
}

/// [re:d2c86d].
pub fn pad_to_velocity(pad: u32, visible_pads: u32) -> u32 {
    let (p, d) = if visible_pads < 4 { (pad % 16, 16.0f32) } else { (pad, 64.0) };
    (((p + 1) as f32 / d) * 127.0).floor() as u32
}

/// Note-repeat interval in quarter notes for note repeat base 0..=13
/// (note start [re:d8fe7c] switch; out-of-range → 1).
pub fn repeat_interval_qn(base: u32) -> f64 {
    match base {
        0 => 4.0,
        1 => 2.6666666666666665,
        2 => 2.0,
        3 => 1.3333333333333333,
        5 => 0.6666666666666666,
        6 => 0.5,
        7 => 0.3333333333333333,
        8 => 0.25,
        9 => 0.16666666666666666,
        10 => 0.125,
        11 => 0.08333333333333333,
        12 => 0.0625,
        13 => 0.041666666666666664,
        _ => 1.0,
    }
}

/// Normalised MIDI status (MIDI event handling [re:70eb56]): channel messages
/// keep the high nibble, note-on velocity 0 → note-off, CC 123/124 → 0xff.
pub fn status_kind(data: [u8; 4]) -> u8 {
    let s = if data[0] < 0xf0 { data[0] & 0xf0 } else { data[0] };
    match s {
        0xb0 if data[1].wrapping_add(0x85) < 2 => 0xff,
        0x90 if data[2] == 0 => 0x80,
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_map_layout() {
        let m = MidiMap::default();
        assert_eq!(m.get(0, 36), MapItem::Pad { engine: 0, pad: 0 });
        assert_eq!(m.get(3, 99), MapItem::Pad { engine: 3, pad: 63 });
        assert_eq!(m.get(2, 12), MapItem::Pattern { engine: 2, pattern: 0 });
        assert_eq!(m.get(8, 40), MapItem::Pad { engine: SELECTED, pad: 4 });
        assert_eq!(m.get(9, 35), MapItem::Pad { engine: 2, pad: 3 });
        assert_eq!(m.get(10, 95), MapItem::Scene { scene: 95 });
        assert_eq!(m.get(15, 52), MapItem::Scene { scene: 52 });
        assert_eq!(m.get(0, 102), MapItem::Function { engine: 0, function: 3 });
        assert_eq!(m.get(0, 100), MapItem::None);
    }

    #[test]
    fn pad_velocity() {
        assert_eq!(pad_to_velocity(15, 0), 127);
        assert_eq!(pad_to_velocity(17, 0), 15);
        assert_eq!(pad_to_velocity(63, 4), 127);
    }
}
