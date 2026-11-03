//! Layer envelopes. `Envelope` is the ADSR+hold used by every playback mode
//! except `OneShotLengthBound`, which uses `AhrEnvelope` scaled to the sample
//! length ([re:5b6361]).

/// Final normalized envelope inputs (modulated value plus step offset, clamped
/// to `0..=1`), in Layer parameter order: `AmpEnvAttack .. AmpEnvReleaseCurve`
/// or the `FreeEnv*` equivalents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopeParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub hold: f32,
    pub release: f32,
    pub attack_curve: f32,
    pub decay_curve: f32,
    pub release_curve: f32,
}

/// Coefficients of the curve shared by all three ADSR segments
/// ([re:0fc460]). A centred curve is linear.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Curve {
    a: f32,
    b: f32,
    c: f32,
}

impl Curve {
    const LINEAR: Curve = Curve { a: 0.0, b: 1.0, c: 0.0 };

    /// `k` is `c - 0.5` for attack and `0.5 - c` for decay/release, with the
    /// sign of `a`/`b` flipped between the two ([re:58aad4]).
    fn new(k: f32, attack: bool) -> Curve {
        if k.abs() < 1e-6 {
            return Curve::LINEAR;
        }
        let s = if attack { 1.0 } else { -1.0 };
        Curve { a: k * -1.2000002 * s, b: k * 1.6970563 * s, c: -0.0 }
    }

    fn eval(self, x: f32) -> f32 {
        let d = (self.b - 2.0) * self.b + 1.0 + (self.c * -4.0 + x * -5.656854) * self.a;
        let root = if d == 0.0 { 0.0 } else { d.sqrt() };
        let t = (root + self.b - 1.0) / (self.a * -2.0);
        ((t + self.c + (self.a * t + self.b) * t) * 0.70710677).max(0.0)
    }
}

/// `Envelope` ([re:3e1d71] .. [re:9c0d73]).
#[derive(Debug, Clone)]
pub struct Envelope {
    sample_rate: f32,
    one_shot: bool,
    release_len: f32,
    attack_end: f32,
    decay_end: f32,
    hold_end: f32,
    sustain: f32,
    attack: (f32, f32, Curve),
    decay: (f32, f32, Curve),
    release: (f32, f32, Curve),
    time: f32,
    rate: f32,
    stage: u32,
    value: f32,
    last: f32,
    fade_step: f32,
}

impl Envelope {
    pub fn new(sample_rate: f32) -> Envelope {
        Envelope {
            sample_rate,
            one_shot: true,
            release_len: 0.0,
            attack_end: 0.0,
            decay_end: 0.0,
            hold_end: 0.0,
            sustain: 0.0,
            attack: (0.0, 0.0, Curve::default()),
            decay: (0.0, 0.0, Curve::default()),
            release: (0.0, 0.0, Curve::default()),
            time: 0.0,
            rate: 1.0,
            stage: 0,
            value: 0.0,
            last: 0.0,
            fade_step: 0.0625,
        }
    }

    /// [re:97a3c3]. `one_shot` is `PlaybackMode < 2`: after decay the
    /// envelope holds sustain for the hold time and then releases by itself.
    /// `rate` is the time advance per sample (1 unless timestretch sets it).
    pub fn initialize(&mut self, p: &EnvelopeParams, one_shot: bool, rate: f32) {
        let pow4 = |x: f32| {
            let x = x * x;
            x * x
        };
        let sr = self.sample_rate;
        let attack = (pow4(p.attack) * (sr * 10.0)).floor().max(4.0);
        let decay = (pow4(p.decay) * (sr * 10.0)).floor();
        let release = (pow4(p.release) * (sr * 15.0)).floor().max(32.0);
        let hold = pow4(p.hold) * (sr * 60.0);
        self.rate = rate;
        self.one_shot = one_shot;
        self.sustain = p.sustain;
        self.attack_end = attack;
        self.decay_end = attack + decay;
        self.hold_end = attack + decay + hold;
        self.release_len = release;
        let inc = |len: f32, num: f32| if len > 1.0 { num / len } else { 1e-5 };
        self.attack = (inc(attack, 1.0) * rate, p.attack_curve, Curve::new(p.attack_curve - 0.5, true));
        self.decay = (inc(decay, 1.0) * rate, 1.0 - p.decay_curve, Curve::new(0.5 - p.decay_curve, false));
        self.release = (inc(release, p.sustain) * rate, 1.0 - p.release_curve, Curve::new(0.5 - p.release_curve, false));
    }

    /// [re:dedd03].
    pub fn note_on(&mut self) {
        self.time = 0.0;
        self.stage = 2;
        self.value = 0.0;
        self.last = 0.0;
    }

