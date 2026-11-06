//! REX files (`.rx2`, `.rex`, `.rcy`, `.rex2`, `.rcso`; [re:c0f57c]
//! [re:dcca9f], see the dreist-re sampler notes "REX").
//!
//! v2 links Propellerhead's REX SDK 1.7.0 and loads the user's
//! "REX Shared Library.bundle" ([re:273ba9]). That library has no
//! arm64 code, so the port runs a small x86_64 helper (`rex/rex_helper.c`,
//! under Rosetta) that makes the same calls and streams the slices back.
//!
//! [re:ed55c5] / slice rendering [re:e4edba]:
//! output rate 44100; each slice rendered on its own; its position is
//! the sample offset of `ppq` at time base 15360, original tempo / 1000, 44100 Hz.
//! The composite [re:655d99] lays the slices end to
//! end (slice markers at their starts); a path ending in ".truncated"
//! ([re:e54ca4]) mixes each slice in at its
//! musical position in a buffer of the loop's PPQ length.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// `REXInfo` plus the rendered slices (position in PPQ, channels).
#[derive(Clone, Debug, PartialEq)]
pub struct Rex {
    pub channels: usize,
    pub slice_count: usize,
    pub tempo: i32,
    pub original_tempo: i32,
    pub ppq_length: i32,
    pub sign_num: i32,
    pub sign_den: i32,
    pub bit_depth: i32,
    pub slices: Vec<(i32, Vec<Vec<f32>>)>,
}

/// The decoded composite.
#[derive(Clone, Debug, PartialEq)]
pub struct Composite {
    pub left: Vec<f32>,
    pub right: Option<Vec<f32>>,
    /// Slice starts (frames), sorted.
    pub slices: Vec<usize>,
    /// Original tempo in BPM.
    pub tempo: f32,
    /// Bars and beats of the loop (PPQ length / 61440 ×
    /// denominator, over the numerator).
    pub bars: f32,
}

const HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dreist-rex-helper"));

/// Output sample rate (`REXSetOutputSampleRate(44100)`).
pub const SAMPLE_RATE: f32 = 44100.0;

/// [re:c0f57c].
pub fn is_rex(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["rcy", "rx2", "rex", "rex2", "rcso", "truncated"].iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The file behind a ".truncated" path, and whether it was one.
pub fn strip_truncated(path: &Path) -> (PathBuf, bool) {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("truncated")) { (path.with_extension(""), true) } else { (path.to_path_buf(), false) }
}

/// The helper extracted next to the other temporary files.
fn helper() -> Option<PathBuf> {
    if HELPER.is_empty() {
        return None;
    }
    let p = std::env::temp_dir().join(format!("dreist-rex-helper-{}", HELPER.len()));
    if std::fs::metadata(&p).ok().map(|m| m.len() as usize) != Some(HELPER.len()) {
        let tmp = p.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, HELPER).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).ok()?;
        }
        std::fs::rename(&tmp, &p).ok()?;
    }
    Some(p)
}

fn parse(d: &[u8]) -> Option<Rex> {
    if d.get(..4)? != b"GRX1" {
        return None;
    }
    let mut pos = 4;
    let int = |pos: &mut usize| -> Option<i32> {
        let v = i32::from_le_bytes(d.get(*pos..*pos + 4)?.try_into().ok()?);
        *pos += 4;
        Some(v)
    };
    let (channels, _rate, count) = (int(&mut pos)?, int(&mut pos)?, int(&mut pos)?);
    let (tempo, original_tempo, ppq_length) = (int(&mut pos)?, int(&mut pos)?, int(&mut pos)?);
    let (sign_num, sign_den, bit_depth) = (int(&mut pos)?, int(&mut pos)?, int(&mut pos)?);
    let chans = if channels == 2 { 2 } else { 1 };
    let mut slices = Vec::new();
    for _ in 0..count.max(0) {
        let ppq = int(&mut pos)?;
        let len = int(&mut pos)?.max(0) as usize;
        let mut ch = Vec::new();
        for _ in 0..chans {
            let bytes = d.get(pos..pos + len * 4)?;
            ch.push(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect());
            pos += len * 4;
        }
        slices.push((ppq, ch));
    }
    Some(Rex { channels: chans, slice_count: count.max(0) as usize, tempo, original_tempo, ppq_length, sign_num, sign_den, bit_depth, slices })
}

/// Decode `path` (without ".truncated") through the helper; each file
/// once per process.
pub fn decode(path: &Path) -> Option<Arc<Rex>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<Arc<Rex>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(r) = cache.lock().ok()?.get(path) {
        return r.clone();
    }
    let out = std::process::Command::new(helper()?).arg(path).output().ok()?;
    let r = out.status.success().then(|| parse(&out.stdout)).flatten().map(Arc::new);
    cache.lock().ok()?.insert(path.to_path_buf(), r.clone());
    r
}

/// [re:c30adb].
fn samples_for_ppq(ppq: i32, bpm: f64) -> usize {
    if bpm <= 0.0 {
        return 0;
    }
    (ppq as f64 / 15360.0 * 60.0 / bpm * SAMPLE_RATE as f64).max(0.0) as usize
}

/// The composite for `path` (".truncated" mixes slices at their musical
/// positions in the loop's length).
pub fn load(path: &Path) -> Option<Composite> {
    let (file, truncated) = strip_truncated(path);
    let rex = decode(&file)?;
    let bpm = rex.original_tempo as f64 * 0.001;
    let len = if truncated { samples_for_ppq(rex.ppq_length, bpm) } else { rex.slices.iter().map(|s| s.1[0].len()).sum() };
    let mut chans = vec![vec![0.0f32; len]; rex.channels];
    let mut marks = Vec::new();
    let mut at = 0usize;
    for (ppq, data) in &rex.slices {
        let start = if truncated { samples_for_ppq(*ppq, bpm) } else { at };
        if start < len || len == 0 {
            marks.push(start);
        }
        for (c, src) in data.iter().enumerate() {
            for (i, v) in src.iter().enumerate() {
                let k = start + i;
                if k >= len {
                    break;
                }
                if truncated {
                    chans[c][k] += v;
                } else {
                    chans[c][k] = *v;
                }
            }
        }
        at += data[0].len();
    }
    marks.sort_unstable();
    let quarters = rex.ppq_length as f32 / 15360.0;
    let bars = if rex.sign_num > 0 { (quarters / 4.0 * rex.sign_den as f32 / rex.sign_num as f32).floor() } else { 0.0 };
    let mut it = chans.into_iter();
    let left = it.next().unwrap_or_default();
    Some(Composite { left, right: it.next(), slices: marks, tempo: bpm as f32, bars })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_rex_loop_through_the_helper() {
        let home = std::env::var("HOME").unwrap_or_default();
        let f = PathBuf::from(home).join(dreist_formats::legacy::LITE_FACTORY).join("Loops/Breaks/130 Whitekit 03 KS.rx2");
        if !f.exists() || HELPER.is_empty() {
            return;
        }
        let Some(c) = load(&f) else { return };
        assert_eq!(c.slices.len(), 14);
        assert_eq!(c.slices[0], 0);
        assert!((c.tempo - 130.052).abs() < 1e-3);
        assert_eq!(c.bars, 1.0);
        let t = load(&f.with_extension("rx2.truncated")).unwrap();
        // One 4/4 bar at 130.052 BPM.
        assert_eq!(t.left.len(), (4.0 * 60.0 / 130.052 * 44100.0) as usize);
    }
}
