//! Parameter tables for Layer, Pad and Engine, as the original engine has them.
//!
//! Files store every parameter as a normalized `0..=1` float and omit values
//! equal to the default. These tables give each parameter's serialized name,
//! default, display range and the curve between normalized and display units.
//! Sources (v2 2.1.2.13 VST, see the dreist-re ledger):
//! - names: [re:aa9ed1], [re:b4517a]
//!   [re:e480d7], [re:386b20]
//! - defaults: [re:39d754], [re:bbfa97]
//! - ranges/curves: the per-object range and range-conversion functions
//! - dB curve: [re:e947f6] / dB to normalized [re:d8eda3]

/// Normalized value that maps to 0 dB: the cube root of 0.5 (`0x3f4b2ff5`).
pub const UNITY_GAIN: f32 = f32::from_bits(0x3f4b_2ff5);

/// Envelope stage maxima in seconds ([re:f36b8e]..[re:ac5929]).
pub const MAX_ATTACK_SECS: f32 = 10.0;
pub const MAX_DECAY_SECS: f32 = 10.0;
pub const MAX_HOLD_SECS: f32 = 60.0;
pub const MAX_RELEASE_SECS: f32 = 15.0;

/// `dB = 20·log10(2x³)`, floored at -1000 dB near silence and capped at +6 dB.
pub fn value01_to_db(x: f32) -> f32 {
    let a = (2.0 * x * x * x).abs();
    let db = if a < 1e-6 { -1000.0 } else { 20.0 * a.log10() };
    db.min(6.0)
}

/// Inverse of [`value01_to_db`]: `x = ∛0.5 · 10^(dB/60)`, clamped to `0..=1`.
pub fn db_to_value01(db: f32) -> f32 {
    let db = db.clamp(-1000.0, 6.0);
    ((db * (std::f32::consts::LN_10 / 60.0)).exp() * UNITY_GAIN).clamp(0.0, 1.0)
}

/// How a normalized value maps to display units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    /// [`value01_to_db`], floored at -100 dB for display.
    Decibels,
    /// `lo + (hi - lo)·x`.
    Linear(f32, f32),
    /// Filter cutoff in Hz: `440·2^(12x - 6)`, 6.875 Hz ..= 28160 Hz.
    Cutoff,
    /// Timestretch amount in %: `200x` below 0.5, `1400x - 600` above (0..=800).
    Stretch,
}

impl Curve {
    pub fn range(self) -> (f32, f32) {
        match self {
            Curve::Decibels => (-100.0, 6.0),
            Curve::Linear(lo, hi) => (lo, hi),
            Curve::Cutoff => (6.875, 28160.0),
            Curve::Stretch => (0.0, 800.0),
        }
    }

    pub fn to_units(self, x: f32) -> f32 {
        match self {
            Curve::Decibels => value01_to_db(x).max(-100.0),
            Curve::Linear(lo, hi) => (hi - lo) * x + lo,
            Curve::Cutoff => (x * 12.0 - 6.0).exp2() * 440.0,
            Curve::Stretch if x < 0.5 => x * 200.0,
            Curve::Stretch => x * 1400.0 - 600.0,
        }
    }

    pub fn from_units(self, v: f32) -> f32 {
        match self {
            Curve::Decibels => db_to_value01(v),
            Curve::Linear(lo, hi) => (lo - v) / (lo - hi),
            Curve::Cutoff => ((v.clamp(6.0, 28160.0) / 440.0).log2() + 6.0) / 12.0,
            Curve::Stretch if v > 100.0 => (v - 100.0) / 1400.0 + 0.5,
            Curve::Stretch => v * 0.005,
        }
    }
}

const UNIT: Curve = Curve::Linear(0.0, 1.0);
const PAN: Curve = Curve::Linear(-1.0, 1.0);
const PERCENT: Curve = Curve::Linear(0.0, 100.0);
const SEMITONES: Curve = Curve::Linear(-12.0, 12.0);

macro_rules! param_enum {
    ($(#[$m:meta])* $ty:ident { $($v:ident $(= $stored:literal)?),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum $ty { $($v),* }

        impl $ty {
            pub const ALL: &[$ty] = &[$($ty::$v),*];

            /// Attribute name used in files: the map entry after
            /// [re:c38326]'s replacements where one exists,
            /// else the enum's own name.
            pub fn name(self) -> &'static str {
                match self { $($ty::$v => param_enum!(@stored $v $($stored)?)),* }
            }

            /// The enum's own name (`Create*ParametersMap`).
            pub fn enum_name(self) -> &'static str {
                match self { $($ty::$v => stringify!($v)),* }
            }

            /// A stored attribute name; the enum name is accepted too (older
            /// port builds wrote it).
            pub fn from_name(s: &str) -> Option<$ty> {
                $($(if s == $stored { return Some($ty::$v); })?)*
                match s { $(stringify!($v) => Some($ty::$v),)* _ => None }
            }
        }
    };
    (@stored $v:ident $stored:literal) => { $stored };
    (@stored $v:ident) => { stringify!($v) };
}

