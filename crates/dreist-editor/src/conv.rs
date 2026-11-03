//! Value texts of [re:0c929e] (knob tooltips) and the generic
//! tooltip providers.
//!
//! [re:48679f]: the name (or
//! the description function's text), ": " unless it already has a ':',
//! the value text, always a space, then the suffix — so "Pan: C " keeps a
//! trailing space and percentages read "Mix: 050 %".

use dreist_formats::LayerParam;

/// A number typed into a value box (or read from a text file): finite
/// only, since Rust's parser also takes "inf" and "NaN".
pub fn number(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// [`number`] in double precision.
pub fn number64(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// C `%.Nd`: at least `n` digits, sign only when negative.
fn digits(v: i64, n: usize) -> String {
    let s = format!("{:0n$}", v.unsigned_abs());
    if v < 0 { format!("-{s}") } else { s }
}

/// C `%+.Nd`.
fn signed_digits(v: i64, n: usize) -> String {
    let s = format!("{:0n$}", v.unsigned_abs());
    if v < 0 { format!("-{s}") } else { format!("+{s}") }
}

/// A value converter: its value text and suffix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Conv {
    /// Percent [re:ae320e]: `%.3d` of ⌊100v⌋, "%".
    Percent,
    /// Depth [re:9d4b8d]: `%+03d` of ⌊200v − 100⌋, "%".
    Depth,
    /// On / off [re:7c2c57].
    OnOff,
    /// Pan [re:843664]: L / R / C at the ends and centre, else
    /// `%+03d` of ⌊200v − 100⌋.
    Pan,
    /// Decibels [re:81a04e]: 20·log10(2v³) ≤ 6, `%0.2f`, "-∞"
    /// below −100, "dB".
    Decibels,
    /// Semitones [re:33e2e0]: `%+d` semitones.
    Semitones,
    /// Layer octave ([re:40a771]).
    Octaves,
    /// Layer fine tune: `%+.3d` cents.
    Cents,
    /// Layer pre-delay: note values, or `%+.3d` msec.
    PreDelay { msec: bool },
    /// Layer cutoff: Hz / KHz.
    Cutoff,
    /// Envelope stage seconds: v² · max.
    Seconds(f32),
    /// Time stretch.
    Stretch,
    /// Degrees [re:663133]: `%03d` of ⌊360v⌋, "°".
    Degrees,
    /// AD envelope stages ([re:aa1c09]
    /// [re:b0d075]): `%0.2f` of 5.5v, "secs".
    AdSecs,
    /// Sequencer step count ([re:d967c0]): `%d` of ⌊15v + 1⌋.
    Steps,
}

const PREDELAY_NOTES: [&str; 16] = ["Off", "1/128d", "1/128", "1/128t", "1/64d", "1/64", "1/64t", "1/32d", "1/32", "1/32t", "1/16d", "1/16", "1/16t", "1/8d", "1/8", "1/8t"];

impl Conv {
    pub fn value(self, v: f32) -> String {
        match self {
            Conv::Percent => digits((v * 100.0).floor() as i64, 3),
            Conv::Depth => format!("{:+03}", (v * 200.0 - 100.0).floor() as i64),
            Conv::OnOff => if v >= 0.5 { "On" } else { "Off" }.into(),
            Conv::Pan => {
                if v.abs() <= 1e-6 {
                    "L".into()
                } else if (v - 1.0).abs() <= 1e-6 {
                    "R".into()
                } else if (v - 0.5).abs() <= 1e-6 {
                    "C".into()
                } else {
                    format!("{:+03}", (v * 200.0 - 100.0).floor() as i64)
                }
            }
            Conv::Decibels => {
                let c = 2.0 * v * v * v;
                let db = if c.abs() < 1e-6 { -1000.0 } else { (20.0 * c.log10()).min(6.0) };
                if db < -100.0 { "-∞".into() } else { format!("{db:.2}") }
            }
            Conv::Semitones => format!("{:+}", dreist_dsp::convert::semitones(v, true) as i64),
            Conv::Octaves => format!("{:+}", dreist_dsp::convert::octave(v, true) as i64),
            Conv::Cents => signed_digits((dreist_dsp::convert::fine_semitones(v) * 100.0).floor() as i64, 3),
            Conv::PreDelay { msec: false } => PREDELAY_NOTES[((v * 15.0).floor().max(0.0) as usize).min(15)].into(),
            Conv::PreDelay { msec: true } => signed_digits((v * v * v * 1000.0) as i64, 3),
            Conv::Cutoff => {
                let f = 440.0 * 2f32.powf(12.0 * v - 6.0);
                if f >= 1000.0 { format!("{:.2}", f / 1000.0) } else { format!("{}", f.floor() as i64) }
            }
            Conv::Seconds(max) => format!("{:.2}", v * v * max),
            Conv::Stretch => format!("{:.2}", (if v < 0.5 { 2.0 * v } else { 14.0 * v - 6.0 }) * 100.0),
            Conv::Degrees => format!("{:03}", (v * 360.0).floor() as i64),
            Conv::AdSecs => format!("{:.2}", v * 5.5),
            Conv::Steps => format!("{}", (15.0 * v + 1.0).floor() as i64),
        }
    }

    pub fn suffix(self, v: f32) -> &'static str {
        match self {
            Conv::Percent | Conv::Depth | Conv::Stretch => "%",
            Conv::Decibels => "dB",
            Conv::Semitones => "semitones",
            Conv::Octaves => "octaves",
            Conv::Cents => "cents",
            Conv::PreDelay { msec: true } => "msec",
            Conv::Cutoff => {
                if 440.0 * 2f32.powf(12.0 * v - 6.0) >= 1000.0 {
                    "KHz"
                } else {
                    "Hz"
                }
            }
            Conv::Seconds(_) | Conv::AdSecs => "secs",
            Conv::Degrees => "°",
            _ => "",
        }
    }

    /// Value, space, suffix.
    pub fn text(self, v: f32) -> String {
        format!("{} {}", self.value(v), self.suffix(v))
    }

    /// The value text under `name`.
    pub fn named(self, name: &str, v: f32) -> String {
        let mut s = name.to_string();
        if !s.is_empty() && !s.contains(':') {
            s.push_str(": ");
        }
        s + &self.text(v)
    }

    /// [re:046869].
    pub fn layer(p: LayerParam, predelay_msec: bool) -> Conv {
        match p as u8 {
            0 | 2 => Conv::Decibels,
            1 | 3 => Conv::Pan,
            4 => Conv::Octaves,
            5 => Conv::Semitones,
            6 => Conv::Cents,
            7 => Conv::PreDelay { msec: predelay_msec },
            8 => Conv::Cutoff,
            0xd | 0x22 | 0x23 | 0x26 | 0x28 | 0x29 => Conv::OnOff,
            0xe | 0x16 | 0xf | 0x17 => Conv::Seconds(10.0),
            0x11 | 0x19 => Conv::Seconds(60.0),
            0x12 | 0x1a => Conv::Seconds(15.0),
            0x13..=0x15 | 0x1b..=0x1d | 0x25 => Conv::Depth,
            0x27 => Conv::Stretch,
            _ => Conv::Percent,
        }
    }

    /// [re:7274d9] (type, param).
    pub fn modulator(t: dreist_formats::params::ModulatorType, param: &str) -> Conv {
        use dreist_formats::params::ModulatorType as M;
        match (t, param) {
            (_, "Sync" | "Drunk" | "Reverse" | "Trig") => Conv::OnOff,
            (M::LFO | M::MorphingLFO | M::SampleAndHold, "Phase") => Conv::Degrees,
            (M::MorphingLFO, "Swing") | (M::ADEnvelope, "AttackCurve" | "DecayCurve") => Conv::Depth,
            (M::ADEnvelope, "Attack" | "Decay") => Conv::AdSecs,
            (M::MiniSequencer, "NumberOfSteps") => Conv::Steps,
            _ => Conv::Percent,
        }
    }
}