    /// [re:fb36ac]. Gated modes release from the current level.
    pub fn note_off(&mut self) {
        self.stage = 6;
        if self.one_shot {
            return;
        }
        let v = self.last;
        self.release.0 = if self.release_len <= 1.0 { 1e-5 } else { v / self.release_len };
        self.sustain = v;
        self.value = v;
    }

    pub fn is_active(&self) -> bool {
        self.stage != 0
    }

    fn shaped((_, shape, curve): (f32, f32, Curve), x: f32) -> f32 {
        if (shape - 0.5).abs() > 1e-6 { curve.eval(x) } else { x }
    }

    /// [re:cf759f]. Returns whether the envelope is still running.
    pub fn process(&mut self, out: &mut [f32]) -> bool {
        if out.is_empty() {
            return false;
        }
        if self.stage == 0 {
            out.fill(0.0);
            return false;
        }
        for o in out.iter_mut() {
            let mut y = 0.0;
            match self.stage {
                1 => {
                    self.value = (self.value - self.fade_step).max(0.0);
                    y = self.value;
                    if y.abs() < 1e-6 {
                        self.stage = 0;
                    }
                }
                2 => {
                    self.value = (self.value + self.attack.0).min(1.0);
                    y = Self::shaped(self.attack, self.value);
                    if y >= 1.0 || self.attack_end <= self.time {
                        self.stage = 3;
                        self.value = 1.0;
                        y = 1.0;
                    }
                }
                3 => {
                    self.value -= self.decay.0;
                    let s = self.sustain;
                    y = ((1.0 - s) * Self::shaped(self.decay, self.value) + s).max(0.0);
                    if self.decay_end <= self.time {
                        if s.abs() >= 1e-6 {
                            self.stage = if self.one_shot { 5 } else { 4 };
                            self.value = s;
                            y = s;
                        } else if y.abs() >= 1e-6 {
                            self.stage = 1;
                            self.fade_step = y * 0.03125;
                        } else {
                            self.stage = 0;
                        }
                    }
                }
                4 => {
                    self.value = self.sustain;
                    y = self.sustain;
                }
                5 => {
                    self.value = self.sustain;
                    y = self.sustain;
                    if self.hold_end <= self.time {
                        self.stage = 6;
                    }
                }
                6 => {
                    let s = self.sustain;
                    self.value -= self.release.0;
                    let x = if s.abs() >= 1e-6 { self.value / s } else { self.value };
                    y = (Self::shaped(self.release, x) * s).max(0.0);
                    if y.abs() < 1e-6 {
                        self.stage = 0;
                    }
                }
                _ => {}
            }
            *o = y;
            self.last = y;
            self.time += self.rate;
        }
        self.stage != 0
    }
}

/// [re:947936]: exponent for `powf`, 1 when centred.
fn ahr_exponent(c: f32) -> f32 {
    if (c - 0.5).abs() <= 1e-6 {
        1.0
    } else if c < 0.5 {
        c + 0.5
    } else {
        c * 4.0 - 1.0
    }
}

/// [re:6c731c] ([re:9d0742] .. [re:afdb1f]): attack/hold/release laid out over the
/// played sample length, for `PlaybackMode::OneShotLengthBound`.
#[derive(Debug, Clone, Default)]
pub struct AhrEnvelope {
    releases_on_note_off: bool,
    attack_end: f32,
    attack_inc: f32,
    attack_exp: f32,
    release_start: f32,
    end: f32,
    release_inc: f32,
    release_exp: f32,
    time: f32,
    rate: f32,
    stage: u32,
    value: f32,
}

impl AhrEnvelope {
    /// [re:44c2b1]. `length` is the played length in
    /// frames; `playback_mode` is the layer's `PlaybackMode` index.
    pub fn initialize(&mut self, p: &EnvelopeParams, length: u32, playback_mode: u32, rate: f32) {
        self.rate = rate;
        self.attack_exp = ahr_exponent(1.0 - p.attack_curve);
        self.release_exp = ahr_exponent(1.0 - p.release_curve);
        self.releases_on_note_off = playback_mode > 1;
        let n = length as f32;
        self.time = 0.0;
        self.attack_end = p.attack * n * 0.5;
        self.release_start = n - n * 0.5 * p.release;
        self.end = n;
        if playback_mode < 2 {
            let shrink = (self.release_start - self.attack_end) * (1.0 - p.hold);
            self.release_start -= shrink;
            self.end -= shrink;
        }
        self.attack_inc = if self.attack_end.abs() > 1e-6 { 1.0 / self.attack_end } else { 1.0 };
        let r = self.end - self.release_start;
        self.release_inc = if r.abs() > 1e-6 { 1.0 / r } else { 1.0 };
    }

    pub fn note_on(&mut self) {
        self.time = 0.0;
        self.stage = 1;
        self.value = 0.0;
    }

