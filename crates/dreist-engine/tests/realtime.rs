//! Real-time safety: once warmed up, the audio path must not touch the heap
//! (no allocation, reallocation or free) while it plays every harness
//! scenario at host block sizes of 512, 33 and 4096 frames with what the
//! plugin feeds it: live MIDI notes, knob drags (direct and posted), host
//! automation, editor pad hits, sample previews, MIDI out, sampler
//! recording, transport stops / starts and panics, with the editor queues
//! drained each block as the plugin does.

use dreist_engine::binding::{Message, id};
use dreist_engine::harness::{doc_for, engine, name, presets, scenarios};
use std::sync::Arc;

use dreist_dsp::voice::SampleData;
use dreist_engine::Dreist;
use dreist_engine::sampler::Take;
use dreist_testkit::CountingAlloc;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const SR: f32 = 44_100.0;

/// What the editor side owns: the preview sample (kept alive, as the
/// plugin keeps recent previews) and the sampler takes it collects.
struct Gui {
    preview: Arc<SampleData>,
    takes: Vec<Take>,
}

/// The plugin drains these for its editor after every block and hands the
/// sampler a reserved buffer for its next take.
fn drain(g: &mut Dreist, gui: &mut Gui) {
    g.editor_messages.clear();
    g.automation_out.clear();
    g.retro_events.clear();
    g.step_writes.clear();
    g.songrec.events.clear();
    g.midi_log.clear();
    g.scenes_mut().events.clear();
    gui.takes.append(&mut g.sampler.finished);
    if g.sampler.needs_spare() {
        let t = g.sampler.new_spare();
        g.sampler.give_spare(t);
    }
}

/// Run `blocks` host blocks; returns the allocator calls made inside the
/// audio path (`process` plus the live input it is fed).
fn play(g: &mut Dreist, gui: &mut Gui, block: usize, blocks: usize, live: bool) -> usize {
    let (mut l, mut r) = (vec![0.0f32; block], vec![0.0f32; block]);
    let mut subs_store: Vec<[Vec<f32>; 2]> = (0..16).map(|_| [vec![0.0; block], vec![0.0; block]]).collect();
    let input = [vec![0.0f32; block], vec![0.0f32; block]];
    let mut calls = 0;
    for b in 0..blocks {
        let mut subs: Vec<[&mut [f32]; 2]> = subs_store.iter_mut().map(|[a, b]| [&mut a[..], &mut b[..]]).collect();
        let ((), n) = CountingAlloc::count(|| {
            if live {
                // Pad notes on MIDI channel 1 and their releases.
                if b % 4 == 0 {
                    let n = 36 + (b / 4 % 16) as u8;
                    g.push_midi(17.min(block as u32 - 1), [0x90, n, 100]);
                    g.push_midi((block as u32 / 2).min(block as u32 - 1), [0x80, n, 0]);
                }
                // Knob drags: engine level, pad tune, layer cutoff, an FX parameter.
                let v = (b % 100) as f32 / 100.0;
                g.apply_message(&Message::new(id::SET_ENGINE_PARAMETER, &[0], &[v, 0.0, 0.0]));
                g.apply_message(&Message::new(id::SET_PAD_PARAMETER, &[0, 0], &[v, 1.0, 0.0]));
                g.apply_message(&Message::new(id::SET_LAYER_PARAMETER, &[0, 0, 0], &[8.0, v, 0.0]));
                g.apply_message(&Message::new(id::SET_FX_PARAMETER, &[0, -1, -1, 0], &[0.0, v, 0.0]));
                // Editor commands: posted messages, pad hits, previews.
                g.post_message(Message::new(id::SET_LAYER_PARAMETER, &[0, 1, 0], &[0.0, v, 0.0]));
                g.set_host_parameter(0, v);
                match b % 8 {
                    1 => g.pad_hit(0, b / 8 % 16, 90, 0),
                    5 => g.pad_release(0, (b / 8) % 16, 0),
                    _ => {}
                }
                if b % 40 == 0 {
                    g.start_preview_region(gui.preview.clone(), 0, 20_000, b % 80 == 0);
                }
                // A one-second sampler take every 120 blocks.
                if b % 120 == 0 {
                    g.post_message(Message::new(id::SAMPLER_SET_ARMED, &[], &[1.0]));
                    g.post_message(Message::new(id::SAMPLER_START_RECORD, &[], &[1.0]));
                }
            }
            g.process_with_input(&mut l, &mut r, &mut subs, Some([&input[0], &input[1]]));
        });
        calls += n;
        drain(g, gui);
    }
    calls
}