/// A transmod knob's text ([re:7eb9d2] and the
/// other panels): with a source shown the description is "<source> ->
/// <name>:\n", and a non-zero depth adds " -> " and the modulated value.
pub fn transmod_text(conv: Conv, name: &str, v: f32, source: Option<(&str, f32)>) -> String {
    match source {
        Some((src, d)) => {
            let mut t = conv.named(&format!("{src} -> {name}:\n"), v);
            if d.abs() >= 1e-6 {
                t.push_str(" -> ");
                t.push_str(&conv.text((v + d).clamp(0.0, 1.0)));
            }
            t
        }
        None => conv.named(name, v),
    }
}

/// [re:672ea0].
pub fn bool_text(name: &str, on: bool) -> String {
    format!("{name}: {}", if on { "On" } else { "Off" })
}

/// [re:c9d5c3] ("N: -3.52dB", no space).
pub fn db15_text(name: &str, v: f32) -> String {
    format!("{name}: {}", db15(v))
}

pub fn db15(v: f32) -> String {
    let c = 2.0 * v * v * v;
    let db = if c.abs() < 1e-6 { -1000.0 } else { (20.0 * c.log10()).min(6.0) };
    if db >= -100.0 { format!("{db:.2}dB") } else { "-∞dB".into() }
}