    pub fn note_off(&mut self) {
        if self.releases_on_note_off {
            self.stage = 3;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != 0
    }

    /// [re:5d2359].
    pub fn process(&mut self, out: &mut [f32]) -> bool {
        if self.stage == 0 {
            out.fill(0.0);
            return false;
        }
        let mut y = 0.0;
        for o in out.iter_mut() {
            match self.stage {
                1 => {
                    self.value += self.attack_inc;
                    y = self.value.powf(self.attack_exp);
                    if y >= 1.0 || self.end <= self.time {
                        self.stage = 2;
                        self.value = 1.0;
                        y = 1.0;
                    }
                }
                2 => {
                    self.value = 1.0;
                    y = 1.0;
                    if !self.releases_on_note_off && self.release_start <= self.time {
                        self.stage = 3;
                    }
                }
                3 => {
                    self.value -= self.release_inc;
                    y = self.value.powf(self.release_exp);
                    // `jbe` ([re:f7fbca]): a NaN from a value rounded below 0
                    // ends the envelope too.
                    if !(y > 0.0) || self.end <= self.time {
                        self.stage = 0;
                        self.value = 0.0;
                        y = 0.0;
                    }
                }
                _ => {}
            }
            *o = y;
            self.time += self.rate;
        }
        self.stage != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 1000.0;

    fn params(attack: f32, decay: f32, sustain: f32, hold: f32, release: f32) -> EnvelopeParams {
        EnvelopeParams { attack, decay, sustain, hold, release, attack_curve: 0.5, decay_curve: 0.5, release_curve: 0.5 }
    }

    #[test]
    fn gated_adsr_shape() {
        let mut e = Envelope::new(SR);
        // attack^4·10 s = 0.1 s = 100 samples, decay 100 samples to 0.5.
        let x = 0.01f32.powf(0.25);
        e.initialize(&params(x, x, 0.5, 0.0, x), false, 1.0);
        e.note_on();
        let mut buf = vec![0.0; 400];
        assert!(e.process(&mut buf));
        assert!((buf[49] - 0.5).abs() < 0.02, "mid-attack {}", buf[49]);
        assert_eq!(buf[100], 1.0);
        assert!((buf[150] - 0.75).abs() < 0.02, "mid-decay {}", buf[150]);
        assert_eq!(buf[399], 0.5);
        e.note_off();
        let mut tail = vec![0.0; 2000];
        e.process(&mut tail);
        assert!(!e.is_active());
        assert!(tail[0] < 0.5 && tail[0] > 0.49);
    }

    /// The release value rounds below 0 before the end on long notes; with
    /// a fractional curve exponent `powf` gives NaN, which must end the
    /// envelope (as the original's unordered `jbe` does) rather than play.
    #[test]
    fn ahr_release_never_emits_nan() {
        let mut e = AhrEnvelope::default();
        let mut p = params(0.1, 0.0, 1.0, 1.0, 1.0);
        p.release_curve = 0.9;
        e.initialize(&p, 288_000, 0, 1.0);
        e.note_on();
        let mut buf = vec![0.0; 1024];
        for _ in 0..300 {
            e.process(&mut buf);
            assert!(buf.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn one_shot_holds_then_releases() {
        let mut e = Envelope::new(SR);
        e.initialize(&params(0.0, 0.0, 1.0, 0.0, 0.0), true, 1.0);
        e.note_on();
        let mut buf = vec![0.0; 100];
        e.process(&mut buf);
        // Minimum 4-sample attack, no hold, 32-sample minimum release.
        assert_eq!(buf[4], 1.0);
        assert!(!e.is_active());
        assert_eq!(buf[99], 0.0);
    }

    #[test]
    fn curved_segments_stay_in_range() {
        let mut e = Envelope::new(SR);
        let mut p = params(0.5, 0.5, 0.3, 0.0, 0.5);
        for c in [0.0, 0.25, 0.75, 1.0] {
            p.attack_curve = c;
            p.decay_curve = c;
            p.release_curve = c;
            e.initialize(&p, true, 1.0);
            e.note_on();
            let mut buf = vec![0.0; 20000];
            e.process(&mut buf);
            assert!(buf.iter().all(|v| (0.0..=1.0).contains(v)), "curve {c}");
        }
    }

    #[test]
    fn ahr_spans_sample_length() {
        let mut e = AhrEnvelope::default();
        e.initialize(&params(0.2, 0.0, 0.0, 1.0, 0.2), 1000, 1, 1.0);
        e.note_on();
        let mut buf = vec![0.0; 1100];
        e.process(&mut buf);
        assert!((buf[49] - 0.5).abs() < 0.02);
        assert_eq!(buf[500], 1.0);
        assert!(buf[999] < 0.02);
        assert!(!e.is_active());
    }
}