param_enum! {
    /// Layer parameters, in enum order. [re:c38326]
    /// renames five entries of the map [re:87acba] uses
    /// ([re:bec18e]: 0, 1, 8, 9, 7), so files store
    /// `Cutoff`, `Cutoff_FreeEnvelope`, ….
    LayerParam {
        Gain = "LayerGain", Pan = "LayerPan", VoiceGain, VoicePan, Octave, CoarseTune, FineTune, PreDelay = "PreDelayTime",
        FilterCutoff = "Cutoff", FilterResonance = "Resonance", FilterType, FilterDrive, FilterAlgorithm, FilterActive,
        AmpEnvAttack, AmpEnvDecay, AmpEnvSustain, AmpEnvHold, AmpEnvRelease,
        AmpEnvAttackCurve, AmpEnvDecayCurve, AmpEnvReleaseCurve,
        FreeEnvAttack, FreeEnvDecay, FreeEnvSustain, FreeEnvHold, FreeEnvRelease,
        FreeEnvAttackCurve, FreeEnvDecayCurve, FreeEnvReleaseCurve,
        SampleStartPoint, SampleEndPoint, SampleLoopStartPoint, SampleLoopEndPoint,
        LoopInRelease, AltLoopMode, XFadeLoopTime, XFadeLoopCurve,
        TimestretchActive, TimestretchAmount, TimestretchTracksBPM, TimestretchMode,
    }
}

param_enum! {
    /// [re:56009e]. Source 0 is the parameter itself; every other
    /// source's depth is stored as `<Param>_<Source>` ([re:0cef81]).
    TransmodSource {
        ActualParameter, AmpEnvelope, FreeEnvelope, Velocity, Random,
        Slot1, Slot2, Slot3, Slot4, Slot5, Slot6, Slot7, Slot8,
        Slot9, Slot10, Slot11, Slot12, Slot13, Slot14, Slot15, Slot16,
        Macro1, Macro2, Macro3, Macro4, ModWheel, PitchWheel, ChannelPress, Transport,
    }
}

impl TransmodSource {
    /// Attribute holding this source's depth on `param`; `None` for the base value.
    pub fn attr(self, param: &str) -> Option<String> {
        (self != TransmodSource::ActualParameter).then(|| format!("{param}_{}", self.name()))
    }
}

param_enum! {
    /// Pad parameters.
    PadParam { Gain, Pan, V2A, Pitch }
}

param_enum! {
    /// Engine parameters.
    EngineParam { Gain, Pan, Pitch }
}

impl LayerParam {
    /// The layer parameter default for source 0. Modulation depths default to 0.
    /// Hold's -1 is clamped to 0 on load like every base value.
    pub fn default_value(self) -> f32 {
        use LayerParam::*;
        match self {
            Gain | VoiceGain => UNITY_GAIN,
            PreDelay | FilterResonance | FilterType | FilterActive | AmpEnvAttack | FreeEnvAttack | LoopInRelease | AltLoopMode | XFadeLoopTime | TimestretchActive | TimestretchTracksBPM
            | TimestretchMode => 0.0,
            FilterCutoff | AmpEnvSustain | FreeEnvSustain => 1.0,
            FilterDrive | FilterAlgorithm => 0.3,
            AmpEnvDecay | FreeEnvDecay => 0.02,
            AmpEnvHold | FreeEnvHold => -1.0,
            AmpEnvRelease | FreeEnvRelease => 0.001,
            _ => 0.5,
        }
    }

