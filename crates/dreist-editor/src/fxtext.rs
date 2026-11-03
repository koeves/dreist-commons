//! [re:81c8ad] of the effect node ports (the effect
//! parameter knobs' tooltip values); the table is in `fxtext_table.rs`.

// Generated table: kept as written.
#[rustfmt::skip]
#[path = "fxtext_table.rs"]
mod table;

use table::{C, Fx, TABLE};

/// `f2s` [re:288cb0]: N decimals, rounded half up (in double), '+' on
/// request, no "-0".
fn f2s(x: f64, n: usize, plus: bool) -> String {
    let k = (x * 10f64.powi(n as i32) + 0.5).floor() as i64;
    let digits = format!("{:0w$}", k.unsigned_abs(), w = n + 1);
    let (int, frac) = digits.split_at(digits.len() - n);
    let body = if n == 0 { int.to_string() } else { format!("{int}.{frac}") };
    if k < 0 {
        format!("-{body}")
    } else if plus {
        format!("+{body}")
    } else {
        body
    }
}

/// `time2s` [re:bf5efa].
fn time2s(t: f32) -> String {
    if t >= 1.0 { format!("{} s", f2s(t as f64, 2, false)) } else { format!("{} ms", f2s((t * 1000.0) as f64, 2, false)) }
}

fn freq(f: f32) -> String {
    if f >= 1000.0 { format!("{} kHz", f2s((f * 0.001) as f64, 2, false)) } else { format!("{} Hz", f2s(f as f64, 2, false)) }
}

fn db_amp(a: f32) -> String {
    if a <= 1e-5 { "-oo dB".into() } else { format!("{} dB", f2s((20.0 * a.log10()) as f64, 1, false)) }
}

/// SI prefix of [re:df3abf]: the scaled value and
/// prefix ('\0' when none, which ends the C string).
fn si(f: f32) -> (f32, Option<char>) {
    let e = (f.abs() + 1e-10).log10().floor();
    if e <= -3.0 {
        (f * 1000.0, Some('m'))
    } else if (3.0..=5.0).contains(&e) {
        (f * 0.001, Some('k'))
    } else if e >= 6.0 {
        (f * 1e-6, Some('M'))
    } else {
        (f, None)
    }
}

