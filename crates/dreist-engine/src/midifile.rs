//! Pattern as a standard MIDI file ([re:448650],
//! notes [re:ac7c09], CCs [re:cfb56a],
//! step graphs [re:f59d20], [re:eb804f]
//! [re:d92ff4]). See the dreist-re GUI notes (File menu › Save Pattern As MIDI).

use crate::pattern::{LANES, PatternData, Swing, lane};

/// Ticks per quarter note (the file's division is 0x1e0).
pub const PPQ: u32 = 480;
/// One tick in QN: the note-shortening and CC-reset step (0.0020833).
const TICK: f64 = 1.0 / 480.0;

/// What the export reads besides the pattern.
pub struct Export<'a> {
    pub pattern: &'a PatternData,
    /// MIDI channel = engine index ([re:00087f]).
    pub engine: usize,
    pub bpm: f64,
    pub time_sig: (u32, u32),
    /// [re:01dbd2].
    pub tempo_mult: f64,
    pub swing: &'a Swing,
    /// Engine swing depth.
    pub swing_depth: f64,
    /// Per pad: no layer with audio plays gated (playback mode < 2), so tied
    /// steps are not merged.
    pub one_shot: &'a [bool],
    /// Option `AutoMIDICC` (default on): graphs with data but no MIDI
    /// binding get a free CC.
    pub auto_cc: bool,
}

/// Graph lanes sent as CCs: 1..=92 except Repeats, Shift, Duration and
/// Link (mask 0x60c over the first 11).
pub fn cc_lane(l: usize) -> bool {
    (1..LANES).contains(&l) && !(l <= 10 && (0x60cu32 >> l) & 1 != 0)
}

#[derive(Debug, Clone)]
struct Note {
    start: f64,
    dur: f64,
    pitch: u8,
    vel: u8,
}

/// `(int)(v·127 ± 0.5)`.
fn to7(v: f32) -> u8 {
    let x = v * 127.0;
    ((x + if x < 0.0 { -0.5 } else { 0.5 }) as i32).clamp(0, 127) as u8
}

