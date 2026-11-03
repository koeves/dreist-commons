//! Export as audio ([re:a3f6c0] / [re:8c1655], see the dreist-re GUI notes
//! "Export panel"): offline render of the song or the current patterns,
//! as a stereo mixdown, one output or every assigned output, with tail,
//! loop, normalisation and auto-crop options, saved as 24-bit WAV.

use std::path::{Path, PathBuf};

use dreist_formats::Node;

use crate::{Dreist, RenderOptions, SUB_OUTPUTS, audio, render_multi_progress};

/// `SourceButtonGroup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// "All Engine Patterns": every engine's current pattern.
    AllEngines,
    /// "Current Engine Pattern".
    Engine(usize),
    Song,
}

/// `OutputTypeButtonGroup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    /// "Stereo Mixdown".
    Mixdown,
    /// "Single Output": 0 = main, n = Sub output n.
    Single(usize),
    /// "All assigned outputs (multiple files)".
    AllOutputs,
}

/// `NormalizeComboBox`: "Prompt if clipping detected", "Normalize if
/// clipping detected", "Always Normalize".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalize {
    Prompt,
    IfClipping,
    Always,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub source: Source,
    pub files: Files,
    pub normalize: Normalize,
    /// `TailTextBox` seconds.
    pub tail: f64,
    pub looped: bool,
    pub auto_crop: bool,
    /// The user's engine options (layer fall-through, aux pre-fade,
    /// pre-delay units), so the export sounds like playback.
    pub render: RenderOptions,
}

impl Default for Options {
    fn default() -> Self {
        Options { source: Source::Song, files: Files::Mixdown, normalize: Normalize::IfClipping, tail: 2.0, looped: false, auto_crop: false, render: RenderOptions::default() }
    }
}

/// Export result: files written and clipped samples (left unnormalised in
/// Prompt mode; the original asks before writing).
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub files: Vec<PathBuf>,
    pub clipped: usize,
}

/// Output names ([re:59154a]): main, then
/// "Sub output n".
pub fn output_name(o: usize) -> String {
    if o == 0 { "Main".into() } else { format!("Sub output {o}") }
}

