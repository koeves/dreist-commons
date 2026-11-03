//! [re:c39e6e] ([re:cd8a9c] ..): song parts switch each engine's pattern on and
//! off at quarter-note positions. See the dreist-re song notes.

use dreist_formats::Node;

/// Song times are quantized to 1/960 QN (`SongPart::QuantizeTime*` [re:4c3473]).
const Q: f64 = 960.0;

pub fn quantize_round(t: f64) -> f64 {
    let x = t * Q;
    (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32 as f64 * 0.0010416666666666667
}

pub fn quantize_floor(t: f64) -> f64 {
    (t * Q).floor() as i32 as f64 * 0.0010416666666666667
}

pub fn quantize_ceil(t: f64) -> f64 {
    (t * Q).ceil() as i32 as f64 * 0.0010416666666666667
}

/// [re:025fb9].
pub fn phase_mod(x: f64, m: f64) -> f64 {
    if x < 0.0 {
        let y = x % m + m;
        if y < m {
            return y;
        }
        return y % m;
    }
    if x >= m { x % m } else { x }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SongPart {
    pub start: f64,
    pub length: f64,
    pub pattern: usize,
    /// Pattern phase at the part start, QN.
    pub phase: f64,
    pub mute: bool,
}

impl SongPart {
    pub fn end(&self) -> f64 {
        quantize_round(self.start + self.length)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Song {
    pub active: bool,
    /// [re:fc9225] song loop (`SongLoopEnabled` +0xed, `SongLoopStart` +0xf0,
    /// `SongLoopEnd` +0xf8; defaults off, 0, 16; rounded on load).
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    /// Host cycle ([re:e95dcb] on, +0x58 start, +0x60 end, QN); takes
    /// the internal loop's place while the host reports it.
    pub host_loop: Option<(f64, f64)>,
    /// One track per engine, parts sorted by start.
    pub tracks: Vec<Vec<SongPart>>,
}

impl Song {
    /// [re:b1f31a] / [re:219e60].
    pub fn from_node(n: &Node) -> Song {
        let mut song = Song {
            active: n.bool("SongActive").unwrap_or(false),
            loop_enabled: n.bool("SongLoopEnabled").unwrap_or(false),
            loop_start: quantize_round(n.attr("SongLoopStart").and_then(|v| v.parse().ok()).unwrap_or(0.0)),
            loop_end: quantize_round(n.attr("SongLoopEnd").and_then(|v| v.parse().ok()).unwrap_or(16.0)),
            host_loop: None,
            tracks: vec![Vec::new(); 8],
        };
        for s in n.children_of("Song") {
            for t in s.children_of("SongTrack") {
                let Some(i) = t.index().map(|i| i as usize).filter(|i| *i < 8) else { continue };
                let num = |p: &Node, k: &str| p.attr(k).and_then(|v| v.parse::<f64>().ok());
                let mut parts: Vec<SongPart> = t
                    .children_of("SongPart")
                    .map(|p| SongPart {
                        start: num(p, "starttime").unwrap_or(0.0),
                        length: num(p, "length").unwrap_or(0.0),
                        pattern: p.u32("patt").unwrap_or(9999) as usize,
                        phase: num(p, "phase").unwrap_or(0.0),
                        mute: p.bool("mute").unwrap_or(false),
                    })
                    .collect();
                parts.sort_by(|a, b| a.start.total_cmp(&b.start));
                song.tracks[i] = parts;
            }
        }
        song
    }

    /// `IsLoopActive` [re:7aa2a4]: the host cycle when the host reports one,
    /// else the internal loop.
    pub fn loop_active(&self) -> bool {
        self.host_loop.is_some() || self.loop_enabled
    }

    /// The active loop range (start, end) QN.
    pub fn loop_range(&self) -> (f64, f64) {
        self.host_loop.unwrap_or((self.loop_start, self.loop_end))
    }

    /// `GetPpqPosAccountingForInternalLoop` [re:455502] / `GetEffectivePpqPos`
    /// [re:5634e1]: wrap positions past the loop end back into the loop (the
    /// host cycle +0x58/+0x60 when the host loop flag +0x50 is on, else the
    /// internal loop).
    pub fn effective_ppq(&self, ppq: f64) -> f64 {
        let (s, e) = self.loop_range();
        if self.loop_active() && e < ppq && s < e { (ppq - e) % (e - s) + s } else { ppq }
    }

    /// `GetSongLengthQN`: end of the last part.
    pub fn length_qn(&self) -> f64 {
        self.tracks.iter().flatten().map(|p| p.end()).fold(0.0, f64::max)
    }

    /// [re:f3517d], last match.
    fn last_overlapping(&self, track: usize, a: f64, b: f64) -> Option<&SongPart> {
        self.tracks.get(track)?.iter().rev().find(|p| {
            if a <= p.start && p.start < b {
                return true;
            }
            let e = p.end();
            (a < e && e <= b) || (p.start < a && b < e)
        })
    }
}

/// What a song change does to an engine (`ActivateTrackChange` [re:dce201]).
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeKind {
    /// Type 2: pattern power on, select `pattern`, resync to `phase` (QN).
    Start { pattern: usize, phase: f64 },
    /// Type 0: stop the song-triggered pattern (message 0xab).
    Stop,
    /// Type 1: `ResyncSong` [re:662b0c] — every engine's gate off (loop wrap).
    Resync,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Sample offset in the host block.
    pub offset: u32,
    pub engine: usize,
    pub kind: ChangeKind,
}

/// Engine facts the cueing logic needs.
#[derive(Debug, Clone, Copy, Default)]
pub struct EngineView {
    pub gate: bool,
    pub current_pattern: usize,
}

/// Song playback cursor ([re:4b40ed], the end of the last cued
/// range; `+0x80` set after a loop wrap).
#[derive(Debug, Clone)]
pub struct SongCursor {
    cued_to: Option<f64>,
    wrapped: bool,
}

impl SongCursor {
    pub fn new() -> SongCursor {
        SongCursor { cued_to: None, wrapped: false }
    }

    /// `CueUpTrackChanges` [re:ff4062] for a host block starting at `ppq` QN,
    /// `frames` long. With the internal loop on, a range crossing the loop end
    /// is cued up to the end, a `Resync` is placed at the loop point, and the
    /// rest is cued from the loop start (and the next block resyncs again).
    pub fn cue(&mut self, song: &Song, ppq: f64, frames: u32, spbeat: f64, engines: &[EngineView], out: &mut Vec<Change>) {
        let bps = 1.0 / spbeat;
        // A fixed copy (this runs on the audio thread).
        let mut buf = [EngineView::default(); 8];
        let nv = engines.len().min(8);
        buf[..nv].copy_from_slice(&engines[..nv]);
        let views = &mut buf[..nv];
        let eff = song.effective_ppq(ppq);
        let (ls, le) = song.loop_range();
        let loop_end = quantize_ceil(le);
        let loop_start = quantize_floor(ls);
        let len = le - ls;
        let a = quantize_floor(eff);
        let resync = |views: &mut [EngineView], out: &mut Vec<Change>| {
            for v in views.iter_mut() {
                v.gate = false;
            }
            out.push(Change { offset: 0, engine: 0, kind: ChangeKind::Resync });
        };
        let mut cued = *self.cued_to.get_or_insert(a);
        let mut resynced = false;
        if (cued - a).abs() > 0.5 {
            // [re:c10d76] jump: RecordAllPatternStops, ResyncSong, restart here.
            resync(views, out);
            cued = a;
            resynced = true;
        }
        if self.wrapped {
            resync(views, out);
            self.wrapped = false;
        }
        let mut start = a;
        if a < cued && !resynced {
            start = cued;
        }
        let mut b = quantize_ceil(bps * frames as f64 + eff);
        if b <= start {
            return;
        }
        let mut ref_ppq = eff;
        let mut wrap = false;
        let mut skip_first = false;
        if song.loop_active() && b > loop_end {
            b = quantize_round(b - len);
            if loop_end <= start {
                start = (cued - len).max(quantize_round(a - len));
                ref_ppq = song.effective_ppq(ppq) - len;
                self.wrapped = true;
                skip_first = true;
            } else {
                wrap = true;
            }
        }
        let at = |t: f64, r: f64| ((spbeat * (t - r)).ceil() as i32).max(0) as u32;
        for track in 0..views.len().min(8) {
            if wrap {
                if !skip_first {
                    cue_range(song, track, &views[track], start, loop_end, eff, false, spbeat, frames, out);
                }
                if track == 0 {
                    out.push(Change { offset: at(loop_end, eff), engine: 0, kind: ChangeKind::Resync });
                }
                // The loop-point Resync is still pending: the engine state is unchanged.
                cue_range(song, track, &views[track], loop_start, b, eff - len, true, spbeat, frames, out);
                self.wrapped = true;
            } else {
                cue_range(song, track, &views[track], start, b, ref_ppq, false, spbeat, frames, out);
            }
        }
        out.sort_by_key(|c| c.offset);
        self.cued_to = Some(b);
    }
}

/// `CueUpTrackChangesInRange` [re:b8174e] for one track over `[a, b)`; offsets
/// are relative to `r` (QN at the block start). `fresh` (after a loop wrap)
/// disables the "already playing this pattern" shortcut.
#[allow(clippy::too_many_arguments)]
fn cue_range(song: &Song, track: usize, e: &EngineView, a: f64, b: f64, r: f64, fresh: bool, spbeat: f64, frames: u32, out: &mut Vec<Change>) {
    let at = |t: f64| ((spbeat * (t - r)).ceil() as i32).max(0) as u32;
    match song.last_overlapping(track, a, b) {
        None => {
            if e.gate {
                out.push(Change { offset: at(a).min(frames.saturating_sub(1)), engine: track, kind: ChangeKind::Stop });
            }
        }
        Some(p) => {
            if a <= p.start || p.end() < b || (!fresh && e.gate && e.current_pattern == p.pattern) {
                if p.start < a || b <= p.start {
                    let end = p.end() - 0.0009765625;
                    if a <= end && end < b && e.gate {
                        let off = ((spbeat * (end - r)) as i32).max(0) as u32;
                        out.push(Change { offset: off, engine: track, kind: ChangeKind::Stop });
                    }
                } else {
                    out.push(Change { offset: at(p.start), engine: track, kind: ChangeKind::Start { pattern: p.pattern, phase: p.phase } });
                }
            } else {
                // Inside the part, engine not playing it (skip if already cued).
                let dup = out.iter().any(|c| c.engine == track && matches!(c.kind, ChangeKind::Start { pattern, .. } if pattern == p.pattern));
                if !dup {
                    out.push(Change { offset: at(a), engine: track, kind: ChangeKind::Start { pattern: p.pattern, phase: p.phase + a - p.start } });
                }
            }
        }
    }
}

impl Default for SongCursor {
    fn default() -> Self {
        SongCursor::new()
    }
}

/// Editor edits (the original posts `SongPart` objects with
/// `e_songAddPart` / `e_songDeletePart`; the port uses times in QN).
impl Song {
    /// Insert a part, replacing whatever it overlaps.
    pub fn add_part(&mut self, track: usize, part: SongPart) {
        let Some(t) = self.tracks.get_mut(track) else { return };
        t.retain(|p| p.end() <= part.start || p.start >= part.end());
        t.push(part);
        t.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Remove the part covering `time` (QN).
    pub fn delete_part_at(&mut self, track: usize, time: f64) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.retain(|p| !(p.start <= time && time < p.end()));
        }
    }

    /// `e_songReplaceTrack` (0x11c): a track's parts as a whole (sorted).
    pub fn replace_track(&mut self, track: usize, mut parts: Vec<SongPart>) {
        if track >= self.tracks.len() {
            self.tracks.resize(track + 1, Vec::new());
        }
        parts.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        self.tracks[track] = parts;
    }

    /// [`Song::replace_track`] on the audio thread: `parts` (sorted by
    /// start, as the editor sends them; sorted here otherwise) swaps in and
    /// the old parts are left in it for the sender to free. Tracks past the
    /// song's are ignored.
    pub fn swap_track(&mut self, track: usize, parts: &mut Vec<SongPart>) {
        let Some(slot) = self.tracks.get_mut(track) else { return };
        if !parts.is_sorted_by(|a, b| a.start <= b.start) {
            parts.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        }
        std::mem::swap(slot, parts);
    }

    /// Write the tracks into a [re:c39e6e] node ([re:9a5b97] /
    /// `SongPart` children of its `Song`).
    pub fn write_tracks(&self, seq: &mut Node) {
        let body = seq.child_mut_or_insert("Song", "Song");
        for (k, parts) in self.tracks.iter().enumerate() {
            let tr = body.child_mut_or_insert("SongTrack", &format!("Track{k}"));
            tr.children.retain(|c| c.class != "SongPart");
            for (pi, p) in parts.iter().enumerate() {
                let mut n = Node::new("SongPart", Some(&format!("Part{pi}")));
                n.set("length", p.length);
                n.set("mute", if p.mute { "true" } else { "false" });
                n.set("patt", p.pattern);
                n.set("phase", p.phase);
                n.set("starttime", p.start);
                tr.children.push(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        let part = |start, length, pattern| SongPart { start, length, pattern, phase: 0.0, mute: false };
        Song { active: true, loop_enabled: false, loop_start: 0.0, loop_end: 0.0, host_loop: None, tracks: vec![vec![part(0.0, 4.0, 2), part(8.0, 4.0, 5)]] }
    }

    #[test]
    fn starts_and_stops_parts() {
        let s = song();
        let mut cur = SongCursor::new();
        let spbeat = 24000.0;
        let mut ev = Vec::new();
        let mut gate = false;
        let mut pat = 0;
        let mut log = Vec::new();
        let n = 512u32;
        for blk in 0..(14 * 24000 / 512) {
            let ppq = (blk * n) as f64 / spbeat;
            ev.clear();
            cur.cue(&s, ppq, n, spbeat, &[EngineView { gate, current_pattern: pat }], &mut ev);
            for c in &ev {
                match c.kind {
                    ChangeKind::Start { pattern, .. } => {
                        gate = true;
                        pat = pattern;
                    }
                    ChangeKind::Stop | ChangeKind::Resync => gate = false,
                }
                log.push((((blk * n) + c.offset) as f64 / spbeat * 100.0).round() / 100.0);
            }
        }
        assert_eq!(log, vec![0.0, 4.0, 8.0, 12.0]);
    }

    #[test]
    fn host_loop_replaces_the_internal_loop() {
        let mut s = Song { active: true, loop_enabled: true, loop_start: 0.0, loop_end: 4.0, host_loop: None, tracks: vec![Vec::new()] };
        assert_eq!(s.effective_ppq(5.0), 1.0);
        s.host_loop = Some((8.0, 16.0));
        assert_eq!(s.loop_range(), (8.0, 16.0));
        assert_eq!(s.effective_ppq(5.0), 5.0, "inside the host cycle's span");
        assert_eq!(s.effective_ppq(18.0), 10.0);
        s.loop_enabled = false;
        assert!(s.loop_active());
    }

    #[test]
    fn internal_loop_wraps_and_resyncs() {
        let part = |start, length, pattern| SongPart { start, length, pattern, phase: 0.0, mute: false };
        let s = Song { active: true, loop_enabled: true, loop_start: 0.0, loop_end: 4.0, host_loop: None, tracks: vec![vec![part(0.0, 4.0, 2)]] };
        let mut cur = SongCursor::new();
        let spbeat = 24000.0;
        let n = 512u32;
        let mut gate = false;
        let mut log = Vec::new();
        let mut ev = Vec::new();
        for blk in 0..(9 * 24000 / 512) {
            let ppq = (blk * n) as f64 / spbeat;
            ev.clear();
            cur.cue(&s, ppq, n, spbeat, &[EngineView { gate, current_pattern: 2 }], &mut ev);
            for c in &ev {
                let t = ((blk * n) + c.offset) as f64 / spbeat;
                match c.kind {
                    ChangeKind::Start { phase, .. } => {
                        gate = true;
                        log.push(('S', (t * 100.0).round() / 100.0, (phase * 100.0).round() / 100.0));
                    }
                    ChangeKind::Stop => gate = false,
                    ChangeKind::Resync => {
                        gate = false;
                        log.push(('R', (t * 100.0).round() / 100.0, 0.0));
                    }
                }
            }
        }
        // Start at 0; at each loop point a Resync + restart from phase 0, then
        // the next block resyncs again and re-enters the part at its phase.
        assert_eq!(&log[..4], &[('S', 0.0, 0.0), ('R', 4.0, 0.0), ('S', 4.0, 0.0), ('R', 4.01, 0.0)]);
        assert_eq!(log[4].0, 'S');
        assert!((log[4].1 - 4.01).abs() < 0.02 && log[4].2 > 0.0 && log[4].2 < 0.05);
        assert!(log.iter().any(|e| e.0 == 'R' && (e.1 - 8.0).abs() < 1e-9));
    }

    #[test]
    fn quantizers() {
        assert_eq!(quantize_floor(1.00001), 1.0);
        assert!((quantize_ceil(1.00001) - (1.0 + 1.0 / 960.0)).abs() < 1e-12);
        assert_eq!(phase_mod(-1.0, 4.0), 3.0);
        assert_eq!(phase_mod(9.0, 4.0), 1.0);
    }
}