    pub fn curve(self) -> Curve {
        use LayerParam::*;
        match self {
            Gain | VoiceGain => Curve::Decibels,
            Pan | VoicePan => PAN,
            Octave => Curve::Linear(-4.0, 4.0),
            CoarseTune => SEMITONES,
            FineTune => Curve::Linear(-100.0, 100.0),
            FilterCutoff => Curve::Cutoff,
            FilterResonance | FilterDrive | AmpEnvSustain | FreeEnvSustain | XFadeLoopTime => PERCENT,
            AmpEnvAttack | FreeEnvAttack => Curve::Linear(0.0, MAX_ATTACK_SECS),
            AmpEnvDecay | FreeEnvDecay => Curve::Linear(0.0, MAX_DECAY_SECS),
            AmpEnvHold | FreeEnvHold => Curve::Linear(0.0, MAX_HOLD_SECS),
            AmpEnvRelease | FreeEnvRelease => Curve::Linear(0.0, MAX_RELEASE_SECS),
            TimestretchAmount => Curve::Stretch,
            _ => UNIT,
        }
    }
}

impl PadParam {
    pub fn default_value(self) -> f32 {
        match self {
            PadParam::Gain => UNITY_GAIN,
            PadParam::V2A => 1.0,
            PadParam::Pan | PadParam::Pitch => 0.5,
        }
    }

    pub fn curve(self) -> Curve {
        match self {
            PadParam::Gain => Curve::Decibels,
            PadParam::Pan => PAN,
            PadParam::V2A => PERCENT,
            PadParam::Pitch => SEMITONES,
        }
    }
}

impl EngineParam {
    /// Defaults inlined in [re:3bcb63]: Gain ∛0.5, Pan and Pitch 0.5.
    pub fn default_value(self) -> f32 {
        match self {
            EngineParam::Gain => UNITY_GAIN,
            EngineParam::Pan | EngineParam::Pitch => 0.5,
        }
    }

    pub fn curve(self) -> Curve {
        match self {
            EngineParam::Gain => Curve::Decibels,
            EngineParam::Pan => PAN,
            EngineParam::Pitch => SEMITONES,
        }
    }
}

param_enum! {
    /// Modulator types, serialized as the `Type` attribute.
    ModulatorType {
        LFO, MorphingLFO, SampleAndHold, ADEnvelope, MiniSequencer, Maths, Constant, BouncingBall,
    }
}

/// Mini sequencer: `Rate, NumberOfSteps, Slew, Drunk, Reverse, Trig, Step1..16, Gain`.
const SEQUENCER_PARAMS: [&str; 23] = [
    "Rate", "NumberOfSteps", "Slew", "Drunk", "Reverse", "Trig", "Step1", "Step2", "Step3", "Step4", "Step5", "Step6", "Step7", "Step8", "Step9", "Step10", "Step11", "Step12", "Step13", "Step14",
    "Step15", "Step16", "Gain",
];

impl ModulatorType {
    /// Parameter names in index order (per-type parameter maps [re:7abac1]..[re:5c6661];
    /// counts match the per-source parameter counts [re:5e71ee]). Modulator parameters
    /// are stored as `<Name>_ActualParameter`, depths as `<Name>_<Source>`.
    pub fn params(self) -> &'static [&'static str] {
        match self {
            ModulatorType::LFO => &["Sync", "Rate", "Shape", "Phase", "Gain"],
            ModulatorType::MorphingLFO => &["Sync", "Rate", "Asymmetry", "Phase", "Swing", "Gain"],
            ModulatorType::SampleAndHold => &["Sync", "Rate", "Slew", "Phase", "Gain"],
            ModulatorType::ADEnvelope => &["Attack", "Decay", "AttackCurve", "DecayCurve", "Gain"],
            ModulatorType::MiniSequencer => &SEQUENCER_PARAMS,
            ModulatorType::Maths => &["Operator", "SourceA", "SourceB", "Gain"],
            ModulatorType::Constant => &["Constant", "Gain"],
            ModulatorType::BouncingBall => &["Gravity", "Speed", "Gain"],
        }
    }

    pub fn param_index(self, name: &str) -> Option<usize> {
        self.params().iter().position(|p| *p == name)
    }

    /// [re:75c991] (tables at [re:fdf223]..[re:35ccb8]).
    pub fn default_value(self, index: usize) -> f32 {
        const LFO: [f32; 5] = [1.0, 0.67, 0.0, 0.0, 1.0];
        const MLFO: [f32; 6] = [1.0, 0.67, 0.5, 0.0, 0.5, 1.0];
        const AD: [f32; 5] = [0.05, 0.5, 0.5, 0.5, 1.0];
        let gain = index + 1 == self.params().len();
        match self {
            ModulatorType::LFO | ModulatorType::SampleAndHold => LFO.get(index).copied().unwrap_or(0.0),
            ModulatorType::MorphingLFO => MLFO.get(index).copied().unwrap_or(0.0),
            ModulatorType::ADEnvelope => AD.get(index).copied().unwrap_or(0.5),
            // Rate 0.67, 16 steps, switches off, steps ramp 0, 1/16 .. 15/16, gain 1.
            ModulatorType::MiniSequencer => match index {
                0 => 0.67,
                1 | 22 => 1.0,
                6..=21 => (index - 6) as f32 / 16.0,
                _ => 0.0,
            },
            ModulatorType::Maths => {
                if gain {
                    1.0
                } else {
                    0.0
                }
            }
            ModulatorType::Constant | ModulatorType::BouncingBall => {
                if gain {
                    1.0
                } else {
                    0.5
                }
            }
        }
    }
}

