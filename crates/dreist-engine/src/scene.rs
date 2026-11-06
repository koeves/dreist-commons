//! The scene manager ([re:832d2b]): 96 scenes, each a snapshot of every
//! engine's current pattern and pattern power, triggered live (UI/MIDI) and
//! applied at sample-accurate offsets, optionally quantised to the next beat
//! or bar. See the dreist-re song notes ("Scenes").

use dreist_formats::Node;

pub const SCENES: usize = 96;
/// At most 128 pending scene changes.
const MAX_CHANGES: usize = 128;
/// Bypass track 8 = no engine bypassed.
pub const NO_BYPASS: usize = 8;

/// A scene's start mode ([re:a964e3]: "imm", "beat", "bar").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartMode {
    #[default]
    Immediate,
    Beat,
    Bar,
}

impl StartMode {
    pub fn from_name(s: &str) -> StartMode {
        match s {
            "beat" => StartMode::Beat,
            "bar" => StartMode::Bar,
            _ => StartMode::Immediate,
        }
    }
}

/// `SceneEngine` (`patt`, `pattpower`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneEngine {
    pub pattern: usize,
    pub power: bool,
}

/// `Scene`: `used`, `start`, `Eng0..7`.
/// Its `name` (VFixedString_64) lives only in the document (the
/// editor's): scene edits on the audio thread copy no strings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scene {
    pub used: bool,
    pub start: StartMode,
    pub engines: [SceneEngine; 8],
}

impl Scene {
    /// [re:3f0cbb].
    pub fn from_node(n: &Node) -> Scene {
        let mut s = Scene { used: n.bool("used").unwrap_or(false), start: n.attr("start").map(StartMode::from_name).unwrap_or_default(), engines: Default::default() };
        for (i, e) in s.engines.iter_mut().enumerate() {
            if let Some(c) = n.child("SceneEngine", &format!("Eng{i}")) {
                e.pattern = c.u32("patt").unwrap_or(0) as usize;
                e.power = c.bool("pattpower").unwrap_or(false);
            }
        }
        s
    }
}

/// [re:7f7a57] (12 bytes: scene, delta, on).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneChange {
    pub scene: usize,
    pub delta: u32,
    pub on: bool,
}

/// Host timing the quantiser needs ([re:99c841] PPQ at block start).
#[derive(Debug, Clone, Copy)]
pub struct SceneClock {
    pub ppq: f64,
    pub samples_per_beat: f64,
    pub beats_per_bar: f64,
}

/// Editor notifications: `e_sceneChangeCued` 0x1eb, `e_sceneChangeActivated`
/// 0x1ec, `e_sceneChangesCleared` 0x1ed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SceneEvent {
    Cued(usize),
    Activated(usize),
    /// `e_sceneSelectionChanged` 0x1ee: activation makes the scene current.
    Selected(usize),
    Cleared,
}

#[derive(Debug, Clone)]
pub struct SceneManager {
    /// `Scene` clipboard (and its valid flag).
    pub clipboard: Option<Scene>,
    /// For the editor (drained by the host).
    pub events: Vec<SceneEvent>,
    pub scenes: Vec<Scene>,
    /// `CurrentScene`, -1 = none.
    pub current: i32,
    /// The bypassed engine.
    pub bypass: usize,
    changes: Vec<SceneChange>,
}

impl Default for SceneManager {
    fn default() -> Self {
        SceneManager { clipboard: None, events: Vec::with_capacity(64), scenes: vec![Scene::default(); SCENES], current: -1, bypass: NO_BYPASS, changes: Vec::with_capacity(MAX_CHANGES) }
    }
}

impl SceneManager {
    /// [re:eb8ed3]: children `Scene%d`, missing ones cleared.
    pub fn from_node(n: &Node) -> SceneManager {
        let mut m = SceneManager::default();
        for (i, s) in m.scenes.iter_mut().enumerate() {
            if let Some(c) = n.child("Scene", &format!("Scene{i}")) {
                *s = Scene::from_node(c);
            }
        }
        m.current = n.attr("CurrentScene").and_then(|v| v.parse().ok()).unwrap_or(-1);
        m
    }

