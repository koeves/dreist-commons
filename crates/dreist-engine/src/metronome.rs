//! `Metronome` ([re:8ef4dc]): a [re:b978a6] (muted by default) that
//! clicks on the selected engine's 16th grid through two dedicated voices.
//! See the dreist-re live-input notes ("Metronome").

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dreist_dsp::convert::gain_to_amp;
use dreist_dsp::voice::SampleData;
use dreist_formats::Node;

use crate::engine::Route;

/// One of the two metronome voices ([re:1cf147]):
/// plays its sample from the start at unity pitch, scaled by the
/// metronome gain ([re:f01d09]).
#[derive(Debug, Clone, Default)]
pub struct ClickVoice {
    pub sample: Option<Arc<SampleData>>,
    pos: f64,
    delay: f64,
    active: bool,
}

impl ClickVoice {
    fn start(&mut self, delay: f64) {
        if self.sample.is_some() {
            self.pos = 0.0;
            self.delay = delay;
            self.active = true;
        }
    }

    /// Released at once ([re:bf71ff]).
    pub fn kill(&mut self) {
        self.active = false;
    }

    /// Render `n` frames into `out` (added), linear interpolation.
    fn render(&mut self, gain: f32, host_sr: f32, out: [&mut [f32]; 2]) {
        let Some(s) = &self.sample else { return };
        if !self.active {
            return;
        }
        let [l, r] = out;
        let n = l.len();
        // [re:27a47b]: the delay counts down per sub-block.
        if self.delay > n as f64 {
            self.delay -= n as f64;
            return;
        }
        let first = (self.delay.round().max(0.0) as usize).min(n);
        self.delay = 0.0;
        let step = s.sample_rate as f64 / host_sr as f64;
        let len = s.left.len();
        let right = s.right.as_ref().unwrap_or(&s.left);
        for i in first..n {
            let k = self.pos as usize;
            if k + 1 >= len {
                self.active = false;
                break;
            }
            let f = (self.pos - k as f64) as f32;
            l[i] += (s.left[k] + (s.left[k + 1] - s.left[k]) * f) * gain;
            r[i] += (right[k] + (right[k + 1] - right[k]) * f) * gain;
            self.pos += step;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Metronome {
    pub muted: bool,
    /// Normalized gain (reset 0.7937 = 0 dB) and output location.
    pub gain: f32,
    pub route: Route,
    /// Click state: −1 idle, 2 bar, 1 beat; countdown.
    state: i32,
    counter: i32,
    /// The bar voice holds `Bar.wav`, the beat voice `Beat.wav`.
    pub bar_voice: ClickVoice,
    pub beat_voice: ClickVoice,
}

impl Default for Metronome {
    /// [re:297adf]: muted, idle.
    fn default() -> Self {
        Metronome { muted: true, gain: 0.5f32.cbrt(), route: Route::Master, state: -1, counter: -1, bar_voice: ClickVoice::default(), beat_voice: ClickVoice::default() }
    }
}

impl Metronome {
    /// [re:61adb6]: mix state + output state.
    pub fn from_node(n: &Node) -> Metronome {
        let mut m = Metronome::default();
        m.muted = n.bool("Mute").unwrap_or(true);
        m.gain = n.f32("Gain").unwrap_or(m.gain).clamp(0.0, 1.0);
        m.route = n.attr("OutputLocation").map(Route::from_name).unwrap_or(Route::Master);
        m
    }

    /// [re:814f3a]: `<Resources>/Metronomes/<sound>/
    /// {Bar,Beat}.wav`, falling back to `Default`.
    pub fn load_sounds(&mut self, metronomes: &Path, sound: &str) -> bool {
        let mut dir: PathBuf = metronomes.join(sound);
        if !dir.is_dir() {
            dir = metronomes.join("Default");
            if !dir.is_dir() {
                return false;
            }
        }
        self.bar_voice.sample = crate::audio::load(&dir.join("Bar.wav")).ok().map(Arc::new);
        self.beat_voice.sample = crate::audio::load(&dir.join("Beat.wav")).ok().map(Arc::new);
        self.bar_voice.sample.is_some() || self.beat_voice.sample.is_some()
    }

    /// Advance the click [re:23b2a1] for a new 16th `n16`, `delay` samples ahead.
    /// Bars (n16 % 16 == 0) start the beat voice and other beats the bar
    /// voice, so the downbeat plays `Beat.wav` (original quirk, kept).
    pub fn progress_click(&mut self, n16: i32, delay: i32) {
        if self.muted || self.state != -1 || n16 & 3 != 0 {
            return;
        }
        self.counter = delay;
        if n16 & 0xf == 0 {
            self.state = 2;
            self.beat_voice.start(delay as f64);
        } else {
            self.state = 1;
            self.bar_voice.start(delay as f64);
        }
    }

    /// A click is sounding (state 1 beat / 2 bar).
    pub fn clicking(&self) -> bool {
        self.state > 0
    }

    /// [re:677402]: the click state returns to idle once
    /// the countdown reaches −100/state.
    pub fn process(&mut self, n: usize) {
        if self.state <= 0 {
            return;
        }
        for _ in 0..n {
            self.counter -= 1;
            if self.counter as f64 <= -100.0 / self.state as f64 {
                self.state = -1;
                self.counter = -1;
                break;
            }
        }
    }

    /// Render both voices (end of the voice processing) into `out`.
    pub fn render(&mut self, host_sr: f32, out: [&mut [f32]; 2]) {
        let g = gain_to_amp(self.gain);
        let [l, r] = out;
        self.bar_voice.render(g, host_sr, [&mut *l, &mut *r]);
        self.beat_voice.render(g, host_sr, [l, r]);
    }
}

/// The bundled metronome folder: Dreist's own synthesised clicks
/// (`<ui>/Metronomes`, made by `scripts/uigen/sounds.py`).
pub fn default_metronomes_dir() -> PathBuf {
    dreist_formats::paths::ui_dir().join("Metronomes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_on_beats_with_quirky_voice_choice() {
        let s = Arc::new(SampleData { left: vec![1.0; 100], right: None, sample_rate: 44100.0 });
        let mut m = Metronome { muted: false, ..Metronome::default() };
        m.bar_voice.sample = Some(s.clone());
        m.beat_voice.sample = Some(s);
        m.progress_click(1, 0);
        assert_eq!(m.state, -1);
        m.progress_click(16, 5);
        assert_eq!(m.state, 2);
        assert!(m.beat_voice.active && !m.bar_voice.active);
        m.process(54);
        assert_eq!(m.state, 2);
        m.process(2);
        assert_eq!(m.state, -1);
        m.progress_click(4, 0);
        assert!(m.bar_voice.active);
    }
}