/// [re:0caabd]: `AuxSend0..3` (plus
/// `_<Source>` depths) on Engine, Pad and Layer, default 0.
pub const AUX_SENDS: usize = 4;

pub fn aux_send_name(i: usize) -> String {
    format!("AuxSend{i}")
}

/// Effect types, serialized as `EffectType` ([re:8e9a01],
/// jump table at [re:2dad4e]). Index 0 is "No Effect".
pub const EFFECT_TYPES: [&str; 48] = [
    "No Effect",
    "Gain",
    "Stereo Width",
    "Parametric EQ",
    "DJ EQ",
    "Vari-EQ",
    "Hi-Lo Balance",
    "Enhancer",
    "Channel Compressor",
    "Bus Compressor",
    "VCA Compressor",
    "Noise Gate",
    "Envelope Shaper",
    "Drum Dynamics",
    "Limiter",
    "Delay",
    "Flanger",
    "Chorus",
    "Phaser",
    "Distortion",
    "Shaped Distortion",
    "Driver",
    "Maul",
    "Bit Crusher",
    "Ring Mod",
    "Frequency Shifter",
    "Amber Chorus",
    "PhaseMod Array",
    "Filter",
    "Modulated Filter",
    "Comb Filter",
    "Three Oh! Three Filter",
    "Hoover Filter",
    "Grandad Filter",
    "Little Fatty Filter",
    "Child Genius Filter",
    "Japan Filter",
    "Classic Guru Filter",
    "Audio Freezer",
    "Breverb Hall",
    "Breverb Plate",
    "Breverb Room",
    "Breverb Inverse",
    "Tin Can Verb",
    crate::legacy::REVERB_TYPE,
    "Bloom",
    "Dirty DAC",
    "Analog Ring Mod",
];

/// An effect type as menus and panel titles show it (the reverb is
/// stored under its legacy type name).
pub fn effect_label(kind: usize) -> &'static str {
    match EFFECT_TYPES.get(kind).copied().unwrap_or("") {
        crate::legacy::REVERB_TYPE => "Reverb",
        n => n,
    }
}

const SHAPES: &[&str] = &[
    "Thru", "Tanh", "Diode", "Op-amp", "JFET", "Transistor", "OTA", "Tube", "Diff", "Half Rect", "Tri", "RM - Sin", "RM - Tri", "RM - Saw", "RM - Sqr", "RM - White", "RM - Pink", "OD Asym",
    "OD Soft", "OD Shred", "OD Tannin", "Clip", "Clip Hard", "Clip Rect", "Clip Half", "Clip Half Rec", "Clip Full Rec", "Poly 1", "Poly 2", "Poly 3", "Poly 4", "Bit Redux", "SR Redux",
];
const MODEL_A: &[&str] = &[
    "L1", "L2", "L3", "L4", "B2", "B4", "H1", "H2", "H3", "H4", "H2+L1", "H3+L1", "N2", "N2+L1", "N2+L2", "H1+N2", "H1+N2+L1", "A3", "A3+L1", "N2+L1 SEP", "N2+L2 SEP", "H1+N2 SEP", "H1+N2+L1 SEP",
    "P2",
];
const MODEL_B: &[&str] = &[
    "L2", "L4", "B2", "B4", "H2", "H4", "P2", "NP2", "N2+P2 T", "N2+L2 T", "N2+H1+L1 T", "P2+N2 T", "H2+N2 T", "H1+L1+N2 T", "N2+P2+N2 T", "N2", "N4", "N2+L2", "H2+N2", "H1+N2+L1", "N2+N2A T",
    "N2+N2B T",
];