/// [re:d73792].
pub fn long_percent_text(name: &str, v: f32) -> String {
    format!("{name}: {:03}%", (v * 100.0) as i64)
}

/// [re:0ce4fd].
pub fn depth_text(name: &str, v: f32) -> String {
    format!("{name}: {:+04}%", ((2.0 * v - 1.0) * 100.0) as i64)
}

/// [re:4f4da9].
pub fn midi_text(name: &str, v: f32) -> String {
    format!("{name}: {:03}", (v * 128.0) as i64)
}

/// [re:5b3fd8].
#[allow(dead_code)]
pub fn pan_text(v: f32) -> String {
    let p = (v.clamp(0.0, 1.0) * 200.0 - 100.0) as i64;
    match p {
        0 => "C".into(),
        p if p < 0 => format!("{:3}% L", -p),
        p => format!("{p:3}% R"),
    }
}

/// [re:7a7913] (natural log, kept).
#[allow(dead_code)]
pub fn db_unity(v: f32) -> String {
    let db = if v > 1.9287e-22 { 20.0 * v.ln() } else { -1000.0 };
    if db < -100.0 { "-oodB".into() } else { format!("{db:.2}dB") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_texts() {
        assert_eq!(Conv::Percent.named("Mix", 0.5), "Mix: 050 %");
        assert_eq!(Conv::Pan.named("Pan", 0.5), "Pan: C ");
        assert_eq!(Conv::Depth.named("Variance", 0.75), "Variance: +50 %");
        assert_eq!(Conv::Depth.value(0.5), "+00");
        assert_eq!(Conv::Decibels.value(0.0), "-∞");
        assert_eq!(Conv::Decibels.named("Gain", 1.0), "Gain: 6.00 dB");
        assert_eq!(Conv::OnOff.named("Reverse", 1.0), "Reverse: On ");
        assert_eq!(Conv::Cutoff.named("Filter Cutoff", 0.5), "Filter Cutoff: 440 Hz");
        assert_eq!(Conv::Cutoff.text(1.0), "28.16 KHz");
        assert_eq!(Conv::PreDelay { msec: false }.value(0.5), "1/32d");
        assert_eq!(Conv::Seconds(10.0).text(0.5), "2.50 secs");
        assert_eq!(transmod_text(Conv::Percent, "Cutoff", 0.5, Some(("LFO 1", 0.25))), "LFO 1 -> Cutoff:\n050 % -> 075 %");
        assert_eq!(pan_text(0.25), " 50% L");
        assert_eq!(db15(0.0), "-∞dB");
        assert_eq!(depth_text("Scale", 0.75), "Scale: +050%");
        assert_eq!(digits(-5, 3), "-005");
        use dreist_formats::params::ModulatorType as M;
        assert_eq!(Conv::modulator(M::LFO, "Phase").named("Phase", 0.25), "Phase: 090 °");
        assert_eq!(Conv::modulator(M::ADEnvelope, "Attack").named("Attack", 1.0), "Attack: 5.50 secs");
        assert_eq!(Conv::modulator(M::MiniSequencer, "NumberOfSteps").named("Number Of Steps", 1.0), "Number Of Steps: 16 ");
    }
}
