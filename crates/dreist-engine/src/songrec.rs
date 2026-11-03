//! Song recording ([re:c7a961] record state, +0xbc recorded
//! pattern per track, +0xe4 "played" flags).
//!
//! The engine only reports: while a pattern started or stopped by the
//! user (not by the song) plays, `RecordPatternStart` [re:118fd2] /
//! `RecordPatternStop` [re:02c0ef] post `e_songPatternStarted` 0x1f2 and
//! `e_songPatternStopped` 0x1f3 (499) to the editor, flagged with whether
//! the song is recording. The editor turns them into parts: recorded ones
//! go into the song, the others into the retro buffer ([re:6342d9]
//!  [re:2af005], `PatternStopped` [re:be8eab]).
//!
//! A stop starts a "stopped" part ([`STOP_PART`], pattern 0x19, never
//! activated by `ActivateTrackChange` [re:dce201]) that lasts until the next
//! start or power-on.

use crate::song::quantize_round;

/// Pattern id of a stopped stretch.
pub const STOP_PART: usize = 0x19;
const TRACKS: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum SongEvent {
    /// `e_songRecordStateChanged` (500): 0 idle, 1 armed, 2 recording.
    State(u8),
    /// 0x1f2 [track, -, -, pattern] [time, recording, phase].
    Started { track: usize, pattern: usize, time: f64, recording: bool, phase: f64 },
    /// 499 [track, -, -, pattern] [time, recording].
    Stopped { track: usize, pattern: usize, time: f64, recording: bool },
    /// `e_songStartNewRetroRecord` 0x1f8 (transport started).
    NewRetro,
}

/// Pattern retro recording input for the editor's [re:e8cb5c]: every
/// live-recorded step ([re:4e20da], also when record is off) and
/// pattern cycles (`e_updatePatternCycled` 0x1a2).
// Pushed into a reserved Vec on the audio thread: boxing the step would
// allocate there.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum RetroEvent {
    Step { engine: usize, track: usize, step: usize, data: crate::pattern::StepData },
    Cycled { engine: usize },
}

#[derive(Clone, Debug, Default)]
pub struct SongRecorder {
    pub state: u8,
    current: [Option<usize>; TRACKS],
    played: [bool; TRACKS],
    /// For the editor (drained by the host).
    pub events: Vec<SongEvent>,
}

impl SongRecorder {
    fn recording(&self) -> bool {
        self.state == 2
    }

    /// `RecordPatternStart`: closes the previous recorded pattern of the
    /// track, then opens `pattern` at the rounded time.
    pub fn record_start(&mut self, track: usize, pattern: usize, time: f64, phase: f64) {
        if track >= TRACKS || (pattern >= 24 && pattern != STOP_PART) || self.current[track] == Some(pattern) {
            return;
        }
        let t = quantize_round(time).max(0.0);
        let phase = phase.max(0.0);
        let recording = self.recording();
        if let Some(prev) = self.current[track] {
            self.events.push(SongEvent::Stopped { track, pattern: prev, time: t, recording });
        }
        self.current[track] = Some(pattern);
        self.events.push(SongEvent::Started { track, pattern, time: t, recording, phase });
    }

    /// `RecordPatternStop`; `stop_part` opens a [`STOP_PART`] afterwards.
    pub fn record_stop(&mut self, track: usize, time: f64, stop_part: bool) {
        if track >= TRACKS {
            return;
        }
        let t = quantize_round(time).max(0.0);
        let recording = self.recording();
        if let Some(prev) = self.current[track].take() {
            self.events.push(SongEvent::Stopped { track, pattern: prev, time: t, recording });
        }
        if stop_part {
            self.events.push(SongEvent::Started { track, pattern: STOP_PART, time: t, recording, phase: 0.0 });
            self.current[track] = Some(STOP_PART);
        }
    }

    /// `RecordAllPatternStops` [re:a5b1e3] (the event carries the flag given).
    fn record_all_stops(&mut self, time: f64, recording: bool) {
        for track in 0..TRACKS {
            if let Some(prev) = self.current[track].take() {
                self.events.push(SongEvent::Stopped { track, pattern: prev, time, recording });
            }
        }
    }

    /// `StartRecording` [re:f375c2].
    fn start(&mut self, time: f64) {
        if self.state == 2 {
            return;
        }
        self.record_all_stops(time, false);
        self.played = [false; TRACKS];
        self.state = 2;
        self.events.push(SongEvent::State(2));
    }

    /// `StopRecording` [re:6ebf71].
    fn stop(&mut self, time: f64) {
        if self.state != 2 {
            return;
        }
        self.record_all_stops(time, true);
        self.state = 0;
        self.events.push(SongEvent::State(0));
    }

    /// `ArmRecording` [re:e1dad3] (record on while the song is active): starts
    /// at once when the transport runs.
    pub fn arm(&mut self, playing: bool, time: f64) {
        if self.state != 0 {
            return;
        }
        self.state = 1;
        if playing {
            self.start(time);
        } else {
            self.events.push(SongEvent::State(1));
        }
    }