/// [re:c3c53e]: the menu items of an
/// effect's option parameter ([re:d1711c]);
/// the popup's value is item/(count − 1).
pub fn effect_option_strings(kind: usize, param: usize) -> Option<Vec<&'static str>> {
    let v: &[&str] = match (kind, param) {
        (5, 0) => &["Bell", "Low Shelf", "High Shelf", "LPF", "BPF", "BPF 0dB", "HPF", "Notch", "Allpass"],
        (7, 1) => &["I", "II", "III", "IV"],
        (8, 3) => &["4", "8", "12", "20", "Nuke!"],
        (0x12, 1) => &["P2+", "P2-", "P4+", "P4-", "P6+", "P6-", "P8+", "P8-"],
        (0x13, 0) => &["Diode", "OTA", "Op-amp", "Half-rect"],
        (0x14..=0x16, 0) => SHAPES,
        (0x18, 1) => &["Sine", "Triangle", "Sawtooth", "Square", "Parabola", "White noise", "Pink noise"],
        (0x1a, 0) => &["1975", "1977", "1981", "1984", "1985", "1986"],
        (0x1b, 1) => &["A", "B", "C"],
        (0x1d, 0) => &["Low", "Band", "High", "Peak", "Notch"],
        // Three Oh! Three spells one item "N2+H1 SEP" (the others "H1+N2 SEP").
        (0x1f, 1) => {
            return Some(
                MODEL_A
                    .iter()
                    .map(|s| match *s {
                        "H1+N2 SEP" => "N2+H1 SEP",
                        "H1+N2+L1 SEP" => "N2+H1+L1 SEP",
                        o => o,
                    })
                    .collect(),
            );
        }
        (0x20 | 0x23, 1) => MODEL_A,
        (0x21, 1) => MODEL_B,
        (0x24, 1) => return Some(MODEL_B.iter().map(|s| if *s == "NP2" { "N+P2" } else { *s }).collect()),
        (0x22, 1) => &["Low Pass", "High Pass", "Band Pass", "Notch"],
        (0x2d, 0) => &["Digital", "B.B.D", "Tape"],
        (0x2f, 0) => &["Sin", "Tri", "Saw", "Square"],
        _ => return None,
    };
    Some(v.to_vec())
}

pub fn effect_type_index(name: &str) -> Option<usize> {
    EFFECT_TYPES.iter().position(|t| *t == name)
}

/// Front-panel parameter names per effect type ([re:c5d0fd]
/// [re:737dba]); lengths match the per-effect parameter counts (table [re:73854e]).
/// Presets store the underlying node values as `Parameter0..N`, plus `IsActive`,
/// `Mix`, `Level`, `Trim`; factory defaults are each effect's `Default.fxpreset`.
pub fn effect_param_names(effect: usize) -> &'static [&'static str] {
    const FILTER4: &[&str] = &["Drive", "Mode", "Pitch", "Resonance"];
    const BREVERB: &[&str] = &["Reverb Time", "Pre Delay", "Diffusion", "Size", "Spread", "Shape"];
    match effect {
        1 => &["Gain"],
        2 => &["Gain", "Width"],
        3 => &["Low Q", "Low freq", "Low gain", "High Q", "High freq", "High gain"],
        4 => &["Low gain", "Mid gain", "Mid freq", "High gain"],
        5 => &["Mode", "Frequency", "Q", "Gain"],
        6 => &["Balance", "Crossover"],
        7 => &["Frequency", "Mode", "Depth"],
        8 => &["Input", "Attack", "Release", "Ratio", "Output"],
        9 => &["Threshold", "Attack", "Ratio", "Release", "Makeup", "Limit"],
        10 => &["Attack", "Release", "Threshold", "In Gain", "Out Gain", "Ratio"],
        11 => &["Attack", "Hold", "Release", "Threshold", "Hysteresis", "LPF"],
        12 => &["Attack", "Sustain", "Gain", "Sensitivity", "Linked"],
        13 => &["Attack", "Sustain", "Fast", "Sidechain"],
        14 => &["Hardness", "Pre-Gain", "Post-Gain"],
        15 => &["Delay time", "Feedback", "Low cut", "Hi cut", "Swing", "Sync"],
        16 => &["Frequency", "Depth", "Position", "Feedback", "Spread", "Phase"],
        17 => &["Frequency", "Depth", "Spread"],
        18 => &["Drive", "Mode", "Pitch", "Resonance", "Frequency", "Depth"],
        19 => &["Mode", "Drive", "Hi Cut", "Low Cut", "Tone", "Clean"],
        20 => &["Mode", "Drive", "Post Freq", "Restore", "Gain", "Saturate"],
        21 => &["Mode", "Drive", "In Gain", "Out Gain", "Saturate", "Limit"],
        22 => &["Mode", "Freq Low", "Rez Low", "Drive Low", "Gain Low", "Freq High", "Rez High", "Drive High", "Gain High"],
        23 => &["Drive", "Frequency", "Bits", "Hi Cut", "Low Cut", "Tone"],
        24 => &["Drive", "Type", "Pitch"],
        25 => &["Pitch", "Drive"],
        26 => &["Mode", "Speed", "Spread", "Bright"],
        27 => &["Pitch", "Mode", "Stack", "Width", "Spread", "Depth"],
        28 => &["HP active", "HP freq", "LP active", "LP freq"],
        29 => &["Mode", "Cutoff", "Resonance", "Attack", "Decay", "Env depth"],
        30..=36 => FILTER4,
        37 => &["Mode", "Cutoff", "Resonance"],
        38 => &["Gate", "Loop Length", "Grain Length", "Smooth", "Scratch", "Speed"],
        39 => BREVERB,
        40 | 41 => &BREVERB[..5],
        42 => &BREVERB[..3],
        43 => &["Decay", "Size", "Damping", "Pinch", "Squeeze"],
        44 => &["Room Size", "Decay", "Predelay", "Density", "Damping Freq", "Damping Gain"],
        45 => &["Model", "Delay Time", "Feedback", "Model Tone", "Sync", "Freeze", "Ping Pong", "Direction"],
        46 => &["Frequency", "AntiAlias"],
        // As the original lists them ([re:737dba]): its option table
        // ([re:d1711c]) makes panel parameter 0 the four-wave popup and its
        // map ([re:72c1b1], [1, 0, 2, 3]) sends it to node port 1 (Mode),
        // so the "Drive" label sits on the wave selector and "Mode" drives
        // port 0 (Drive) ([re:a863c4]). Kept as shipped.
        47 => &["Drive", "Mode", "Pitch", "Tone"],
        _ => &[],
    }
}

