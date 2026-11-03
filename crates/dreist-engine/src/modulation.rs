//! Transmod host and the 16 modulator slots ([re:bf1705], `Modulator`,
//! [re:75b211]). See the dreist-re modulation notes.

use dreist_dsp::modulators::{AdEnvelope, BouncingBall, Lfo, MathModulator, MiniSequencer, MorphLfo, SampleAndHold, frequency_hz};
use dreist_dsp::voice::SOURCES;
use dreist_formats::{ModulatorType, Node, TransmodSource};

use crate::engine::{Row, param_sum};

/// Transmod source indices ([re:56009e]).
pub mod src {
    pub const RANDOM: usize = 4;
    pub const SLOT1: usize = 5;
    pub const MACRO1: usize = 21;
    pub const MOD_WHEEL: usize = 25;
    pub const PITCH_WHEEL: usize = 26;
    pub const CHANNEL_PRESS: usize = 27;
    pub const TRANSPORT: usize = 28;
}

/// [re:bf1705] ([re:3b6b9b]): raw source values (+0x194), polarity
/// (`Polar_<Source>`, +0x88), and the derived scale/offset (+0xa8/+0x11c):
/// unipolar sources map `v → v`, bipolar ones `v → 2v − 1`.
#[derive(Debug, Clone)]
pub struct TransmodHost {
    pub values: [f32; SOURCES],
    pub unipolar: [bool; SOURCES],
}

impl Default for TransmodHost {
    fn default() -> Self {
        let mut values = [0.0; SOURCES];
        values[0] = 1.0;
        TransmodHost { values, unipolar: [true; SOURCES] }
    }
}

impl TransmodHost {
    /// `SerializeTransmodHost` [re:49d996] (polarity) + macro values ([re:92292a]).
    pub fn from_root(root: &Node) -> TransmodHost {
        let mut h = TransmodHost::default();
        for (i, s) in TransmodSource::ALL.iter().enumerate() {
            if let Some(b) = root.bool(&format!("Polar_{}", s.name())) {
                h.unipolar[i] = b;
            }
        }
        for m in 0..4 {
            h.values[src::MACRO1 + m] = root.f32(&format!("Macro{m}")).unwrap_or(0.0);
        }
        h
    }

    pub fn scale(&self) -> [f32; SOURCES] {
        std::array::from_fn(|i| if self.unipolar[i] { 1.0 } else { 2.0 })
    }

    pub fn offset(&self) -> [f32; SOURCES] {
        std::array::from_fn(|i| if self.unipolar[i] { 0.0 } else { 1.0 })
    }

    /// `PrecomputeModulationValuesForParameterSum`: `M_i = v_i·S_i − O_i`.
    pub fn m(&self) -> [f32; SOURCES] {
        let mut m: [f32; SOURCES] = std::array::from_fn(|i| self.values[i] * if self.unipolar[i] { 1.0 } else { 2.0 } - if self.unipolar[i] { 0.0 } else { 1.0 });
        m[0] = 1.0;
        m
    }
}

enum Gen {
    Lfo(Lfo),
    Morph(MorphLfo),
    SampleHold(SampleAndHold),
    Ad(AdEnvelope),
    Seq(MiniSequencer),
    Math(MathModulator),
    Const,
    Ball(BouncingBall),
}

/// One modulator slot (`Modulator`, 800 bytes).
/// More than any modulator type's parameter count (the sequencer's 22 plus gain).
const MAX_PARAMS: usize = 32;

pub struct Modulator {
    pub kind: ModulatorType,
    rows: Vec<Row>,
    /// `ResetAtStart` (+0x38, default on).
    pub reset_at_start: bool,
    /// `PadTrig<e>-<p>` (+0x39 + e·64 + p).
    pub pad_trig: Box<[[bool; 64]; 8]>,
    /// `EngTrig<e>` (+0x239).
    pub eng_trig: [bool; 8],
    generator: Gen,
    sr: f32,
    buf: Vec<f32>,
}

