//! v2 engine runtime ported from the 2.1.2.13 VST engine: step sequencer,
//! song sequencer, voice allocation and the layer → pad → engine → master mix.
//! See the dreist-re sequencer, song and mixer notes.

pub mod audio;
pub mod binding;
pub mod engine;
pub mod export;
pub mod fx;
pub mod gsa;
pub mod harness;
pub mod hold;
pub mod live;
pub mod rex;
// Generated table: kept as written.
#[rustfmt::skip]
pub mod message_names;
pub mod metronome;
pub mod midifile;
pub mod modulation;
pub mod pattern;
pub mod sampler;
pub mod scene;
pub mod sequencer;
pub mod song;
pub mod songrec;
pub mod tap;
pub mod velsplit;

pub use engine::{Dreist, Meters, RenderOptions, Route, SUB_OUTPUTS, StepWrite};

/// Write a 32-bit float stereo WAV.
pub fn write_wav(path: &std::path::Path, sr: u32, left: &[f32], right: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let frames = left.len() as u32;
    let data_len = frames * 8;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 8).to_le_bytes())?;
    f.write_all(&8u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for (l, r) in left.iter().zip(right) {
        f.write_all(&l.to_le_bytes())?;
        f.write_all(&r.to_le_bytes())?;
    }
    f.flush()
}

/// Render `frames` of a project from the start in host blocks of `block` frames.
/// The transport starts first when stopped.
pub fn render(g: &mut Dreist, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    if !g.is_playing() {
        g.start();
    }
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut pos = 0;
    while pos < frames {
        let end = (pos + block).min(frames);
        let (l, r) = (&mut left[pos..end], &mut right[pos..end]);
        g.process(l, r);
        pos = end;
    }
    (left, right)
}

/// Offline render with `subs` stereo sub outputs: (main left, main right, subs).
pub fn render_multi(g: &mut Dreist, frames: usize, block: usize, subs: usize) -> (Vec<f32>, Vec<f32>, Vec<[Vec<f32>; 2]>) {
    render_multi_progress(g, frames, block, subs, &mut |_| true).expect("not aborted")
}

/// [`render_multi`] reporting the rendered fraction after each block;
/// `progress` returning false aborts (None).
pub fn render_multi_progress(g: &mut Dreist, frames: usize, block: usize, subs: usize, progress: &mut dyn FnMut(f32) -> bool) -> Option<(Vec<f32>, Vec<f32>, Vec<[Vec<f32>; 2]>)> {
    if !g.is_playing() {
        g.start();
    }
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut out: Vec<[Vec<f32>; 2]> = (0..subs).map(|_| [vec![0.0; frames], vec![0.0; frames]]).collect();
    let mut pos = 0;
    while pos < frames {
        let end = (pos + block).min(frames);
        let mut views: Vec<[&mut [f32]; 2]> = out.iter_mut().map(|[a, b]| [&mut a[pos..end], &mut b[pos..end]]).collect();
        g.process_multi(&mut left[pos..end], &mut right[pos..end], &mut views);
        pos = end;
        if !progress(pos as f32 / frames.max(1) as f32) {
            return None;
        }
    }
    Some((left, right, out))
}
