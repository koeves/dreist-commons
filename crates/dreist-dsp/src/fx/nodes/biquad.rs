//! [re:6735da] ([re:c76d33]): RBJ-cookbook biquads in double
//! precision, recomputed only when mode/frequency/Q/gain change; a mode
//! change clears the history.

#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    key: Option<(i32, f32, f32, f32)>,
}

/// [re:bf85e4]: Bell, Low Shelf, High Shelf, LPF, BPF, BPF 0dB, HPF, Notch, Allpass.
pub const EQ_MODES: usize = 9;

impl Biquad {
    /// `SetCoeffs(mode, f/fs, Q, gain dB)`.
    pub fn set(&mut self, mode: i32, f: f32, q: f32, gain: f32) {
        if let Some((m, a, b, c)) = self.key {
            if m == mode && a == f && b == q && c == gain {
                return;
            }
            if m != mode {
                *self = Biquad::default();
            }
        } else {
            *self = Biquad::default();
        }
        self.key = Some((mode, f, q, gain));
        let fd = f as f64;
        let w = (((fd - 9.999999747378752e-06).abs() + 9.999999747378752e-06 + 0.44999998807907104) - (fd - 0.44999998807907104).abs()) * 0.5 * std::f64::consts::TAU;
        let (sn, cs) = (w.sin(), w.cos());
        if !(0..=8).contains(&mode) {
            return;
        }
        let qd = q as f64;
        let q = (((qd - 0.5).abs() + 0.5 + 1000.0) - (qd - 1000.0).abs()) * 0.5;
        let a_gain = || 10f32.powf(gain / 40.0) as f64;
        match mode {
            0 => {
                let al = sn / (q + q);
                let a = a_gain();
                let d = al / a;
                let n = 1.0 / (d + 1.0);
                let au = al * a;
                self.b0 = (au + 1.0) * n;
                self.b1 = n * cs * -2.0;
                self.b2 = n * (1.0 - au);
                self.a1 = self.b1;
                self.a2 = (1.0 - d) * n;
            }
            1 => {
                let a = a_gain();
                let (ap, am) = (a + 1.0, a - 1.0);
                let s = (a.sqrt() / q) * sn;
                let d7 = ap + am * cs;
                let n = 1.0 / (s + d7);
                let d9 = ap - am * cs;
                self.b0 = a * (s + d9) * n;
                self.b1 = (am - ap * cs) * (a + a) * n;
                self.b2 = (d9 - s) * a * n;
                self.a1 = (ap * cs + am) * -2.0 * n;
                self.a2 = (d7 - s) * n;
            }
            2 => {
                let a = a_gain();
                let (ap, am) = (a + 1.0, a - 1.0);
                let d7 = ap - cs * am;
                let s = sn * (a.sqrt() / q);
                let n = 1.0 / (s + d7);
                let d9 = cs * am + ap;
                self.b0 = a * (s + d9) * n;
                self.b1 = a * -2.0 * (cs * ap + am) * n;
                self.b2 = a * (d9 - s) * n;
                self.a1 = (am - cs * ap) * 2.0 * n;
                self.a2 = (d7 - s) * n;
            }
            3 | 6 => {
                let al = sn / (q + q);
                let n = 1.0 / (al + 1.0);
                let k = if mode == 3 { n * (1.0 - cs) } else { n * (cs + 1.0) };
                self.b0 = k * 0.5;
                self.b1 = if mode == 3 { k } else { -k };
                self.b2 = k * 0.5;
                self.a1 = n * cs * -2.0;
                self.a2 = (1.0 - al) * n;
            }
            4 => {
                let al = sn / (q + q);
                let n = 1.0 / (al + 1.0);
                self.b0 = sn * 0.5 * n;
                self.b1 = n * 0.0;
                self.b2 = sn * -0.5 * n;
                self.a1 = cs * -2.0 * n;
                self.a2 = (1.0 - al) * n;
            }
            5 => {
                let al = sn / (q + q);
                let n = 1.0 / (al + 1.0);
                self.b0 = al * n;
                self.b1 = n * 0.0;
                self.b2 = -(al * n);
                self.a1 = cs * -2.0 * n;
                self.a2 = (1.0 - al) * n;
            }
            7 => {
                let al = sn / (q + q);
                let n = 1.0 / (al + 1.0);
                self.b0 = n;
                self.b1 = n * cs * -2.0;
                self.b2 = n;
                self.a1 = self.b1;
                self.a2 = n * (1.0 - al);
            }
            _ => {
                let al = sn / (q + q);
                let d = al + 1.0;
                let n = 1.0 / d;
                self.b0 = (1.0 - al) * n;
                self.b1 = n * cs * -2.0;
                self.b2 = n * d;
                self.a1 = self.b1;
                self.a2 = (1.0 - al) * n;
            }
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let xd = x as f64;
        let y = ((self.b2 * self.x2 + xd * self.b0 + self.b1 * self.x1) - self.a1 * self.y1) - self.y2 * self.a2;
        self.x2 = self.x1;
        self.x1 = xd;
        self.y2 = self.y1;
        self.y1 = y;
        y as f32
    }
}