impl Modulator {
    /// [re:5da30c]: the mini
    /// sequencer's current step, −1 for the other types.
    pub fn visualization(&self) -> i32 {
        match &self.generator {
            Gen::Seq(s) => s.current_step() as i32,
            _ => -1,
        }
    }

    /// [re:b4277f] (load).
    pub fn from_node(n: &Node, sr: f32, seed: u32) -> Option<Modulator> {
        let kind = ModulatorType::ALL.iter().copied().find(|t| Some(t.name()) == n.attr("Type"))?;
        let rows = kind
            .params()
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let mut r = [0.0; SOURCES];
                r[0] = n.f32(&format!("{name}_ActualParameter")).unwrap_or(kind.default_value(i)).clamp(0.0, 1.0);
                for (si, s) in TransmodSource::ALL.iter().enumerate().skip(1) {
                    r[si] = n.f32(&format!("{name}_{}", s.name())).unwrap_or(0.0).clamp(-1.0, 1.0);
                }
                r
            })
            .collect();
        let mut pad_trig = Box::new([[false; 64]; 8]);
        let mut eng_trig = [false; 8];
        for e in 0..8 {
            eng_trig[e] = n.bool(&format!("EngTrig{e}")).unwrap_or(false);
            for p in 0..64 {
                pad_trig[e][p] = n.bool(&format!("PadTrig{e}-{p}")).unwrap_or(false);
            }
        }
        let generator = match kind {
            ModulatorType::LFO => Gen::Lfo(Lfo::new(sr, seed)),
            ModulatorType::MorphingLFO => Gen::Morph(MorphLfo::new(sr)),
            ModulatorType::SampleAndHold => Gen::SampleHold(SampleAndHold::new(sr, seed)),
            ModulatorType::ADEnvelope => Gen::Ad(AdEnvelope::new(sr)),
            ModulatorType::MiniSequencer => Gen::Seq(MiniSequencer::new(sr, seed)),
            ModulatorType::Maths => Gen::Math(MathModulator::default()),
            ModulatorType::Constant => Gen::Const,
            ModulatorType::BouncingBall => Gen::Ball(BouncingBall::new(sr)),
        };
        Some(Modulator { kind, rows, reset_at_start: n.bool("ResetAtStart").unwrap_or(true), pad_trig, eng_trig, generator, sr, buf: Vec::with_capacity(crate::engine::SUB_BLOCK) })
    }

    /// Keep `old`'s running state when it is the same type (an engine
    /// rebuild does not retrigger or re-phase the modulator).
    pub fn adopt(&mut self, old: &mut Modulator) {
        if self.kind == old.kind {
            std::mem::swap(&mut self.generator, &mut old.generator);
        }
    }

    /// [re:acb35e] (message 0x30).
    pub fn set_param(&mut self, param: usize, source: usize, value: f32) {
        if let Some(r) = self.rows.get_mut(param) {
            if source < SOURCES {
                r[source] = if source == 0 { value.clamp(0.0, 1.0) } else { value.clamp(-1.0, 1.0) };
            }
        }
    }

    /// Parameter values (a fixed array: this runs on the audio thread).
    fn params(&self, m: &[f32; SOURCES]) -> [f32; MAX_PARAMS] {
        std::array::from_fn(|i| self.rows.get(i).map_or(0.0, |r| param_sum(r, m)))
    }

    /// `GetGain` [re:5092ad]: the last parameter.
    pub fn gain(&self, m: &[f32; SOURCES]) -> f32 {
        self.rows.last().map(|r| param_sum(r, m)).unwrap_or(1.0)
    }

    /// [re:134274]; returns the block maximum.
    pub fn process(&mut self, n: usize, host: &TransmodHost, bpm: f32) -> f32 {
        let m = host.m();
        let p = self.params(&m);
        self.buf.clear();
        self.buf.resize(n, 0.0);
        let sync = self.rows.first().map(|r| r[0]).unwrap_or(1.0);
        let sr = self.sr;
        match &mut self.generator {
            Gen::Lfo(l) => l.process(&mut self.buf, frequency_hz(p[1], sync, bpm), (p[2] * 8.0).round() as u32),
            Gen::Morph(l) => l.process(&mut self.buf, frequency_hz(p[1], sync, bpm), p[2], p[4]),
            Gen::SampleHold(s) => s.process(&mut self.buf, frequency_hz(p[1], sync, bpm), p[2]),
            Gen::Ad(a) => a.process(&mut self.buf, p[0], p[1], p[2], p[3]),
            Gen::Seq(s) => {
                for (i, v) in s.values.iter_mut().enumerate() {
                    *v = p[6 + i];
                }
                let hz = frequency_hz(p[0], 1.0, bpm);
                let sps = ((1.0 / hz) * sr).floor() as u32;
                let last = (p[1] * 15.0).floor() as u32;
                s.process(&mut self.buf, sps, p[3] >= 0.5, p[4] >= 0.5, p[5] >= 0.5, last, p[2]);
            }
            Gen::Math(mm) => {
                let pick = |x: f32| {
                    let k = (x * 25.0 + if x * 25.0 < 0.0 { -0.5 } else { 0.5 }) as i32;
                    host.values.get((3 + k).clamp(0, 28) as usize).copied().unwrap_or(0.0)
                };
                let op = (p[0] * 34.0).round() as u32;
                mm.process(&mut self.buf, pick(p[1]), pick(p[2]), op);
            }
            Gen::Const => self.buf.fill(p[0]),
            Gen::Ball(b) => b.process(&mut self.buf, p[0], p[1]),
        }
        self.buf.iter().fold(f32::MIN, |a, b| a.max(*b)).max(if n == 0 { 0.0 } else { f32::MIN })
    }

    /// [re:4ecad8]. `note` = triggered by a pad/engine;
    /// otherwise the phase follows the song position `ppq` (transport start).
    pub fn reset(&mut self, ppq: f32, bpm: f32, host: &TransmodHost, note: bool) {
        let p = self.params(&host.m());
        let sync = self.rows.first().map(|r| r[0]).unwrap_or(1.0);
        let phase_at = |hz: f32, base: f32| {
            let mut x = (ppq * 60.0 * hz) / bpm + base;
            if x < 0.0 {
                x = x % 1.0 + 1.0;
            }
            if 1.0 <= x {
                x %= 1.0;
            }
            x
        };
        match &mut self.generator {
            Gen::Lfo(l) => l.reset_phase(if note { p[3] } else { phase_at(frequency_hz(p[1], sync, bpm), p[3]) } as f64),
            Gen::Morph(l) => l.reset_phase(if note { p[3] } else { phase_at(frequency_hz(p[1], sync, bpm), p[3]) }),
            Gen::SampleHold(s) => s.reset(if note { p[3] } else { phase_at(frequency_hz(p[1], sync, bpm), p[3]) }),
            Gen::Ad(a) => {
                if note {
                    a.note_on(1.0);
                }
            }
            Gen::Seq(s) => {
                if note {
                    s.reset(0);
                } else if p[5] < 0.5 {
                    let hz = frequency_hz(p[0], 1.0, bpm);
                    let sps = ((1.0 / hz) * self.sr).floor() as i32;
                    let total = (((p[1] * 15.0).floor() as i32) + 1) * sps;
                    let x = phase_at(hz, 0.0);
                    let pos = (x * total as f32) as i32;
                    s.set_step(if sps > 0 && pos > 0 { (pos / sps) as u32 } else { 0 });
                }
            }
            Gen::Math(mm) => {
                if note {
                    mm.reset();
                }
            }
            Gen::Const => {}
            Gen::Ball(b) => {
                if note {
                    b.note_on(1.0);
                }
            }
        }
    }

    /// [re:e9c7da]; whether it retriggered
    /// (`WillTriggerOnPad` [re:6571c3]).
    pub fn note_on(&mut self, engine: usize, pad: usize, host: &TransmodHost, bpm: f32) -> bool {
        if self.pad_trig.get(engine).is_some_and(|e| e[pad]) {
            self.reset(0.0, bpm, host, true);
            return true;
        }
        false
    }

    /// [re:33ac70].
    pub fn engine_changed_pattern(&mut self, engine: usize, host: &TransmodHost, bpm: f32) {
        if self.eng_trig.get(engine).copied().unwrap_or(false) {
            self.reset(0.0, bpm, host, true);
        }
    }

    /// [re:a1abf2].
    pub fn transport_started(&mut self, ppq: f32, bpm: f32, host: &TransmodHost) {
        if let Gen::Seq(s) = &mut self.generator {
            s.running = true;
        }
        if self.reset_at_start {
            if let Gen::Seq(s) = &mut self.generator {
                s.set_step(0);
            }
            self.reset(ppq, bpm, host, false);
        }
    }
}