/// [re:72c1b1]: which
/// node parameter (`Parameter%d`) the `n`-th front-panel/step lane drives.
pub fn effect_node_param(effect: usize, n: usize) -> usize {
    let t: &[usize] = match effect {
        3 => &[4, 5, 6, 8, 9, 10],
        4 => &[2, 10, 9, 14],
        8 => &[0, 1, 2, 5, 6],
        9 => &[0, 1, 2, 3, 4, 9],
        10 => &[0, 1, 2, 3, 4, 6],
        15 => &[0, 2, 5, 6, 3, 1],
        19 => &[0, 1, 2, 3, 6, 5],
        20 => &[3, 0, 6, 2, 4, 8],
        21 => &[2, 0, 1, 3, 4, 5],
        23 => &[0, 1, 2, 3, 4, 7],
        27 => &[1, 2, 3, 4, 5, 8],
        29 => &[1, 2, 3, 8, 9, 10],
        38 => &[0, 1, 2, 4, 5, 6],
        39..=42 => &[1, 2, 3, 6, 7, 8],
        44 => &[1, 2, 8, 3, 5, 4],
        45 => &[0, 3, 7, 11, 4, 12, 2, 13],
        47 => &[1, 0, 2, 3],
        12 | 14 => return n + 1,
        26 => return if n > 3 { n } else { n + 1 },
        _ => return n,
    };
    t.get(n).copied().unwrap_or(n)
}

/// Step graph lanes. Serialized names are the display
/// names set in [re:cf74e8] with spaces removed
/// ([re:9b6bed]); attributes equal to the default are omitted.
pub const STEP_LANES: [&str; 93] = [
    "Velocity", "Pan", "Repeats", "Shift", "Reverse", "Octave", "CoarsePitch", "FinePitch", "StartPoint", "Duration", "LinkToPrevious", "FilterCutoff", "FilterResonance", "FilterType", "FilterDrive",
    "ForceLayer", "Probability", "LoopStart", "LoopEnd", "Send1", "Send2", "Send3", "Send4", "AmpEnvAttack", "AmpEnvDecay", "AmpEnvSustain", "AmpEnvHold", "AmpEnvRelease", "AmpEnvAttackCurve",
    "AmpEnvDecayCurve", "AmpEnvReleaseCurve", "FreeEnvAttack", "FreeEnvDecay", "FreeEnvSustain", "FreeEnvHold", "FreeEnvRelease", "FreeEnvAttackCurve", "FreeEnvDecayCurve", "FreeEnvReleaseCurve",
    "FX1Active", "FX1Mix", "FX1OutputLevel", "FX1Param1", "FX1Param2", "FX1Param3", "FX1Param4", "FX1Param5", "FX1Param6", "FX2Active", "FX2Mix", "FX2OutputLevel", "FX2Param1", "FX2Param2",
    "FX2Param3", "FX2Param4", "FX2Param5", "FX2Param6", "FX3Active", "FX3Mix", "FX3OutputLevel", "FX3Param1", "FX3Param2", "FX3Param3", "FX3Param4", "FX3Param5", "FX3Param6", "FX4Active", "FX4Mix",
    "FX4OutputLevel", "FX4Param1", "FX4Param2", "FX4Param3", "FX4Param4", "FX4Param5", "FX4Param6", "FX5Active", "FX5Mix", "FX5OutputLevel", "FX5Param1", "FX5Param2", "FX5Param3", "FX5Param4",
    "FX5Param5", "FX5Param6", "FX6Active", "FX6Mix", "FX6OutputLevel", "FX6Param1", "FX6Param2", "FX6Param3", "FX6Param4", "FX6Param5", "FX6Param6",
];

