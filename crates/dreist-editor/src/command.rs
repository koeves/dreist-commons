//! Edits the editor sends to the audio-thread engine. The GUI applies the
//! same edits to its own document so both stay in sync.

use dreist_engine::Dreist;
use dreist_engine::binding::Message;
use dreist_engine::live::PadMode;

#[derive(Clone, Debug)]
pub enum Command {
    /// A synth message ([re:1aa701], applied at the next block).
    Message(Message),
    /// Pads panel hit (message 0x3b `e_playSound`), velocity 0..127.
    PadHit {
        engine: usize,
        pad: usize,
        velocity: u32,
    },
    /// Pad released (0x3d `e_stopSound`).
    PadRelease {
        engine: usize,
        pad: usize,
    },
    SetPadMode {
        engine: usize,
        mode: PadMode,
    },
    /// `e_selectEngine` (0x41): the engine MIDI channel 9 and the editor follow.
    SelectEngine(usize),
    /// Standalone transport (plugins follow the host).
    Play(bool),
    /// Open a project (`.geist`); the editor turns this into `Rebuild`.
    LoadProject(std::path::PathBuf),
    /// Structural edit (samples, kits, projects): rebuild the engine from
    /// the document off the audio thread and swap it in.
    Rebuild {
        doc: Box<dreist_formats::Node>,
        samples: Option<std::path::PathBuf>,
    },
    /// The project was saved to this path (the host remembers it).
    Saved(std::path::PathBuf),
    /// Start (`Some`) or stop the preview voice.
    Preview(Option<std::sync::Arc<dreist_dsp::voice::SampleData>>),
    /// Preview frames `start..end` of a sample, optionally looping.
    PreviewRegion {
        data: std::sync::Arc<dreist_dsp::voice::SampleData>,
        start: u32,
        end: u32,
        looping: bool,
    },
    /// `e_setEngineSwingTemplate` (0x96): engine, template node.
    SwingTemplate(usize, Box<dreist_formats::Node>, Box<dreist_engine::pattern::Swing>),
    /// `e_songReplaceTrack` (0x11c) for each (track, parts).
    SongTracks(Box<Vec<(usize, Vec<dreist_engine::song::SongPart>)>>),
    /// "Sync to next division" ([re:0d0d93], set
    /// by the GUI directly in the original).
    NoteRepeatSync(usize, bool),
    /// Add a binding (MIDI learn) to the running engine.
    AddBinding(Box<dreist_engine::binding::NewBinding>),
    /// Render the document to an audio file (Tools › Export as audio).
    /// `status`: the export panel's job state (progress, abort, the
    /// normalise prompt and the outcome), shared with the host's worker.
    ExportAudio {
        doc: Box<dreist_formats::Node>,
        samples: Option<std::path::PathBuf>,
        path: std::path::PathBuf,
        options: dreist_engine::export::Options,
        reveal: bool,
        status: Option<ExportProgress>,
    },
}

impl Command {
    /// Handled by the host on a worker thread, not by the audio thread.
    pub fn is_structural(&self) -> bool {
        matches!(self, Command::LoadProject(_) | Command::Rebuild { .. } | Command::Saved(_) | Command::ExportAudio { .. })
    }

    /// Engine state a rebuilt engine takes from the document, which neither
    /// the plugin's message replay nor [re:57f537] carries: queued while
    /// a rebuild is in flight, it needs a newer one.
    pub fn outlives_rebuild(&self) -> bool {
        matches!(self, Command::AddBinding(_) | Command::SongTracks(_) | Command::SwingTemplate(..) | Command::SetPadMode { .. })
    }

    /// Apply to the engine (on the audio thread: nothing here allocates;
    /// what the engine gives back stays in the command, freed by its
    /// sender).
    pub fn apply(&mut self, g: &mut Dreist) {
        match self {
            Command::Message(m) => g.post_message(m.clone()),
            Command::AddBinding(b) => g.add_binding(b),
            Command::SongTracks(t) => g.replace_song_tracks(t),
            Command::SwingTemplate(e, _, sw) => g.set_swing_template(*e, sw),
            Command::Preview(d) => g.start_preview(d.clone()),
            Command::PreviewRegion { data, start, end, looping } => g.start_preview_region(data.clone(), *start, *end, *looping),
            Command::PadHit { engine, pad, velocity } => g.pad_hit(*engine, *pad, *velocity, 0),
            Command::PadRelease { engine, pad } => g.pad_release(*engine, *pad, 0),
            Command::SetPadMode { engine, mode } => g.set_pad_mode(*engine, *mode),
            Command::SelectEngine(e) => g.select_engine(*e),
            Command::NoteRepeatSync(e, on) => g.set_note_repeat_sync(*e, *on),
            Command::LoadProject(_) | Command::Rebuild { .. } | Command::Saved(_) | Command::ExportAudio { .. } => {}
            Command::Play(on) => {
                if *on {
                    g.play()
                } else {
                    g.stop()
                }
            }
        }
    }
}

/// Message helper: `kind` with indices and values.
pub fn msg(kind: u32, index: &[i32], value: &[f32]) -> Command {
    Command::Message(Message::new(kind, index, value))
}

/// An export job as the panel sees it ([re:8c1655]: progress,
/// abort, the clipping prompt, a fatal error / the error text).
#[derive(Debug, Default)]
pub struct ExportStatus {
    pub progress: f32,
    pub abort: bool,
    /// The question to ask before writing (Prompt mode with clipping).
    pub prompt: Option<String>,
    /// Its answer: normalise or not.
    pub answer: Option<bool>,
    /// The outcome: files written, or the error ("aborted" on abort).
    pub result: Option<Result<Vec<std::path::PathBuf>, String>>,
}

pub type ExportProgress = std::sync::Arc<std::sync::Mutex<ExportStatus>>;

/// The host side of [re:a61043]: render (reporting progress,
/// stopping on abort), ask through `status` when the export would clip in
/// Prompt mode, then write.
pub fn run_export(
    doc: &dreist_formats::Node,
    samples: &std::path::Path,
    path: &std::path::Path,
    sr: u32,
    options: &dreist_engine::export::Options,
    status: Option<&ExportProgress>,
) -> Result<dreist_engine::export::Report, String> {
    let set = |f: &mut dyn FnMut(&mut ExportStatus)| {
        if let Some(Ok(mut g)) = status.map(|s| s.lock()) {
            f(&mut g);
        }
    };
    let mut progress = |v: f32| {
        let mut go = true;
        set(&mut |g| {
            g.progress = v;
            go = !g.abort;
        });
        go
    };
    let out = dreist_engine::export::render_export(doc, samples, sr, options, &mut progress).and_then(|r| {
        let mut answer = None;
        if let (Some(q), Some(st)) = (r.prompt(), status) {
            set(&mut |g| g.prompt = Some(q.clone()));
            loop {
                let (a, abort) = st.lock().map(|g| (g.answer, g.abort)).unwrap_or((Some(false), true));
                if abort {
                    return Err("aborted".to_string());
                }
                if a.is_some() {
                    answer = a;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        r.finish(path, answer)
    });
    set(&mut |g| g.result = Some(out.as_ref().map(|r| r.files.clone()).map_err(|e| e.clone())));
    out
}