/// [re:75b211]: run the 16 slots and store their
/// gain-scaled block maxima as sources Slot1..16.
pub fn process_transmod(mods: &mut [Option<Modulator>], host: &mut TransmodHost, n: usize, bpm: f32) {
    for (i, slot) in mods.iter_mut().enumerate().take(16) {
        let v = match slot {
            None => 0.0,
            Some(m) => {
                let mx = m.process(n, host, bpm);
                let g = m.gain(&host.m());
                if host.unipolar[src::SLOT1 + i] { g * mx } else { ((mx + mx - 1.0) * g + 1.0) * 0.5 }
            }
        };
        host.values[src::SLOT1 + i] = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(xml: &str) -> Node {
        Node::parse(xml).unwrap()
    }

    #[test]
    fn mini_sequencer_reports_its_playing_step() {
        let n = node(r#"<Modulator name="Modulator0" Type="MiniSequencer" />"#);
        let m = Modulator::from_node(&n, 1000.0, 1).unwrap();
        assert_eq!(m.visualization(), 0);
        let lfo = Modulator::from_node(&node(r#"<Modulator name="Modulator0" Type="LFO" />"#), 1000.0, 1).unwrap();
        assert_eq!(lfo.visualization(), -1);
        let mut host = TransmodHost::default();
        let mut m = m;
        m.transport_started(0.0, 120.0, &host);
        let mut mods = vec![Some(m)];
        // Run long enough for the sequencer to move on.
        for _ in 0..200 {
            process_transmod(&mut mods, &mut host, 64, 120.0);
        }
        assert!(mods[0].as_ref().unwrap().visualization() > 0);
    }

    #[test]
    fn square_lfo_slot_value() {
        // Free-running 1 Hz-ish square: block maxima alternate 1 and 0.
        let n = node(r#"<Modulator name="Modulator0" Type="LFO" Sync_ActualParameter="0" Rate_ActualParameter="0.3299" Shape_ActualParameter="0.625" />"#);
        let mut m = Modulator::from_node(&n, 1000.0, 1).unwrap();
        assert_eq!(m.kind, ModulatorType::LFO);
        let mut host = TransmodHost::default();
        let mut mods = vec![Some(m)];
        process_transmod(&mut mods, &mut host, 32, 120.0);
        assert_eq!(host.values[src::SLOT1], 1.0);
        m = mods.pop().unwrap().unwrap();
        // Rate 0.3299 → 250·r⁵ ≈ 0.98 Hz; after ~0.6 s the square is low.
        for _ in 0..19 {
            m.process(32, &host, 120.0);
        }
        assert_eq!(m.process(32, &host, 120.0), 0.0);
    }

    #[test]
    fn bipolar_slot_mapping_and_macros() {
        let root = node(r#"<root Macro2="0.25" Polar_Slot1="false" />"#);
        let host = TransmodHost::from_root(&root);
        assert_eq!(host.values[src::MACRO1 + 2], 0.25);
        assert!(!host.unipolar[src::SLOT1]);
        let m = host.m();
        assert_eq!(m[src::SLOT1], -1.0);
        assert_eq!(m[0], 1.0);
    }
}

#[cfg(test)]
mod max_params_tests {
    #[test]
    fn every_type_fits_the_parameter_array() {
        for t in dreist_formats::ModulatorType::ALL {
            assert!(t.params().len() <= super::MAX_PARAMS, "{:?}", t);
        }
    }
}