#[test]
fn audio_path_does_not_touch_the_heap() {
    let mut failures = Vec::new();
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        for (sname, f) in scenarios() {
            let doc = doc_for(&p, f);
            for block in [512usize, 33, 4096] {
                let mut g = engine(&doc, &samples, SR);
                // As in the plugin: the editor owns the document; MIDI out
                // on; a timed (1 s) sampler recording the master.
                g.set_document_mirroring(false);
                g.apply_message(&Message::new(id::REGISTRY_MIDI_OPTIONS_CHANGED, &[], &[1.0, 1.0]));
                g.apply_message(&Message::new(id::SAMPLER_TIMED_MODE, &[], &[1.0]));
                g.apply_message(&Message::new(id::SAMPLER_RECORD_SECS, &[], &[1.0]));
                let mut gui = Gui { preview: Arc::new(SampleData { left: (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect(), right: None, sample_rate: 48_000.0 }), takes: Vec::new() };
                drain(&mut g, &mut gui);
                g.start();
                let blocks = |s: f32| ((SR * s) as usize).div_ceil(block);
                // Warm up (lazily sized buffers settle, voices cycle).
                play(&mut g, &mut gui, block, blocks(4.0), true);
                let mut calls = play(&mut g, &mut gui, block, blocks(4.0), true);
                // Transport stop / start and a panic mid-stream.
                let ((), n) = CountingAlloc::count(|| {
                    g.stop();
                    g.start();
                    g.panic();
                });
                calls += n;
                calls += play(&mut g, &mut gui, block, blocks(2.0), true);
                assert!(!gui.takes.is_empty(), "the sampler recorded takes");
                if calls > 0 {
                    failures.push(format!("{}-{sname} block {block}: {calls} allocator calls", name(&p)));
                }
            }
        }
    }
    let sites: Vec<String> = CountingAlloc::sites().iter().take(25).map(|(s, n)| format!("{n:6}  {s}")).collect();
    assert!(failures.is_empty(), "{}\n(ALLOC_TRACE=1 for call sites)\n{}", failures.join("\n"), sites.join("\n"));
}

/// The plugin swaps a rebuilt engine in on the audio thread
/// ([re:57f537]); the old one is dropped elsewhere.
#[test]
fn engine_swap_does_not_touch_the_heap() {
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        let doc = doc_for(&p, |_| {});
        let mut g = engine(&doc, &samples, SR);
        g.start();
        let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
        play(&mut g, &mut gui, 512, 100, true);
        let mut n = engine(&doc, &samples, SR);
        let ((), calls) = CountingAlloc::count(|| n.adopt(&mut g));
        assert_eq!(calls, 0, "{}: adopt made {calls} allocator calls", name(&p));
        drop(g);
    }
}

/// Pattern edits the editor posts (applied on the audio thread): step
/// counts up to the 1024 maximum and back, step writes past the old
/// length, track and pattern clears, nudges, lane reverts and quantizes.
#[test]
fn pattern_edits_do_not_touch_the_heap() {
    for p in presets() {
        let samples = dreist_formats::model::samples_dir(&p);
        let doc = doc_for(&p, |_| {});
        let mut g = engine(&doc, &samples, SR);
        g.set_document_mirroring(false);
        g.start();
        let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
        play(&mut g, &mut gui, 512, 20, false);
        let pat = |kind: u32, track: i32, step: i32, v: &[f32]| Message::new(kind, &[0, -1, -1, -1, track, step], v);
        let edits = [
            pat(id::SET_PATTERN_STEP_COUNT, -1, -1, &[64.0]),
            pat(id::SET_STEP_ON, 3, 40, &[0.8, 0.5, 0.0]),
            pat(id::SET_STEP_GRAPH_STATE, 3, 41, &[1.0, 0.25, 0.0]),
            pat(id::SET_PATTERN_STEP_COUNT, -1, -1, &[1024.0]),
            pat(id::SET_STEP_ON, 63, 1000, &[0.8, 0.5, 0.0]),
            pat(id::NUDGE_TRACK_FORWARD, 3, -1, &[]),
            pat(id::NUDGE_PATTERN_BACKWARDS, -1, -1, &[]),
            pat(id::SET_PATTERN_STEP_COUNT, -1, -1, &[8.0]),
            pat(id::SET_STEP_OFF, 3, 2, &[]),
            pat(id::SET_TRACK_LENGTH, 3, -1, &[5.0]),
            pat(id::CLEAR_TRACK_AUTOMATION, 3, -1, &[0.0]),
            pat(id::QUANTIZE_PATTERN_TIMING, -1, -1, &[]),
            pat(id::CLEAR_TRACK, 3, -1, &[]),
            pat(id::CLEAR_PATTERN, -1, -1, &[]),
            pat(id::SET_PATTERN_STEP_COUNT, -1, -1, &[32.0]),
        ];
        for m in &edits {
            let ((), n) = CountingAlloc::count(|| {
                g.apply_message(m);
                let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
                g.process(&mut l, &mut r);
            });
            assert_eq!(n, 0, "{}: message {:#x} {:?} made {n} allocator calls", name(&p), m.kind, m.value);
            drain(&mut g, &mut gui);
        }
    }
}