/// [re:f7ed4f].
pub fn step_lane_default(lane: usize) -> f32 {
    match lane {
        0 | 9 | 16 => 1.0,
        2 | 4 | 10 | 13 | 15 => 0.0,
        1 | 3 | 5..=8 | 11 | 12 | 14 | 17..=38 => 0.5,
        // v1's routing depths: neutral.
        V1_ROUTING_LANE..LANE_COUNT => 0.5,
        // Per FX slot: Active 0, everything else 1.
        39..=92 => {
            if (lane - 39).is_multiple_of(9) {
                0.0
            } else {
                1.0
            }
        }
        _ => 0.0,
    }
}

/// The first of the original v1's four routing-depth lanes, after the 93
/// lanes both versions share (see [`crate::legacy::ROUTING_DEPTH_LANES`]).
pub const V1_ROUTING_LANE: usize = STEP_LANES.len();

/// Every step lane: the shared ones and v1's routing depths.
pub const LANE_COUNT: usize = V1_ROUTING_LANE + 4;

/// A step lane's attribute name (shared lanes, then v1's routing depths).
pub fn step_lane_name(lane: usize) -> Option<&'static str> {
    STEP_LANES.get(lane).or_else(|| crate::legacy::ROUTING_DEPTH_LANES.get(lane.wrapping_sub(V1_ROUTING_LANE))).copied()
}

pub fn step_lane_index(name: &str) -> Option<usize> {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<&'static str, usize>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| (0..LANE_COUNT).filter_map(|i| step_lane_name(i).map(|n| (n, i))).collect()).get(name).copied()
}

/// [re:9c4e55] (table [re:af5cce]): number of discrete values a
/// lane snaps to, `None` = continuous.
pub fn step_lane_quantization(lane: usize) -> Option<u32> {
    const Q: [i32; 15] = [128, 128, -1, -1, -1, -1, -1, 200, 100, -1, -1, 100, 100, -1, 100];
    let q = match lane {
        0..=14 => Q[lane],
        15 => -1,
        16..=92 => {
            if lane >= 39 && (lane - 39).is_multiple_of(9) {
                -1
            } else {
                100
            }
        }
        _ => -1,
    };
    u32::try_from(q).ok()
}

