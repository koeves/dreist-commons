//! Lookup tables built at load time by the original ([re:a770d0]
//! [re:fddb1b], called from `__GLOBAL__sub_I_VAnalog.cpp` with the constants
//! below; [re:cdebed]), and the shared table reader.

use std::sync::OnceLock;

/// 2049 entries over `x = (i - 1024) / 32`, i.e. -32..=32.
pub const ANALOG_LEN: usize = 2049;

/// [re:1cff01] arguments.
const P1: f32 = 5.09e-9;
const P2: f32 = 3.07e-9;
const P3: f32 = 0.025;
const P4: f32 = 0.0245;
const P5: f32 = 5.0726e-9;
const P6: f32 = 4.234e-9;
const P7: f32 = 5.1e-9;
const P8: f32 = 3.0e-9;

pub struct AnalogTables {
    /// `ms_tanhlim_tab`: `3.3·tanh(x/3.3)`.
    pub tanhlim: Vec<f32>,
    /// `ms_diodesvflim_tab`: diode clipper solved by Newton iteration.
    pub diodesvflim: Vec<f32>,
    /// `ms_otalim_s_tab`: OTA transfer, scaled by 32.
    pub otalim_s: Vec<f32>,
    /// `ms_otalim_tab`: OTA transfer.
    pub otalim: Vec<f32>,
    /// `ms_otalim1_tab`: OTA transfer on the unscaled input.
    pub otalim1: Vec<f32>,
    /// `ms_diodelim_tab`: diode pair clipper.
    pub diodelim: Vec<f32>,
    /// `ms_diodelim_s_tab`: the same, scaled by 32 before the division.
    pub diodelim_s: Vec<f32>,
    /// `ms_diodeonelim_tab`: single diode against the forward rail.
    pub diodeonelim: Vec<f32>,
    /// `ms_jfetlim_tab`: square-law JFET into the reverse diode.
    pub jfetlim: Vec<f32>,
    /// `ms_transisterlim_tab`: diode with a 6× junction slope.
    pub transisterlim: Vec<f32>,
    /// `ms_mooglim_tab`: OTA-style transfer with the P7/P8 pair.
    pub mooglim: Vec<f32>,
}

/// Build every shared table now (they are otherwise built on first use).
pub fn warm() {
    analog();
    sin_tab();
    tri_tab();
    crate::modulators::asin_tab();
}

pub fn analog() -> &'static AnalogTables {
    static T: OnceLock<AnalogTables> = OnceLock::new();
    T.get_or_init(build_analog)
}

/// Newton solve shared by the diode tables: `f(y) = s·ln(y + p) + k·y − c ± x`,
/// started from y = 1e-12 (where the original folds `1e-12·k` into the literal
/// `k0`), at most five steps, stopping once a step moves less than 0.1 %.
#[allow(clippy::too_many_arguments)]
fn diode_newton(x: f32, s: f32, p: f32, c: f32, lnp: f32, k: f32, k0: f32, negative: bool) -> f32 {
    let signed = |v: f32| if negative { v + x } else { v - x };
    let mut y = (1e-12 - signed((lnp * s + k0) - c) / (s / (p + 1e-12) + k)).max(0.0);
    let mut prev = 1e-12f32;
    for _ in 0..4 {
        if (y - prev).abs() < y * 0.001 {
            break;
        }
        let next = (y - signed(((y + p).ln() * s + y * k) - c) / (s / (y + p) + k)).max(0.0);
        prev = y;
        y = next;
    }
    y
}