/// Normalised value to repeat count.
fn repeats(v: f32) -> u32 {
    (v * 7.0 + 1.0).floor().max(0.0) as u32
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The notes and CCs of the export, before writing. `cc_for(pad, lane,
/// allocate)` returns the CC of the lane's MIDI binding on the engine's
/// channel, or with `allocate` a new one (the first free CC;
/// None when all are taken).
fn collect(x: &Export, v1: bool, cc_for: &mut dyn FnMut(usize, usize, bool) -> Option<u8>, rng: &mut dyn FnMut() -> f32) -> (Vec<Note>, Vec<(f64, u8, u8)>) {
    if v1 {
        return collect_v1(x, cc_for);
    }
    let p = x.pattern;
    let steps = p.steps.max(1);
    // Steps to cover: the LCM of the step count and every shorter track length.
    let mut n = steps;
    for t in &p.tracks {
        if t.length > 0 && t.length < steps {
            let g = gcd(n, t.length);
            n = n.checked_div(g).map_or(0, |q| q * t.length);
        }
    }
    let q = 0.25 / p.resolution();
    let scale = q / x.tempo_mult.max(1e-9);
    let none_solo = !p.tracks.iter().any(|t| t.solo);
    let mut notes: Vec<Note> = Vec::new();
    let mut ccs: Vec<(f64, u8, u8)> = Vec::new();
    let mut first: Vec<Option<crate::pattern::StepData>> = vec![None; p.tracks.len()];
    let mut last_start = vec![0.0f64; p.tracks.len()];
    for (pad, track) in p.tracks.iter().enumerate() {
        let len = if track.length > 0 { track.length } else { steps };
        let one_shot = x.one_shot.get(pad).copied().unwrap_or(true);
        let mut i = 0u32;
        while i < n {
            let step = track.step(i % len).clone();
            if !step.on || track.mute || !(track.solo || none_solo) {
                i += 1;
                continue;
            }
            let prob = step.lanes[lane::PROBABILITY];
            if (prob - 1.0).abs() > 1e-6 {
                let w = rng().clamp(0.0, 1.0);
                if prob * 0.99 + 0.01 < w {
                    i += 1;
                    continue;
                }
            }
            let sw = x.swing.value(i % x.swing.steps.max(1));
            let shift = step.lanes[lane::SHIFT] as f64;
            if first[pad].is_none() {
                first[pad] = Some(step.clone());
            }
            let start = ((i as f64 + sw * x.swing_depth + (shift + shift - 1.0)) * scale).max(0.0);
            last_start[pad] = start;
            let mut dur = step.lanes[lane::DURATION] as f64 * q;
            if !one_shot && i + 1 < n {
                let mut next = track.step((i + 1) % len).clone();
                while next.on && next.tied() {
                    let d = next.lanes[lane::DURATION] as f64;
                    next = track.step((i + 2) % len).clone();
                    dur += d * q;
                    i += 1;
                    if i + 1 >= n {
                        break;
                    }
                }
            }
            // Step graphs, just before the note.
            let at = (start - 0.001).max(0.0);
            for l in (1..LANES).filter(|&l| cc_lane(l)) {
                let cc = cc_for(pad, l, false).or_else(|| (x.auto_cc && has_graph_data(track, l)).then(|| cc_for(pad, l, true)).flatten());
                if let Some(cc) = cc {
                    ccs.push((at, cc, to7(step.lanes[l])));
                }
            }
            let pitch = crate::live::PAD_NOTE_BASE as usize + pad;
            let vel = to7(step.lanes[lane::VELOCITY]);
            let r = repeats(step.lanes[lane::REPEATS]);
            if pitch < 128 {
                if r < 2 {
                    add_note(&mut notes, pitch as u8, vel, start, dur);
                } else {
                    let d = dur / r as f64;
                    for k in 0..r {
                        add_note(&mut notes, pitch as u8, vel, k as f64 * d + start, d);
                    }
                }
            }
            i += 1;
        }
    }
    // Reset each graph to the track's first played step one step after its
    // last note (minus a tick).
    let q2 = q - TICK;
    for (pad, f) in first.iter().enumerate() {
        let Some(f) = f.as_ref().filter(|f| f.on) else { continue };
        let t = last_start[pad] + q2;
        for l in (1..LANES).filter(|&l| cc_lane(l)) {
            if let Some(cc) = cc_for(pad, l, false) {
                ccs.push((t, cc, to7(f.lanes[l])));
            }
        }
    }
    (notes, ccs)
}

/// [re:3cb7c3]: any step whose lane differs from its
/// default.
fn has_graph_data(t: &crate::pattern::TrackData, l: usize) -> bool {
    let d = crate::pattern::StepData::off();
    t.steps.iter().any(|s| (s.lanes[l] - d.lanes[l]).abs() >= 1e-6)
}

/// v1's graph lanes ([re:a106a5]): 1..=76 of v1's step lanes but Repeats,
/// Shift, Duration and Link, in v1's order; here the lanes v1 has
/// (`cc_lane` without Octave, Probability, the loop points and the
/// envelope lanes).
pub fn cc_lane_v1(l: usize) -> bool {
    cc_lane(l) && !matches!(l, 5 | 16..=18 | 23..=38)
}

/// The original v1's export ([re:aa2636]): the pattern's step count, a
/// step past a shorter track off ([re:ff1ffa]); no probability draw; tied
/// steps always merged; the note time not clamped ([re:b46b2f]); graphs
/// 0.1 QN early, a note at 0 moved 0.007 QN later (a merged one a tick
/// shorter); notes added as they are ([re:3ec7ba]).
fn collect_v1(x: &Export, cc_for: &mut dyn FnMut(usize, usize, bool) -> Option<u8>) -> (Vec<Note>, Vec<(f64, u8, u8)>) {
    let p = x.pattern;
    let n = p.steps.max(1);
    let q = p.length_qn() / n as f64;
    let any_solo = p.tracks.iter().any(|t| t.solo);
    let mut notes: Vec<Note> = Vec::new();
    let mut ccs: Vec<(f64, u8, u8)> = Vec::new();
    let mut first: Vec<Option<crate::pattern::StepData>> = vec![None; p.tracks.len()];
    let mut last_start = vec![0.0f64; p.tracks.len()];
    for (pad, track) in p.tracks.iter().enumerate().take(16) {
        let len = if track.length > 0 { track.length } else { n };
        let step_at = |i: u32| if i < len { track.step(i).clone() } else { crate::pattern::StepData::off() };
        let mut i = 0u32;
        while i < n {
            let step = step_at(i);
            if !step.on || track.mute || (any_solo && !track.solo) {
                i += 1;
                continue;
            }
            let sw = x.swing.value(i % x.swing.steps.max(1));
            let shift = step.lanes[lane::SHIFT];
            let mut start = (1.0 / x.tempo_mult) * (i as f64 * q + ((shift + shift) - 1.0) as f64 * q + sw * x.swing_depth * q);
            if first[pad].is_none() {
                first[pad] = Some(step.clone());
            }
            last_start[pad] = start;
            let mut dur = step.lanes[lane::DURATION] as f64 * q;
            let mut merged = false;
            if i + 1 < n {
                let mut next = step_at(i + 1);
                while next.on && next.tied() {
                    dur += next.lanes[lane::DURATION] as f64 * q;
                    merged = true;
                    i += 1;
                    next = step_at(i + 1);
                    if i + 1 >= n {
                        break;
                    }
                }
            }
            let mut at = (start - 0.1).max(0.0);
            let mut moved = false;
            if start == at {
                at -= 0.1;
                moved = true;
                if at < 0.0 {
                    at += 0.1;
                    start += 0.007;
                }
            }
            for l in (1..LANES).filter(|&l| cc_lane_v1(l)) {
                let cc = cc_for(pad, l, false).or_else(|| (x.auto_cc && has_graph_data(track, l)).then(|| cc_for(pad, l, true)).flatten());
                if let Some(cc) = cc {
                    ccs.push((at, cc, to7(step.lanes[l])));
                }
            }
            if moved && merged {
                dur -= TICK;
            }
            let pitch = crate::live::PAD_NOTE_BASE as usize + pad;
            let vel = to7(step.lanes[lane::VELOCITY]);
            let r = repeats(step.lanes[lane::REPEATS]);
            if pitch < 128 {
                let (count, d) = if r < 2 { (1, dur) } else { (r, dur / r as f64) };
                for k in 0..count {
                    notes.push(Note { start: k as f64 * d + start, dur: d, pitch: pitch as u8, vel });
                }
            }
            i += 1;
        }
    }
    let q2 = q - TICK;
    for (pad, f) in first.iter().enumerate() {
        let Some(f) = f.as_ref().filter(|f| f.on) else { continue };
        let t = q2 + last_start[pad];
        for l in (1..LANES).filter(|&l| cc_lane_v1(l)) {
            if let Some(cc) = cc_for(pad, l, false) {
                ccs.push((t, cc, to7(f.lanes[l])));
            }
        }
    }
    (notes, ccs)
}

/// Adding a note: earlier notes of the same pitch that the new one
/// starts inside are shortened a tick at a time.
fn add_note(notes: &mut Vec<Note>, pitch: u8, vel: u8, start: f64, dur: f64) {
    for n in notes.iter_mut().rev().filter(|n| n.pitch == pitch) {
        let mut d = n.dur;
        while start <= d + n.start {
            d -= TICK;
            if d <= 0.0 {
                break;
            }
            n.dur = d;
        }
    }
    notes.push(Note { start, dur, pitch, vel });
}

fn var_len(mut v: u32, out: &mut Vec<u8>) {
    let mut buf = [0u8; 5];
    let mut n = 0;
    loop {
        buf[n] = (v & 0x7f) as u8 | if n > 0 { 0x80 } else { 0 };
        n += 1;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    for k in (0..n).rev() {
        out.push(buf[k]);
    }
}

/// The `.mid` bytes: format 1, one track (tempo, time signature, empty
/// copyright and name metas, then the events with running status).
/// `midi_export_v1` in the installed rules selects the original v1's
/// notes and graphs.
pub fn export(x: &Export, cc_for: &mut dyn FnMut(usize, usize, bool) -> Option<u8>, rng: &mut dyn FnMut() -> f32) -> Vec<u8> {
    export_as(x, dreist_dsp::rules::get().midi_export_v1, cc_for, rng)
}

fn export_as(x: &Export, v1: bool, cc_for: &mut dyn FnMut(usize, usize, bool) -> Option<u8>, rng: &mut dyn FnMut() -> f32) -> Vec<u8> {
    let (notes, ccs) = collect(x, v1, cc_for, rng);
    let ch = (x.engine & 15) as u8;
    // (QN, order, bytes): note offs before ons at the same time.
    let mut evs: Vec<(f64, u8, [u8; 3])> = Vec::new();
    for n in &notes {
        evs.push((n.start, 1, [0x90 | ch, n.pitch, n.vel]));
        evs.push((n.start + n.dur, 0, [0x80 | ch, n.pitch, n.vel]));
    }
    for &(t, cc, v) in &ccs {
        evs.push((t, 1, [0xb0 | ch, cc, v]));
    }
    evs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut trk = Vec::new();
    // Tempo / time signature at 0.
    let us = (60_000_000.0 / x.bpm.max(1.0)) as u32;
    trk.extend([0x00, 0xff, 0x51, 0x03, (us >> 16) as u8, (us >> 8) as u8, us as u8]);
    let den_pow = x.time_sig.1.max(1).trailing_zeros() as u8;
    trk.extend([0x00, 0xff, 0x58, 0x04, x.time_sig.0.clamp(1, 255) as u8, den_pow, 24, 8]);
    trk.extend([0x00, 0xff, 0x02, 0x00]);
    trk.extend([0x00, 0xff, 0x03, 0x00]);
    let mut last_tick = 0i64;
    let mut status = 0u8;
    for (t, _, b) in evs {
        let tick = (t * PPQ as f64) as i64;
        var_len((tick - last_tick).max(0) as u32, &mut trk);
        last_tick = tick.max(last_tick);
        if b[0] != status {
            trk.push(b[0]);
            status = b[0];
        }
        trk.extend([b[1], b[2]]);
    }
    trk.extend([0x00, 0xff, 0x2f, 0x00]);
    let mut out = Vec::with_capacity(trk.len() + 22);
    out.extend(b"MThd");
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((PPQ as u16).to_be_bytes());
    out.extend(b"MTrk");
    out.extend((trk.len() as u32).to_be_bytes());
    out.extend(trk);
    out
}

/// A note or controller of a MIDI file track, in QN.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Note { time: f64, dur: f64, pitch: u8, vel: u8 },
    Cc { time: f64, cc: u8, value: u8 },
}

/// [re:c7e6c1] (port): the tracks of a standard MIDI
/// file as notes (paired on/off) and controllers, times in QN.
pub fn read(bytes: &[u8]) -> Option<Vec<Vec<Event>>> {
    if bytes.len() < 14 || &bytes[..4] != b"MThd" {
        return None;
    }
    let be16 = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]);
    let be32 = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let hlen = be32(4) as usize;
    let division = be16(12);
    if division & 0x8000 != 0 || division == 0 {
        return None;
    }
    let ppq = division as f64;
    let mut pos = 8 + hlen;
    let mut tracks = Vec::new();
    while pos + 8 <= bytes.len() {
        let len = be32(pos + 4) as usize;
        let id = &bytes[pos..pos + 4];
        let body = bytes.get(pos + 8..(pos + 8 + len).min(bytes.len()))?;
        pos += 8 + len;
        if id != b"MTrk" {
            continue;
        }
        let mut evs = Vec::new();
        let mut open: Vec<(u8, u8, f64, u8)> = Vec::new();
        let (mut i, mut tick, mut status) = (0usize, 0u64, 0u8);
        let varlen = |i: &mut usize| -> u64 {
            let mut v = 0u64;
            while *i < body.len() {
                let b = body[*i];
                *i += 1;
                v = (v << 7) | (b & 0x7f) as u64;
                if b & 0x80 == 0 {
                    break;
                }
            }
            v
        };
        while i < body.len() {
            tick += varlen(&mut i);
            let t = tick as f64 / ppq;
            let mut b = *body.get(i)?;
            if b & 0x80 != 0 {
                i += 1;
                if b < 0xf0 {
                    status = b;
                }
            } else {
                b = status;
            }
            match b {
                0xff => {
                    let _kind = *body.get(i)?;
                    i += 1;
                    let l = varlen(&mut i) as usize;
                    i += l;
                }
                0xf0 | 0xf7 => {
                    let l = varlen(&mut i) as usize;
                    i += l;
                }
                _ => {
                    let n = if matches!(b & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
                    let d1 = *body.get(i).unwrap_or(&0);
                    let d2 = if n == 2 { *body.get(i + 1).unwrap_or(&0) } else { 0 };
                    i += n;
                    let ch = b & 15;
                    match b & 0xf0 {
                        0x90 if d2 > 0 => open.push((ch, d1, t, d2)),
                        0x80 | 0x90 => {
                            if let Some(k) = open.iter().position(|o| o.0 == ch && o.1 == d1) {
                                let (_, pitch, start, vel) = open.remove(k);
                                evs.push(Event::Note { time: start, dur: t - start, pitch, vel });
                            }
                        }
                        0xb0 => evs.push(Event::Cc { time: t, cc: d1, value: d2 }),
                        _ => {}
                    }
                }
            }
        }
        for (_, pitch, start, vel) in open {
            evs.push(Event::Note { time: start, dur: 0.0, pitch, vel });
        }
        evs.sort_by(|a, b| {
            let t = |e: &Event| match e {
                Event::Note { time, .. } | Event::Cc { time, .. } => *time,
            };
            t(a).total_cmp(&t(b))
        });
        tracks.push(evs);
    }
    Some(tracks)
}