/// [re:029dbd]: FX slot and front-panel parameter of an
/// `FXnParamM` lane.
pub fn step_lane_fx_param(lane: usize) -> Option<(usize, usize)> {
    let k = lane.checked_sub(39)?;
    let (slot, i) = (k / 9, k % 9);
    (slot < 6 && i >= 3).then(|| (slot, i - 3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_order_matches_binary_name_list() {
        let list = "Gain, Pan, VoiceGain, VoicePan, Octave, CoarseTune, FineTune, PreDelay, FilterCutoff, FilterResonance, FilterType, FilterDrive, FilterAlgorithm, FilterActive, AmpEnvAttack, AmpEnvDecay, AmpEnvSustain, AmpEnvHold, AmpEnvRelease, AmpEnvAttackCurve, AmpEnvDecayCurve, AmpEnvReleaseCurve, FreeEnvAttack, FreeEnvDecay, FreeEnvSustain, FreeEnvHold, FreeEnvRelease, FreeEnvAttackCurve, FreeEnvDecayCurve, FreeEnvReleaseCurve, SampleStartPoint, SampleEndPoint, SampleLoopStartPoint, SampleLoopEndPoint, LoopInRelease, AltLoopMode, XFadeLoopTime, XFadeLoopCurve, TimestretchActive, TimestretchAmount, TimestretchTracksBPM, TimestretchMode";
        let names: Vec<_> = LayerParam::ALL.iter().map(|p| p.enum_name()).collect();
        assert_eq!(names.join(", "), list);
        assert_eq!(LayerParam::FilterCutoff.name(), "Cutoff");
        assert_eq!(LayerParam::from_name("Resonance"), Some(LayerParam::FilterResonance));
        assert_eq!(LayerParam::AmpEnvHold as u8, 0x11);
        assert_eq!(LayerParam::TimestretchMode as u8, 0x29);
        assert_eq!(LayerParam::from_name("FilterCutoff"), Some(LayerParam::FilterCutoff));
        assert_eq!(TransmodSource::ALL.len(), 0x1d);
        // The original packs the per-source counts as bytes of 0x0302041705050605.
        let counts: Vec<usize> = ModulatorType::ALL.iter().map(|t| t.params().len()).collect();
        assert_eq!(counts, 0x0302_0417_0505_0605u64.to_le_bytes().map(usize::from));
        assert_eq!(ModulatorType::MiniSequencer.default_value(21), 0.9375);
        // Effect parameter counts from the original's table ([re:73854e]).
        let counts = [1, 2, 6, 4, 4, 2, 3, 5, 6, 6, 6, 5, 4, 3, 6, 6, 3, 6, 6, 6, 6, 9, 6, 3, 2, 4, 6, 4, 6, 4, 4, 4, 4, 4, 4, 4, 3, 6, 6, 5, 5, 3, 5, 6, 8, 2, 4];
        for (i, n) in counts.iter().enumerate() {
            assert_eq!(effect_param_names(i + 1).len(), *n, "{}", EFFECT_TYPES[i + 1]);
        }
        assert_eq!(step_lane_fx_param(42), Some((0, 0)));
        assert_eq!(step_lane_fx_param(56), Some((1, 5)));
        assert_eq!(step_lane_fx_param(48), None);
        assert_eq!(TransmodSource::Slot3.attr("Gain").as_deref(), Some("Gain_Slot3"));
    }

    #[test]
    fn unity_gain_is_zero_db() {
        assert!(value01_to_db(UNITY_GAIN).abs() < 1e-4);
        assert_eq!(value01_to_db(1.0), 6.0);
        assert_eq!(value01_to_db(0.0), -1000.0);
        assert!((db_to_value01(0.0) - UNITY_GAIN).abs() < 1e-6);
    }

    #[test]
    fn curves_round_trip() {
        let curves = [Curve::Decibels, PAN, PERCENT, SEMITONES, Curve::Cutoff, Curve::Stretch];
        for c in curves {
            // From 0.02: below ~0.0171 the -100 dB display floor is lossy, as in the original.
            for i in 2..100 {
                let x = i as f32 / 100.0;
                let back = c.from_units(c.to_units(x));
                assert!((back - x).abs() < 1e-4, "{c:?} {x} -> {back}");
            }
        }
        assert_eq!(Curve::Cutoff.to_units(0.5), 440.0);
        assert_eq!(Curve::Stretch.to_units(0.5), 100.0);
    }
}

#[cfg(test)]
mod option_tests {
    #[test]
    fn effect_options() {
        let k = super::effect_type_index("Vari-EQ").unwrap();
        assert_eq!(super::effect_option_strings(k, 0).unwrap()[1], "Low Shelf");
        assert!(super::effect_option_strings(k, 1).is_none());
        assert_eq!(super::effect_option_strings(super::effect_type_index("Maul").unwrap(), 0).unwrap().len(), 33);
        assert_eq!(super::effect_option_strings(0x1f, 1).unwrap()[21], "N2+H1 SEP");
    }
}

/// Effect menu category of each type (1..=47): 0 Distortion, 1 Dynamics,
/// 2 EQ, 3 Filters, 4 FX, 5 Reverbs.
const CATEGORY_OF: [u8; 47] = [1, 1, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 4, 4, 4, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 5, 5, 5, 5, 5, 5, 4, 0, 0];

/// The effect menu category of a type.
pub fn effect_category(kind: usize) -> usize {
    kind.checked_sub(1).and_then(|i| CATEGORY_OF.get(i)).map(|&c| c as usize).unwrap_or(4)
}

/// The full effect menu: every type of category `c`, in type order.
pub fn effect_menu(c: usize) -> Vec<usize> {
    (1..EFFECT_TYPES.len()).filter(|&k| effect_category(k) == c).collect()
}