    /// Carry the runtime state across an engine rebuild ([re:57f537]):
    /// the scenes themselves come from the document; the current scene,
    /// the bypassed engine, queued changes, the clipboard and undelivered
    /// events carry over.
    pub fn adopt(&mut self, old: &mut SceneManager) {
        self.current = old.current;
        self.bypass = old.bypass;
        self.clipboard = old.clipboard;
        std::mem::swap(&mut self.changes, &mut old.changes);
        std::mem::swap(&mut self.events, &mut old.events);
    }

    pub fn pending(&self) -> &[SceneChange] {
        &self.changes
    }

    /// Queue a scene change [re:e448a6]. A release (`on == false`) while changes are
    /// pending re-queues every pending "on" change as an "off" at the same
    /// delta (the release's own scene/delta are then ignored).
    pub fn add(&mut self, scene: usize, delta: u32, on: bool, clock: Option<&SceneClock>) {
        if !on && !self.changes.is_empty() {
            // A copy on the stack (the audio thread runs this).
            let mut ons = [SceneChange { scene: 0, delta: 0, on: false }; MAX_CHANGES];
            let mut n = 0;
            for c in self.changes.iter().filter(|c| c.on) {
                ons[n] = *c;
                n += 1;
            }
            for c in &ons[..n] {
                self.add_no_gate_check(c.scene, c.delta, false, clock);
            }
            return;
        }
        self.add_no_gate_check(scene, delta, on, clock);
    }

    /// Queue without the gate check [re:3c6b4d]: quantise "on" changes by the
    /// scene's start mode, append (≤ 128), sort by delta.
    pub fn add_no_gate_check(&mut self, scene: usize, mut delta: u32, on: bool, clock: Option<&SceneClock>) {
        if scene > SCENES {
            return;
        }
        if on {
            if let (Some(t), Some(s)) = (clock, self.scenes.get(scene)) {
                let target = match s.start {
                    StartMode::Bar => Some(((delta as f64 / t.samples_per_beat + t.ppq) / t.beats_per_bar + 0.99999).floor() * t.beats_per_bar),
                    StartMode::Beat => Some((t.ppq + 0.99999 + delta as f64 / t.samples_per_beat).floor()),
                    StartMode::Immediate => None,
                };
                if let Some(target) = target {
                    let d = t.samples_per_beat * (target - t.ppq);
                    delta = if d >= 0.0 { d as i64 as u32 } else { 0 };
                }
            }
        }
        if self.changes.len() >= MAX_CHANGES {
            return;
        }
        self.changes.push(SceneChange { scene, delta, on });
        // [re:3c6b4d]: 0x1eb for a queued "on"
        // change that is not immediate (delta ≠ 0, any start mode).
        if on && delta != 0 {
            self.notify(SceneEvent::Cued(scene));
        }
        // Ordering [re:97e4ab]: by delta, "on" before "off" at equal deltas.
        self.changes.sort_by(|a, b| a.delta.cmp(&b.delta).then(b.on.cmp(&a.on)));
    }

    /// [re:74956d]: the first change after `start`, capped at `end`.
    pub fn block_end(&self, start: u32, end: u32) -> u32 {
        match self.changes.iter().find(|c| c.delta > start) {
            Some(c) => c.delta.min(end),
            None => end,
        }
    }

    /// [re:99dd06]: pop the changes due at `start` (late
    /// ones are clamped to `start`), in order. Scene 96 entries are inert.
    pub fn pop_due(&mut self, start: u32) -> Option<SceneChange> {
        while let Some(c) = self.changes.first_mut() {
            if c.delta < start {
                c.delta = start;
            }
            if c.scene != SCENES && c.delta != start {
                return None;
            }
            let c = self.changes.remove(0);
            if c.scene != SCENES {
                return Some(c);
            }
        }
        None
    }

    /// End of block [re:ee20a2]: rebase the remaining deltas.
    pub fn end_of_block(&mut self, n: u32) {
        if self.changes.iter().any(|c| c.delta < n) {
            self.changes.clear();
            return;
        }
        for c in &mut self.changes {
            c.delta -= n;
        }
    }

    /// Transport change [re:8b7b7d]: transport stopped → drop pending changes.
    /// Queue an editor notification (bounded when nobody drains them).
    pub fn notify(&mut self, e: SceneEvent) {
        if self.events.len() < 256 {
            self.events.push(e);
        }
    }

    pub fn transport_stopped(&mut self) {
        self.changes.clear();
        self.notify(SceneEvent::Cleared);
    }