/// Every message kind the engine knows, applied on a warm engine with a
/// few index / value patterns (what the editor, bindings and host post
/// into the audio thread), followed by a block.
#[test]
fn every_message_kind_leaves_the_heap_alone() {
    use dreist_engine::message_names::MESSAGE_NAMES;
    let Some(p) = presets().into_iter().next() else { return };
    let samples = dreist_formats::model::samples_dir(&p);
    let doc = doc_for(&p, |_| {});
    let mut failures = Vec::new();
    let variants: [(&[i32], &[f32]); 3] = [(&[0, 0, 0, -1, 0, 0], &[0.5, 0.5, 0.5, 0.5]), (&[1, 2, 1, 3, 4, 5], &[1.0, 0.0, 1.0, 0.0]), (&[0, -1, -1, -1, -1, -1], &[0.0, 1.0, 0.0, 1.0])];
    let mut g = engine(&doc, &samples, SR);
    g.set_document_mirroring(false);
    g.start();
    let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
    drain(&mut g, &mut gui);
    play(&mut g, &mut gui, 512, 20, true);
    for kind in 1..MESSAGE_NAMES.len() as u32 {
        for (index, value) in variants {
            let m = Message::new(kind, index, value);
            let ((), n) = CountingAlloc::count(|| {
                g.apply_message(&m);
                let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
                g.process(&mut l, &mut r);
            });
            let sites: Vec<String> = CountingAlloc::take_sites().iter().map(|(s, n)| format!("\n    {n:4}  {s}")).collect();
            drain(&mut g, &mut gui);
            if n > 0 {
                failures.push(format!("{kind:#x} {} {index:?} {value:?}: {n}{}", MESSAGE_NAMES[kind as usize], sites.concat()));
            }
        }
    }
    assert!(failures.is_empty(), "{} allocating messages (ALLOC_TRACE=1 for call sites):\n{}", failures.len(), failures.join("\n"));
}

/// MIDI-CC and host-automation bindings firing (continuous and toggle
/// kinds, smoothed and immediate) on the audio thread.
#[test]
fn bindings_fire_without_touching_the_heap() {
    let Some(p) = presets().into_iter().next() else { return };
    let samples = dreist_formats::model::samples_dir(&p);
    let doc = doc_for(&p, |_| {});
    let mut g = engine(&doc, &samples, SR);
    g.set_document_mirroring(false);
    let binding = |name: &str, kind: &str, proto: Message| {
        let mut n = dreist_formats::Node::new("Binding", Some(name));
        n.set("Min", 0);
        n.set("Max", 1);
        let mut t = dreist_formats::Node::new("VString", Some("Type"));
        t.set("string", kind);
        n.children.push(t);
        n.children.push(proto.to_node("MessagePrototype"));
        n
    };
    g.start();
    let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
    drain(&mut g, &mut gui);
    play(&mut g, &mut gui, 512, 20, false);
    // MIDI learn: prepared off the audio thread, added on it.
    let learned = [
        binding("MIDI0CC20Binding0", "Continuous", Message::new(id::SET_ENGINE_PARAMETER, &[0], &[])),
        binding("MIDI0CC20Binding1", "Continuous", Message::new(id::SET_ENGINE_PARAMETER, &[0], &[0.0, 1.0])),
        binding("MIDI0CC21Binding0", "Momentary", Message::new(id::MUTE_PAD, &[0, 3], &[])),
        binding("Auto0Binding0", "Continuous", Message::new(id::SET_PAD_PARAMETER, &[0, 1], &[0.0, 1.0])),
        binding("Auto1Binding0", "Toggle", Message::new(id::SOLO_PAD, &[0, 2], &[])),
    ];
    for n in &learned {
        let mut b = dreist_engine::binding::NewBinding::from_node(n).expect("binding name");
        let ((), calls) = CountingAlloc::count(|| g.add_binding(&mut b));
        assert_eq!(calls, 0, "{:?}: adding made {calls} allocator calls", n.name);
        assert!(b.added(), "{:?} added", n.name);
    }
    for b in 0..200u32 {
        let v = (b % 50) as f32 / 49.0;
        let ((), n) = CountingAlloc::count(|| {
            g.push_midi(10, [0xb0, 20, (v * 127.0) as u8]);
            g.push_midi(20, [0xb0, 21, if b % 2 == 0 { 127 } else { 0 }]);
            g.set_host_parameter(0, v);
            g.set_host_parameter(1, if b % 3 == 0 { 1.0 } else { 0.0 });
            let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
            g.process(&mut l, &mut r);
        });
        assert_eq!(n, 0, "block {b}: {n} allocator calls (ALLOC_TRACE=1 for call sites)\n{:?}", CountingAlloc::take_sites());
        drain(&mut g, &mut gui);
    }
}