/// Outputs any pad, layer or engine is routed to, main first.
pub fn assigned_outputs(doc: &Node) -> Vec<usize> {
    fn walk(n: &Node, out: &mut Vec<usize>) {
        if let Some(k) = n.attr("OutputLocation").and_then(|v| v.trim().strip_prefix("Sub output ")).and_then(|k| k.trim().parse::<usize>().ok()) {
            if (1..=SUB_OUTPUTS).contains(&k) && !out.contains(&k) {
                out.push(k);
            }
        }
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut v = vec![0];
    walk(doc, &mut v);
    v[1..].sort_unstable();
    v
}

/// 24-bit PCM stereo WAV ([re:12499e]).
pub fn write_wav24(path: &Path, sr: u32, left: &[f32], right: &[f32]) -> std::io::Result<()> {
    write_wav24_channels(path, sr, &[left, right])
}

/// 24-bit PCM WAV with any number of (equal length) channels.
pub fn write_wav24_channels(path: &Path, sr: u32, chans: &[&[f32]]) -> std::io::Result<()> {
    use std::io::Write;
    let ch = chans.len().max(1) as u32;
    let frames = chans.first().map_or(0, |c| c.len()) as u32;
    let data_len = frames * 3 * ch;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&(ch as u16).to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 3 * ch).to_le_bytes())?;
    f.write_all(&((3 * ch) as u16).to_le_bytes())?;
    f.write_all(&24u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for i in 0..frames as usize {
        for c in chans {
            let v = c.get(i).copied().unwrap_or(0.0);
            let s = (v.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
            f.write_all(&s.to_le_bytes()[..3])?;
        }
    }
    f.flush()
}

/// [re:a61043] length: song length × samples per beat, or for
/// patterns the longest `ceil(steps × sr / (multiplier × bpm / 15))` of the
/// engines whose patterns are on.
pub fn body_length(g: &Dreist, source: Source) -> usize {
    match source {
        Source::Song => g.song_length_samples().unwrap_or(0) as usize,
        Source::AllEngines | Source::Engine(_) => {
            let only = if let Source::Engine(e) = source { Some(e) } else { None };
            (0..crate::engine::ENGINES)
                .filter(|e| only.is_none_or(|o| o == *e))
                .filter_map(|e| g.export_pattern_steps(e))
                .map(|(steps, mult)| (steps as f64 * (g.sample_rate() as f64 / (mult * g.tempo() / 15.0))).ceil() as usize)
                .max()
                .unwrap_or(0)
        }
    }
}

/// [re:3ecd83]: in the tail, cut after the last sample
/// above −96 dB (1.5849e-5) plus 10 ms, fading that 10 ms out.
fn auto_crop(l: &mut Vec<f32>, r: &mut Vec<f32>, tail_start: usize, sr: f32) {
    let fade = (sr * 0.01) as usize;
    let n = l.len();
    if tail_start >= n {
        return;
    }
    let last = (tail_start..n).rev().find(|&i| l[i].abs() > 1.584_893_2e-5 || r[i].abs() > 1.584_893_2e-5);
    let end = match last {
        Some(i) => (i + fade).min(n),
        None => tail_start,
    };
    if let Some(i) = last {
        if i + fade < n {
            for k in 0..fade {
                let g = 1.0 - k as f32 / fade as f32;
                l[i + k] *= g;
                r[i + k] *= g;
            }
        }
    }
    l.truncate(end);
    r.truncate(end);
}

/// A finished render waiting to be written ([re:8c1655] between rendering
/// and writing).
pub struct Rendered {
    bufs: Vec<(Vec<f32>, Vec<f32>)>,
    outputs: Vec<usize>,
    body: usize,
    loop_len: usize,
    sr: u32,
    opts: Options,
    /// Peak over every buffer.
    pub peak: f32,
    /// Samples above 1.0 and samples in all.
    pub clipped: usize,
    pub total: usize,
}

/// Render for export. `progress` gets the rendered fraction and returns
/// false to abort ([re:d5167e]), which is the error "aborted".
pub fn render_export(doc: &Node, samples: &Path, sr: u32, opts: &Options, progress: &mut dyn FnMut(f32) -> bool) -> Result<Rendered, String> {
    let mut doc = doc.clone();
    // Patterns, not the song, for pattern sources; other engines' patterns
    // off for "Current Engine Pattern".
    if opts.source != Source::Song {
        if let Some(s) = doc.children.iter_mut().find(|c| c.class == "Song") {
            s.set("SongActive", "false");
        }
        set_attr_all(&mut doc, "SongActive", "false");
    }
    if let Source::Engine(only) = opts.source {
        for eng in doc.children.iter_mut().filter(|c| c.class == "Engine") {
            let idx = eng.name.as_deref().and_then(|n| n.strip_prefix("Engine")).and_then(|n| n.parse::<usize>().ok());
            if idx != Some(only) {
                let st = match eng.children.iter_mut().position(|c| c.class == "EngineState") {
                    Some(i) => &mut eng.children[i],
                    None => {
                        eng.children.push(Node::new("EngineState", Some("EngineState")));
                        eng.children.last_mut().unwrap()
                    }
                };
                st.set("PatternPower", "false");
            }
        }
    }
    let mut cache = audio::SampleCache::default();
    let mut g = Dreist::from_document(&doc, samples, sr as f32, opts.render, &mut cache).ok_or("document did not build")?;
    let mut body = body_length(&g, opts.source);
    if body == 0 {
        return Err("nothing to export".into());
    }
    let loop_len = body;
    if opts.looped {
        body *= 2;
    }
    let tail = if opts.tail > 0.0 { (sr as f64 * opts.tail) as usize } else { 0 };
    let frames = body + tail;
    let outputs: Vec<usize> = match opts.files {
        Files::Mixdown => vec![0],
        Files::Single(o) => vec![o],
        Files::AllOutputs => assigned_outputs(&doc),
    };
    // The mixdown sums every output, so it renders them all.
    let subs = if opts.files == Files::Mixdown { SUB_OUTPUTS } else { outputs.iter().copied().max().unwrap_or(0).min(SUB_OUTPUTS) };
    g.start();
    let (ml, mr, sub) = render_multi_progress(&mut g, frames, 512, subs, progress).ok_or("aborted")?;
    let bufs: Vec<(Vec<f32>, Vec<f32>)> = outputs
        .iter()
        .map(|&o| match (opts.files, o) {
            // The mixdown sums every output (the multi-export mixdown flag).
            (Files::Mixdown, _) => {
                let (mut l, mut r) = (ml.clone(), mr.clone());
                for [a, b] in &sub {
                    l.iter_mut().zip(a).for_each(|(x, y)| *x += y);
                    r.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                }
                (l, r)
            }
            (_, 0) => (ml.clone(), mr.clone()),
            (_, k) => (sub[k - 1][0].clone(), sub[k - 1][1].clone()),
        })
        .collect();
    // Peaks and clipped counts (end of rendering).
    let peak = bufs.iter().flat_map(|(l, r)| l.iter().chain(r)).fold(0.0f32, |m, v| m.max(v.abs()));
    let clipped = bufs.iter().flat_map(|(l, r)| l.iter().chain(r)).filter(|v| v.abs() > 1.0).count();
    let total = bufs.iter().map(|(l, r)| l.len() + r.len()).sum();
    Ok(Rendered { bufs, outputs, body, loop_len, sr, opts: opts.clone(), peak, clipped, total })
}

impl Rendered {
    /// [re:6fb3f8] in Prompt mode: with clipped
    /// samples (and a peak above 1 or no samples counted) the question to
    /// ask before writing.
    pub fn prompt(&self) -> Option<String> {
        if self.opts.normalize != Normalize::Prompt || self.clipped == 0 || !(self.peak > 1.0 || self.total == 0) {
            return None;
        }
        let pct = self.clipped as f64 * 100.0 / self.total.max(1) as f64;
        let db = 20.0 * (self.peak as f64).log10();
        Some(format!("{pct:.1}% of exported samples will be clipped ({} of {}, max level +{db:.2}dB) unless you normalize. Should the export be normalized?", self.clipped, self.total))
    }

    /// Write the files (after the prompt):
    /// `normalise` is the prompt's answer; without one the mode decides
    /// (normalising [re:b989c3]: 0.998 / overall peak). `out` is the chosen
    /// path; several outputs append the output name to its stem, loop mode
    /// writes `_start` / main / `_tail`.
    pub fn finish(self, out: &Path, normalise: Option<bool>) -> Result<Report, String> {
        let Rendered { mut bufs, outputs, body, loop_len, sr, opts, peak, clipped, .. } = self;
        let normalise = normalise.unwrap_or(match opts.normalize {
            Normalize::Always => true,
            Normalize::IfClipping => clipped > 0,
            Normalize::Prompt => false,
        });
        if normalise && peak >= 1e-5 {
            let gain = 0.998 / peak;
            for (l, r) in &mut bufs {
                l.iter_mut().chain(r.iter_mut()).for_each(|v| *v *= gain);
            }
        }
        let stem = out.with_extension("");
        let mut report = Report { files: Vec::new(), clipped: if normalise { 0 } else { clipped } };
        let save = |p: PathBuf, l: &[f32], r: &[f32], report: &mut Report| -> Result<(), String> {
            write_wav24(&p, sr, l, r).map_err(|e| format!("Failed to save audio buffer to disk. {e}"))?;
            report.files.push(p);
            Ok(())
        };
        for (i, (mut l, mut r)) in bufs.into_iter().enumerate() {
            if opts.auto_crop {
                auto_crop(&mut l, &mut r, body, sr as f32);
            }
            let base = if outputs.len() > 1 { PathBuf::from(format!("{}{}", stem.display(), output_name(outputs[i]))) } else { stem.clone() };
            let path = |suffix: &str| PathBuf::from(format!("{}{suffix}.wav", base.display()));
            if !opts.looped {
                save(path(""), &l, &r, &mut report)?;
                continue;
            }
            // Loop mode: first pass, second pass (the loop),
            // and what rings past it.
            let n = l.len();
            let a = loop_len.min(n);
            let b = (2 * loop_len).min(n);
            save(path("_start"), &l[..a], &r[..a], &mut report)?;
            save(path(""), &l[a..b], &r[a..b], &mut report)?;
            if n > b {
                save(path("_tail"), &l[b..], &r[b..], &mut report)?;
            }
        }
        Ok(report)
    }
}

/// Render and save without asking (Prompt mode writes unnormalised).
pub fn export(doc: &Node, samples: &Path, out: &Path, sr: u32, opts: &Options) -> Result<Report, String> {
    render_export(doc, samples, sr, opts, &mut |_| true)?.finish(out, None)
}

fn set_attr_all(n: &mut Node, key: &str, value: &str) {
    if n.attr(key).is_some() {
        n.set(key, value);
    }
    for c in &mut n.children {
        set_attr_all(c, key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_crop_cuts_the_silent_tail() {
        let mut l = vec![0.5f32; 1000];
        l.extend(std::iter::repeat_n(0.0, 2000));
        let mut r = l.clone();
        auto_crop(&mut l, &mut r, 800, 1000.0);
        assert_eq!(l.len(), 1009);
        assert!(l[1008] < 0.5);
    }

    #[test]
    fn assigned_outputs_lists_routed_subs() {
        let mut doc = Node::new(dreist_formats::legacy::ROOT_ELEMENT, None);
        let mut p = Node::new("Pad", Some("Pad0"));
        p.set("OutputLocation", "Sub output 3");
        doc.children.push(p);
        let mut q = Node::new("Pad", Some("Pad1"));
        q.set("OutputLocation", "Master");
        doc.children.push(q);
        assert_eq!(assigned_outputs(&doc), vec![0, 3]);
    }

    /// Pads routed to a sub output are part of the stereo mixdown.
    #[test]
    fn mixdown_includes_sub_outputs() {
        let p = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(dreist_formats::legacy::CONTENT_DIR).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
        let Ok(mut proj) = dreist_formats::Project::load(&p) else { return };
        fn route(n: &mut Node) {
            if n.class == "Pad" {
                n.set("OutputLocation", "Sub output 2");
            }
            n.children.iter_mut().for_each(route);
        }
        route(&mut proj.raw);
        let opts = Options { source: Source::Engine(0), tail: 0.0, ..Options::default() };
        let r = render_export(&proj.raw, &dreist_formats::model::samples_dir(&p), 44100, &opts, &mut |_| true).unwrap();
        assert!(r.peak > 0.01, "mixdown is silent (peak {})", r.peak);
    }

    #[test]
    fn exports_the_current_pattern_looped() {
        let p = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(dreist_formats::legacy::CONTENT_DIR).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
        let Ok(proj) = dreist_formats::Project::load(&p) else { return };
        let dir = std::env::temp_dir().join(format!("dreist-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = Options { source: Source::Engine(0), looped: true, tail: 1.0, normalize: Normalize::Always, ..Options::default() };
        let r = export(&proj.raw, &dreist_formats::model::samples_dir(&p), &dir.join("out.wav"), 44100, &opts).unwrap();
        let names: Vec<String> = r.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["out_start.wav", "out.wav", "out_tail.wav"]);
        let size = |n: &str| std::fs::metadata(dir.join(n)).unwrap().len();
        assert_eq!(size("out_start.wav"), size("out.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