    /// `e_sceneTriggerNext` / `Prev` 0x10b / 0x10c (message handler
    /// [re:6105bb]): press triggers the scene after / before the current one
    /// (scene 0 when none; nothing past either end), release turns the
    /// pending "on" changes (or else the current scene) off.
    pub fn trigger_step(&mut self, next: bool, on: bool, clock: Option<&SceneClock>) {
        if on {
            let target = match (self.current, next) {
                (-1, _) => Some(0),
                (c, true) if c < SCENES as i32 - 1 => Some(c as usize + 1),
                (c, false) if c > 0 => Some(c as usize - 1),
                _ => None,
            };
            if let Some(t) = target {
                self.add_no_gate_check(t, 0, true, clock);
            }
        } else if !self.changes.is_empty() {
            self.add(0, 0, false, clock);
        } else if self.current >= 0 {
            self.add_no_gate_check(self.current as usize, 0, false, clock);
        }
    }

    /// `e_sceneCut` 0x111 / `e_sceneCopy` 0x112 / paste
    /// [re:99a82e] (0x113).
    pub fn copy(&mut self, scene: usize, cut: bool) {
        if let Some(&s) = self.scenes.get(scene) {
            self.clipboard = Some(s);
            if cut {
                self.scenes[scene] = Scene::default();
            }
        }
    }

    pub fn paste(&mut self, scene: usize) {
        if let (Some(c), Some(s)) = (self.clipboard, self.scenes.get_mut(scene)) {
            *s = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> SceneClock {
        SceneClock { ppq: 1.25, samples_per_beat: 1000.0, beats_per_bar: 4.0 }
    }

    #[test]
    fn quantises_to_beat_and_bar() {
        let mut m = SceneManager::default();
        m.scenes[1].start = StartMode::Beat;
        m.scenes[2].start = StartMode::Bar;
        m.add(0, 10, true, Some(&clock()));
        m.add(1, 10, true, Some(&clock()));
        m.add(2, 10, true, Some(&clock()));
        let d: Vec<u32> = m.pending().iter().map(|c| c.delta).collect();
        // beat: floor(1.25 + 0.99999 + 0.01) = 2 → 750; bar: 4 → 2750.
        assert_eq!(d, vec![10, 750, 2750]);
    }

    #[test]
    fn release_requeues_pending_ons_as_offs() {
        let mut m = SceneManager::default();
        m.add(3, 100, true, None);
        m.add(5, 7, false, None);
        assert_eq!(m.pending(), &[SceneChange { scene: 3, delta: 100, on: true }, SceneChange { scene: 3, delta: 100, on: false }]);
        assert_eq!(m.block_end(0, 512), 100);
        assert!(m.pop_due(0).is_none());
        let (a, b) = (m.pop_due(100).unwrap(), m.pop_due(100).unwrap());
        assert!(a.on && !b.on);
        assert!(m.pop_due(100).is_none());
        assert!(m.pending().is_empty());
    }

    /// A rebuild keeps a cued scene change, the current scene and the
    /// clipboard; the scenes come from the new document.
    #[test]
    fn adopt_carries_runtime_state() {
        let mut old = SceneManager::default();
        old.scenes[2] = Scene { used: true, start: StartMode::Bar, engines: Default::default() };
        old.add(2, 0, true, Some(&clock()));
        old.current = 4;
        old.copy(2, false);
        let mut new = SceneManager::default();
        new.scenes[9].used = true;
        new.adopt(&mut old);
        assert_eq!(new.pending().len(), 1);
        assert_eq!((new.current, new.clipboard.map(|c| c.used)), (4, Some(true)));
        assert!(new.scenes[9].used && !new.scenes[2].used);
        assert_eq!(new.events.len(), 1, "the cue event is still to be delivered");
    }

    #[test]
    fn next_prev_and_clipboard() {
        let mut m = SceneManager::default();
        m.trigger_step(true, true, None);
        assert_eq!(m.pending()[0], SceneChange { scene: 0, delta: 0, on: true });
        m.pop_due(0);
        m.current = 5;
        m.trigger_step(false, true, None);
        assert_eq!(m.pending()[0].scene, 4);
        m.pop_due(0);
        m.trigger_step(false, false, None);
        assert_eq!(m.pending()[0], SceneChange { scene: 5, delta: 0, on: false });
        m.scenes[3].used = true;
        m.copy(3, true);
        assert!(!m.scenes[3].used);
        m.paste(7);
        assert!(m.scenes[7].used);
    }
}