fn round_away(x: f32) -> i32 {
    (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32
}

/// Beat power text [re:729835] + " note" ([re:d4b3a1]).
fn beat_note(x: f32) -> String {
    if x <= -8.0 {
        return "0 note".into();
    }
    let f = x - 2.0;
    let q = round_away(f);
    let i = round_away((f - q as f32) * 3.0);
    let (q, suffix) = match i {
        1 => (q + 1, "t"),
        -1 => (q - 1, "d"),
        _ => (q, ""),
    };
    let n = round_away((q.unsigned_abs() as f32).exp2());
    format!("{}{n}{suffix} note", if q < 0 { "1/" } else { "" })
}

fn conv(c: C, x: f32) -> String {
    match c {
        C::Percentage => format!("{} %", f2s((x * 100.0) as f64, 0, false)),
        C::Float => {
            if x.abs() < 1e-8 {
                "0".into()
            } else {
                f2s(x as f64, 2, false)
            }
        }
        C::Switch => if x >= 0.5 { "on" } else { "off" }.into(),
        C::Amp => db_amp(x * x * x),
        C::AmpLog => db_amp(((x.clamp(-10.0, 10.0)).exp2() - 0.016_367_754).max(0.0)),
        C::Decibel => {
            if x <= -144.0 {
                "-oo dB".into()
            } else {
                format!("{} dB", f2s(x as f64, 1, false))
            }
        }
        C::Frequency => freq(x),
        C::FrequencyLog => freq((x + 1.0).exp2()),
        C::FrequencyPitch => freq((x + 1.0).exp2() * 400.0),
        C::Gain18dB => {
            let a = (1.5835 * x).powi(3);
            if (2.0 * a).abs() < 1e-6 {
                return "-∞dB".into();
            }
            let d = (20.0 * a.log10()).min(18.0);
            if d < -100.0 { "-∞dB".into() } else { format!("{d:.2}dB") }
        }
        C::Time => time2s(x),
        C::TimeLog | C::TimeLogFine => time2s((x - 1.0).exp2()),
        C::PitchRelative => format!("{} st", f2s((12.0 * x) as f64, 2, true)),
        C::DoubleLog => {
            let (f, p) = si(x.exp2());
            match p {
                Some(c) => format!("{} {c}", f2s(f as f64, 2, false)),
                None => format!("{} ", f2s(f as f64, 2, false)),
            }
        }
        C::Angle => format!("{} deg", f2s((360.0 * x) as f64, 0, false)),
        C::SslDecibel => {
            let d = (8.0 * (1.19029 * x as f64).tan()) as f32;
            if d < -144.0 {
                "-oo dB".into()
            } else if d.abs() < 1e-8 {
                "0 dB".into()
            } else {
                format!("{d:.4} dB")
            }
        }
        C::SslQ => format!("{:.4}", (-x).exp()),
    }
}

/// The value text of front parameter `p` of effect `kind` at `v`;
/// `bpm_sync` is the Delay / Bloom Sync port (BPM when true).
pub fn text(kind: usize, p: usize, v: f32, bpm_sync: bool) -> Option<String> {
    let &(_, _, lo, hi, fx) = TABLE.iter().find(|r| r.0 == kind && r.1 == p)?;
    let x = (lo as f64 + (hi as f64 - lo as f64) * v as f64) as f32;
    Some(match fx {
        Fx::Conv(c) => conv(c, x),
        Fx::Enum(items) => items[((x.clamp(0.0, (items.len() - 1) as f32)) + 0.5) as usize].to_string(),
        Fx::Scaled(min, max, unit, dec) => {
            let f = min + (max - min) * x;
            if f.abs() < 1e-24 {
                format!("0{unit}")
            } else {
                match si(f) {
                    (g, Some(c)) => format!("{} {c}{unit}", f2s(g as f64, dec, false)),
                    (g, None) => format!("{} ", f2s(g as f64, dec, false)),
                }
            }
        }
        Fx::TimeBeat { synced: true } if bpm_sync => beat_note(x),
        Fx::TimeBeat { .. } => time2s(if x.abs() < 8.0 {
            (x - 1.0).exp2()
        } else if x > -8.0 {
            1e10
        } else {
            1e-10
        }),
        Fx::BreverbTime(a, b) => format!("{:.2} s", a * (b / a).powf(v)),
        Fx::BreverbPre => format!("{:.2} s", 0.5 * v),
        Fx::Td(scale) => format!("{:.2} dB", (v - 0.5) * scale),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_texts_match_the_samples() {
        assert_eq!(text(1, 0, 0.0, true).as_deref(), Some("-oo dB"));
        assert_eq!(text(1, 0, 1.0, true).as_deref(), Some("36.0 dB"));
        assert_eq!(text(3, 1, 0.5, true).as_deref(), Some("400.00 Hz"));
        assert_eq!(text(3, 0, 0.0, true).as_deref(), Some("2.5000"));
        assert_eq!(text(5, 0, 0.5, true).as_deref(), Some("BPF 0dB"));
        assert_eq!(text(5, 2, 1.0, true).as_deref(), Some("16.00 "));
        assert_eq!(text(8, 2, 0.5, true).as_deref(), Some("244.95 ms"));
        assert_eq!(text(9, 0, 0.5, true).as_deref(), Some("-5.000 "));
        assert_eq!(text(15, 0, 0.0, true).as_deref(), Some("1/64 note"));
        assert_eq!(text(15, 0, 0.5, true).as_deref(), Some("1/8 note"));
        assert_eq!(text(15, 0, 1.0, true).as_deref(), Some("2 note"));
        assert_eq!(text(15, 0, 0.5, false).as_deref(), Some("353.55 ms"));
        assert_eq!(text(39, 0, 1.0, true).as_deref(), Some("20.00 s"));
        assert_eq!(text(12, 1, 0.0, true).as_deref(), Some("-24.00 dB"));
        assert_eq!(f2s(0.0, 2, true), "+0.00");
        assert_eq!(f2s(-0.001, 2, false), "0.00");
    }
}
