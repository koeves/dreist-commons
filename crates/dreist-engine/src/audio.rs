//! Sample file loading (WAV/AIFF) into `dreist_dsp::voice::SampleData`.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dreist_dsp::voice::SampleData;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

/// Decode a whole file to de-interleaved f32 (integer PCM scaled by 2^-(bits-1)).
pub fn load(path: &Path) -> Result<SampleData, LoadError> {
    load_unless(path, &|| false)
}

/// [`load`], giving up (with an error) between packets once `cancelled`
/// returns true.
pub fn load_unless(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<SampleData, LoadError> {
    let err = |e: &dyn std::fmt::Display| LoadError(format!("{}: {e}", path.display()));
    // REX loops through the helper ([re:e1396a]).
    if crate::rex::is_rex(path) {
        let c = crate::rex::load(path).ok_or_else(|| err(&"REX DLL was unable to create REXHandle"))?;
        return Ok(SampleData { left: c.left, right: c.right, sample_rate: crate::rex::SAMPLE_RATE });
    }
    let file = File::open(path).map_err(|e| err(&e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|e| err(&e))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| err(&"no audio track"))?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100) as f32;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| err(&e))?;
    let mut channels = 0usize;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        if cancelled() {
            return Err(err(&"cancelled"));
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(err(&e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| err(&e))?;
        channels = decoded.spec().channels.count();
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buf.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(buf.samples());
    }
    let channels = channels.max(1);
    let frames = interleaved.len() / channels;
    let left: Vec<f32> = (0..frames).map(|i| interleaved[i * channels]).collect();
    let right = (channels > 1).then(|| (0..frames).map(|i| interleaved[i * channels + 1]).collect());
    Ok(SampleData { left, right, sample_rate })
}

/// Frame count and sample rate from the file's header (no decoding).
pub fn info(path: &Path) -> Option<(u64, f32)> {
    if crate::rex::is_rex(path) {
        return crate::rex::load(path).map(|c| (c.left.len() as u64, crate::rex::SAMPLE_RATE));
    }
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let track = probed.format.default_track()?;
    Some((track.codec_params.n_frames?, track.codec_params.sample_rate.unwrap_or(44100) as f32))
}

/// Loads each file once.
#[derive(Default)]
pub struct SampleCache {
    files: HashMap<PathBuf, Option<Arc<SampleData>>>,
    /// `.gsa` BPMs by sidecar path, with the file's (size, modification
    /// time) when read.
    sidecars: HashMap<PathBuf, (Option<(u64, std::time::SystemTime)>, Option<f32>)>,
    pub errors: Vec<String>,
    /// Called once per lookup (the loader's progress for the browser's
    /// loading bar).
    pub on_lookup: Option<Box<dyn FnMut() + Send>>,
}

impl SampleCache {
    pub fn get(&mut self, path: &Path) -> Option<Arc<SampleData>> {
        if let Some(f) = self.on_lookup.as_mut() {
            f();
        }
        if let Some(s) = self.files.get(path) {
            return s.clone();
        }
        let s = match load(path) {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                self.errors.push(e.0);
                None
            }
        };
        self.files.insert(path.to_owned(), s.clone());
        s
    }

    /// Tempo from the sample's `.gsa` sidecar (positive): the active
    /// analysis (User, else Master; [re:fb32a7] →
    /// [re:1d38e7], read by the timestretch parameter update at
    /// [re:dc2d9d]). Read again only when the file changed.
    pub fn sidecar_bpm(&mut self, sample: &Path) -> Option<f32> {
        let gsa = sample.with_extension("gsa");
        let stamp = std::fs::metadata(&gsa).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
        if let Some((_, bpm)) = self.sidecars.get(&gsa).filter(|(s, _)| *s == stamp) {
            return *bpm;
        }
        let bpm = stamp.and_then(|_| crate::gsa::SliceFile::load(&gsa)).map(|f| f.bpm).filter(|b| *b > 0.0);
        self.sidecars.insert(gsa, (stamp, bpm));
        bpm
    }

    /// Forget samples nothing else holds any more and failed lookups (a
    /// file may have appeared since; its error is reported again).
    pub fn prune(&mut self) {
        self.files.retain(|_, s| s.as_ref().is_some_and(|d| Arc::strong_count(d) > 1));
        self.errors.clear();
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn prune_keeps_samples_in_use() {
        let mut c = SampleCache::default();
        let kept = Arc::new(SampleData { left: vec![0.0; 4], right: None, sample_rate: 44100.0 });
        c.files.insert("a".into(), Some(kept.clone()));
        c.files.insert("b".into(), Some(Arc::new(SampleData { left: vec![0.0; 4], right: None, sample_rate: 44100.0 })));
        c.files.insert("c".into(), None);
        c.prune();
        assert_eq!(c.files.len(), 1);
        assert!(c.files.contains_key(Path::new("a")));
    }

    /// The `.gsa` BPM is read again when the sidecar changes or goes.
    #[test]
    fn sidecar_bpm_follows_the_file() {
        let dir = std::env::temp_dir().join(format!("dreist-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("loop.wav");
        let write = |bpm: &str| {
            let xml = format!(r#"<root><SliceAnalysis name="Master" version="3000"><AudioFileMetaData name="Meta" BPM="{bpm}"/></SliceAnalysis></root>"#);
            std::fs::write(wav.with_extension("gsa"), xml).unwrap();
        };
        let mut c = SampleCache::default();
        assert_eq!(c.sidecar_bpm(&wav), None);
        write("100");
        assert_eq!(c.sidecar_bpm(&wav), Some(100.0));
        write("120.5");
        assert_eq!(c.sidecar_bpm(&wav), Some(120.5));
        std::fs::remove_file(wav.with_extension("gsa")).unwrap();
        assert_eq!(c.sidecar_bpm(&wav), None);
        let _ = std::fs::remove_dir(&dir);
    }
}