/// A MIDI file's load info.
#[derive(Debug, Clone, Copy)]
struct LoadInfo {
    note: bool,
    pad: usize,
    step: i32,
    vel: f32,
    shift: f32,
    dur_frac: f32,
    dur_steps: i32,
    lane: usize,
    value: f32,
    used: bool,
}

const MAX_STEPS: i32 = 1024;

/// [re:1044d8] for the default MIDI map
/// (channel 0 notes 36..99 → pads 0..63): the file's most populous track
/// ([re:3ce638]) as a `StepPattern` node named
/// `name`, plus its step count. `cc_lanes(cc)` lists the (track, lane)
/// targets of MIDI-channel-1 CC bindings through 0xd9
/// (by message type).
pub fn import(bytes: &[u8], name: &str, cc_lanes: &dyn Fn(u8) -> Vec<(usize, usize)>) -> Option<dreist_formats::Node> {
    let tracks = read(bytes)?;
    let notes = |t: &Vec<Event>| t.iter().filter(|e| matches!(e, Event::Note { .. })).count();
    let track = tracks.iter().max_by_key(|t| notes(t))?;
    // MIDI file to load info [re:57f67e].
    let mut infos: Vec<LoadInfo> = Vec::new();
    let mut max_step = -1i32;
    let last = track.len().saturating_sub(1);
    for (k, e) in track.iter().enumerate() {
        let time = match e {
            Event::Note { time, .. } | Event::Cc { time, .. } => *time,
        };
        let x = time * 4.0;
        let rounded = (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32;
        let mut frac = x - x.trunc();
        let mut idx = x.trunc() as i32;
        if rounded != idx {
            frac -= 1.0;
            idx = rounded;
        }
        if k == last && idx & 15 == 0 {
            frac += 1.0;
            idx -= 1;
        }
        if idx as f64 >= 1023.0 || idx < 0 {
            continue;
        }
        let shift = (frac * 0.5 + 0.5) as f32;
        match *e {
            Event::Cc { cc, value, .. } if cc <= 128 => {
                for (pad, lane) in cc_lanes(cc) {
                    if pad < 64 {
                        infos.push(LoadInfo { note: false, pad, step: idx, vel: 0.0, shift, dur_frac: 0.0, dur_steps: 0, lane, value: value as f32 / 127.0, used: false });
                        max_step = max_step.max(idx);
                    }
                }
            }
            Event::Note { dur, pitch, vel, .. } if vel > 0 && (36..100).contains(&pitch) => {
                let d = dur * 4.0;
                let ip = d.trunc();
                let dur_frac = (d - ip) as f32;
                let dur_steps = ip as i32 + (ip > 1.0 && (d - ip) < 1.0) as i32;
                infos.push(LoadInfo { note: true, pad: pitch as usize - 36, step: idx, vel: vel as f32 / 127.0, shift, dur_frac, dur_steps, lane: 0, value: 0.0, used: false });
                max_step = max_step.max(idx);
                if dur_steps > 1 && idx < 1022 {
                    let mut s = idx + 1;
                    let mut n = 2;
                    loop {
                        max_step = max_step.max(s);
                        if dur_steps <= n {
                            break;
                        }
                        s += 1;
                        n += 1;
                        if s >= 1023 {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Load info to pattern [re:2fcb47]: per track, sorted by step.
    let defaults = crate::pattern::StepData::off();
    let mut steps: Vec<Vec<Option<crate::pattern::StepData>>> = vec![vec![None; MAX_STEPS as usize]; 64];
    for (pad, track_steps) in steps.iter_mut().enumerate() {
        let mut list: Vec<LoadInfo> = infos.iter().copied().filter(|i| i.pad == pad).collect();
        list.sort_by_key(|i| i.step);
        // Sorting [re:29145b]: one note per step, taken from the step
        // or a neighbour leaning into it; their CCs follow.
        let mut out: Vec<LoadInfo> = Vec::new();
        for s in 0..MAX_STEPS {
            let pick = list.iter().position(|i| i.note && !i.used && (i.step == s || (i.step == s + 1 && i.shift <= 0.5) || (i.step == s - 1 && i.shift >= 0.5)));
            let Some(j) = pick else { continue };
            list[j].used = true;
            let n = list[j];
            let shift = if s < n.step {
                n.shift + 0.5
            } else if s > n.step {
                n.shift - 0.5
            } else {
                n.shift
            };
            out.push(LoadInfo { step: s, shift, ..n });
            for c in list.iter_mut().filter(|c| !c.note && !c.used && c.step == n.step && (c.shift - n.shift).abs() < 0.01) {
                c.used = true;
                out.push(LoadInfo { step: s, ..*c });
            }
        }
        out.extend(list.iter().filter(|c| !c.note && !c.used).copied());
        // One track's load info to the pattern [re:95dd17].
        for i in out {
            let Some(slot) = track_steps.get_mut(i.step.clamp(0, MAX_STEPS - 1) as usize) else { continue };
            let st = slot.get_or_insert_with(|| defaults.clone());
            if !i.note {
                if let Some(v) = st.lanes.get_mut(i.lane) {
                    *v = i.value;
                }
                continue;
            }
            st.on = true;
            st.lanes[lane::LINK] = 0.0;
            st.lanes[lane::VELOCITY] = i.vel;
            st.lanes[lane::SHIFT] = i.shift;
            if i.dur_steps == 0 {
                st.lanes[lane::DURATION] = i.dur_frac;
            } else if i.dur_steps > 1 && i.step < 1022 {
                // As the original: the loop rewrites the same step, so it
                // ends up tied with the last fraction as its Duration.
                st.lanes[lane::LINK] = 1.0;
                st.lanes[lane::DURATION] = i.dur_frac;
            }
        }
    }
    // Step count: the next multiple of 16 from the last step used.
    let mut n = max_step.max(0);
    while n <= 0x3ff && n & 15 != 0 {
        n += 1;
    }
    let count = if n > 0 { n.min(MAX_STEPS) as usize } else { MAX_STEPS as usize };
    // Timing conversion [re:24c9ad]: a step leaning ≥ 0.75
    // into an empty next step moves there (Shift − 0.5).
    for track_steps in steps.iter_mut() {
        for i in 0..count.saturating_sub(1) {
            let lean = track_steps[i].as_ref().filter(|s| s.on).map(|s| s.lanes[lane::SHIFT]);
            let next_on = track_steps[i + 1].as_ref().is_some_and(|s| s.on);
            if let (Some(f), false) = (lean, next_on) {
                if f >= 0.75 {
                    let mut moved = track_steps[i].take().unwrap();
                    moved.lanes[lane::SHIFT] = f - 0.5;
                    track_steps[i + 1] = Some(moved);
                }
            }
        }
    }
    let mut sp = dreist_formats::Node::new("StepPattern", Some("StepPattern"));
    sp.set("PatternSteps", count);
    let mut pname = dreist_formats::Node::new("VFixedString_64", Some("PatternName"));
    pname.set("string", name);
    sp.children.push(pname);
    for (pad, track_steps) in steps.iter().enumerate() {
        let used: Vec<(usize, &crate::pattern::StepData)> = track_steps.iter().take(count).enumerate().filter_map(|(k, s)| s.as_ref().map(|s| (k, s))).collect();
        if used.is_empty() {
            continue;
        }
        let mut tr = dreist_formats::Node::new("Track", Some(&format!("Track{pad}")));
        for (k, s) in used {
            let mut st = dreist_formats::Node::new("Step", Some(&format!("S{k}")));
            for (l, lname) in dreist_formats::STEP_LANES.iter().enumerate() {
                if (s.lanes[l] - defaults.lanes[l]).abs() > 1e-9 {
                    st.set(lname, s.lanes[l]);
                }
            }
            st.set("State", if s.on { "true" } else { "false" });
            tr.children.push(st);
        }
        sp.children.push(tr);
    }
    Some(sp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{PatternData, StepData, TrackData};

    /// The notes and CCs of a parsed export: (tick, status, data1, data2).
    fn events(bytes: &[u8]) -> Vec<(u32, u8, u8, u8)> {
        let trk = &bytes[22..];
        let mut i = 0;
        let (mut tick, mut status, mut out) = (0u32, 0u8, Vec::new());
        while i < trk.len() {
            let mut d = 0u32;
            loop {
                let b = trk[i];
                i += 1;
                d = (d << 7) | (b & 0x7f) as u32;
                if b & 0x80 == 0 {
                    break;
                }
            }
            tick += d;
            if trk[i] == 0xff {
                let len = trk[i + 2] as usize;
                i += 3 + len;
                continue;
            }
            if trk[i] & 0x80 != 0 {
                status = trk[i];
                i += 1;
            }
            out.push((tick, status, trk[i], trk[i + 1]));
            i += 2;
        }
        out
    }

    #[test]
    fn v1_export_rules() {
        let mut p = PatternData::empty();
        p.steps = 16;
        p.tracks = (0..64).map(|_| TrackData::empty(16)).collect();
        let mut s = StepData::off();
        s.on = true;
        // Step 0 and step 4 tied to step 5; probability 0 (v1 has no draw).
        s.lanes[lane::PROBABILITY] = 0.0;
        p.tracks[0].steps[0] = s.clone();
        p.tracks[0].steps[4] = s.clone();
        let mut tied = s.clone();
        tied.lanes[lane::LINK] = 1.0;
        p.tracks[0].steps[5] = tied;
        // A pan graph bound to CC 20, an octave graph bound to CC 21 (no v1 lane).
        p.tracks[0].steps[4].lanes[1] = 0.25;
        // Track 1 is 4 steps long: its step 8 is past the end.
        p.tracks[1].length = 4;
        p.tracks[1].steps[8] = s.clone();
        let swing = Swing::default();
        let x = Export { pattern: &p, engine: 0, bpm: 120.0, time_sig: (3, 4), tempo_mult: 1.0, swing: &swing, swing_depth: 0.0, one_shot: &[true; 64], auto_cc: false };
        let mut cc = |_: usize, l: usize, _: bool| match l {
            1 => Some(20),
            5 => Some(21),
            _ => None,
        };
        let ev = events(&export_as(&x, true, &mut cc, &mut || 1.0));
        let ons: Vec<_> = ev.iter().filter(|e| e.1 == 0x90).map(|e| (e.0, e.2)).collect();
        // The note at 0 moves 0.007 QN (3 ticks); step 4 = 1 QN; no track-1 note.
        assert_eq!(ons, vec![(3, 36), (480, 36)]);
        // Step 4's off: two 16ths (merged, one-shot or not) = 240 ticks.
        assert!(ev.iter().any(|e| e.1 == 0x80 && e.0 == 720));
        // Pan CC 0.1 QN (48 ticks) before step 4; never the octave lane.
        assert!(ev.iter().any(|e| e.1 == 0xb0 && e.2 == 20 && e.3 == 32 && e.0 == 432));
        assert!(!ev.iter().any(|e| e.1 == 0xb0 && e.2 == 21));
        // The v2 rules draw (probability 0 drops the notes) and lead by 0.001 QN.
        let ev2 = events(&export_as(&x, false, &mut cc, &mut || 1.0));
        assert!(!ev2.iter().any(|e| e.1 == 0x90));
    }

    #[test]
    fn exports_notes_with_lengths_and_ccs() {
        let mut p = PatternData::empty();
        p.steps = 16;
        p.tracks = (0..64).map(|_| TrackData::empty(16)).collect();
        let mut s = StepData::off();
        s.on = true;
        s.lanes[lane::VELOCITY] = 1.0;
        p.tracks[0].steps[0] = s.clone();
        p.tracks[0].steps[4] = s.clone();
        // Pan lane differs: auto CC.
        p.tracks[0].steps[4].lanes[6] = 0.25;
        let swing = Swing::default();
        let x = Export { pattern: &p, engine: 2, bpm: 120.0, time_sig: (4, 4), tempo_mult: 1.0, swing: &swing, swing_depth: 0.0, one_shot: &[true; 64], auto_cc: true };
        let mut alloc = Vec::new();
        let bytes = export(
            &x,
            &mut |pad, l, new| {
                if new {
                    alloc.push((pad, l));
                    Some(1)
                } else {
                    alloc.contains(&(pad, l)).then_some(1)
                }
            },
            &mut || 0.0,
        );
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(u16::from_be_bytes([bytes[12], bytes[13]]), 480);
        assert_eq!(alloc, vec![(0, 6)], "a free CC for the pan graph");
        // Note on, channel 3 (engine 2), C2 + 0 = 36, velocity 127.
        let on = bytes.windows(3).position(|w| w == [0x92, 36, 127]).expect("note on");
        assert!(on > 22);
        assert!(bytes.windows(3).any(|w| w == [0xb2, 1, 32]), "pan 0.25 → 32");
        assert_eq!(&bytes[bytes.len() - 3..], &[0xff, 0x2f, 0x00]);
    }

    #[test]
    fn export_then_import_round_trips_notes() {
        let mut p = PatternData::empty();
        p.steps = 16;
        p.tracks = (0..64).map(|_| TrackData::empty(16)).collect();
        let mut s = StepData::off();
        s.on = true;
        s.lanes[lane::VELOCITY] = 1.0;
        for k in [0, 4, 8, 12] {
            p.tracks[2].steps[k] = s.clone();
        }
        p.tracks[5].steps[6] = s.clone();
        let swing = Swing::default();
        let x = Export { pattern: &p, engine: 0, bpm: 120.0, time_sig: (4, 4), tempo_mult: 1.0, swing: &swing, swing_depth: 0.0, one_shot: &[true; 64], auto_cc: false };
        let bytes = export(&x, &mut |_, _, _| None, &mut || 0.0);
        let tracks = read(&bytes).unwrap();
        assert_eq!(tracks[0].iter().filter(|e| matches!(e, Event::Note { .. })).count(), 5);
        let sp = import(&bytes, "Test", &|_| Vec::new()).unwrap();
        let back = PatternData::from_node(&sp);
        assert_eq!(back.steps, 16);
        for k in [0, 4, 8, 12] {
            assert!(back.tracks[2].steps[k].on, "step {k}");
        }
        assert!(back.tracks[5].steps[6].on);
        assert!(!back.tracks[2].steps[1].on);
    }

    #[test]
    fn same_pitch_overlap_shortens_the_earlier_note() {
        let mut v = Vec::new();
        add_note(&mut v, 36, 100, 0.0, 1.0);
        add_note(&mut v, 36, 100, 0.5, 1.0);
        assert!(v[0].dur < 0.5 && v[0].dur > 0.49, "{}", v[0].dur);
    }
}