    /// `DisarmRecording` [re:1058a5].
    pub fn disarm(&mut self, time: f64) {
        if self.state == 2 {
            self.stop(time);
        }
        if self.state == 1 {
            self.state = 0;
            self.events.push(SongEvent::State(0));
        }
    }

    /// `ProcessTransportChange` [re:816b04].
    pub fn transport(&mut self, playing: bool, time: f64, song_active: bool) {
        if playing {
            self.events.push(SongEvent::NewRetro);
        } else {
            let rec = self.recording();
            self.record_all_stops(quantize_round(time), rec);
        }
        if song_active {
            if self.state == 2 && !playing {
                self.stop(time);
            } else if self.state == 1 && playing {
                self.start(time);
            }
            self.played = [false; TRACKS];
        }
    }

    /// `SetActive(false)` stops recording.
    pub fn song_deactivated(&mut self, time: f64) {
        if self.state != 0 {
            self.stop(time);
            self.state = 0;
        }
    }

    /// `e_updatePatternStarted` 0x1a5 (user start): a non-empty pattern
    /// marks the track played; played tracks record.
    pub fn pattern_started(&mut self, track: usize, pattern: usize, empty: bool, time: f64, phase: f64) {
        if track >= TRACKS {
            return;
        }
        if !empty {
            self.played[track] = true;
        }
        if self.played[track] {
            self.record_start(track, pattern, time, phase);
        }
    }

    /// `e_updatePatternStopped` 0x1a6 (user stop).
    pub fn pattern_stopped(&mut self, track: usize, time: f64) {
        if track >= TRACKS {
            return;
        }
        self.played[track] = true;
        self.record_stop(track, time, true);
    }

    /// `e_songSetEraseState` 0x123 ([re:380ce6]): the
    /// track counts as played; pressing closes its recorded pattern and
    /// opens a [`STOP_PART`]; releasing closes that and, with `restart`
    /// (the song not active), reopens the engine's current pattern.
    pub fn erase(&mut self, track: usize, on: bool, time: f64, restart: Option<(usize, f64)>) {
        if track >= TRACKS {
            return;
        }
        self.played[track] = true;
        if on {
            self.record_start(track, STOP_PART, time, 0.0);
        } else {
            if self.current[track] == Some(STOP_PART) {
                self.record_stop(track, time, false);
            }
            if let Some((pattern, phase)) = restart {
                self.record_start(track, pattern, time, phase);
            }
        }
    }

    /// `e_updatePatternPower` 0x1a7: power on ends a stopped stretch.
    pub fn power_on(&mut self, track: usize, time: f64) {
        if track < TRACKS && self.played[track] && self.current[track] == Some(STOP_PART) {
            self.record_stop(track, time, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_starts_stops_and_stop_parts() {
        let mut r = SongRecorder::default();
        r.arm(true, 0.0);
        assert_eq!(r.events, vec![SongEvent::State(2)]);
        r.events.clear();
        // An empty pattern on an unplayed track is not recorded.
        r.pattern_started(0, 3, true, 1.0, 0.0);
        assert!(r.events.is_empty());
        r.pattern_started(0, 1, false, 4.0, 0.5);
        r.pattern_started(0, 2, false, 8.0, 0.0);
        r.pattern_stopped(0, 12.0);
        r.power_on(0, 14.0);
        assert_eq!(
            r.events,
            vec![
                SongEvent::Started { track: 0, pattern: 1, time: 4.0, recording: true, phase: 0.5 },
                SongEvent::Stopped { track: 0, pattern: 1, time: 8.0, recording: true },
                SongEvent::Started { track: 0, pattern: 2, time: 8.0, recording: true, phase: 0.0 },
                SongEvent::Stopped { track: 0, pattern: 2, time: 12.0, recording: true },
                SongEvent::Started { track: 0, pattern: STOP_PART, time: 12.0, recording: true, phase: 0.0 },
                SongEvent::Stopped { track: 0, pattern: STOP_PART, time: 14.0, recording: true },
            ]
        );
        r.events.clear();
        r.pattern_started(1, 0, false, 16.0, 0.0);
        r.disarm(20.0);
        assert_eq!(r.events.last(), Some(&SongEvent::State(0)));
        assert!(r.events.contains(&SongEvent::Stopped { track: 1, pattern: 0, time: 20.0, recording: true }));
    }

    #[test]
    fn erase_records_a_stopped_stretch() {
        let mut r = SongRecorder::default();
        r.arm(true, 0.0);
        r.pattern_started(2, 4, false, 0.0, 0.0);
        r.events.clear();
        r.erase(2, true, 8.0, None);
        r.erase(2, false, 12.0, Some((4, 0.5)));
        assert_eq!(
            r.events,
            vec![
                SongEvent::Stopped { track: 2, pattern: 4, time: 8.0, recording: true },
                SongEvent::Started { track: 2, pattern: STOP_PART, time: 8.0, recording: true, phase: 0.0 },
                SongEvent::Stopped { track: 2, pattern: STOP_PART, time: 12.0, recording: true },
                SongEvent::Started { track: 2, pattern: 4, time: 12.0, recording: true, phase: 0.5 },
            ]
        );
    }
}
