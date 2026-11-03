//! [re:cc7e1d] subclasses: text → internal value (`ConvertFromText`,
//! asm-checked in [re:cbf488]..[re:3e015d]). Only the forms that appear in the
//! effect port tables are handled. See the dreist-re effects notes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
    Percentage,
    Switch,
    /// `sc_double` ([re:5379ec]).
    Float,
    Amp,
    AmpLog,
    Decibel,
    Frequency,
    FrequencyLog,
    FrequencyPitch,
    Gain18dB,
    Time,
    TimeLog,
    TimeLogFine,
    TimeCube,
    PitchRelative,
    DoubleLog,
    Int,
    Angle,
    Cube,
    /// [re:1e86bb]: `atan(dB/8)/1.19029`.
    SslDecibel,
    /// [re:8a91e8] (`ConvertFromText` [re:814520]): `−ln(max(q, 1e-6))`.
    SslQ,
    /// A node-specific converter; the node supplies the values.
    Custom,
}

/// Split `"12.5 kHz"` into a number and a lower-case unit with spaces removed.
fn split(s: &str) -> (Option<f32>, String) {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let t = t.to_ascii_lowercase();
    let end = t.char_indices().find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || ((*c == '-' || *c == '+') && *i == 0))).map(|(i, _)| i).unwrap_or(t.len());
    (t[..end].parse().ok(), t[end..].to_string())
}

fn is_minus_inf(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("-oo") || t.contains("-00")
}

impl Conv {
    /// [re:fc9225] value of a port bound given as text (string-form constructors).
    pub fn from_text(self, s: &str) -> f32 {
        let (num, unit) = split(s);
        let n = num.unwrap_or(0.0);
        let kilo = if unit.starts_with('k') { 1000.0 } else { 1.0 };
        let milli = if unit.starts_with("ms") { 0.001 } else { 1.0 };
        match self {
            Conv::Switch => match unit.as_str() {
                "on" => 1.0,
                _ => 0.0,
            },
            Conv::Percentage => n * 0.01,
            Conv::SslDecibel => {
                if is_minus_inf(s) {
                    -144.0
                } else {
                    ((n as f64 * 0.125).atan() / 1.19029) as f32
                }
            }
            Conv::SslQ => -n.max(1e-6).ln(),
            Conv::Amp => {
                if is_minus_inf(s) {
                    0.0
                } else {
                    10f32.powf(n / 20.0).powf(1.0 / 3.0)
                }
            }
            Conv::AmpLog => {
                if is_minus_inf(s) {
                    -5.933
                } else {
                    let a = 10f32.powf(n / 20.0);
                    ((a.max(0.0) + 0.016367754).ln() * 1.442695).clamp(-10.0, 10.0)
                }
            }
            Conv::Decibel => {
                if is_minus_inf(s) {
                    -144.0
                } else {
                    n
                }
            }
            Conv::Frequency => n * kilo,
            Conv::FrequencyLog => (n * kilo).ln() * 1.442695 - 1.0,
            Conv::FrequencyPitch => (n * kilo / 400.0).ln() * 1.442695 - 1.0,
            Conv::Gain18dB => (10f32.powf(n.clamp(-1000.0, 18.0) / 60.0) / 1.995085).clamp(0.0, 1.0),
            Conv::Time => (n * milli).max(1e-8),
            Conv::TimeLog | Conv::TimeLogFine => (n * milli).max(1e-10).ln() * 1.442695 + 1.0,
            Conv::TimeCube => ((n * milli) * 0.1).max(0.0).powf(1.0 / 3.0),
            Conv::PitchRelative => {
                if unit.starts_with("st") {
                    n * 0.083333336
                } else {
                    n.max(1e-12).ln() * 1.442695
                }
            }
            Conv::DoubleLog => n.ln() * 1.442695,
            Conv::Int => n.round(),
            Conv::Angle => n / 360.0,
            Conv::Cube => n.abs().powf(1.0 / 3.0) * n.signum(),
            Conv::Float | Conv::Custom => {
                if unit == "db" {
                    10f32.powf(n / 20.0)
                } else {
                    n.clamp(-1e8, 1e8)
                }
            }
        }
    }
}

/// [re:5192bb]: optional "note" suffix, an
/// expression (`"1/64"`, `"2"`), then `clamp(log2(4·max(x, 1e-10)), ±8)`.
pub fn bpm_from_text(s: &str) -> f32 {
    let t = s.to_ascii_lowercase();
    let t = t.trim().trim_end_matches("note").trim();
    let x = match t.split_once('/') {
        Some((a, b)) => a.trim().parse::<f32>().unwrap_or(0.0) / b.trim().parse::<f32>().unwrap_or(1.0),
        None => t.parse::<f32>().unwrap_or(0.0),
    };
    ((x.max(1e-10) * 4.0).ln() * 1.442695).clamp(-8.0, 8.0)
}

/// [re:b70feb] (seconds).
/// Mode 0 is time (`exp2(v − 1)` s); mode 1 is note length in beats with
/// thirds (dotted/triplet steps of log2(0.75)), scaled by `unit` (60/bpm).
pub fn time_beat_to_units(v: f32, mode: f32, unit: f32) -> f32 {
    let m = (((mode.abs() + 1.0) - (mode - 1.0).abs()) * 0.5).round();
    if m != 1.0 {
        return if v.abs() >= 8.0 { if v > -8.0 { 1e10 } else { 1e-10 } } else { (v - 1.0).exp2() };
    }
    if v.abs() >= 8.0 {
        return if v < 8.0 { 1e-10 } else { 1e10 } * unit;
    }
    let half = if v > -0.0 { -0.5 } else { 0.5 };
    let q = (half - v) as i32 as f32;
    let f = (-v - q) * 3.0;
    let r = (f + if f < 0.0 { -0.5 } else { 0.5 }) as i32 as f32;
    (r * -0.415037513 - q).exp2() * unit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converters() {
        assert!((Conv::AmpLog.from_text("0 dB") - 0.023420).abs() < 1e-4);
        assert_eq!(Conv::AmpLog.from_text("-oo dB"), -5.933);
        assert!((Conv::Amp.from_text("18 dB") - 1.99526).abs() < 1e-4);
        assert_eq!(Conv::Amp.from_text("-oo dB"), 0.0);
        assert!((Conv::FrequencyLog.from_text("1KHz") - (1000f32.log2() - 1.0)).abs() < 1e-4);
        assert!((Conv::TimeLog.from_text("100ms") - (0.1f32.log2() + 1.0)).abs() < 1e-4);
        assert_eq!(Conv::Switch.from_text("On"), 1.0);
        assert_eq!(Conv::Percentage.from_text("100%"), 1.0);
        assert!((Conv::SslDecibel.from_text("16.5dB") - (2.0625f64.atan() / 1.19029) as f32).abs() < 1e-6);
        assert!((Conv::SslQ.from_text("2.5") + 2.5f32.ln()).abs() < 1e-6);
        assert!((Conv::PitchRelative.from_text("12 st") - 1.0).abs() < 1e-6);
        assert_eq!(bpm_from_text("1/64 note"), -4.0);
        assert_eq!(bpm_from_text("2 note"), 3.0);
        assert_eq!(time_beat_to_units(-4.0, 1.0, 0.5), 0.5 / 16.0);
        assert_eq!(time_beat_to_units(1.0, 0.0, 0.5), 1.0);
    }
}