/// Editor commands that hand the engine whole values (swing templates,
/// song tracks) swap them in; the old values go back to be freed.
#[test]
fn swapped_in_values_do_not_touch_the_heap() {
    use dreist_engine::song::SongPart;
    let Some(p) = presets().into_iter().next() else { return };
    let samples = dreist_formats::model::samples_dir(&p);
    let mut g = engine(&doc_for(&p, |_| {}), &samples, SR);
    g.start();
    let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
    drain(&mut g, &mut gui);
    play(&mut g, &mut gui, 512, 10, false);
    for round in 0..3 {
        let mut swing = dreist_engine::pattern::Swing { steps: 8, values: (0..128).map(|i| (i % 7) as f64 * 0.01).collect() };
        let part = |start: f64| SongPart { start, length: 4.0, pattern: 1, phase: 0.0, mute: false };
        let mut tracks = vec![(0, (0..40).map(|i| part(i as f64 * 4.0)).collect::<Vec<_>>()), (3, vec![part(8.0), part(0.0)])];
        let ((), n) = CountingAlloc::count(|| {
            g.set_swing_template(0, &mut swing);
            g.replace_song_tracks(&mut tracks);
            let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
            g.process(&mut l, &mut r);
        });
        assert_eq!(n, 0, "round {round}: {n} allocator calls");
        drain(&mut g, &mut gui);
    }
}

/// Every message kind with the last valid and out-of-range indices and
/// huge, negative and NaN values: no panic, finite output afterwards.
#[test]
fn every_message_kind_survives_extreme_arguments() {
    use dreist_engine::message_names::MESSAGE_NAMES;
    let Some(p) = presets().into_iter().next() else { return };
    let samples = dreist_formats::model::samples_dir(&p);
    let mut g = engine(&doc_for(&p, |_| {}), &samples, SR);
    g.set_document_mirroring(false);
    g.start();
    let mut gui = Gui { preview: Arc::new(SampleData { left: vec![0.0; 1000], right: None, sample_rate: SR }), takes: Vec::new() };
    drain(&mut g, &mut gui);
    play(&mut g, &mut gui, 512, 10, true);
    let variants: [(&[i32], &[f32]); 4] = [
        (&[7, 63, 7, 23, 63, 1023], &[1e6, -1e6, 1e6, -1e6]),
        (&[8, 64, 8, 24, 64, 1024], &[-1.0, 2.0, 255.0, 1e9]),
        (&[99, 999, 99, 99, 999, 99_999], &[f32::NAN; 4]),
        (&[i32::MAX, i32::MIN, -5, i32::MAX, -7, i32::MIN], &[f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN]),
    ];
    for kind in 1..MESSAGE_NAMES.len() as u32 {
        for (index, value) in variants {
            let m = Message::new(kind, index, value);
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                g.apply_message(&m);
                let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
                g.process(&mut l, &mut r);
                l.iter().chain(&r).all(|v| v.is_finite())
            }));
            match r {
                Ok(true) => {}
                Ok(false) => panic!("{kind:#x} {} {index:?} {value:?}: non-finite output", MESSAGE_NAMES[kind as usize]),
                Err(_) => panic!("{kind:#x} {} {index:?} {value:?}: panicked", MESSAGE_NAMES[kind as usize]),
            }
            drain(&mut g, &mut gui);
        }
    }
}