fn build_analog() -> AnalogTables {
    let s2 = P4 + P4;
    let c2 = P2.ln() * s2;
    let s1 = P3 + P3;
    let c1 = P1.ln() * s1;
    let a = (P5 + P5) / (P5 + P6);
    let b = (P6 + P6) / (P5 + P6);
    let ln2 = (P2 + 1e-12).ln();
    let ln1 = (P1 + 1e-12).ln();

    let mut tanhlim = Vec::with_capacity(ANALOG_LEN);
    let mut diodesvflim = Vec::with_capacity(ANALOG_LEN);
    let mut otalim_s = Vec::with_capacity(ANALOG_LEN);
    let mut otalim = Vec::with_capacity(ANALOG_LEN);
    let mut otalim1 = Vec::with_capacity(ANALOG_LEN);
    let mut diodelim = Vec::with_capacity(ANALOG_LEN);
    let mut diodelim_s = Vec::with_capacity(ANALOG_LEN);
    let mut diodeonelim = Vec::with_capacity(ANALOG_LEN);
    let mut jfetlim = Vec::with_capacity(ANALOG_LEN);
    let mut transisterlim = Vec::with_capacity(ANALOG_LEN);
    let mut mooglim = Vec::with_capacity(ANALOG_LEN);
    let s6 = P3 * 6.0;
    let c6 = P1.ln() * s6;
    let a7 = (P7 + P7) / (P7 + P8);
    let b7 = (P8 + P8) / (P7 + P8);
    const K1: f32 = 4077.67;
    const K1_0: f32 = 4.0776698e-9;
    for i in -1024i32..=1024 {
        let x = i as f32 * 0.03125;

        let xd = x * 0.7281553;
        let v = if xd >= 0.0 { diode_newton(xd, s1, P1, c1, ln1, K1, K1_0, false) * -K1 + xd } else { -(diode_newton(xd, s2, P2, c2, ln2, K1, K1_0, true) * -K1 - xd) };
        diodelim.push(v / 0.7281553);
        diodelim_s.push((v * 32.0) / 0.7281553);

        let h = x * 0.5;
        let d = if h <= 0.0 { diode_newton(h, s2, P2, c2, ln2, 1500.0, 1.5e-9, true) * -1000.0 } else { diode_newton(h, s1, P1, c1, ln1, 1500.0, 1.5e-9, false) * 1000.0 };
        diodesvflim.push(d + d);

        let xc = x.min(2.0);
        let v21 = (xc * xc * -0.25 + xc) * 0.7281553;
        let f20 = -4.0 - v21;
        let y = diode_newton(-f20, s2, P2, c2, ln2, K1, K1_0, true);
        jfetlim.push((y * -K1 - v21) / -0.7281553);

        let y = diode_newton(xd, s6, P1, c6, ln1, K1, K1_0, false);
        transisterlim.push((xd + y * -K1) / 0.7281553);

        let xn = x * -0.7281553;
        let y = diode_newton(xn, s1, P1, c1, ln1, K1, K1_0, false);
        diodeonelim.push((xn + y * -K1) / -0.7281553);

        let u = x / 3.3;
        let v = 1.0 / (b * (u * -2.0).exp() + a) - 1.0 / (a * (u + u).exp() + b);
        otalim.push(v * 3.3);
        otalim_s.push(v * 32.0 * 3.3);
        let o1 = 1.0 / ((x * -2.0).exp() * b + a) - 1.0 / ((x + x).exp() * a + b);
        otalim1.push(o1);
        tanhlim.push(u.tanh() * 3.3);
        let m = 1.0 / ((u * -2.0).exp() * b7 + a7) - 1.0 / ((u + u).exp() * a7 + b7);
        mooglim.push(m * 3.3);
    }
    AnalogTables { tanhlim, diodesvflim, otalim_s, otalim, otalim1, diodelim, diodelim_s, diodeonelim, jfetlim, transisterlim, mooglim }
}

/// [re:19db55] (5121 entries): `0.5·sin(2π·frac(k/2048 − 0.5))`.
pub fn sin_tab() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..0x1401)
            .map(|k| {
                let x = k as f32 * 0.00048828125 - 0.5;
                ((x - x.floor()) * 6.2831855).sin() * 0.5
            })
            .collect()
    })
}

/// [re:bb0ede] (5122 entries): twice the folded triangle of
/// `frac(k/2048 − 0.5)`.
pub fn tri_tab() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..0x1402)
            .map(|k| {
                let x = k as f32 * 0.00048828125 - 0.5;
                let x = x - x.floor();
                let x = if x < 0.75 { x } else { x - 1.0 };
                let t = if x < 0.25 { x } else { 0.5 - x };
                t + t
            })
            .collect()
    })
}

/// Table read used by the analog filters: integer part saturated like
/// `cvttps2dq` + `packssdw`, offset by 1024 (16-bit wrap), clamped to
/// `0..=2047`, then linear interpolation with the next entry by `v - trunc(v)`.
/// Returns `(index, frac)` so callers can read several tables at one point.
#[inline]
pub fn lut_pos(v: f32) -> (usize, f32) {
    let t = if v.is_nan() || v >= 2147483648.0 || v < -2147483648.0 { i32::MIN } else { v as i32 };
    let s = t.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let i = s.wrapping_add(0x400).min(0x7ff).max(0) as usize;
    (i, v - t as f32)
}

#[inline]
pub fn lerp(tab: &[f32], (i, f): (usize, f32)) -> f32 {
    (tab[i + 1] - tab[i]) * f + tab[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_odd_and_saturating() {
        let t = analog();
        assert_eq!(t.tanhlim[1024], 0.0);
        assert!((t.tanhlim[1024 + 32] - 3.3 * (1.0f32 / 3.3).tanh()).abs() < 1e-6);
        assert!(t.tanhlim[2048] <= 3.3 && t.tanhlim[2048] > 3.2);
        // OTA curve is odd and slope 1 at the origin (×32).
        assert!(t.otalim_s[1024].abs() < 1e-5);
        assert!((t.otalim_s[1025] - 1.0).abs() < 0.01, "{}", t.otalim_s[1025]);
        // Diode table is monotonic and centred.
        assert!(t.diodesvflim[1024].abs() < 1e-3);
        assert!(t.diodesvflim.windows(2).all(|w| w[1] >= w[0]));
        assert!(t.diodelim[1024].abs() < 1e-3);
        assert!(t.diodelim.windows(2).all(|w| w[1] >= w[0]));
        assert!((t.otalim[1056] * 32.0 - t.otalim_s[1056]).abs() < 1e-4);
    }

    #[test]
    fn sin_table_quarter_wave() {
        let s = sin_tab();
        assert_eq!(s[1024], 0.0);
        assert!((s[1024 + 512] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn lut_clamps() {
        assert_eq!(lut_pos(0.5), (1024, 0.5));
        assert_eq!(lut_pos(-0.5).0, 1024);
        assert_eq!(lut_pos(2000.0).0, 0x7ff);
        assert_eq!(lut_pos(-2000.0).0, 0);
        // Quirk kept from the original: the 16-bit add wraps past 31743.
        assert_eq!(lut_pos(31743.0).0, 0x7ff);
        assert_eq!(lut_pos(40000.0).0, 0);
    }
}
