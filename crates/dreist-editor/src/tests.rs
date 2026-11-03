use super::*;
use crate::editor::engine_menu::ENGINE_NAME_EDIT;
use crate::editor::knobs::KNOB_EDIT;

/// A file of the installed factory content (`DREIST_CONTENT` overrides
/// its location).
fn demo(rel: &str) -> std::path::PathBuf {
    dreist_formats::legacy::demo(rel)
}

/// An editor on a factory project; `None` when the content or the skin is
/// not installed.
fn demo_editor(rel: &str) -> Option<Editor> {
    let proj = dreist_formats::Project::load(&demo(rel)).ok()?;
    Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)).ok()
}

#[test]
fn modulator_trigger_panel_and_popups() {
    use dreist_gui::app::UiEvent;
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    ed.model.doc.children.retain(|c| c.class != "Modulator");
    let mut n = dreist_formats::Node::new("Modulator", Some("Modulator0"));
    n.set("Type", "LFO");
    ed.model.doc.children.push(n);
    ed.sync();
    // Paths of named controls inside slot 0.
    fn find(c: &Ctrl, name: &str, path: &mut Vec<usize>, slot: bool) -> Option<Vec<usize>> {
        let slot = slot || c.attr("modSlot") == Some("0");
        if slot && c.name == name {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = find(k, name, path, slot) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let shape = find(&ed.app.root, "Control2", &mut Vec::new(), false).unwrap();
    assert_eq!(ed.app.ctrl(&shape).unwrap().text, "Sin");
    ed.handle(&UiEvent::Clicked { path: shape.clone(), name: "Control2".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: shape.clone(), name: "Control2".into(), index: 3 });
    assert_eq!(ed.app.ctrl(&shape).unwrap().text, "Saw Up");
    let trig = find(&ed.app.root, "Trigger", &mut Vec::new(), false).unwrap();
    ed.handle(&UiEvent::Clicked { path: trig, name: "Trigger".into(), id: 0, on: false });
    assert_eq!(ed.trigger.open, Some(0));
    let ov = ed.app.root.path_of("TriggerOverlay").unwrap();
    assert!(ed.app.ctrl(&ov).unwrap().visible);
    let pad = (ed.app.ctrl(&ov).unwrap()).path_of("Pad5").map(|p| [ov.clone(), p].concat()).unwrap();
    ed.handle(&UiEvent::Clicked { path: pad.clone(), name: "Pad5".into(), id: 0, on: true });
    let e = ed.model.engine;
    let m = ed.model.doc.children.iter().find(|c| c.name.as_deref() == Some("Modulator0")).unwrap();
    assert_eq!(m.bool(&format!("PadTrig{e}-5")), Some(true));
    assert!(ed.app.ctrl(&pad).unwrap().on);
    let tstart = (ed.app.ctrl(&ov).unwrap()).path_of("TStart").map(|p| [ov.clone(), p].concat()).unwrap();
    ed.handle(&UiEvent::Clicked { path: tstart, name: "TStart".into(), id: 0, on: false });
    let m = ed.model.doc.children.iter().find(|c| c.name.as_deref() == Some("Modulator0")).unwrap();
    assert_eq!(m.bool("ResetAtStart"), Some(false));
}

#[test]
fn pad_labels_show_names() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let name = ed.model.pad_name(ed.model.engine, 0).unwrap();
    assert!(name.starts_with("P1 "), "{name}");
    let m = ed.app.root.find("PadsMatrix").unwrap();
    assert_eq!(m.children.len(), 16);
    assert!(m.children.iter().all(|p| p.rect.right() <= m.rect.w), "pads overflow the matrix");
    let label = m.children[0].find("NameLabel").unwrap();
    assert_eq!(label.text, name);
    assert!(label.rect.w > 0 && label.rect.h > 0, "{:?}", label.rect);
}

#[test]
fn grid_multi_tool_draws_erases_and_chains() {
    use dreist_gui::app::MouseButton;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let gp = ed.app.root.path_of("PatternGrid").unwrap();
    let (gx, gy) = ed.app.origin(&gp);
    let e = ed.model.engine;
    let pat = ed.model.current_pattern(e);
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let at = |step: usize, row: usize| (gx as f32 + lay.step_x(step) + lay.step_w / 2.0, gy as f32 + lay.row_y(row) + lay.row_h / 2.0);
    let send = |ed: &mut Editor, evs: Vec<UiEvent>| {
        for ev in evs {
            ed.handle(&ev);
        }
    };
    // Clear track 0, then a left click turns step 0 on; another left
    // click leaves it on (no toggle); a right click turns it off.
    for st in 0..16 {
        ed.handle(&UiEvent::Mouse { path: gp.clone(), overload: "PatternGrid".into(), kind: dreist_gui::app::MouseKind::RightDown, x: lay.step_x(st) + 1.0, y: lay.row_y(0) + 1.0 });
        ed.handle(&UiEvent::Mouse { path: gp.clone(), overload: "PatternGrid".into(), kind: dreist_gui::app::MouseKind::Up, x: lay.step_x(st) + 1.0, y: lay.row_y(0) + 1.0 });
    }
    let on = |ed: &Editor, k: usize| ed.model.pattern(e, pat).tracks[0].steps[k].on;
    assert!(!on(&ed, 0));
    let (x, y) = at(0, 0);
    {
        let evs = ed.app.mouse_move(x, y);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_down(MouseButton::Left);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_up(MouseButton::Left);
        send(&mut ed, evs);
    }
    assert!(on(&ed, 0));
    {
        let evs = ed.app.mouse_down(MouseButton::Left);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_up(MouseButton::Left);
        send(&mut ed, evs);
    }
    assert!(on(&ed, 0), "a left click does not toggle off");
    {
        let evs = ed.app.mouse_down(MouseButton::Right);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_up(MouseButton::Right);
        send(&mut ed, evs);
    }
    assert!(!on(&ed, 0));
    // A gated pad: dragging from step 2 to step 5 makes one tied chain.
    let l0 = ed.model.pad_mut(e, 0).child_mut_or_insert("Layer", "Layer0");
    l0.set("PlaybackMode", "Gated");
    if l0.attr("AudioFileName").is_none_or(|f| f.is_empty()) {
        l0.set("AudioFileName", "x.wav");
    }
    assert!(!ed.model.pad_one_shot(e, 0));
    let (x2, y2) = at(2, 0);
    let (x5, _) = at(5, 0);
    {
        let evs = ed.app.mouse_move(x2, y2);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_down(MouseButton::Left);
        send(&mut ed, evs);
    }
    {
        let evs = ed.app.mouse_move(x5, y2);
        send(&mut ed, evs);
    }

    {
        let evs = ed.app.mouse_up(MouseButton::Left);
        send(&mut ed, evs);
    }
    let t = &ed.model.pattern(e, pat).tracks[0];
    assert!((2..=5).all(|k| t.steps[k].on), "{:?}", (0..8).map(|k| t.steps[k].on).collect::<Vec<_>>());
    assert!(!t.steps[2].tied() && (3..=5).all(|k| t.steps[k].tied()));
    // The hover cursor over the chain's end.
    let c = ed.pattern.as_ref().unwrap().hover_cursor(&ed.app, &ed.model, &gp, "PatternGrid", lay.step_x(5) + 2.0, lay.row_y(0) + 2.0);
    assert_eq!(c, Some(dreist_gui::cursor::Cursor::image("ResizeRightThin")));
}

#[test]
fn x0x_pads_show_step_states() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    ed.model.engine_mut(e).set("EnginePadMode", "x0x");
    assert_eq!(ed.model.pad_mode(e), dreist_engine::live::PadMode::X0x);
    let (track, cur) = (ed.model.pad[e], ed.model.current_pattern(e));
    let steps = ed.model.pattern(e, cur).steps as usize;
    let mut on = dreist_engine::pattern::StepData::off();
    on.on = true;
    on.lanes[0] = 1.0;
    let mut half = on.clone();
    half.lanes[0] = 0.5;
    let off = dreist_engine::pattern::StepData::off();
    let target = if track == 1 { 2 } else { 1 };
    ed.step_writes(&[
        dreist_engine::StepWrite { engine: e, pattern: cur, track, step: 0, data: on },
        dreist_engine::StepWrite { engine: e, pattern: cur, track, step: target, data: half },
        dreist_engine::StepWrite { engine: e, pattern: cur, track, step: 3, data: off },
    ]);
    let style = |ed: &Editor, i: usize| {
        fn find<'a>(c: &'a dreist_gui::Ctrl, idx: &str) -> Option<&'a dreist_gui::Ctrl> {
            if c.overload == "EditorPad" && c.attr("padIndex") == Some(idx) {
                return Some(c);
            }
            c.children.iter().find_map(|k| find(k, idx))
        }
        let c = find(&ed.app.root, &i.to_string()).unwrap();
        (c.style.clone(), c.enabled)
    };
    let sel0 = track == 0;
    assert_eq!(style(&ed, 0).0, if sel0 { "PadAccentSelectedStyle" } else { "PadAccentStyle" });
    assert_eq!(style(&ed, target).0, if target == track { "PadOnSelectedStyle" } else { "PadOnStyle" });
    assert!(style(&ed, 3).0.starts_with("PadOff"));
    if steps < 64 {
        if let Some(v) = (steps..64).find(|&i| i < ed.model.visible_pads().1) {
            assert!(!style(&ed, v).1, "pads past the step count are disabled");
        }
    }
}

#[test]
fn pad_mode_popup_menu() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let pp = ed.app.root.path_of("PadMode").unwrap();
    let (x, y) = ed.app.origin(&pp);
    let click = |ed: &mut Editor, x: f32, y: f32| {
        for ev in ed.app.mouse_move(x, y) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
    };
    click(&mut ed, x as f32 + 10.0, y as f32 + 10.0);
    let m = ed.app.menu.clone().expect("menu open");
    assert_eq!(m.items.len(), 6);
    if let Ok(out) = std::env::var("DREIST_MENU_SHOT") {
        ed.render(1.0).save_png(out).unwrap();
    }
    // Pick "Chromatic" (item 1).
    click(&mut ed, m.x + 20.0, m.y + 3.0 + 18.0 * 1.5);
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::SetPadMode { mode: dreist_engine::live::PadMode::Chromatic, .. })), "{cmds:?}");
    assert_eq!(ed.model.pad_mode(ed.model.engine), dreist_engine::live::PadMode::Chromatic);
    assert!(ed.app.menu.is_none());
}

#[test]
fn loading_a_sample_rebuilds_with_it() {
    let content = browser::content_dir();
    let p = content.join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Some(hit) =
        std::fs::read_dir(content.join(dreist_formats::legacy::DEMO_FACTORY).join("Hits/Kicks")).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "wav")))
    else {
        return;
    };
    let mut model = Model::new(proj.raw);
    model.samples = Some(dreist_formats::model::samples_dir(&p));
    model.engine = 0;
    model.pad[0] = 5;
    model.layer[0] = 0;
    browser::load_sample_into_doc(&mut model, &hit);
    assert_eq!(model.layer_sample(0, 5, 0).as_deref(), Some(hit.as_path()));
    let mut cache = dreist_engine::audio::SampleCache::default();
    let g = dreist_engine::Dreist::from_document(&model.doc, model.samples.as_ref().unwrap(), 44100.0, dreist_engine::RenderOptions::default(), &mut cache).unwrap();
    assert!(g.missing_samples.is_empty(), "{:?}", g.missing_samples);
    assert!(model.layer(0, 5, 0).valid);
}

#[test]
fn typing_a_tempo() {
    let p = browser::content_dir().join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let bp = ed.app.root.path_of("BPMTextBox").unwrap();
    let (x, y) = ed.app.origin(&bp);
    ed.app.mouse_move(x as f32 + 5.0, y as f32 + 5.0);
    for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    assert!(ed.app.is_editing());
    for _ in 0..8 {
        ed.app.key_named("Backspace");
    }
    assert!(ed.app.key_text("98"));
    let (evs, used) = ed.app.key_named("Enter");
    assert!(used);
    for ev in &evs {
        ed.handle(ev);
    }
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_TEMPO && (m.value[0] - 98.0).abs() < 1e-3)), "{cmds:?}");
    assert!((ed.model.tempo() - 98.0).abs() < 1e-6);
}

#[test]
fn undo_restores_the_document() {
    let p = browser::content_dir().join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let orig = ed.model.doc.clone();
    ed.handle(&UiEvent::TextCommitted { path: vec![], name: "PatternNameBox".into(), text: "Renamed".into() });
    assert_ne!(ed.model.doc, orig);
    let undo = ed.app.root.path_of("UndoButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: undo, name: "UndoButton".into(), id: 0, on: false });
    assert_eq!(ed.model.doc, orig);
    assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })));
    let redo = ed.app.root.path_of("RedoButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: redo, name: "RedoButton".into(), id: 0, on: false });
    assert_ne!(ed.model.doc, orig);
}

#[test]
fn sampler_source_list_offers_external_and_the_outputs() {
    use dreist_engine::binding::id;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let e = ed.model.engine;
    ed.model.engine_mut(e).set("EnginePadMode", "Sampler");
    ed.sync();
    let Some(host) = ed.app.root.path_of("SamplerPanel") else { return };
    let Some(sl) = ed.app.ctrl(&host).and_then(|c| c.path_of("SourceList")).map(|q| [host.clone(), q].concat()) else { return };
    // A new document records the master (the constructor's default).
    assert_eq!(ed.app.ctrl(&sl).unwrap().text, "Master");
    ed.handle(&UiEvent::Clicked { path: sl.clone(), name: "SourceList".into(), id: 0, on: false });
    let items = ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap();
    assert_eq!(items.len(), 34);
    assert_eq!((items[0].as_str(), items[1].as_str(), items[33].as_str()), ("External", "Master", "Sub output 32"));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: sl.clone(), name: "SourceList".into(), index: 0 });
    let src: Vec<f32> = ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { (m.kind == id::SAMPLER_RECORD_SOURCE).then_some(m.value[0]) } else { None }).collect();
    assert_eq!(src, vec![38.0]);
    ed.sync();
    assert_eq!(ed.app.ctrl(&sl).unwrap().text, "External");
}

#[test]
fn editor_only_bindings_learn_and_act() {
    use dreist_engine::binding::Message;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    // Learning AllPads makes an editor-only binding (0x1c6).
    let Some(ap) = ed.app.root.path_of("AllPads") else { return };
    let ctrl = ed.app.ctrl(&ap).unwrap().clone();
    let t = ed.editor_binding_target(&ap, &ctrl).expect("editor binding");
    assert_eq!((t.proto.kind, t.proto.editor, t.name.as_str(), t.group.as_str()), (0x1c6, true, "All Pads", "Global"));
    let node = midimap::add_binding(&mut ed.model.doc, true, 0, 20, &t.proto, 0, &t.group, &t.name, t.kind);
    assert_eq!(node.attr("EditorOnly"), Some("true"));
    // The binding's message sets the toggle (value ≥ 0.5) and acts.
    let msg = |kind: u32, v: f32| {
        let mut m = Message::new(kind, &[], &[v]);
        m.editor = true;
        m
    };
    ed.binding_messages(&[msg(0x1c6, 1.0)]);
    assert!(ed.model.all_pads);
    ed.binding_messages(&[msg(0x1c6, 0.0)]);
    assert!(!ed.model.all_pads);
    // Panels: Song tab.
    ed.binding_messages(&[msg(0x148, 1.0)]);
    assert_eq!(ed.model.editor_tab, 2);
    // Pattern length from the value: ⌊0.5·1023 + 1⌋ = 512.
    ed.binding_messages(&[msg(0x146, 1.0)]);
    ed.take_commands();
    ed.binding_messages(&[msg(0x1b2, 0.5)]);
    let cmds = ed.take_commands();
    let steps: Vec<f32> = cmds.iter().filter_map(|c| if let Command::Message(m) = c { (m.kind == dreist_engine::binding::id::SET_PATTERN_STEP_COUNT).then_some(m.value[0]) } else { None }).collect();
    assert_eq!(steps, vec![512.0]);
    // The longer pattern comes with a rebuild (the engine does not
    // allocate step storage on the audio thread), last.
    assert!(matches!(cmds.last(), Some(Command::Rebuild { .. })), "rebuild after growing the pattern");
    ed.binding_messages(&[msg(0x1b2, 0.0)]);
    assert!(!ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })), "no rebuild when it shrinks");
    // Undo through its binding.
    ed.model.doc.set("Probe", 1);
    let before = ed.model.doc.clone();
    ed.history.undo.push_back(("Edit".into(), project::new_document()));
    ed.binding_messages(&[msg(0x18d, 1.0)]);
    assert_ne!(ed.model.doc, before);
}

#[test]
fn sampler_mode_opens_the_sampler_panel() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let e = ed.model.engine;
    ed.model.engine_mut(e).set("EnginePadMode", "Sampler");
    ed.sync();
    let host = ed.app.root.find("SamplerPanel").unwrap();
    assert!(host.visible);
    assert_eq!(host.rect.h, sampler::OPEN_HEIGHT);
    let m = ed.app.root.find("PadsMatrix").unwrap();
    assert!(m.rect.y > sampler::OPEN_HEIGHT, "{:?}", m.rect);
    assert!(m.children.iter().all(|p| p.rect.bottom() <= m.rect.h), "pads overflow the matrix");
    let ap = ed.app.root.path_of("ArmButton").unwrap();
    let (x, y) = ed.app.origin(&ap);
    for ev in ed.app.mouse_move(x as f32 + 3.0, y as f32 + 3.0) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SAMPLER_TOGGLE_ARMED)), "{cmds:?}");
}

#[test]
fn pattern_zoom_and_scroll() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let click = |ed: &mut Editor, name: &str| {
        let bp = ed.app.root.path_of(name).unwrap();
        let (x, y) = ed.app.origin(&bp);
        for ev in ed.app.mouse_move(x as f32 + 3.0, y as f32 + 3.0) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
    };
    click(&mut ed, "ZoomInH");
    let e = ed.model.engine;
    assert_eq!(ed.model.pattern_view[e].h_zoom, 2);
    let hb = ed.app.root.find("HScrollBar").unwrap();
    assert!(hb.attr_f32("realPage", 1.0) < 1.0, "zoomed in: the bar has a thumb");
    click(&mut ed, "NudgeTrackForwardButton");
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::NUDGE_TRACK_FORWARD)), "{cmds:?}");
    // Follow playback cursor: a playing step past the view scrolls to it.
    let pat = ed.model.pattern(e, ed.model.current_pattern(e));
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let last = pat.steps - 1;
    assert!((last as usize) >= lay.first_step + lay.visible_steps);
    let mut steps = [None; 8];
    steps[e] = Some(last);
    ed.set_playback(true, steps, None);
    assert_eq!(ed.model.pattern_view[e].h_scroll, 0, "follow is off by default");
    ed.model.follow_playback = true;
    steps[e] = Some(last - 1);
    ed.set_playback(true, steps, None);
    assert_eq!(ed.model.pattern_view[e].h_scroll, last as usize - 1);
    steps[e] = Some(last);
    ed.set_playback(true, steps, None);
    assert_eq!(ed.model.pattern_view[e].h_scroll, last as usize - 1, "still in view");
}

#[test]
fn graph_lane_click_sets_the_step_value() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    // The single global graph (toolbar View).
    if let Some(pp) = ed.pattern.as_mut() {
        pp.multi = false;
    }
    ed.sync();
    let gp = ed.app.root.path_of("PatternGraph").unwrap();
    let (x, y) = ed.app.origin(&gp);
    let h = ed.app.ctrl(&gp).unwrap().rect.h as f32;
    for ev in ed.app.mouse_move(x as f32 + 5.0, y as f32 + h * 0.75) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    let e = ed.model.engine;
    let pat = ed.model.pattern(e, ed.model.current_pattern(e));
    let v = pat.tracks[ed.model.pad[e]].steps[0].lanes[0];
    assert!((v - 0.25).abs() < 0.05, "velocity {v}");
}

#[test]
fn midi_learn_binds_the_next_cc() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let click = |ed: &mut Editor, path: &[usize]| {
        let (x, y) = ed.app.origin(path);
        for ev in ed.app.mouse_move(x as f32 + 3.0, y as f32 + 3.0) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
    };
    let lb = ed.app.root.path_of("LearnButton").unwrap();
    click(&mut ed, &lb);
    assert!(ed.learn.active);
    // Target: the first layer param knob (Gain) in the pad inspector.
    let knob = ed.app.root.path_of("VoiceGain").unwrap();
    let before = ed.model.doc.clone();
    click(&mut ed, &knob);
    let _ = ed.take_commands();
    assert!(ed.learn.target.is_some(), "a learn target");
    assert_eq!(ed.model.doc, before, "learn mode does not operate the control");
    ed.midi_cc(1, 0, 7);
    let rows = crate::midimap::rows(&ed.model.doc);
    assert!(rows.iter().any(|r| r.locale() == "MIDI Ch 1 CC 7"), "{rows:?}");
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::AddBinding(_))));
}

#[test]
fn song_recording_and_retro() {
    use dreist_engine::songrec::SongEvent;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let song = |ed: &Editor| ed.model.doc.children.iter().find(|c| c.class == "SongSequencer").map(dreist_engine::song::Song::from_node).unwrap();
    // Recording: a part lands in the song when its pattern stops.
    ed.song_events(&[
        SongEvent::State(2),
        SongEvent::Started { track: 1, pattern: 2, time: 4.0, recording: true, phase: 0.0 },
        SongEvent::Stopped { track: 1, pattern: 2, time: 12.0, recording: true },
    ]);
    let s = song(&ed);
    assert_eq!(s.tracks[1].len(), 1);
    assert_eq!((s.tracks[1][0].start, s.tracks[1][0].length, s.tracks[1][0].pattern), (4.0, 8.0, 2));
    assert!(!ed.history.undo.is_empty(), "recording leaves an undo point");
    // Not recording: the part goes to the retro buffer; Retro appends it
    // after the song's end (rounded up to the pattern length).
    ed.song_events(&[
        SongEvent::State(0),
        SongEvent::Started { track: 0, pattern: 0, time: 2.0, recording: false, phase: 0.0 },
        SongEvent::Stopped { track: 0, pattern: 0, time: 6.0, recording: false },
    ]);
    assert_eq!(song(&ed).tracks[0].len(), 0);
    // The Retro button grabs the song retro buffer while the song is on.
    if let Some(seq) = ed.model.doc.children.iter_mut().find(|c| c.class == "SongSequencer") {
        seq.set("SongActive", "true");
    }
    let rb = ed.app.root.path_of("RetroButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: rb, name: "RetroButton".into(), id: 0, on: false });
    let s = song(&ed);
    assert_eq!(s.tracks[0].len(), 1);
    let len = super::song::retro_len_for_test(&ed.model, 1, 2);
    let end = dreist_engine::song::quantize_round(((12.0 + len) / len).floor() * len);
    let len0 = super::song::retro_len_for_test(&ed.model, 0, 0);
    let earliest = dreist_engine::song::quantize_round((2.0f64 / len0).floor() * len0);
    assert!((s.tracks[0][0].start - (2.0 + end - earliest)).abs() < 1e-9, "{:?}", s.tracks[0]);
}

#[test]
fn graph_rec_knob_writes_the_current_step() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    ed.model.pattern_view[e].lane = 0;
    let knob = ed.app.root.path_of("InstoGraph").unwrap();
    let rec = ed.app.root.path_of("RecButton").unwrap();
    // Not recording: nothing is written.
    ed.model.playing = true;
    ed.model.steps[e] = Some(3);
    let pat = ed.model.current_pattern(e);
    let track = ed.model.pad[e];
    let before = ed.model.pattern(e, pat).tracks[track].steps[3].lanes[0];
    ed.handle(&UiEvent::ValueChanged { path: knob.clone(), name: "InstoGraph".into(), id: 0, value: 0.25 });
    assert_eq!(ed.model.pattern(e, pat).tracks[track].steps[3].lanes[0], before);
    ed.app.ctrl_mut(&rec).unwrap().on = true;
    ed.handle(&UiEvent::ValueChanged { path: knob, name: "InstoGraph".into(), id: 0, value: 0.25 });
    assert!((ed.model.pattern(e, pat).tracks[track].steps[3].lanes[0] - 0.25).abs() < 1e-6);
}

#[test]
fn scene_copy_paste_and_activity() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 1;
    ed.sync();
    let used = |ed: &Editor, n: usize| {
        dreist_engine::scene::SceneManager::from_node(ed.model.doc.children.iter().find(|c| c.class == "SceneManager").unwrap_or(&dreist_formats::Node::new("SceneManager", None))).scenes[n].used
    };
    // Snapshot scene 0, copy it, paste into scene 2.
    let snap = ed.app.root.path_of("SnapshotButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: snap, name: "SnapshotButton".into(), id: 0, on: false });
    assert!(used(&ed, 0));
    let m = ed.app.root.path_of("ScenesPanel").unwrap();
    let cell = |k: usize| [m.clone(), vec![k]].concat();
    let ev = |kind, k| UiEvent::Mouse { path: cell(k), overload: "EditorScene".into(), kind, x: 5.0, y: 5.0 };
    ed.handle(&ev(dreist_gui::app::MouseKind::RightDown, 0));
    ed.handle(&UiEvent::MenuSelected { anchor: cell(0), name: String::new(), index: 1 });
    ed.handle(&ev(dreist_gui::app::MouseKind::RightDown, 2));
    ed.handle(&UiEvent::MenuSelected { anchor: cell(2), name: String::new(), index: 2 });
    assert!(used(&ed, 2));
    // Cut clears the source.
    ed.handle(&ev(dreist_gui::app::MouseKind::RightDown, 0));
    ed.handle(&UiEvent::MenuSelected { anchor: cell(0), name: String::new(), index: 0 });
    assert!(!used(&ed, 0));
    // An activation shows the activity label.
    ed.scene_events(&[dreist_engine::scene::SceneEvent::Activated(1)]);
    ed.sync();
    let label = ed.app.ctrl(&cell(1)).unwrap().find("ActivityLabel").unwrap();
    assert_eq!(label.text, "\u{25b6}");
}

#[test]
fn aux_pre_post_button_sets_pre_fade() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    fn all(c: &Ctrl, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if !c.visible {
            return;
        }
        if c.name == "PrePostButton" {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            all(k, path, out);
            path.pop();
        }
    }
    let mut found = Vec::new();
    for tab in 0..8 {
        ed.model.editor_tab = tab;
        ed.sync();
        all(&ed.app.root, &mut Vec::new(), &mut found);
        if !found.is_empty() {
            break;
        }
    }
    let Some(b) = found.first().cloned() else { panic!("an aux strip with PrePostButton") };
    ed.handle(&UiEvent::Clicked { path: b.clone(), name: "PrePostButton".into(), id: 0, on: true });
    let pre = ed.model.doc.children.iter().filter(|c| c.class == "GlobalAux").any(|c| c.attr("IsPreFade") == Some("true"));
    assert!(pre, "a GlobalAux marked pre-fade");
    ed.sync();
    assert!(ed.app.ctrl(&b).unwrap().on);
}

#[test]
fn mixer_strip_selection_show_fx_and_compact_effects_mode() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    fn find_all(c: &Ctrl, path: &mut Vec<usize>, f: &dyn Fn(&Ctrl) -> bool, out: &mut Vec<Vec<usize>>) {
        if !c.visible {
            return;
        }
        if f(c) {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            find_all(k, path, f, out);
            path.pop();
        }
    }
    let find = |ed: &Editor, f: &dyn Fn(&Ctrl) -> bool| {
        let mut v = Vec::new();
        find_all(&ed.app.root, &mut Vec::new(), f, &mut v);
        v
    };
    let label = |ed: &Editor| find(ed, &|c| c.name == "IdentifierLabel").first().and_then(|p| ed.app.ctrl(p)).map(|c| c.text.clone());
    let page = ed.mixers.iter().find(|m| m.kind == mixer::Kind::Global).map(|m| m.page.clone()).unwrap();
    let panel = [page.clone(), vec![0]].concat();
    let r = ed.app.ctrl(&panel).unwrap().rect;
    // Tall: the chain on its own 200 px row.
    ed.app.ctrl_mut(&panel).unwrap().set_rect(dreist_gui::Rect::new(r.x, r.y, r.w, 420));
    ed.sync();
    assert_eq!(label(&ed).as_deref(), Some("E 1"));
    // A press on aux 2's strip shows its chain and highlights its title.
    let aux = find(&ed, &|c| c.overload == "MixerStrip" && c.attr("channel") == Some("Aux(1)"));
    let aux = aux.first().cloned().expect("aux strip");
    ed.handle(&UiEvent::Mouse { path: aux.clone(), overload: "MixerStrip".into(), kind: dreist_gui::app::MouseKind::Down, x: 5.0, y: 5.0 });
    assert_eq!(label(&ed).as_deref(), Some("A 2"));
    let title = ed.app.ctrl(&aux).and_then(|c| c.find("NameLabel")).map(|c| c.style.clone());
    assert_eq!(title.as_deref(), Some("MixerChannelEditTitleSelectedStyle"));
    // Show FX › M: the master chain.
    let v = find(&ed, &|c| c.name == "EffectViewGroup");
    let v = v.first().cloned();
    // Tall layout: the popup is hidden and Effects disabled.
    assert!(v.is_none(), "Show FX hidden when the chain has its own row");
    // Compact: Effects shows the chain beside the mode buttons.
    ed.app.ctrl_mut(&panel).unwrap().set_rect(dreist_gui::Rect::new(r.x, r.y, r.w, 300));
    ed.sync();
    let effects = find(&ed, &|c| c.text == "Effects" && c.class == "CtrlButton");
    let effects = effects.first().cloned().expect("Effects button");
    assert!(ed.app.ctrl(&effects).unwrap().enabled);
    ed.handle(&UiEvent::Clicked { path: effects, name: String::new(), id: 0, on: true });
    ed.sync();
    let v = find(&ed, &|c| c.name == "EffectViewGroup").first().cloned().expect("Show FX shown in Effects mode");
    assert!(find(&ed, &|c| c.name == "ChannelScroller").is_empty(), "strips hidden");
    ed.handle(&UiEvent::Clicked { path: v.clone(), name: "EffectViewGroup".into(), id: 0, on: false });
    assert_eq!(ed.app.menu.as_ref().map(|m| m.items.len()), Some(13));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: v.clone(), name: "EffectViewGroup".into(), index: 12 });
    assert_eq!(label(&ed).as_deref(), Some("Mst"));
    assert_eq!(ed.app.ctrl(&v).unwrap().text, "M");
    // E3 selects engine 3.
    ed.handle(&UiEvent::Clicked { path: v.clone(), name: "EffectViewGroup".into(), id: 0, on: false });
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: v, name: "EffectViewGroup".into(), index: 2 });
    assert_eq!(ed.model.engine, 2);
    assert_eq!(label(&ed).as_deref(), Some("E 3"));
}

#[test]
fn browser_loading_bar_follows_loader_progress() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(b) = ed.browser.as_mut() else { return };
    b.loading = Some((std::path::PathBuf::from("/x/Kick.wav"), 0.0));
    ed.load_progress(0.5);
    assert_eq!(ed.browser.as_ref().unwrap().loading.as_ref().map(|l| l.1), Some(0.5));
    ed.load_progress(1.5);
    assert!(ed.browser.as_ref().unwrap().loading.is_none());
}

#[test]
fn pattern_key_drag_out_writes_a_midi_file() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(k) = ed.app.root.path_of("Key2") else { return };
    ed.handle(&UiEvent::DragOut { from: k });
    let files = ed.app.drag_out.take().expect("files for the OS drag");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("mid"));
    assert!(files[0].exists());
    let _ = std::fs::remove_file(&files[0]);
}

#[test]
fn mixer_output_menu_routes_the_channel() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    // First visible OutputCombo with a strip channel.
    fn all(c: &Ctrl, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.name == "OutputCombo" && c.visible {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            all(k, path, out);
            path.pop();
        }
    }
    let mut combos = Vec::new();
    all(&ed.app.root, &mut Vec::new(), &mut combos);
    let before = ed.model.doc.clone();
    for c in combos {
        ed.handle(&UiEvent::Clicked { path: c.clone(), name: "OutputCombo".into(), id: 0, on: false });
        ed.handle(&UiEvent::MenuSelected { anchor: c.clone(), name: "OutputCombo".into(), index: 3 });
        if ed.model.doc != before {
            break;
        }
    }
    fn find(n: &dreist_formats::Node) -> bool {
        n.attr("OutputLocation") == Some("Sub output 2") || n.children.iter().any(find)
    }
    assert!(find(&ed.model.doc), "a channel routed to Sub output 2");
    assert_eq!(super::mixer::output_items()[3], "S 2");
}

#[test]
fn mixer_pad_strip_right_click_opens_the_pad_menu() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    fn all(c: &Ctrl, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.name == "OutputCombo" && c.visible {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            all(k, path, out);
            path.pop();
        }
    }
    let mut combos = Vec::new();
    all(&ed.app.root, &mut Vec::new(), &mut combos);
    let mut opened = false;
    for c in combos {
        let strip = c[..c.len() - 1].to_vec();
        if ed.mixers.iter().any(|m| m.pad_strip_at(&ed.app, &strip).is_some()) {
            ed.handle(&UiEvent::RightClicked { path: strip.clone(), name: String::new() });
            let items = ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap_or_default();
            assert!(items.iter().any(|s| s.ends_with("Lock pad")), "{items:?}");
            opened = true;
            break;
        }
    }
    assert!(opened, "a pad strip");
}

#[test]
fn header_song_loop_restart_panic_and_gain() {
    use dreist_engine::binding::id;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let kinds = |ed: &mut Editor| -> Vec<(u32, f32)> { ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some((m.kind, m.value[0])) } else { None }).collect() };
    let ctrl = |ed: &Editor, n: &str| ed.app.ctrl(&ed.app.root.path_of(n).unwrap()).unwrap().clone();
    // Song mode off: Restart and Loop disabled.
    assert!(!ctrl(&ed, "RestartButton").enabled);
    assert!(!ctrl(&ed, "LoopButton").enabled);
    let p = ed.app.root.path_of("SongButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: p, name: "SongButton".into(), id: 0, on: true });
    assert!(kinds(&mut ed).contains(&(id::SONG_SET_ACTIVE, 1.0)));
    assert!(ctrl(&ed, "SongButton").on);
    assert!(ctrl(&ed, "RestartButton").enabled);
    assert!(ctrl(&ed, "LoopButton").enabled);
    // A host loop disables Loop.
    ed.set_host_loop(true);
    assert!(!ctrl(&ed, "LoopButton").enabled);
    ed.set_host_loop(false);
    let p = ed.app.root.path_of("LoopButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: p, name: "LoopButton".into(), id: 0, on: true });
    assert!(kinds(&mut ed).contains(&(id::SONG_SET_INTERNAL_LOOP_ACTIVE, 1.0)));
    assert!(ctrl(&ed, "LoopButton").on);
    let p = ed.app.root.path_of("RestartButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: p, name: "RestartButton".into(), id: 0, on: false });
    assert!(kinds(&mut ed).iter().any(|k| k.0 == id::RETURN_TO_ZERO));
    // Panic: the message, play and record off.
    let rec = ed.app.root.path_of("RecButton").unwrap();
    ed.app.ctrl_mut(&rec).unwrap().on = true;
    let p = ed.app.root.path_of("PanicButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: p, name: "PanicButton".into(), id: 0, on: false });
    assert!(kinds(&mut ed).iter().any(|k| k.0 == id::PANIC));
    assert!(!ctrl(&ed, "RecButton").on);
    // Master gain: drag, then a double click resets to 0 dB.
    let g = ed.app.root.path_of("GlobalGain").unwrap();
    ed.handle(&UiEvent::ValueChanged { path: g.clone(), name: "GlobalGain".into(), id: 6000, value: 0.5 });
    assert!(kinds(&mut ed).contains(&(id::SET_MASTER_CHANNEL_GAIN, 0.5)));
    assert_eq!(ctrl(&ed, "GlobalGain").value, 0.5);
    ed.handle(&UiEvent::DoubleClicked { path: g, name: "GlobalGain".into() });
    assert!(kinds(&mut ed).contains(&(id::SET_MASTER_CHANNEL_GAIN, dreist_formats::params::UNITY_GAIN)));
    // Indicators: 10 ticks after a MIDI block or a stolen voice.
    let mut m = dreist_engine::Meters { midi_received: true, ..Default::default() };
    ed.update_meters(&m);
    assert!(ctrl(&ed, "MIDIIndi").on);
    assert!(!ctrl(&ed, "VoiceIndi").on);
    m.midi_received = false;
    for _ in 0..9 {
        ed.update_meters(&m);
    }
    assert!(ctrl(&ed, "MIDIIndi").on);
    ed.update_meters(&m);
    assert!(!ctrl(&ed, "MIDIIndi").on);
    // Learn: the tempo box binds tap tempo.
    let t = ed.app.root.path_of("BPMTextBox").unwrap();
    let target = ed.learn_capture(&UiEvent::Pressed { path: t, name: "BPMTextBox".into() }).unwrap();
    assert_eq!((target.proto.kind, target.name.as_str(), target.group.as_str(), target.kind), (id::TAP_TEMPO, "Tap tempo", "Transport", "Gated"));
}

#[test]
fn undo_history_menu_steps_back() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let original = ed.model.doc.clone();
    for k in 0..3 {
        let before = ed.model.doc.clone();
        ed.model.doc.set("Probe", k);
        // Mimic an edit event's snapshot.
        ed.history.undo.push_back((format!("Edit {k}"), before));
    }
    let h = ed.app.root.path_of("UndoHistory").unwrap();
    ed.handle(&UiEvent::Clicked { path: h.clone(), name: "UndoHistory".into(), id: 0, on: false });
    // Items: "Edit 2", divider, "Edit 1", "Edit 0": index 3 undoes all three.
    ed.handle(&UiEvent::MenuSelected { anchor: h, name: "UndoHistory".into(), index: 3 });
    assert_eq!(ed.model.doc, original);
    assert_eq!(ed.history.redo.len(), 3);
    assert_eq!(ed.history.redo.last().map(|r| r.0.as_str()), Some("Edit 0"));
}

#[test]
fn pattern_export_renders_the_engine_pattern() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.patmenu.export = true;
    ed.handle(&UiEvent::Wheel { path: vec![], overload: String::new(), dy: 0.0 });
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Play(false))));
    let e = ed.model.engine;
    assert!(
        cmds.iter().any(
            |c| matches!(c, Command::ExportAudio { options, reveal: true, path, .. } if options.source == dreist_engine::export::Source::Engine(e) && path.extension().is_some_and(|x| x == "wav"))
        )
    );
}

#[test]
fn multitrack_automation_lanes() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let pp = ed.pattern.as_mut().expect("pattern panel");
    pp.multi = true;
    ed.sync();
    // Open track row 0's automation.
    let rows = ed.app.root.path_of("ControlsScrollView").unwrap();
    let row0 = ed.app.ctrl(&rows).unwrap().children.iter().position(|c| c.attr("trackRow") == Some("0")).unwrap();
    let ab = ed.app.ctrl(&[rows.clone(), vec![row0]].concat()).unwrap().children.iter().position(|c| c.name == "AutomateButton").unwrap();
    let ab = [rows.clone(), vec![row0, ab]].concat();
    ed.handle(&UiEvent::Clicked { path: ab, name: "AutomateButton".into(), id: 0, on: true });
    let autos = |ed: &Editor| ed.app.ctrl(&rows).unwrap().children.iter().filter(|c| c.attr("autoTrack").is_some()).count();
    assert_eq!(autos(&ed), 1, "one lane row");
    // Add a second lane.
    let a = ed.app.ctrl(&rows).unwrap().children.iter().position(|c| c.attr("autoTrack").is_some()).unwrap();
    let add = ed.app.ctrl(&[rows.clone(), vec![a]].concat()).unwrap().children.iter().position(|c| c.name == "AddButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: [rows.clone(), vec![a, add]].concat(), name: "AddButton".into(), id: 0, on: false });
    assert_eq!(autos(&ed), 2);
    // Draw on the first lane row: track 0's velocity lane.
    let grid = ed.app.root.path_of("PatternGrid").unwrap();
    let (gx, gy) = ed.app.origin(&grid);
    let e = ed.model.engine;
    let first = ed.model.visible_pads().0;
    let lay = ed.pattern.as_ref().unwrap().layout_for(&ed.model, ed.app.ctrl(&grid).unwrap().rect.w, ed.app.ctrl(&grid).unwrap().rect.h);
    let g = lay.graphs[0];
    assert_eq!((g.track, g.lane), (first, 0));
    let _ = (gx, gy);
    ed.handle(&UiEvent::Mouse { path: grid.clone(), overload: "PatternGrid".into(), kind: dreist_gui::app::MouseKind::Down, x: 2.0, y: g.y + g.h * 0.75 });
    ed.handle(&UiEvent::Mouse { path: grid, overload: "PatternGrid".into(), kind: dreist_gui::app::MouseKind::Up, x: 2.0, y: g.y + g.h * 0.75 });
    let pat = ed.model.pattern(e, ed.model.current_pattern(e));
    let v = pat.tracks[first].steps[lay.first_step].lanes[0];
    assert!((v - 0.25).abs() < 0.02, "velocity drawn {v}");
    if let Ok(out) = std::env::var("DREIST_MULTITRACK_SHOT") {
        ed.render(1.0).save_png(out).unwrap();
    }
}

#[test]
fn song_part_select_move_duplicate() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 2;
    ed.sync();
    let body = ed.app.root.path_of("SongBody").unwrap();
    let (bx, by) = ed.app.origin(&body);
    let press = |ed: &mut Editor, x: f32, y: f32, drag_to: Option<f32>| {
        for ev in ed.app.mouse_move(bx as f32 + x, by as f32 + y) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        if let Some(x2) = drag_to {
            for ev in ed.app.mouse_move(bx as f32 + x2, by as f32 + y) {
                ed.handle(&ev);
            }
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
    };
    // Insert a part on track 1 at bar 1, then drag it two bars right.
    press(&mut ed, 5.0, 30.0, None);
    let song = |ed: &Editor| ed.model.doc.children.iter().find(|c| c.class == "SongSequencer").map(dreist_engine::song::Song::from_node).unwrap();
    assert_eq!(song(&ed).tracks[0].len(), 1);
    let zoom = 16.0;
    press(&mut ed, 5.0, 30.0, Some(5.0 + 8.0 * zoom));
    assert!((song(&ed).tracks[0][0].start - 8.0).abs() < 1e-6, "{:?}", song(&ed).tracks[0]);
    let dup = ed.app.root.path_of("DuplicateButton").unwrap();
    let (dx, dy) = ed.app.origin(&dup);
    for ev in ed.app.mouse_move(dx as f32 + 3.0, dy as f32 + 3.0) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    assert_eq!(song(&ed).tracks[0].len(), 2);
}

#[test]
fn damaged_frames_match_full_renders() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    for scale in [1.0, 2.0] {
        let _ = ed.render(scale);
        for k in 0..4 {
            let mut m = dreist_engine::Meters { master: [0.3 + 0.1 * k as f32, 0.5], ..Default::default() };
            for e in 0..8 {
                m.engine[e] = [0.1 * e as f32 + 0.05 * k as f32, 0.2];
            }
            ed.update_meters(&m);
            assert!(!ed.app.dirty && ed.app.needs_redraw());
            let partial = ed.render_frame(scale).clone();
            let full = ed.render(scale);
            // Shapes crossing a damage edge go through clip masks:
            // only a few anti-aliased edge pixels may differ.
            let px = |d: &[u8]| d.chunks(4).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>();
            let (a, b) = (px(partial.data()), px(full.data()));
            let off = a.iter().zip(&b).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 1)).count();
            assert!(off * 1000 <= a.len(), "scale {scale} tick {k}: {off} pixels differ");
        }
    }
    // Hover over controls, then over an open menu's items.
    let _ = ed.render(1.0);
    let check = |ed: &mut Editor, what: &str| {
        let partial = ed.render_frame(1.0).clone();
        let full = ed.render(1.0);
        let off = partial.data().chunks(4).zip(full.data().chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).take(3).any(|(p, q)| p.abs_diff(*q) > 1)).count();
        assert!(off * 1000 <= partial.data().len() / 4, "{what}: {off} pixels differ");
    };
    for (x, y) in [(1160.0, 20.0), (1200.0, 20.0), (1240.0, 47.0), (100.0, 700.0), (300.0, 140.0)] {
        let _ = ed.app.mouse_move(x, y);
        check(&mut ed, "hover");
    }
    if let Some(p) = ed.app.root.path_of("ToolButton") {
        ed.app.open_menu(&p, vec!["One".into(), "Two".into(), "Three".into()], None);
        let _ = ed.render(1.0);
        let m = ed.app.menu.clone().unwrap();
        for i in 0..3 {
            let _ = ed.app.mouse_move(m.x + 5.0, m.y + 9.0 + i as f32 * 18.0);
            check(&mut ed, "menu hover");
        }
        ed.app.menu = None;
    }
    // Playheads on the song and pattern tabs.
    for tab in [2, 0] {
        ed.model.editor_tab = tab;
        ed.sync();
        let _ = ed.render(1.0);
        for k in 0..5u32 {
            ed.set_playback(true, [Some(k); 8], Some(k as f64 * 1.37));
            let partial = ed.render_frame(1.0).clone();
            let full = ed.render(1.0);
            let off = partial.data().chunks(4).zip(full.data().chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).take(3).any(|(p, q)| p.abs_diff(*q) > 1)).count();
            assert!(off * 1000 <= partial.data().len() / 4, "tab {tab} tick {k}: {off} pixels differ");
        }
    }
}

#[test]
fn keyboard_accelerators() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    ed.modals.options.set("RespondToExtendedKeyboard", false);
    let key = |ed: &mut Editor, k: &str, cmd: bool, shift: bool| {
        ed.app.mods.cmd = cmd;
        ed.app.mods.shift = shift;
        for ev in ed.app.shortcut(k) {
            ed.handle(&ev);
        }
        ed.app.mods = Default::default();
    };
    let plays = |ed: &mut Editor| ed.outbox.drain(..).filter_map(|c| if let Command::Play(on) = c { Some(on) } else { None }).collect::<Vec<_>>();
    // A plugin without "extended keyboard": Space is not a shortcut.
    key(&mut ed, " ", false, false);
    assert!(plays(&mut ed).is_empty());
    // The standalone's menu bar: Space toggles Play.
    ed.menu_bar = true;
    key(&mut ed, " ", false, false);
    assert_eq!(plays(&mut ed), vec![true]);
    assert!(ed.model.playing);
    key(&mut ed, " ", false, false);
    assert_eq!(plays(&mut ed), vec![false]);
    // Number keys switch panels; M toggles the metronome.
    key(&mut ed, "8", false, false);
    assert_eq!(ed.model.editor_tab, 2);
    key(&mut ed, "3", false, false);
    assert_eq!(ed.model.editor_tab, 0);
    key(&mut ed, "m", false, false);
    assert!(ed.outbox.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::TOGGLE_METRONOME && m.value[0] == 1.0)));
    // Menu bar items run regardless of the keyboard options.
    ed.menu_bar = false;
    ed.menu_command(0x1b);
    assert_eq!(plays(&mut ed), vec![true]);
    let (app, menus) = ed.menu_bar_items();
    assert!(app.iter().any(|i| i.title == "Audio Settings..."));
    assert_eq!(menus.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["File", "Edit", "Browser", "Transport", "View"]);
    assert!(menus[0].1.iter().any(|i| i.title == "Open Others..." && i.submenu.iter().any(|s| s.title == "Open Pattern As MIDI")));
    // Cmd+Z runs the header's Undo (nothing to undo: no change).
    let doc = ed.model.doc.clone();
    key(&mut ed, "z", true, false);
    assert_eq!(ed.model.doc, doc);
}

#[test]
fn pattern_select_copy_paste() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    let cur = ed.model.current_pattern(e);
    let on = |ed: &Editor| -> Vec<(usize, usize)> {
        let pat = ed.model.pattern(e, cur);
        (0..16).flat_map(|t| (0..pat.steps as usize).map(move |s| (t, s))).filter(|&(t, s)| pat.tracks[t].steps[s].on).collect()
    };
    let before = on(&ed);
    assert!(!before.is_empty());
    let key = |ed: &mut Editor, k: &str, cmd: bool| {
        ed.app.mods.cmd = cmd;
        for ev in ed.app.shortcut(k) {
            ed.handle(&ev);
        }
        ed.app.mods.cmd = false;
    };
    key(&mut ed, "a", true);
    key(&mut ed, "x", true);
    assert!(on(&ed).is_empty(), "cut removes the steps");
    ed.model.pad[e] = 0;
    key(&mut ed, "v", true);
    let after = on(&ed);
    // Pasted relative to the top track, so the same set when the
    // selection started at track 0 / step 0.
    let (t0, s0) = (before.iter().map(|x| x.0).min().unwrap(), before.iter().map(|x| x.1).min().unwrap());
    let expect: Vec<(usize, usize)> = before.iter().map(|&(t, s)| (t - t0, s - s0)).collect();
    assert_eq!(after, expect);
    key(&mut ed, "right", false);
    assert_eq!(on(&ed).len(), after.len());
}

#[test]
fn pattern_track_level_knob_sets_the_pad_gain() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let rows = ed.app.root.path_of("ControlsScrollView").unwrap();
    // The third track row's knob.
    let mut found = Vec::new();
    fn walk(c: &dreist_gui::Ctrl, p: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.name == "PadLevel" {
            out.push(p.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            p.push(i);
            walk(k, p, out);
            p.pop();
        }
    }
    walk(ed.app.ctrl(&rows).unwrap(), &mut Vec::new(), &mut found);
    let knob = [rows.clone(), found[2].clone()].concat();
    ed.take_commands();
    ed.handle(&UiEvent::ValueChanged { path: knob, name: "PadLevel".into(), id: 0, value: 0.4 });
    let e = ed.model.engine;
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_PAD_PARAMETER && m.index[..2] == [e as i32, 2] && m.value[0] == 0.4)), "{cmds:?}");
    assert!((ed.model.pad_gain(e, 2) - 0.4).abs() < 1e-6);
}

#[test]
fn knob_double_click_resets() {
    use dreist_engine::binding::id;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let dbl = |ed: &mut Editor, path: Vec<usize>, name: &str| {
        ed.take_commands();
        ed.handle(&UiEvent::DoubleClicked { path, name: name.into() });
        ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect::<Vec<_>>()
    };
    // Layer cutoff → 1.0 (base value even with a transmod source shown).
    ed.model.source = 3;
    let cutoff = ed.app.root.path_of("Cutoff").unwrap();
    let m = dbl(&mut ed, cutoff, "Cutoff");
    let cut = dreist_formats::LayerParam::FilterCutoff as u8 as f32;
    assert!(m.iter().any(|m| m.kind == id::SET_LAYER_PARAMETER && m.value[..3] == [cut, 1.0, 0.0]), "{m:?}");
    assert_eq!(ed.model.source, 3);
    // A mixer strip's pan → 0.5.
    ed.model.editor_tab = 4;
    ed.sync();
    let pan = ed.app.root.path_of("PanKnob").unwrap();
    let m = dbl(&mut ed, pan, "PanKnob");
    assert!(m.iter().any(|m| m.value[0] == 0.5 || (m.kind == id::SET_LAYER_PARAMETER && m.value[1] == 0.5)), "{m:?}");
    // Off: a typed edit instead.
    let mut o = options::Options::load();
    o.set("DoubleClickResets", false);
    o.save();
    let cutoff = ed.app.root.path_of("Cutoff").unwrap();
    assert!(dbl(&mut ed, cutoff, "Cutoff").is_empty());
    assert!(ed.app.root.path_of(KNOB_EDIT).is_some());
}

#[test]
fn hovering_a_modulated_layer_knob_outlines_its_sources() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    let (pad, l) = (ed.model.pad[e], ed.model.layer[e]);
    let key = dreist_engine::engine::doc::attr_name(dreist_formats::LayerParam::FilterCutoff.name(), 3).unwrap();
    let layer = ed.model.engine_mut(e).child_mut_or_insert("Pad", &format!("Pad{pad}")).child_mut_or_insert("Layer", &format!("Layer{l}"));
    layer.set(&key, 0.4);
    let style = |ed: &Editor, n: &str| ed.app.ctrl(&ed.app.root.path_of(n).unwrap()).unwrap().style.clone();
    let knob = ed.app.root.path_of("Cutoff").unwrap();
    let (x, y) = ed.app.origin(&knob);
    for ev in ed.app.mouse_move(x as f32 + 4.0, y as f32 + 4.0) {
        ed.handle(&ev);
    }
    assert_eq!(style(&ed, "Src3"), "TransmodButtonStyleHighlight");
    assert_eq!(style(&ed, "Src4"), "TransmodButtonStyle");
    for ev in ed.app.mouse_move(2.0, 2.0) {
        ed.handle(&ev);
    }
    assert_eq!(style(&ed, "Src3"), "TransmodButtonStyle");
}

#[test]
fn knob_right_click_menu() {
    use dreist_engine::binding::id;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let cutoff = ed.app.root.path_of("Cutoff").unwrap();
    let open = |ed: &mut Editor| {
        ed.handle(&UiEvent::RightClicked { path: cutoff.clone(), name: "Cutoff".into() });
        ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap_or_default()
    };
    let pick = |ed: &mut Editor, index: usize| {
        ed.take_commands();
        ed.handle(&UiEvent::MenuSelected { anchor: cutoff.clone(), name: "Cutoff".into(), index });
        ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect::<Vec<_>>()
    };
    assert_eq!(open(&mut ed), ["Reset", "Edit", "", "Remove all modulation from this control"]);
    let cut = dreist_formats::LayerParam::FilterCutoff as u8 as f32;
    let m = pick(&mut ed, 0);
    assert!(m.iter().any(|m| m.kind == id::SET_LAYER_PARAMETER && m.value[..3] == [cut, 1.0, 0.0]));
    let m = {
        open(&mut ed);
        pick(&mut ed, 3)
    };
    assert_eq!(m.iter().filter(|m| m.kind == id::SET_LAYER_PARAMETER && m.value[1] == 0.0).count(), 28);
    // Edit: typed in Hz.
    open(&mut ed);
    pick(&mut ed, 1);
    let edit = ed.app.root.path_of(KNOB_EDIT).unwrap();
    assert!(ed.app.ctrl(&edit).unwrap().text.ends_with("Hz"));
    ed.take_commands();
    ed.handle(&UiEvent::TextCommitted { path: edit, name: KNOB_EDIT.into(), text: "440 Hz".into() });
    let v = dreist_formats::params::Curve::Cutoff.from_units(440.0);
    let m: Vec<_> = ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect();
    assert!(m.iter().any(|m| m.kind == id::SET_LAYER_PARAMETER && m.value[0] == cut && (m.value[1] - v).abs() < 1e-5), "{m:?}");
    assert!(ed.app.root.path_of(KNOB_EDIT).is_none());
}

#[test]
fn pad_bank_menu() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let bank = ed.app.root.path_of("PadBank").unwrap();
    let a = [bank.clone(), vec![0]].concat();
    let b = [bank.clone(), vec![1]].concat();
    let pick = |ed: &mut Editor, at: &Vec<usize>, index: usize| {
        ed.handle(&UiEvent::RightClicked { path: at.clone(), name: "x".into() });
        let items = ed.app.menu.as_ref().unwrap().items.clone();
        ed.handle(&UiEvent::MenuSelected { anchor: at.clone(), name: "x".into(), index });
        items
    };
    let items = pick(&mut ed, &a, 4);
    assert_eq!(items[0], "Cut");
    assert!(items[2].starts_with(dreist_gui::menu::DISABLED));
    let e = ed.model.engine;
    assert!((0..16).all(|q| ed.model.pad_mute(e, q)));
    pick(&mut ed, &a, 9);
    assert!((0..16).all(|q| !ed.model.pad_mute(e, q)));
    // Copy bank A, paste into B.
    let name0 = ed.model.pad_name(e, 0);
    pick(&mut ed, &a, 1);
    pick(&mut ed, &b, 2);
    assert_eq!(ed.model.pad_name(e, 16), name0);
}

#[test]
fn output_selector_route_all_menu() {
    use dreist_engine::binding::id;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    ed.model.editor_tab = 4;
    ed.sync();
    // The first visible OutputCombo of the pad mixer.
    fn find(c: &dreist_gui::Ctrl, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if (k.visible && k.name == "OutputCombo") || (k.visible && find(k, path)) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut oc = Vec::new();
    assert!(find(&ed.app.root, &mut oc));
    ed.handle(&UiEvent::RightClicked { path: oc.clone(), name: "OutputCombo".into() });
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    ed.take_commands();
    ed.handle(&UiEvent::MenuSelected { anchor: oc, name: "OutputCombo".into(), index: items.len() - 1 });
    let m: Vec<_> = ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect();
    let kinds: std::collections::BTreeSet<u32> = m.iter().map(|m| m.kind).collect();
    assert!(kinds.len() == 1 && [id::SET_PAD_OUTPUT, id::SET_LAYER_OUTPUT, id::SET_ENGINE_OUTPUT].contains(kinds.iter().next().unwrap()), "{items:?} {kinds:?}");
    assert!(m.iter().enumerate().all(|(i, m)| m.value[0] == (i + 2) as f32));
}

#[test]
fn engine_selector_menu() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let base = ed.header.selectors.clone().unwrap();
    let sel = |e: usize| {
        let mut p = [base.clone(), vec![e]].concat();
        let s = ed_ctrl_child(&ed, &p, "EngineSelectionButton");
        p.push(s);
        p
    };
    fn ed_ctrl_child(ed: &Editor, p: &[usize], n: &str) -> usize {
        ed.app.ctrl(p).unwrap().children.iter().position(|c| c.name == n).unwrap()
    }
    let s6 = sel(6);
    let s0 = sel(0);
    let pick = |ed: &mut Editor, at: &Vec<usize>, index: usize| {
        ed.handle(&UiEvent::RightClicked { path: at.clone(), name: "EngineSelectionButton".into() });
        let items = ed.app.menu.as_ref().unwrap().items.clone();
        ed.handle(&UiEvent::MenuSelected { anchor: at.clone(), name: "x".into(), index });
        items
    };
    let items = pick(&mut ed, &s6, 3);
    assert_eq!(items[0], "Rename engine");
    assert!(items[4].starts_with(dreist_gui::menu::DISABLED));
    let pads = |ed: &Editor, e: usize| ed.model.engine_node(e).map(|n| n.children.iter().filter(|c| c.class == "Pad").count()).unwrap_or(0);
    let pads6 = pads(&ed, 6);
    assert!(pads6 > 0);
    // Paste engine 7's copy over engine 1.
    pick(&mut ed, &s0, 4);
    assert_eq!(pads(&ed, 0), pads6);
    // Reset engine 1.
    pick(&mut ed, &s0, 10);
    assert!(ed.model.engine_node(0).is_none());
    // Rename.
    pick(&mut ed, &s6, 0);
    let edit = ed.app.root.path_of(ENGINE_NAME_EDIT).unwrap();
    ed.handle(&UiEvent::TextCommitted { path: edit, name: ENGINE_NAME_EDIT.into(), text: "Drums".into() });
    assert_eq!(ed.model.engine_name(6), "Drums");
}

#[test]
fn source_button_menu() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let src = ed.app.root.path_of("Src3").unwrap();
    let pick = |ed: &mut Editor, index: usize, open: bool| {
        if open {
            ed.handle(&UiEvent::RightClicked { path: src.clone(), name: "Src3".into() });
        }
        let items = ed.app.menu.as_ref().unwrap().items.clone();
        ed.handle(&UiEvent::MenuSelected { anchor: src.clone(), name: "Src3".into(), index });
        items
    };
    let items = pick(&mut ed, 0, true);
    assert_eq!(items[2], "Remove all modulation from Dreist");
    assert!(items[0].starts_with(dreist_gui::menu::CHECK));
    let name = dreist_formats::TransmodSource::ALL[3].name();
    assert_eq!(ed.model.doc.bool(&format!("Polar_{name}")), Some(false));
    // Remove all modulation from this source, confirmed.
    let e = ed.model.engine;
    let (pad, l) = (ed.model.pad[e], ed.model.layer[e]);
    let key = dreist_engine::engine::doc::attr_name(dreist_formats::LayerParam::FilterCutoff.name(), 3).unwrap();
    ed.model.layer_mut(e, pad, l).set(&key, 0.5);
    pick(&mut ed, 1, true);
    let confirm = pick(&mut ed, 2, false);
    assert!(confirm[0].starts_with("Are you sure"));
    assert!(ed.model.layer_node(e, pad, l).unwrap().attr(&key).is_none());
}

#[test]
fn effect_panel_cut_copy_paste() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    fn find(c: &dreist_gui::Ctrl, path: &mut Vec<usize>, f: &dyn Fn(&dreist_gui::Ctrl) -> bool) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            if !k.visible {
                continue;
            }
            path.push(i);
            if f(k) || find(k, path, f) {
                return true;
            }
            path.pop();
        }
        false
    }
    for tab in [3, 4, 5] {
        ed.model.editor_tab = tab;
        ed.sync();
        // The mixer's Effects mode shows the selected strip's chain.
        let mut mg = Vec::new();
        if find(&ed.app.root, &mut mg, &|k| k.name == "ModeGroup") {
            mg.push(ed.app.ctrl(&mg).unwrap().children.len() - 1);
            ed.handle(&UiEvent::Clicked { path: mg, name: "x".into(), id: 0, on: true });
            ed.sync();
        }
        let mut panel = Vec::new();
        if !find(&ed.app.root, &mut panel, &|k| k.attr("fxSlot").is_some() && k.attr("fxKind").is_some_and(|v| v != "0")) {
            continue;
        }
        ed.handle(&UiEvent::RightClicked { path: panel.clone(), name: "x".into() });
        let items = ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap_or_default();
        assert_eq!(items[..2], ["Cut".to_string(), "Copy".into()]);
        ed.handle(&UiEvent::MenuSelected { anchor: panel.clone(), name: "x".into(), index: 1 });
        ed.handle(&UiEvent::RightClicked { path: panel.clone(), name: "x".into() });
        let items = ed.app.menu.as_ref().unwrap().items.clone();
        assert_eq!(items[2], "Paste");
        ed.take_commands();
        ed.handle(&UiEvent::MenuSelected { anchor: panel, name: "x".into(), index: 2 });
        assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })));
        return;
    }
    panic!("no effect panel found");
}

#[test]
fn track_length_marker_drag_and_reset() {
    use dreist_gui::app::MouseKind;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let grid = ed.app.root.path_of("PatternGrid").unwrap();
    let w = ed.app.ctrl(&grid).unwrap().rect.w as f32;
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let e = ed.model.engine;
    let first = lay.first_track;
    let mouse = |ed: &mut Editor, kind: MouseKind, x: f32, y: f32| ed.handle(&UiEvent::Mouse { path: grid.clone(), overload: "PatternGrid".into(), kind, x, y });
    ed.take_commands();
    // The full-length marker sits at the right edge.
    mouse(&mut ed, MouseKind::Down, w - 5.0, lay.row_y(0) + 3.0);
    mouse(&mut ed, MouseKind::Drag, lay.step_x(4) + 2.0, lay.row_y(0) + 3.0);
    mouse(&mut ed, MouseKind::Up, lay.step_x(4) + 2.0, lay.row_y(0) + 3.0);
    let m: Vec<_> = ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect();
    assert!(m.iter().any(|m| m.kind == dreist_engine::binding::id::SET_TRACK_LENGTH && m.index[1] == first as i32 && m.value[0] == 4.0), "{m:?}");
    let cur = ed.model.current_pattern(e);
    assert_eq!(ed.model.pattern(e, cur).tracks[first].length, 4);
    // Right press on the marker resets it.
    let x = lay.step_x(4) + 3.0;
    mouse(&mut ed, MouseKind::RightDown, x, lay.row_y(0) + 3.0);
    let steps = ed.model.pattern(e, cur).steps;
    assert_eq!(ed.model.pattern(e, cur).tracks[first].length, steps);
}

#[test]
fn source_and_macro_names_and_rename() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let text = |ed: &Editor, n: &str| ed.app.ctrl(&ed.app.root.path_of(n).unwrap()).unwrap().text.clone();
    // Modulator 0 is an LFO, 1 S&H; macro 1 is "Reverb".
    assert_eq!(text(&ed, "Src5"), "LFO");
    assert_eq!(text(&ed, "Src6"), "S&H");
    assert_eq!(text(&ed, "Src21"), "Reverb");
    let m0 = ed.app.root.path_of("Macro0").unwrap();
    assert_eq!(ed.app.ctrl(&m0).unwrap().display_name, "Reverb");
    // Rename modulator 0 from its source button.
    let src = ed.app.root.path_of("Src5").unwrap();
    ed.handle(&UiEvent::RightClicked { path: src.clone(), name: "Src5".into() });
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert_eq!(items.last().map(String::as_str), Some("Rename"));
    ed.handle(&UiEvent::MenuSelected { anchor: src, name: "Src5".into(), index: items.len() - 1 });
    let edit = ed.app.root.path_of("TransmodNameEdit").unwrap();
    ed.handle(&UiEvent::TextCommitted { path: edit, name: "TransmodNameEdit".into(), text: "Wobble".into() });
    assert_eq!(text(&ed, "Src5"), "Wobble");
    // Rename macro 1 from its knob.
    ed.handle(&UiEvent::RightClicked { path: m0.clone(), name: "Macro0".into() });
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert_eq!(items, ["Reset", "Edit", "Rename"]);
    ed.handle(&UiEvent::MenuSelected { anchor: m0, name: "Macro0".into(), index: 2 });
    let edit = ed.app.root.path_of("TransmodNameEdit").unwrap();
    ed.handle(&UiEvent::TextCommitted { path: edit, name: "TransmodNameEdit".into(), text: "".into() });
    assert_eq!(text(&ed, "Src21"), "M 1");
}

#[test]
fn automation_reaches_the_document() {
    use dreist_engine::binding::{Message, id};
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let gain = dreist_formats::LayerParam::Gain as i32 as f32;
    ed.automation(&[Message::new(id::SET_LAYER_PARAMETER, &[0, 0, 0], &[gain, 0.25, 0.0])]);
    let l = ed.model.engine_node(0).and_then(|e| e.children.iter().find(|c| c.class == "Pad")).and_then(|p| p.children.iter().find(|c| c.class == "Layer"));
    assert_eq!(l.and_then(|l| l.f32("LayerGain")), Some(0.25));
    assert!(ed.history.undo.is_empty());
}

#[test]
fn header_options_menu() {
    use dreist_engine::binding::id;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let header = ed.app.root.path_of("Header").unwrap();
    let ob = [header.clone(), (ed.app.ctrl(&header).unwrap()).path_of("OptionsButton").unwrap()].concat();
    let open = |ed: &mut Editor| {
        ed.handle(&UiEvent::Clicked { path: ob.clone(), name: "OptionsButton".into(), id: 100001, on: true });
        ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap_or_default()
    };
    let pick = |ed: &mut Editor, index: usize| {
        ed.outbox.clear();
        ed.handle(&UiEvent::MenuSelected { anchor: ob.clone(), name: "OptionsButton".into(), index });
        ed.outbox.iter().filter_map(|c| if let Command::Message(m) = c { Some((m.kind, m.value[0], m.value[1])) } else { None }).collect::<Vec<_>>()
    };
    let items = open(&mut ed);
    assert_eq!(items.len(), 15, "{items:?}");
    assert_eq!(items[1], "✓ Pad selection follows MIDI");
    assert_eq!(items[4], "✓ No pre-roll");
    assert!(items[14].ends_with('▸'));
    assert_eq!(pick(&mut ed, 6), [(id::SET_PREROLL_MODE, 2.0, 0.0)]);
    assert_eq!(open(&mut ed)[6], "✓ 2 bars");
    assert_eq!(pick(&mut ed, 2), [(id::REGISTRY_FOLLOW_OPTIONS_CHANGED, 1.0, 1.0)]);
    // "Metronome only when recording" shares the pre-roll key.
    open(&mut ed);
    assert_eq!(pick(&mut ed, 9), [(id::SET_PREROLL_ONLY_ON_REC, 0.0, 0.0)]);
    open(&mut ed);
    assert_eq!(pick(&mut ed, 10), [(id::SET_METRO_ONLY_DURING_REC, 1.0, 0.0)]);
    assert!(open(&mut ed)[9].starts_with('✓'));
    assert!(!ed.model.follow_playback);
    pick(&mut ed, 0);
    assert!(ed.model.follow_playback);
    // The output sub menu.
    open(&mut ed);
    assert!(pick(&mut ed, 14).is_empty());
    let outs = ed.app.menu.as_ref().unwrap().items.clone();
    assert_eq!((outs.len(), outs[0].as_str(), outs[31].as_str()), (32, "✓ Master", "Sub output 31"));
    assert_eq!(pick(&mut ed, 3), [(id::SET_METRONOME_OUTPUT_LOCATION, 4.0, 0.0)]);
    let metro = |ed: &Editor, k: &str| ed.model.doc.children.iter().find(|c| c.class == "Metronome").and_then(|n| n.attr(k)).map(str::to_string);
    assert_eq!(metro(&ed, "OutputLocation").as_deref(), Some("Sub output 3"));
    // The volume slider and the metronome button write the document.
    open(&mut ed);
    ed.handle(&UiEvent::MenuSlider { anchor: ob.clone(), index: 13, value: 0.25 });
    assert_eq!(metro(&ed, "Gain").as_deref(), Some("0.25"));
    let mb = ed.app.root.path_of("MetroButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: mb, name: "MetroButton".into(), id: 0, on: true });
    assert_eq!(metro(&ed, "Mute").as_deref(), Some("false"));
    // Engine follow messages select.
    let m = |kind, index: &[i32]| dreist_engine::binding::Message { editor: true, ..dreist_engine::binding::Message::new(kind, index, &[]) };
    ed.binding_messages(&[m(id::UPDATE_ENGINE_SELECTION_CHANGED, &[3]), m(id::UPDATE_PAD_SELECTION_CHANGED, &[3, 9])]);
    assert_eq!((ed.model.engine, ed.model.pad[3]), (3, 9));
}

#[test]
fn graph_menu_inserts_a_ramp() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    // The single global graph (toolbar View).
    if let Some(pp) = ed.pattern.as_mut() {
        pp.multi = false;
    }
    ed.sync();
    let ac = ed.app.root.path_of("SingleAutoControl").unwrap();
    let ob = {
        let c = ed.app.ctrl(&ac).unwrap();
        let mut p = ac.clone();
        p.extend(c.path_of("OptionsButton").unwrap());
        p
    };
    let (x, y) = ed.app.origin(&ob);
    for ev in ed.app.mouse_move(x as f32 + 3.0, y as f32 + 3.0) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
        ed.handle(&ev);
    }
    ed.handle(&UiEvent::MenuSelected { anchor: ob.clone(), name: "OptionsButton".into(), index: 12 });
    ed.handle(&UiEvent::MenuSelected { anchor: ob.clone(), name: "OptionsButton".into(), index: 0 });
    let e = ed.model.engine;
    let pat = ed.model.pattern(e, ed.model.current_pattern(e));
    let t = &pat.tracks[ed.model.pad[e]];
    assert!(t.steps[0].lanes[0] < 0.01 && t.steps[8].lanes[0] > 0.4, "ramp up on velocity");
}

#[test]
fn pattern_toolbar_duplicate_quantize_remix_copy() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let e = ed.model.engine;
    let cur = ed.model.current_pattern(e);
    // The toolbar's buttons (the pads and the header share names).
    let click = |ed: &mut Editor, n: &str| {
        let tb = ed.app.root.path_of("ToolBar").unwrap();
        let path = [tb.clone(), (ed.app.ctrl(&tb).unwrap()).path_of(n).unwrap()].concat();
        ed.handle(&UiEvent::Clicked { path, name: n.into(), id: 0, on: true });
    };
    let before = ed.model.pattern(e, cur);
    let n = before.steps as usize;
    click(&mut ed, "RepeatButton");
    let after = ed.model.pattern(e, cur);
    assert_eq!(after.steps as usize, (n * 2).min(1024));
    for t in 0..16 {
        for s in 0..n {
            assert_eq!(after.tracks[t].steps[s + n].on, before.tracks[t].steps[s].on, "track {t} step {s}");
        }
    }
    // Quantize resets every Shift lane.
    ed.handle(&UiEvent::Clicked { path: vec![], name: "QuantizeButton".into(), id: 0, on: true });
    let d = dreist_formats::params::step_lane_default(3);
    assert!(ed.model.pattern(e, cur).tracks.iter().all(|tr| tr.steps.iter().all(|st| (st.lanes[3] - d).abs() < 1e-4)));
    click(&mut ed, "Remix");
    assert_eq!(ed.model.pattern(e, cur).mono, !after.mono);
    // Copy to the next empty pattern.
    let next = (cur + 1..24).find(|&p| !ed.model.pattern_used(e, p));
    click(&mut ed, "CopyToNext");
    if let Some(to) = next {
        assert_eq!(ed.model.current_pattern(e), to);
        assert!(ed.model.pattern_name(e, to).ends_with(" (Copy)"));
        assert_eq!(ed.model.pattern(e, to).steps, ed.model.pattern(e, cur).steps);
    }
    // Single Track View switches to the single global graph (and back,
    // restoring the saved option).
    let multi = ed.pattern.as_ref().unwrap().multi;
    click(&mut ed, "OptionsButton");
    ed.handle(&UiEvent::MenuSelected { anchor: vec![], name: "OptionsButton".into(), index: 2 });
    assert_eq!(ed.pattern.as_ref().unwrap().multi, !multi);
    click(&mut ed, "OptionsButton");
    ed.handle(&UiEvent::MenuSelected { anchor: vec![], name: "OptionsButton".into(), index: 2 });
    assert_eq!(ed.pattern.as_ref().unwrap().multi, multi);
}

#[test]
fn sampler_take_is_saved_and_loaded_into_the_next_layer() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let dir = std::env::temp_dir().join(format!("dreist-takes-{}", std::process::id()));
    ed.modals.options.set("SamplerRecordPath", dir.to_string_lossy());
    ed.model.sampler.auto_mode = 2;
    ed.model.samples = Some(dir.clone());
    ed.sampler.target = Some((0, 9, 3));
    let tone: Vec<f32> = (0..4410).map(|i| (i as f32 * 0.05).sin() * 2.0).collect();
    let r = ed.sampler_take(&tone, &tone, 44100.0);
    assert_eq!(r, Some(sampler::TakeAction::Loaded));
    let file = ed.model.layer_sample(0, 9, 3).expect("layer 3 of pad 10 holds the take");
    assert!(file.starts_with(&dir) && file.exists());
    let d = dreist_engine::audio::SampleCache::default().get(&file).expect("readable wav");
    let peak = d.left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak <= 0.999 && peak > 0.9, "normalised: {peak}");
    assert_eq!(ed.sampler.target, Some((0, 9, 4)));
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SAMPLER_SET_ARMED)));
    assert!(cmds.iter().any(|c| matches!(c, Command::Rebuild { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn engine_step_writes_reach_the_document() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(crate::project::new_document())) else { return };
    let mut data = dreist_engine::pattern::StepData::off();
    data.on = true;
    ed.step_writes(&[dreist_engine::StepWrite { engine: 0, pattern: 2, track: 5, step: 7, data }]);
    let st = ed
        .model
        .engine_node(0)
        .and_then(|n| n.children.iter().find(|c| c.class == "StepPattern" && c.name.as_deref() == Some("StepPattern2")))
        .and_then(|p| p.children.iter().find(|c| c.name.as_deref() == Some("Track5")))
        .and_then(|t| t.children.iter().find(|c| c.name.as_deref() == Some("S7")));
    assert_eq!(st.and_then(|n| n.attr("State")), Some("true"));
}

#[test]
fn v2a_knob_reset_and_edit() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let show = ed.app.root.path_of("ShowSplits").unwrap();
    ed.handle(&UiEvent::Clicked { path: show, name: "ShowSplits".into(), id: 0, on: true });
    let knob = ed.app.root.path_of("v2a").unwrap();
    let v2a = |cmds: &[Command]| {
        cmds.iter().find_map(|c| match c {
            Command::Message(m) if m.kind == dreist_engine::binding::id::SET_PAD_PARAMETER && m.value[1] == 2.0 => Some(m.value[0]),
            _ => None,
        })
    };
    ed.take_commands();
    ed.handle(&UiEvent::RightClicked { path: knob.clone(), name: "v2a".into() });
    assert_eq!(ed.app.menu.as_ref().unwrap().items, vec!["Reset".to_string(), "Edit".to_string()]);
    ed.handle(&UiEvent::MenuSelected { anchor: knob.clone(), name: "v2a".into(), index: 0 });
    assert_eq!(v2a(&ed.take_commands()), Some(1.0));
    ed.app.menu = None;
    ed.handle(&UiEvent::RightClicked { path: knob.clone(), name: "v2a".into() });
    ed.handle(&UiEvent::MenuSelected { anchor: knob.clone(), name: "v2a".into(), index: 1 });
    assert!(ed.app.is_editing());
    for _ in 0..5 {
        ed.app.key_named("Backspace");
    }
    ed.app.key_text("40");
    let (evs, _) = ed.app.key_named("Enter");
    for ev in evs {
        ed.handle(&ev);
    }
    let got = v2a(&ed.take_commands()).expect("edit posts 0x4e");
    assert!((got - 0.4).abs() < 1e-6, "{got}");
    assert!(ed.app.root.path_of("V2AValueEdit").is_none(), "edit box removed");
}

#[test]
fn file_menu_parts_save_and_open() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    ed.model.samples = Some(dreist_formats::model::samples_dir(&p));
    let dir = std::env::temp_dir().join(format!("dreist-parts-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let d2 = dir.clone();
    ed.dialogs = Some(Box::new(move |d| match d {
        Dialog::SavePart(part) | Dialog::OpenPart(part) => Some(d2.join(format!("x.{}", part.file_type().0))),
        Dialog::SaveMidi | Dialog::OpenMidi => Some(d2.join("x.mid")),
        _ => None,
    }));
    let button = ed.app.root.path_of("LoadButton").unwrap();
    let pick = |ed: &mut Editor, label: &str| {
        let items = ed.app.menu.as_ref().expect("file menu").items.clone();
        let i = items.iter().position(|s| s == label).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: button.clone(), name: "LoadButton".into(), index: i });
    };
    let open = |ed: &mut Editor| ed.handle(&UiEvent::Clicked { path: button.clone(), name: "LoadButton".into(), id: 0, on: false });
    open(&mut ed);
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert!(!items.contains(&"New".to_string()) && items.contains(&"Open Engine".to_string()) && items.contains(&"Save Kit As...".to_string()), "{items:?}");
    pick(&mut ed, "Save Engine As...");
    let text = std::fs::read_to_string(dir.join("x.dreistengine")).expect("engine file");
    assert!(text.contains("<Engine name=\"Engine\""), "{}", &text[..200.min(text.len())]);
    assert!(dir.join("x.samples").is_dir(), "samples collected");
    // Save others ▸ Save Pattern.
    ed.app.menu = None;
    open(&mut ed);
    pick(&mut ed, "Save others... ▸");
    pick(&mut ed, "Save Pattern");
    assert!(std::fs::read_to_string(dir.join("x.dreistpattern")).unwrap().contains("<StepPattern name=\"StepPattern\""));
    ed.app.menu = None;
    open(&mut ed);
    pick(&mut ed, "Save others... ▸");
    pick(&mut ed, "Save Pattern As MIDI");
    let mid = std::fs::read(dir.join("x.mid")).expect("midi file");
    assert_eq!(&mid[..4], b"MThd");
    let ch = (ed.model.engine & 15) as u8;
    assert!(mid.windows(1).any(|w| w[0] == 0x90 | ch), "note ons on the engine's channel");
    // Open Pattern From MIDI brings the same pattern back (steps on).
    let pat = ed.model.current_pattern(ed.model.engine);
    let on_before: usize = ed.model.pattern(ed.model.engine, pat).tracks.iter().map(|t| t.steps.iter().filter(|s| s.on).count()).sum();
    ed.app.menu = None;
    open(&mut ed);
    pick(&mut ed, "Open others... ▸");
    pick(&mut ed, "Open Pattern From MIDI");
    let on_after: usize = ed.model.pattern(ed.model.engine, pat).tracks.iter().map(|t| t.steps.iter().filter(|s| s.on).count()).sum();
    assert!(on_after > 0 && on_after <= on_before, "{on_after} vs {on_before}");
    assert_eq!(ed.model.pattern_name(ed.model.engine, pat), "x");
    // Change the engine, then Open Engine brings the file back.
    let e = ed.model.engine;
    let before = ed.model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "Pad").cloned());
    ed.model.engine_mut(e).children.retain(|c| c.class != "Pad");
    ed.take_commands();
    ed.app.menu = None;
    open(&mut ed);
    pick(&mut ed, "Open Engine");
    let after = ed.model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "Pad").cloned());
    assert_eq!(after.map(|n| n.name), before.map(|n| n.name));
    assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn swing_menu_head_items_and_template_pick() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let Some(drop) = ed.app.root.path_of("SwingDrop") else { return };
    ed.handle(&UiEvent::Clicked { path: drop.clone(), name: "SwingDrop".into(), id: 0, on: false });
    let items = ed.app.menu.as_ref().expect("swing menu").items.clone();
    assert_eq!(&items[..3], &["Extract timing from current pattern".to_string(), "Import MIDI template".into(), "Import Guru shuffle".into()]);
    if items.len() > 4 {
        ed.take_commands();
        ed.handle(&UiEvent::MenuSelected { anchor: drop.clone(), name: "SwingDrop".into(), index: 4 });
        assert!(ed.take_commands().iter().any(|c| matches!(c, Command::SwingTemplate(..))));
    }
    // The Swing knob's right-click adds Reset / Edit.
    ed.app.menu = None;
    if let Some(knob) = ed.app.root.path_of("SwingKnob") {
        ed.handle(&UiEvent::RightClicked { path: knob.clone(), name: "SwingKnob".into() });
        let items = ed.app.menu.as_ref().expect("knob menu").items.clone();
        assert_eq!(&items[items.len() - 2..], &["Reset".to_string(), "Edit".into()]);
        let anchor = ed.app.menu.as_ref().unwrap().anchor.clone();
        ed.take_commands();
        ed.handle(&UiEvent::MenuSelected { anchor, name: "SwingDrop".into(), index: items.len() - 2 });
        assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_ENGINE_SWING_TEMPLATE_DEPTH && m.value[0] == 0.0)));
    }
    // Import MIDI template turns a file's timing into a template.
    let dir = std::env::temp_dir().join(format!("dreist-swing-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mid = dir.join("Groove Test.mid");
    assert!(ed.save_pattern_midi(&mid));
    if let Some(out) = ed.import_midi_template(&mid) {
        assert!(out.ends_with("Groove Test.swingtemplate"));
        let _ = std::fs::remove_file(&out);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn engine_selector_level_overlay() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let mut m = dreist_engine::Meters::default();
    m.engine[0] = [1.0, 1.0];
    ed.update_meters(&m);
    fn find(c: &dreist_gui::Ctrl) -> Option<&dreist_gui::Ctrl> {
        if c.attr("volEngine") == Some("0") {
            return Some(c);
        }
        c.children.iter().find_map(find)
    }
    let a = find(&ed.app.root).and_then(|c| c.attr("overlayAlpha")).and_then(|v| v.parse::<f32>().ok());
    assert_eq!(a, Some(0.75));
    if let Ok(out) = std::env::var("DREIST_HEADER_SHOT") {
        let mut m = dreist_engine::Meters::default();
        m.engine[0] = [1.0, 1.0];
        m.master = [0.8, 0.8];
        ed.update_meters(&m);
        ed.render(1.0).save_png(out).unwrap();
    }
    // Falls by 0.8149 of the gap per tick.
    ed.update_meters(&dreist_engine::Meters::default());
    let a = find(&ed.app.root).and_then(|c| c.attr("overlayAlpha")).and_then(|v| v.parse::<f32>().ok()).unwrap();
    assert!((a - 0.75 * 0.8149294).abs() < 1e-4, "{a}");
}

#[test]
fn layer_wave_zoom_bar() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    ed.model.samples = Some(dreist_formats::model::samples_dir(&p));
    let e = ed.model.engine;
    let Some((pad, l)) = (0..64).flat_map(|p| (0..model::LAYERS).map(move |l| (p, l))).find(|&(p, l)| ed.model.layer_sample(e, p, l).is_some_and(|f| f.exists())) else { return };
    ed.model.pad[e] = pad;
    ed.model.layer[e] = l;
    ed.sync();
    assert_eq!(ed.inspector.view, (0.0, 1.0));
    fn tagged(c: &dreist_gui::Ctrl, name: &str, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if (k.name == name && k.attr("layerZoom").is_some()) || tagged(k, name, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut zin = Vec::new();
    assert!(tagged(&ed.app.root, "ZoomInButton", &mut zin));
    if let Ok(out) = std::env::var("DREIST_WAVE_SHOT") {
        ed.render(1.0).save_png(out).unwrap();
    }
    ed.handle(&UiEvent::Clicked { path: zin, name: "ZoomInButton".into(), id: 0, on: false });
    assert!((ed.inspector.view.1 - 0.5).abs() < 1e-4, "{:?}", ed.inspector.view);
    let mut group = Vec::new();
    assert!(tagged(&ed.app.root, "AutoZoomGroup", &mut group));
    group.push(2);
    ed.handle(&UiEvent::Clicked { path: group.clone(), name: String::new(), id: 0, on: true });
    assert!((ed.inspector.view.1 - 0.01).abs() < 1e-4, "start-marker zoom: {:?}", ed.inspector.view);
    // Back to no auto zoom (the option is restored).
    *group.last_mut().unwrap() = 0;
    ed.handle(&UiEvent::Clicked { path: group, name: String::new(), id: 0, on: true });
}

#[test]
fn pad_menu_multi_selection() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    multipad::select(&mut ed.model, e, 1, false, false);
    multipad::select(&mut ed.model, e, 3, true, false);
    fn find(c: &dreist_gui::Ctrl, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if (k.overload == "EditorPad" && k.attr("padIndex") == Some("1")) || find(k, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut pad1 = Vec::new();
    assert!(find(&ed.app.root, &mut pad1));
    let menu = |ed: &mut Editor, label: &str| {
        ed.handle(&UiEvent::Mouse { path: pad1.clone(), overload: "EditorPad".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 5.0, y: 5.0 });
        let items = ed.app.menu.as_ref().expect("pad menu").items.clone();
        assert!(!items.iter().any(|s| s == "Cut" || s.starts_with("Rename")), "{items:?}");
        assert!(items.contains(&"(Clear pad)".to_string()), "{items:?}");
        let i = items.iter().position(|s| s.ends_with(label)).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: pad1.clone(), name: "EditorPad".into(), index: i });
    };
    menu(&mut ed, "Lock pad");
    assert!(padmenu::pad_locked(&ed.model, e, 1) && padmenu::pad_locked(&ed.model, e, 3));
    assert!(!padmenu::pad_locked(&ed.model, e, 2));
    menu(&mut ed, "Set colour label... ▸");
    ed.handle(&UiEvent::MenuSelected { anchor: pad1.clone(), name: "EditorPad".into(), index: 8 });
    assert_eq!(ed.model.pad_node(e, 3).and_then(|n| n.attr("PadColour")), Some("0, 128, 255, 255"));
}

#[test]
fn layer_row_menu() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let mut model = Model::new(proj.raw);
    model.samples = Some(dreist_formats::model::samples_dir(&p));
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), model) else { return };
    fn find(c: &dreist_gui::Ctrl, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if k.overload == "LayerRow" && k.attr("layer") == Some("0") {
                return true;
            }
            if find(k, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut row = Vec::new();
    assert!(find(&ed.app.root, &mut row), "layer row");
    let e = ed.model.engine;
    let pad = ed.model.pad[e];
    let menu = |ed: &mut Editor, label: &str| -> Vec<String> {
        ed.handle(&UiEvent::Mouse { path: row.clone(), overload: "LayerRow".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 5.0, y: 5.0 });
        let items = ed.app.menu.as_ref().expect("layer menu").items.clone();
        let i = items.iter().position(|s| s.ends_with(label)).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: row.clone(), name: "LayerRow".into(), index: i });
        items
    };
    assert!(ed.model.layer(e, pad, 0).valid);
    ed.model.layer_mut(e, pad, 0).set("Gain", 0.3).set("AmpEnvAttack", 0.4).set("AmpEnvAttack_Slot1", 0.2);
    menu(&mut ed, "Reset Envelopes");
    let n = ed.model.layer_node(e, pad, 0).unwrap();
    assert!(n.attr("AmpEnvAttack").is_none() && n.attr("AmpEnvAttack_Slot1").is_some() && n.attr("Gain").is_some());
    assert!(n.f32("AmpEnvHold").is_some_and(|h| (0.0..=1.0).contains(&h)));
    menu(&mut ed, "Reset");
    let n = ed.model.layer_node(e, pad, 0).unwrap();
    assert!(n.attr("Gain").is_none() && n.attr("AmpEnvAttack_Slot1").is_none() && n.attr("AudioFileName").is_some());
    // Locked rows refuse Cut / Paste / Clear.
    menu(&mut ed, "Copy");
    menu(&mut ed, "Lock");
    assert!(padmenu::layer_locked(&ed.model, e, pad, 0));
    let items = menu(&mut ed, "Lock");
    assert!(items[8].starts_with(dreist_gui::menu::CHECK));
    for i in [0, 2, 4, 5, 6] {
        assert!(items[i].starts_with(dreist_gui::menu::DISABLED), "{}", items[i]);
    }
    menu(&mut ed, "Cut");
    assert!(!ed.model.layer(e, pad, 0).valid);
    menu(&mut ed, "Paste");
    assert!(ed.model.layer(e, pad, 0).valid);
    menu(&mut ed, "Clear");
    assert!(ed.model.layer_node(e, pad, 0).is_some_and(|n| n.attrs.is_empty()));
}

#[test]
fn pad_menu_locks_colours_and_swaps() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    fn find(c: &dreist_gui::Ctrl, idx: &str, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if k.overload == "EditorPad" && k.attr("padIndex") == Some(idx) {
                return true;
            }
            if find(k, idx, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut pad1 = Vec::new();
    assert!(find(&ed.app.root, "1", &mut pad1));
    let menu = |ed: &mut Editor, label: &str| {
        ed.handle(&UiEvent::Mouse { path: pad1.clone(), overload: "EditorPad".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 5.0, y: 5.0 });
        let items = ed.app.menu.as_ref().expect("pad menu").items.clone();
        let i = items.iter().position(|s| s.ends_with(label)).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: pad1.clone(), name: "EditorPad".into(), index: i });
    };
    let e = ed.model.engine;
    menu(&mut ed, "Lock pad");
    assert!(padmenu::pad_locked(&ed.model, e, 1));
    // Locked pads survive kit loads.
    let before = ed.model.pad_node(e, 1).cloned();
    let kit = browser::content_dir().join(dreist_formats::legacy::DEMO_FACTORY);
    let kit = walk_find(&kit, "geistkit");
    if let Some(k) = kit {
        assert!(browser::load_kit_into_doc(&mut ed.model, &k));
        assert_eq!(ed.model.pad_node(e, 1).cloned(), before);
    }
    menu(&mut ed, "Unlock all pads");
    assert!(!padmenu::pad_locked(&ed.model, e, 1));
    menu(&mut ed, "Set colour label... ▸");
    ed.handle(&UiEvent::MenuSelected { anchor: pad1.clone(), name: "EditorPad".into(), index: 8 });
    assert_eq!(ed.model.pad_node(e, 1).and_then(|n| n.attr("PadColour")), Some("0, 128, 255, 255"));
    let (a, b) = (ed.model.pad_name(e, 1), ed.model.pad_name(e, 2));
    menu(&mut ed, "Swap pad to... ▸");
    ed.handle(&UiEvent::MenuSelected { anchor: pad1.clone(), name: "EditorPad".into(), index: 2 });
    assert_eq!((ed.model.pad_name(e, 2), ed.model.pad_name(e, 1)), (a, b));
    assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })));
}

fn walk_find(dir: &std::path::Path, ext: &str) -> Option<std::path::PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = walk_find(&p, ext) {
                return Some(f);
            }
        } else if p.extension().is_some_and(|x| x == ext) {
            return Some(p);
        }
    }
    None
}

#[test]
fn pattern_and_track_menus() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let e = ed.model.engine;
    let sel = ed.app.root.path_of("Selectors").unwrap();
    let key0 = {
        let c = ed.app.ctrl(&sel).unwrap();
        let mut p = sel.clone();
        p.extend(c.path_of("Key0").unwrap());
        p
    };
    let pick = |ed: &mut Editor, anchor: &Vec<usize>, label: &str| {
        let items = ed.app.menu.as_ref().expect("menu").items.clone();
        let i = items.iter().position(|s| s.starts_with(label)).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: anchor.clone(), name: String::new(), index: i });
    };
    ed.handle(&UiEvent::RightClicked { path: key0.clone(), name: "Key0".into() });
    pick(&mut ed, &key0, "Lock pattern");
    assert!(ed.model.pattern_node(e, 0).and_then(|n| n.attr("LockState")).is_some());
    let used0 = ed.model.pattern_used(e, 0);
    let empty = (1..24).find(|&q| !ed.model.pattern_used(e, q)).unwrap();
    ed.handle(&UiEvent::RightClicked { path: key0.clone(), name: "Key0".into() });
    pick(&mut ed, &key0, "Copy to");
    ed.handle(&UiEvent::MenuSelected { anchor: key0.clone(), name: String::new(), index: empty });
    assert_eq!(ed.model.pattern_used(e, empty), used0);
    // Track name area of the first row.
    let rows = ed.app.root.path_of("ControlsScrollView").unwrap();
    let row = ed.app.ctrl(&rows).unwrap();
    let t = row.children[0].attr("track").unwrap().parse::<usize>().unwrap();
    let mut sa = rows.clone();
    sa.push(0);
    sa.push(row.children[0].children.iter().position(|c| c.name == "SelectArea").unwrap());
    // Cut takes only the selected steps (into the clipboard pattern).
    let cur = ed.model.current_pattern(e);
    {
        let sp = ed.model.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{cur}"));
        sp.attrs.retain(|(k, _)| k != "LockState");
        let tr = sp.child_mut_or_insert("Track", &format!("Track{t}"));
        tr.child_mut_or_insert("Step", "S2").set("State", "true");
        tr.child_mut_or_insert("Step", "S5").set("State", "true");
    }
    ed.pattern.as_mut().unwrap().selection = [(t, 2)].into_iter().collect();
    ed.handle(&UiEvent::Mouse { path: sa.clone(), overload: "TrackSelectArea".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 2.0, y: 2.0 });
    pick(&mut ed, &sa, "Cut");
    let steps_of = |n: &dreist_formats::Node| -> Vec<String> {
        n.children.iter().filter(|c| c.name.as_deref() == Some(&format!("Track{t}"))).flat_map(|tr| tr.children.iter().filter_map(|s| s.name.clone())).collect()
    };
    let left = steps_of(ed.model.pattern_node(e, cur).unwrap());
    assert!(left.contains(&"S5".to_string()) && !left.contains(&"S2".to_string()), "{left:?}");
    assert_eq!(steps_of(ed.clipboard.pattern.as_ref().unwrap()), vec!["S2".to_string()]);
    assert!(ed.pattern.as_ref().unwrap().selection.is_empty());
    ed.handle(&UiEvent::Mouse { path: sa.clone(), overload: "TrackSelectArea".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 2.0, y: 2.0 });
    pick(&mut ed, &sa, "Lock track");
    assert!(padmenu::track_node(&ed.model, e, t).and_then(|n| n.attr("LockState")).is_some());
}

#[test]
fn song_resize_start_phase_band_quantize() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 2;
    ed.sync();
    let body = ed.app.root.path_of("SongBody").unwrap();
    let (bx, by) = ed.app.origin(&body);
    let drag = |ed: &mut Editor, x: f32, y: f32, x2: f32, y2: f32, shift: bool| {
        ed.app.mods.shift = shift;
        for ev in ed.app.mouse_move(bx as f32 + x, by as f32 + y) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_move(bx as f32 + x2, by as f32 + y2) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        ed.app.mods.shift = false;
    };
    let song = |ed: &Editor| ed.model.doc.children.iter().find(|c| c.class == "SongSequencer").map(dreist_engine::song::Song::from_node).unwrap();
    let rh = {
        let c = ed.app.ctrl(&body).unwrap();
        ((c.rect.h as f32 - 20.0) / 8.0).floor().max(12.0)
    };
    let mid = 20.0 + rh * 0.3;
    // Insert (one 4 QN pattern = 64 px at zoom 16).
    drag(&mut ed, 5.0, mid, 5.0, mid, false);
    assert_eq!(song(&ed).tracks[0].len(), 1);
    let snap = ed.song.as_ref().unwrap().snap_qn();
    // Right edge: two snap steps longer.
    drag(&mut ed, 63.0, mid, 63.0 + 32.0 * snap as f32, mid, false);
    let p = song(&ed).tracks[0][0].clone();
    assert!((p.length - (4.0 + 2.0 * snap)).abs() < 1e-6, "{p:?}");
    // Left edge: one step later, end fixed, phase follows.
    drag(&mut ed, 1.0, mid, 1.0 + 16.0 * snap as f32, mid, false);
    let p = song(&ed).tracks[0][0].clone();
    assert!((p.start - snap).abs() < 1e-6 && (p.end() - (4.0 + 2.0 * snap)).abs() < 1e-6, "{p:?}");
    assert!((p.phase - snap % 4.0).abs() < 1e-6, "{p:?}");
    // Bottom band: phase drag.
    let x = (p.start as f32 + 1.0) * 16.0;
    let low = 20.0 + rh - 3.0;
    drag(&mut ed, x, low, x + 16.0 * snap as f32, low, false);
    let q = song(&ed).tracks[0][0].clone();
    assert!((q.start - p.start).abs() < 1e-6 && (q.phase - (p.phase - snap).rem_euclid(4.0)).abs() < 1e-6, "{q:?}");
    // Shift band over empty space selects nothing new, over the part selects it.
    drag(&mut ed, 300.0, mid, 2.0, mid, true);
    assert_eq!(ed.song.as_ref().unwrap().selected.len(), 1);
}

#[test]
fn missing_files_auto_find() {
    let dir = std::env::temp_dir().join(format!("dreist-missing-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("found")).unwrap();
    std::fs::write(dir.join("found/hat.wav"), b"").unwrap();
    let mut doc = project::new_document();
    {
        let eng = doc.child_mut_or_insert("Engine", "Engine0");
        let pad = eng.child_mut_or_insert("Pad", "Pad2");
        let l = pad.child_mut_or_insert("Layer", "Layer0");
        l.set("AudioFileName", "/nowhere/at/all/hat.wav");
    }
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.open_document(doc, Some(dir.clone()), None);
    assert!(ed.filefind.open);
    assert_eq!(ed.filefind.items.len(), 1);
    let d2 = dir.clone();
    ed.dialogs = Some(Box::new(move |_| Some(d2.clone())));
    let auto = ed.app.root.path_of("AutoButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: auto, name: "AutoButton".into(), id: 0, on: true });
    // The job runs on a worker; the scanning panel shows meanwhile.
    assert!(ed.filefind.scanning());
    for _ in 0..200 {
        ed.animate();
        if !ed.filefind.scanning() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!ed.filefind.open);
    assert_eq!(ed.model.layer_node(0, 2, 0).and_then(|n| n.attr("AudioFileName")), Some(dir.join("found").join("hat.wav").to_string_lossy().as_ref()));
    assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Rebuild { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn convert_timing_through_messages() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let mut m = Model::new(proj.raw);
    let e = m.engine;
    let cur = m.current_pattern(e);
    let want = m.pattern(e, cur).convert_timing(8, dreist_engine::pattern::resolution_index("1/8"));
    let mut out = Vec::new();
    convertor::convert(&m, cur, 8, dreist_engine::pattern::resolution_index("1/8"), &mut out);
    for c in &out {
        m.apply(c);
    }
    let got = m.pattern(e, cur);
    assert_eq!(got.steps, 8);
    for t in 0..16 {
        for s in 0..8 {
            assert_eq!(got.tracks[t].steps[s].on, want.tracks[t].steps[s].on, "t{t} s{s}");
        }
    }
    assert!(want.tracks.iter().any(|t| t.steps.iter().any(|s| s.on)));
}

#[test]
fn pad_options_velocity_splits() {
    use dreist_gui::app::{MouseKind, UiEvent};
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let e = ed.model.engine;
    let pad = ed.model.pad[e];
    let at = |ed: &Editor, name: &str| ed.app.root.path_of(name).unwrap();
    let show = at(&ed, "ShowSplits");
    ed.handle(&UiEvent::Clicked { path: show, name: "ShowSplits".into(), id: 0, on: true });
    assert!(ed.app.ctrl(&at(&ed, "VelocitySplitPanel")).unwrap().visible);
    // It slides over the layer panel, which hides at the end.
    assert!(ed.app.ctrl(&at(&ed, "LayerPanel")).unwrap().visible);
    assert!(!ed.app.ctrl(&at(&ed, "ShowSplits")).unwrap().enabled);
    std::thread::sleep(std::time::Duration::from_millis(380));
    ed.animate();
    assert!(!ed.app.ctrl(&at(&ed, "LayerPanel")).unwrap().visible);
    assert!(ed.app.ctrl(&at(&ed, "ShowSplits")).unwrap().enabled);
    assert_eq!(ed.app.ctrl(&at(&ed, "VelocitySplitPanel")).unwrap().rect.x, ed.app.ctrl(&at(&ed, "LayerPanel")).unwrap().rect.x);
    // Layer play mode.
    let combo = at(&ed, "SelModeCombo");
    ed.handle(&UiEvent::Clicked { path: combo.clone(), name: "SelModeCombo".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: combo.clone(), name: "SelModeCombo".into(), index: 1 });
    assert_eq!(ed.model.pad_node(e, pad).unwrap().attr("PadMode"), Some("Velocity split"));
    assert_eq!(ed.app.ctrl(&combo).unwrap().text, "Velocity split");
    // Auto split.
    let valid: Vec<bool> = (0..model::LAYERS).map(|l| ed.model.layer(e, pad, l).valid).collect();
    let ins = at(&ed, "Insert");
    assert!(ed.app.ctrl(&ins).unwrap().enabled);
    ed.handle(&UiEvent::Clicked { path: ins.clone(), name: "Insert".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: ins.clone(), name: "Insert".into(), index: 0 });
    let mut want = [dreist_engine::velsplit::DEFAULT; model::LAYERS];
    dreist_engine::velsplit::auto_split(&valid, &mut want);
    let split = |ed: &Editor, l: usize| dreist_engine::engine::doc::layer_split(ed.model.layer_node(e, pad, l).unwrap());
    let first = valid.iter().position(|v| *v).unwrap();
    assert_eq!(split(&ed, first), want[first]);
    assert_eq!(ed.history.undo.back().unwrap().0, format!("Engine {} Pad {} auto velocity split", e + 1, pad + 1));
    // Drag the highest handle of the first layer left by 20 velocities
    // (the high turnover follows).
    let row = at(&ed, &format!("VSplit{first}"));
    let c = ed.app.ctrl(&row).unwrap();
    assert!(c.enabled);
    let (w, h) = (c.rect.w as f32, c.rect.h as f32);
    let px = velsplit::to_px(want[first], w);
    let mx = px[3] + 4.0;
    let dx = (20.0 * (w - 8.0) / 127.0).round();
    let mouse = |kind, x: f32| UiEvent::Mouse { path: row.clone(), overload: "VelocitySplitEditor".into(), kind, x, y: h - 4.0 };
    ed.handle(&mouse(MouseKind::Down, mx));
    ed.handle(&mouse(MouseKind::Drag, mx - dx));
    ed.handle(&mouse(MouseKind::Up, mx - dx));
    let got = split(&ed, first);
    assert!((want[first][3] as i32 - got[3] as i32 - 20).abs() <= 1, "{got:?} vs {:?}", want[first]);
    assert_eq!(want[first][3] - got[3], want[first][2] - got[2]);
    assert_eq!(&got[..2], &want[first][..2]);
    // Pad linking.
    let link = at(&ed, "LinkButton6");
    ed.handle(&UiEvent::Clicked { path: link.clone(), name: "LinkButton6".into(), id: 0, on: true });
    assert_eq!(ed.model.pad_node(e, pad).unwrap().bool("Link5"), Some(true));
    assert!(ed.app.ctrl(&link).unwrap().on);
    // Back to all layers: editors and Auto disable.
    ed.handle(&UiEvent::Clicked { path: combo.clone(), name: "SelModeCombo".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: combo, name: "SelModeCombo".into(), index: 0 });
    assert!(!ed.app.ctrl(&ins).unwrap().enabled);
    assert!(!ed.app.ctrl(&row).unwrap().enabled);
}

#[test]
fn browser_search_box_menu_and_advanced_search() {
    use dreist_gui::app::UiEvent;
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let Some(page) = ed.browser.as_ref().map(|b| b.page().to_vec()) else { return };
    let find = |ed: &Editor, name: &str| (ed.app.ctrl(&page).unwrap()).path_of(name).map(|p| [page.clone(), p].concat()).unwrap();
    let sb = find(&ed, "SearchBox");
    let nav_y = ed.app.ctrl(&find(&ed, "Navigator")).unwrap().rect.y;
    // Search box menu → Advanced search... opens the pane.
    ed.handle(&UiEvent::Pressed { path: sb.clone(), name: "SearchBox".into() });
    ed.handle(&UiEvent::MenuSelected { anchor: sb.clone(), name: "SearchBox".into(), index: 2 });
    let search = find(&ed, "Search");
    assert!(ed.app.ctrl(&search).unwrap().visible);
    // 750 ms animation block.
    std::thread::sleep(std::time::Duration::from_millis(770));
    ed.animate();
    assert_eq!(ed.app.ctrl(&find(&ed, "Navigator")).unwrap().rect.y, nav_y + search::PANE_HEIGHT);
    assert_eq!(ed.app.ctrl(&search).unwrap().rect.h, search::PANE_HEIGHT);
    // Sub folders on.
    let was = ed.browser.as_ref().unwrap().pane_sub_folders();
    ed.handle(&UiEvent::Pressed { path: sb.clone(), name: "SearchBox".into() });
    ed.handle(&UiEvent::MenuSelected { anchor: sb.clone(), name: "SearchBox".into(), index: 0 });
    assert_ne!(ed.browser.as_ref().unwrap().pane_sub_folders(), was);
    if !ed.browser.as_ref().unwrap().pane_sub_folders() {
        ed.handle(&UiEvent::Pressed { path: sb.clone(), name: "SearchBox".into() });
        ed.handle(&UiEvent::MenuSelected { anchor: sb.clone(), name: "SearchBox".into(), index: 0 });
    }
    // Option 1: File name contains "hollow".
    let opt1 = find(&ed, "Option1");
    let term = (ed.app.ctrl(&opt1).unwrap()).path_of("SearchTermBox").map(|p| [opt1.clone(), p].concat()).unwrap();
    ed.handle(&UiEvent::TextCommitted { path: term, name: "SearchTermBox".into(), text: "hollow".into() });
    // Option 2: File type Folder, matched with "any".
    let opt2 = find(&ed, "Option2");
    let ty = (ed.app.ctrl(&opt2).unwrap()).path_of("SearchType").map(|p| [opt2.clone(), p].concat()).unwrap();
    ed.handle(&UiEvent::Clicked { path: ty.clone(), name: "SearchType".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: ty.clone(), name: "SearchType".into(), index: 3 });
    let sub = (ed.app.ctrl(&opt2).unwrap()).path_of("SubSearchType").map(|p| [opt2.clone(), p].concat()).unwrap();
    ed.handle(&UiEvent::Clicked { path: sub.clone(), name: "SubSearchType".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: sub.clone(), name: "SubSearchType".into(), index: 3 });
    assert_eq!(ed.app.ctrl(&sub).unwrap().text, "Folder");
    let go = find(&ed, "DoSearchButton");
    // Search from the content root.
    ed.browser.as_mut().unwrap().browse(browser::content_dir());
    ed.handle(&UiEvent::Clicked { path: go.clone(), name: "DoSearchButton".into(), id: 0, on: false });
    let hits = ed.browser.as_ref().unwrap().entry_paths();
    assert!(hits.iter().any(|p| p.file_name().is_some_and(|n| n == "Hollow.geist")));
    assert!(hits.iter().any(|p| p.is_dir()));
    // Match all: only folders named like "hollow".
    let all = find(&ed, "MatchAllButton");
    ed.handle(&UiEvent::Clicked { path: all, name: "MatchAllButton".into(), id: 0, on: true });
    ed.handle(&UiEvent::Clicked { path: go, name: "DoSearchButton".into(), id: 0, on: false });
    let hits = ed.browser.as_ref().unwrap().entry_paths();
    assert!(hits.iter().all(|p| p.is_dir()));
    // Recent items from a quick search.
    ed.handle(&UiEvent::TextCommitted { path: sb.clone(), name: "SearchBox".into(), text: "Blister".into() });
    assert!(ed.browser.as_ref().unwrap().entry_paths().iter().any(|p| p.file_name().is_some_and(|n| n == "Blister.geist")));
    ed.handle(&UiEvent::Pressed { path: sb.clone(), name: "SearchBox".into() });
    ed.handle(&UiEvent::MenuSelected { anchor: sb.clone(), name: "SearchBox".into(), index: 4 });
    ed.handle(&UiEvent::MenuSelected { anchor: sb.clone(), name: "SearchBox".into(), index: 0 });
    assert!(ed.browser.as_ref().unwrap().entry_paths().iter().any(|p| p.file_name().is_some_and(|n| n == "Blister.geist")));
    // Leave the saved option as it was.
    if !was {
        ed.handle(&UiEvent::Pressed { path: sb.clone(), name: "SearchBox".into() });
        ed.handle(&UiEvent::MenuSelected { anchor: sb, name: "SearchBox".into(), index: 0 });
    }
}

/// A graph stroke (press, drags across steps, release) is one undo
/// step, and undoing it restores every step.
#[test]
fn graph_stroke_is_one_undo_step() {
    use dreist_gui::app::MouseKind;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    ed.sync();
    let gp = ed.app.root.path_of("PatternGraph").unwrap();
    let h = ed.app.ctrl(&gp).unwrap().rect.h as f32;
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let sx = |st: usize| (st - lay.first_step) as f32 * lay.step_w + lay.step_w / 2.0;
    let ev = |kind, st: usize, y: f32| UiEvent::Mouse { path: gp.clone(), overload: "PatternGraph".into(), kind, x: sx(st), y };
    let f = lay.first_step;
    let before = ed.model.doc.clone();
    let steps = ed.history.undo.len();
    ed.handle(&ev(MouseKind::Down, f, h * 0.9));
    for k in 1..6 {
        ed.handle(&ev(MouseKind::Drag, f + k, h * 0.1));
    }
    ed.handle(&ev(MouseKind::Up, f + 5, h * 0.1));
    assert_ne!(ed.model.doc, before, "the stroke edited the pattern");
    assert_eq!(ed.history.undo.len(), steps + 1);
    let undo = ed.app.root.path_of("UndoButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: undo, name: "UndoButton".into(), id: 0, on: false });
    assert!(ed.model.doc == before, "one undo restores the whole stroke");
}

#[test]
fn graph_line_fine_erase_and_menu_from_step() {
    use dreist_gui::app::MouseKind;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    if let Some(pp) = ed.pattern.as_mut() {
        pp.multi = false;
    }
    ed.sync();
    let gp = ed.app.root.path_of("PatternGraph").unwrap();
    let h = ed.app.ctrl(&gp).unwrap().rect.h as f32;
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let sx = |st: usize| (st - lay.first_step) as f32 * lay.step_w + lay.step_w / 2.0;
    let ev = |kind, st: usize, y: f32| UiEvent::Mouse { path: gp.clone(), overload: "PatternGraph".into(), kind, x: sx(st), y };
    let e = ed.model.engine;
    let vel = |ed: &Editor, st: usize| {
        let pat = ed.model.pattern(e, ed.model.current_pattern(e));
        pat.tracks[ed.model.pad[e]].steps[st].lanes[0]
    };
    let f = lay.first_step;
    // Option-drag: a line from 1.0 at the first step to 0.0 four steps on.
    ed.app.mods.alt = true;
    ed.handle(&ev(MouseKind::Down, f, 0.0));
    ed.handle(&ev(MouseKind::Drag, f + 4, h));
    ed.handle(&ev(MouseKind::Up, f + 4, h));
    ed.app.mods.alt = false;
    assert!((vel(&ed, f) - 1.0).abs() < 0.02 && (vel(&ed, f + 2) - 0.5).abs() < 0.02 && vel(&ed, f + 4) < 0.02, "line");
    // Shift: fine adjustment, 0.001 per pixel.
    ed.handle(&ev(MouseKind::Down, f + 6, h * 0.5));
    ed.app.mods.shift = true;
    ed.handle(&ev(MouseKind::Drag, f + 6, h * 0.5 - 100.0));
    ed.app.mods.shift = false;
    ed.handle(&ev(MouseKind::Up, f + 6, h * 0.5 - 100.0));
    assert!((vel(&ed, f + 6) - 0.6).abs() < 0.02, "fine {}", vel(&ed, f + 6));
    // Right-drag erases to the default.
    let def = dreist_formats::params::step_lane_default(0);
    ed.handle(&ev(MouseKind::RightDown, f, h));
    ed.handle(&ev(MouseKind::Drag, f + 2, h));
    ed.handle(&ev(MouseKind::Up, f + 2, h));
    assert!((vel(&ed, f + 1) - def).abs() < 1e-4);
    // Option-right: the menu, Insert Ramp Up from that step.
    ed.app.mods.alt = true;
    ed.handle(&ev(MouseKind::RightDown, f + 8, h));
    ed.app.mods.alt = false;
    ed.handle(&UiEvent::MenuSelected { anchor: gp.clone(), name: "PatternGraph".into(), index: 12 });
    ed.handle(&UiEvent::MenuSelected { anchor: gp.clone(), name: "PatternGraph".into(), index: 0 });
    assert!(vel(&ed, f + 8) < 0.01, "ramp starts at the clicked step");
    assert!((vel(&ed, f + 6) - 0.6).abs() < 0.02, "steps before it untouched");
}

#[test]
fn graph_edit_on_a_selected_step_offsets_the_selection() {
    use dreist_gui::app::MouseKind;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    if let Some(pp) = ed.pattern.as_mut() {
        pp.multi = false;
    }
    ed.sync();
    let gp = ed.app.root.path_of("PatternGraph").unwrap();
    let h = ed.app.ctrl(&gp).unwrap().rect.h as f32;
    let lay = ed.pattern.as_ref().unwrap().layout(&ed.app, &ed.model).unwrap();
    let sx = |st: usize| (st - lay.first_step) as f32 * lay.step_w + lay.step_w / 2.0;
    let e = ed.model.engine;
    let t = ed.model.pad[e];
    let other = (t + 1) % 16;
    let f = lay.first_step;
    let vel = |ed: &Editor, tr: usize, st: usize| ed.model.pattern(e, ed.model.current_pattern(e)).tracks[tr].steps[st].lanes[0];
    let before = [vel(&ed, t, f), vel(&ed, t, f + 1), vel(&ed, t, f + 2), vel(&ed, other, f)];
    ed.pattern.as_mut().unwrap().selection = [(t, f), (t, f + 2), (other, f)].into_iter().collect();
    ed.handle(&UiEvent::Mouse { path: gp.clone(), overload: "PatternGraph".into(), kind: MouseKind::Down, x: sx(f), y: h * 0.75 });
    ed.handle(&UiEvent::Mouse { path: gp, overload: "PatternGraph".into(), kind: MouseKind::Up, x: sx(f), y: h * 0.75 });
    let d = 0.25 - before[0];
    assert!((vel(&ed, t, f) - 0.25).abs() < 0.02);
    assert!((vel(&ed, t, f + 2) - (before[2] + d).clamp(0.0, 1.0)).abs() < 0.02, "selected step moved by the delta");
    assert!((vel(&ed, other, f) - (before[3] + d).clamp(0.0, 1.0)).abs() < 0.02, "selection on another track too");
    assert_eq!(vel(&ed, t, f + 1), before[1], "unselected step untouched");
}

#[test]
fn midi_learn_of_a_graph_lane() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    if let Some(pp) = ed.pattern.as_mut() {
        pp.multi = false;
    }
    ed.sync();
    ed.learn.active = true;
    let ac = ed.app.root.path_of("SingleAutoControl").unwrap();
    let knob = [ac.clone(), (ed.app.ctrl(&ac).unwrap()).path_of("ScaleKnob").unwrap()].concat();
    ed.handle(&UiEvent::Pressed { path: knob, name: "ScaleKnob".into() });
    let t = ed.learn.target.clone().expect("target");
    assert_eq!(t.proto.kind, dreist_engine::binding::id::SET_INSTO_MIDI_GRAPH_VALUE);
    let e = ed.model.engine;
    assert_eq!(t.proto.index[4], ed.model.pad[e] as i32);
    assert_eq!(t.proto.value[1], ed.model.pattern_view[e].lane as f32);
    assert!(t.group.starts_with(&format!("Engine {} | Pattern", e + 1)), "{}", t.group);
    ed.midi_cc(1, 0, 20);
    let rows = crate::midimap::rows(&ed.model.doc);
    assert!(rows.iter().any(|r| r.locale() == "MIDI Ch 1 CC 20"), "{rows:?}");
}

#[test]
fn fx_chain_preset_menu_loads_and_steps() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    fn all(c: &Ctrl, name: &str, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.name == name && c.visible {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            all(k, name, path, out);
            path.pop();
        }
    }
    let mut combos = Vec::new();
    all(&ed.app.root, "PresetCombo", &mut Vec::new(), &mut combos);
    let pc = combos.first().cloned().expect("chain PresetCombo on the global mixer");
    let res = dreist_formats::paths::skin_dir();
    if fxchain::chain_presets(&res).is_empty() {
        return;
    }
    let chain_of = |ed: &Editor| {
        fn find(n: &dreist_formats::Node) -> Option<dreist_formats::Node> {
            if n.class == "EffectChain" && fxchain::chain_name(n).is_some() {
                return Some(n.clone());
            }
            n.children.iter().find_map(find)
        }
        find(&ed.model.doc)
    };
    // Next with no current preset loads the first one.
    let mut nexts = Vec::new();
    all(&ed.app.root, "PresetNext", &mut Vec::new(), &mut nexts);
    ed.handle(&UiEvent::Clicked { path: nexts[0].clone(), name: "PresetNext".into(), id: 0, on: false });
    let first = chain_of(&ed).and_then(|c| fxchain::chain_name(&c)).expect("a preset loaded");
    ed.handle(&UiEvent::Clicked { path: nexts[0].clone(), name: "PresetNext".into(), id: 0, on: false });
    let second = chain_of(&ed).and_then(|c| fxchain::chain_name(&c)).unwrap();
    assert_ne!(first, second);
    // Clear chain from the menu.
    ed.handle(&UiEvent::Clicked { path: pc.clone(), name: "PresetCombo".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: pc.clone(), name: "PresetCombo".into(), index: 1 });
    assert!(chain_of(&ed).is_none(), "cleared");
    // The chain header's Active button: 0x60, and prev / next disable.
    assert!(ed.app.ctrl(&nexts[0]).unwrap().enabled);
    let host = &nexts[0][..nexts[0].len() - 1];
    let active = (0..ed.app.ctrl(host).unwrap().children.len()).map(|i| [host, &[i]].concat()).find(|p| ed.app.ctrl(p).unwrap().name == "ActiveButton").expect("chain ActiveButton beside PresetNext");
    ed.handle(&UiEvent::Clicked { path: active.clone(), name: "ActiveButton".into(), id: 0, on: false });
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_FX_CHAIN_ACTIVE && m.value[0] == 0.0)), "{cmds:?}");
    ed.sync();
    assert!(!ed.app.ctrl(&nexts[0]).unwrap().enabled, "prev / next follow the chain's active state");
    assert!(!ed.app.ctrl(&active).unwrap().on);
}

#[test]
fn slicer_slides_over_the_main_panel() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let f = dreist_formats::legacy::demo("Hits/Cymbals/CY Cym Ar82 01 C2A St GB.wav");
    if !ed.open_slicer(&f) {
        return;
    }
    let ov = ed.app.root.path_of("SlicerOverlay").unwrap();
    let main = ed.app.root.path_of("MainPanel").unwrap();
    assert_eq!(ed.app.ctrl(&ov).unwrap().rect.w, ed.app.ctrl(&main).unwrap().rect.w);
    std::thread::sleep(std::time::Duration::from_millis(370));
    ed.animate();
    let panel = [ov.clone(), vec![0]].concat();
    assert_eq!(ed.app.ctrl(&panel).unwrap().rect.y, 0);
    if let Ok(f) = std::env::var("DREIST_SHOT") {
        ed.render(1.0).save_png(f).unwrap();
    }
}

#[test]
fn modulated_knob_centre_edits_the_base_and_ring_the_depth() {
    use dreist_gui::app::MouseButton;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.source = 5;
    ed.sync();
    let Some(k) = ed.app.root.path_of("VoicePan") else { return };
    assert_eq!(ed.app.ctrl(&k).unwrap().attr("editingDepth"), Some("true"));
    let (ox, oy) = ed.app.origin(&k);
    let r = ed.app.ctrl(&k).unwrap().rect;
    let drag = |ed: &mut Editor, x: f32, y: f32| {
        ed.take_commands();
        for e in ed.app.mouse_move(x, y) {
            ed.handle(&e);
        }
        for e in ed.app.mouse_down(MouseButton::Left) {
            ed.handle(&e);
        }
        for e in ed.app.mouse_move(x, y - 40.0) {
            ed.handle(&e);
        }
        for e in ed.app.mouse_up(MouseButton::Left) {
            ed.handle(&e);
        }
        ed.take_commands().into_iter().filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).rfind(|m| m.kind == dreist_engine::binding::id::SET_LAYER_PARAMETER)
    };
    // Centre: the base (source 0).
    let m = drag(&mut ed, (ox + r.w / 2) as f32, (oy + r.h / 2) as f32).expect("base edit");
    assert_eq!(m.value[2], 0.0);
    // Corner (outside the centre circle): the depth for source 5.
    let m = drag(&mut ed, ox as f32 + 1.0, oy as f32 + 1.0).expect("depth edit");
    assert_eq!(m.value[2], 5.0);
}

#[test]
fn modulator_retrigger_flashes_its_source_button() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(f) = ed.app.root.path_of("FlashSrc5") else { return };
    assert_eq!(ed.app.ctrl(&f).unwrap().attr("alpha"), Some("0"));
    ed.modulators_triggered(1 << 5);
    ed.animate();
    let a: f32 = ed.app.ctrl(&f).unwrap().attr("alpha").unwrap().parse().unwrap();
    assert!(a > 0.9, "{a}");
    std::thread::sleep(std::time::Duration::from_millis(520));
    ed.animate();
    assert_eq!(ed.app.ctrl(&f).unwrap().attr_f32("alpha", 1.0), 0.0);
}

#[test]
fn dialogs_slide_in_from_the_top_and_out() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(ov) = ed.app.root.path_of("PreferencesOverlay") else { return };
    let dialog = [ov.clone(), vec![0]].concat();
    ed.show_modal("preferences");
    assert!(ed.app.ctrl(&ov).unwrap().visible);
    assert!(ed.app.ctrl(&dialog).unwrap().rect.y < 0, "starts above the window");
    std::thread::sleep(std::time::Duration::from_millis(520));
    ed.animate();
    let r = ed.app.ctrl(&dialog).unwrap().rect;
    assert_eq!(r.y, 0);
    assert_eq!(r.x, ed.app.ctrl(&ov).unwrap().rect.w / 2 - r.w / 2);
    let cancel = (ed.app.ctrl(&ov).unwrap()).path_of("CancelButton").map(|p| [ov.clone(), p].concat()).expect("cancel");
    ed.handle(&UiEvent::Clicked { path: cancel, name: "CancelButton".into(), id: 0, on: false });
    assert!(ed.app.ctrl(&ov).unwrap().visible, "visible while sliding out");
    std::thread::sleep(std::time::Duration::from_millis(520));
    ed.animate();
    assert!(!ed.app.ctrl(&ov).unwrap().visible);
}

#[test]
fn marker_buttons_only_in_learn_mode() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.sync();
    let Some(ps) = ed.app.root.path_of("PS") else { return };
    assert!(!ed.app.ctrl(&ps).unwrap().visible, "hidden outside learn");
    ed.set_learn(true);
    assert!(ed.app.ctrl(&ps).unwrap().visible);
    ed.handle(&UiEvent::Pressed { path: ps.clone(), name: "PS".into() });
    let t = ed.learn.target.clone().expect("learn target");
    assert_eq!(t.proto.kind, dreist_engine::binding::id::SET_LAYER_START_SAMPLE01);
    assert_eq!(t.name, "Play Start");
    assert!(t.group.starts_with("Engine 1 | Pad 1 | Layer 1"), "{}", t.group);
    ed.set_learn(false);
    assert!(!ed.app.ctrl(&ps).unwrap().visible);
}

#[test]
fn effect_panels_fold_on_header_double_click() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 5;
    ed.sync();
    let res = dreist_formats::paths::skin_dir();
    if fxchain::chain_presets(&res).is_empty() {
        return;
    }
    fn shown(c: &Ctrl, name: &str, path: &mut Vec<usize>) -> Option<Vec<usize>> {
        if !c.visible {
            return None;
        }
        if c.name == name {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = shown(k, name, path) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let Some(next) = shown(&ed.app.root, "PresetNext", &mut Vec::new()) else { return };
    ed.handle(&UiEvent::Clicked { path: next.clone(), name: "PresetNext".into(), id: 0, on: false });
    ed.sync();
    fn panels(c: &Ctrl, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.attr("fxSlot").is_some() {
            out.push(path.clone());
            return;
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            panels(k, path, out);
            path.pop();
        }
    }
    let mut ps = Vec::new();
    panels(&ed.app.root, &mut Vec::new(), &mut ps);
    // The chain on the shown page.
    let visible = |ed: &Editor, p: &[usize]| (1..=p.len()).all(|n| ed.app.ctrl(&p[..n]).is_some_and(|c| c.visible));
    ps.retain(|p| visible(&ed, p));
    assert!(ps.len() >= 2, "a chain preset with two effects");
    let (p0, p1) = (ps[0].clone(), ps[1].clone());
    let w0 = ed.app.ctrl(&p0).unwrap().rect.w;
    let x1 = ed.app.ctrl(&p1).unwrap().rect.x;
    let active = |ed: &Editor| ed.app.ctrl(&p0).unwrap().find("ActiveButton").map(|c| c.visible);
    assert_eq!(active(&ed), Some(true));
    // Double click in the header (clear of its buttons).
    let (ox, oy) = ed.app.origin(&p0);
    ed.app.mouse_move(ox as f32 + 100.0, oy as f32 + 5.0);
    for _ in 0..2 {
        for e in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&e);
        }
        ed.app.mouse_up(dreist_gui::app::MouseButton::Left);
    }
    // Collapse start hides the controls at once.
    assert_eq!(active(&ed), Some(false));
    std::thread::sleep(std::time::Duration::from_millis(400));
    ed.animate();
    let c = ed.app.ctrl(&p0).unwrap();
    assert_eq!(c.rect.w, 20);
    assert!(dreist_gui::collapse::is_iconic(c));
    assert_eq!(ed.app.ctrl(&p1).unwrap().rect.x, x1 - w0 + 20, "siblings re-stack");
    for e in ed.app.fold(&p0, false) {
        ed.handle(&e);
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    ed.animate();
    assert_eq!(ed.app.ctrl(&p0).unwrap().rect.w, w0);
    assert_eq!(ed.app.ctrl(&p1).unwrap().rect.x, x1);
    assert_eq!(active(&ed), Some(true), "expand end shows them");
}

#[test]
fn scene_panel_pattern_selectors() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    let Some(sc) = ed.app.root.path_of("ScenePanel").or_else(|| ed.app.root.path_of("SnapshotToNextButton").map(|p| p[..p.len() - 1].to_vec())) else { return };
    // The Selectors under the scene page (not the pattern panel's).
    fn all(c: &Ctrl, name: &str, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if c.name == name {
            out.push(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            all(k, name, path, out);
            path.pop();
        }
    }
    let mut sels = Vec::new();
    all(&ed.app.root, "Selectors", &mut Vec::new(), &mut sels);
    let _ = sc;
    let scene_sel = sels
        .iter()
        .find(|s| {
            ed.app.ctrl(s).is_some_and(|c| c.find("Key5").is_some())
                && (1..s.len()).any(|n| ed.app.ctrl(&s[..n]).is_some_and(|c| c.find("SnapshotToNextButton").is_some() && c.find("GraphCombo").is_none()))
        })
        .cloned();
    let ss = scene_sel.expect("scene panel selectors");
    let key = [ss.clone(), (ed.app.ctrl(&ss).unwrap()).path_of("Key5").unwrap()].concat();
    ed.handle(&UiEvent::Clicked { path: key.clone(), name: "Key5".into(), id: 0, on: true });
    let e = ed.model.engine;
    assert_eq!(ed.model.current_pattern(e), 5);
    assert!(ed.app.ctrl(&key).unwrap().on);
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SELECT_PATTERN && m.index[3] == 5)));
    if let Ok(out) = std::env::var("DREIST_SELECTOR_SHOT") {
        ed.sync();
        ed.render(1.0).save_png(out).unwrap();
    }
}

#[test]
fn song_pattern_buttons_change_the_selection() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.model.editor_tab = 2;
    ed.sync();
    let body = ed.app.root.path_of("SongBody").unwrap();
    let (bx, by) = ed.app.origin(&body);
    let click = |ed: &mut Editor, x: f32, y: f32| {
        for ev in ed.app.mouse_move(x, y) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_down(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
        for ev in ed.app.mouse_up(dreist_gui::app::MouseButton::Left) {
            ed.handle(&ev);
        }
    };
    let song = |ed: &Editor| ed.model.doc.children.iter().find(|c| c.class == "SongSequencer").map(dreist_engine::song::Song::from_node).unwrap();
    let next = ed.app.root.path_of("NextPatternButton").unwrap();
    assert!(!ed.app.ctrl(&next).unwrap().enabled, "disabled without a selection");
    // Insert a part (selected), then step its pattern.
    click(&mut ed, bx as f32 + 5.0, by as f32 + 30.0);
    assert!(ed.app.ctrl(&next).unwrap().enabled);
    ed.handle(&UiEvent::Clicked { path: next.clone(), name: "NextPatternButton".into(), id: 0, on: false });
    assert_eq!(song(&ed).tracks[0][0].pattern, 1);
    let prev = ed.app.root.path_of("PrevPatternButton").unwrap();
    for _ in 0..3 {
        ed.handle(&UiEvent::Clicked { path: prev.clone(), name: "PrevPatternButton".into(), id: 0, on: false });
    }
    assert_eq!(song(&ed).tracks[0][0].pattern, 0, "clamped at 0");
    let sel = ed.app.root.path_of("SelectPatternButton").unwrap();
    ed.handle(&UiEvent::Clicked { path: sel.clone(), name: "SelectPatternButton".into(), id: 0, on: false });
    ed.handle(&UiEvent::MenuSelected { anchor: sel, name: "SelectPatternButton".into(), index: 5 });
    ed.app.menu = None;
    assert_eq!(song(&ed).tracks[0][0].pattern, 5);
    // Click the part: its part buttons appear inside it.
    click(&mut ed, bx as f32 + 5.0, by as f32 + 30.0);
    ed.sync();
    let pn = ed.app.root.path_of("PartNextPatternButton").unwrap();
    assert!(ed.app.ctrl(&pn).unwrap().visible);
    ed.handle(&UiEvent::Clicked { path: pn, name: "PartNextPatternButton".into(), id: 0, on: false });
    assert_eq!(song(&ed).tracks[0][0].pattern, 6);
    // Erase: press / release post e_songSetEraseState.
    // The song panel's EraseButton (the pads panel has one too).
    let dup = ed.app.root.path_of("DuplicateButton").unwrap();
    let er = (1..dup.len()).rev().find_map(|n| (ed.app.ctrl(&dup[..n]).unwrap()).path_of("EraseButton").map(|p| [dup[..n].to_vec(), p].concat())).unwrap();
    let _ = ed.take_commands();
    ed.handle(&UiEvent::Pressed { path: er.clone(), name: "EraseButton".into() });
    ed.handle(&UiEvent::Clicked { path: er, name: "EraseButton".into(), id: 0, on: false });
    let cmds = ed.take_commands();
    let erase: Vec<f32> = cmds.iter().filter_map(|c| if let Command::Message(m) = c { (m.kind == dreist_engine::binding::id::SONG_SET_ERASE_STATE).then_some(m.value[0]) } else { None }).collect();
    assert_eq!(erase, vec![1.0, 0.0]);
}

#[test]
fn drag_and_drop_pads_patterns_and_browser_files() {
    use dreist_gui::app::{Mods, MouseKind};
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(proj.raw)) else { return };
    fn find(c: &dreist_gui::Ctrl, f: &dyn Fn(&dreist_gui::Ctrl) -> bool, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if f(k) || find(k, f, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let by = |ed: &Editor, f: &dyn Fn(&dreist_gui::Ctrl) -> bool| {
        let mut v = Vec::new();
        assert!(find(&ed.app.root, f, &mut v));
        v
    };
    let pad = |ed: &Editor, i: usize| by(ed, &|k| k.overload == "EditorPad" && k.attr("padIndex") == Some(&i.to_string()));
    let e = ed.model.engine;
    let sample = |ed: &Editor, p: usize, l: usize| ed.model.layer_node(e, p, l).and_then(|n| n.attr("AudioFileName")).map(std::path::PathBuf::from);
    let name = |ed: &Editor, p: usize| ed.model.pad_node(e, p).map(|n| n.to_xml());
    let (n1, n2) = (name(&ed, 1), name(&ed, 2));
    // Plain drop swaps; the target becomes the selected pad.
    let (a, b) = (pad(&ed, 1), pad(&ed, 2));
    ed.handle(&UiEvent::Dropped { from: a.clone(), to: b.clone(), x: 0.0, y: 0.0 });
    assert_eq!(ed.model.pad[e], 2);
    assert_eq!(name(&ed, 1).map(|s| s.replace("Pad2", "Pad1")), n2.clone().map(|s| s.replace("Pad2", "Pad1")));
    assert_eq!(name(&ed, 2).map(|s| s.replace("Pad1", "Pad2")), n1.clone().map(|s| s.replace("Pad1", "Pad2")));
    // Alt copies 2 onto 3.
    ed.app.mods = Mods { alt: true, ..Mods::default() };
    let c = pad(&ed, 3);
    ed.handle(&UiEvent::Dropped { from: b.clone(), to: c.clone(), x: 0.0, y: 0.0 });
    assert_eq!(sample(&ed, 3, 0), sample(&ed, 2, 0));
    assert!(ed.history.undo.back().is_some_and(|u| u.0.starts_with("Copying pad 3 to pad 4")), "{:?}", ed.history.undo.back().map(|u| &u.0));
    ed.app.mods = Mods::default();
    // Pattern keys: swap.
    let key = |ed: &Editor, n: usize| by(ed, &|k| k.overload == "PatternSelector" && k.name == format!("Key{n}"));
    let pat = |ed: &Editor, n: usize| ed.model.pattern_node(e, n).map_or(0, |p| p.children.len());
    let (p0, p5) = (pat(&ed, 0), pat(&ed, 5));
    ed.handle(&UiEvent::Dropped { from: key(&ed, 0), to: key(&ed, 5), x: 0.0, y: 0.0 });
    assert_eq!((pat(&ed, 0), pat(&ed, 5)), (p5, p0));
    assert_eq!(ed.model.current_pattern(e), 5);
    // Browser: cmd-click two audio files, drop on pad 6 → pads 6 and 7.
    let dir = std::env::temp_dir().join(format!("dreist-dnd-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for f in ["a.wav", "b.wav", "c.wav"] {
        let _ = std::fs::write(dir.join(f), b"");
    }
    ed.browser.as_mut().unwrap().browse(dir.clone());
    let list = by(&ed, &|k| k.overload == "BrowserFileList");
    let down = |ed: &mut Editor, row: f32| ed.handle(&UiEvent::Mouse { path: list.clone(), overload: "BrowserFileList".into(), kind: MouseKind::Down, x: 5.0, y: row * 18.0 + 2.0 });
    down(&mut ed, 0.0);
    ed.app.mods = Mods { cmd: true, ..Mods::default() };
    down(&mut ed, 2.0);
    ed.app.mods = Mods::default();
    assert_eq!(ed.browser.as_ref().unwrap().drag_items(), vec![dir.join("a.wav"), dir.join("c.wav")]);
    // A press on the selection keeps it for the drag.
    down(&mut ed, 2.0);
    ed.handle(&UiEvent::Dropped { from: list.clone(), to: pad(&ed, 6), x: 0.0, y: 0.0 });
    ed.handle(&UiEvent::Mouse { path: list.clone(), overload: "BrowserFileList".into(), kind: MouseKind::Up, x: -50.0, y: 0.0 });
    assert_eq!(sample(&ed, 6, 0), Some(dir.join("a.wav")));
    assert_eq!(sample(&ed, 7, 0), Some(dir.join("c.wav")));
    assert_eq!(ed.browser.as_ref().unwrap().drag_items().len(), 2);
    // One file with alt goes to the first free layer.
    down(&mut ed, 1.0);
    ed.handle(&UiEvent::Mouse { path: list.clone(), overload: "BrowserFileList".into(), kind: MouseKind::Up, x: 5.0, y: 20.0 });
    ed.app.mods = Mods { alt: true, ..Mods::default() };
    ed.handle(&UiEvent::Dropped { from: list.clone(), to: pad(&ed, 6), x: 0.0, y: 0.0 });
    ed.app.mods = Mods::default();
    assert_eq!(sample(&ed, 6, 1), Some(dir.join("b.wav")));
    // A locked pad takes nothing.
    padmenu::set_lock(ed.model.pad_mut(e, 8), true);
    ed.handle(&UiEvent::Dropped { from: list.clone(), to: pad(&ed, 8), x: 0.0, y: 0.0 });
    assert_eq!(sample(&ed, 8, 0).filter(|f| f.starts_with(&dir)), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sample_load_sets_holds_names_pads_and_respects_locks() {
    let wav = demo("Hits/Cymbals/CY Cym Ar82 01 C2A St GB.wav");
    if !wav.exists() {
        return;
    }
    let (frames, sr) = dreist_engine::audio::info(&wav).expect("header info");
    let mut m = Model::new(project::new_document());
    assert!(browser::load_sample_into_layer(&mut m, 0, 3, 0, &wav));
    let n = m.layer_node(0, 3, 0).unwrap();
    let get = |q: dreist_formats::params::LayerParam| q.default_value().clamp(0.0, 1.0);
    let want = dreist_engine::hold::reset_hold_time(&get, false, frames as f64, frames as f64, sr, false);
    assert!((n.f32("AmpEnvHold").unwrap() - want).abs() < 1e-6);
    assert_eq!(n.f32("FreeEnvHold"), n.f32("AmpEnvHold"));
    assert_eq!(m.pad_name(0, 3).as_deref(), Some("P4 CY Cym Ar82 01 C2A St GB"));
    // A second sample keeps the automatic name only while one sample is
    // present: with two the name stays.
    assert!(browser::load_sample_into_layer(&mut m, 0, 3, 1, &wav));
    assert_eq!(m.pad_name(0, 3).as_deref(), Some("P4 CY Cym Ar82 01 C2A St GB"));
    // Locked pads take nothing.
    padmenu::set_lock(m.pad_mut(0, 5), true);
    assert!(!browser::load_sample_into_layer(&mut m, 0, 5, 0, &wav));
    assert!(!m.layer(0, 5, 0).valid);
}

#[test]
fn finder_files_drop_onto_pads() {
    let wav = demo("Hits/Cymbals/CY Cym Ar82 01 C2A St GB.wav");
    if !wav.exists() {
        return;
    }
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    fn pad(c: &Ctrl, idx: &str, path: &mut Vec<usize>) -> Option<Vec<usize>> {
        if c.overload == "EditorPad" && c.attr("padIndex") == Some(idx) {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = pad(k, idx, path) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let Some(p2) = pad(&ed.app.root, "2", &mut Vec::new()) else { return };
    ed.handle(&UiEvent::Files { to: p2.clone(), files: vec![wav.clone()], x: 0.0, y: 0.0, dropped: false });
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::image("DragAudio"));
    ed.handle(&UiEvent::Files { to: p2, files: vec![wav.clone()], x: 0.0, y: 0.0, dropped: true });
    let e = ed.model.engine;
    assert!(ed.model.layer(e, 2, 0).valid);
}

#[test]
fn dragging_files_over_the_pads_flashes_one_pad_per_file() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(m) = ed.pads_matrix() else { return };
    let mc = ed.app.ctrl(&m).unwrap().clone();
    let slot = mc.children.iter().position(|c| c.attr("padIndex") == Some("1")).unwrap();
    let mut to = m.clone();
    to.push(slot);
    let (ox, oy) = ed.app.origin(&to);
    let files: Vec<std::path::PathBuf> = ["a.wav", "b.wav", "c.wav"].iter().map(std::path::PathBuf::from).collect();
    ed.handle(&UiEvent::Files { to: to.clone(), files: files.clone(), x: ox as f32 + 5.0, y: oy as f32 + 5.0, dropped: false });
    ed.animate();
    let lit = |ed: &Editor| -> Vec<String> {
        let mc = ed.app.ctrl(&m).unwrap();
        mc.children.iter().filter(|c| c.attr("flashAlpha").is_some()).filter_map(|c| c.attr("padIndex").map(str::to_string)).collect()
    };
    let mut got = lit(&ed);
    got.sort();
    assert_eq!(got, vec!["1", "2", "3"]);
    // Leaving the pads ends the loops; the flashes then fade out.
    ed.handle(&UiEvent::Files { to: Vec::new(), files, x: 0.0, y: 0.0, dropped: false });
    std::thread::sleep(std::time::Duration::from_millis(250));
    ed.animate();
    assert!(lit(&ed).is_empty());
    // A click flashes the pad.
    let ev = UiEvent::Mouse { path: to.clone(), overload: "EditorPad".into(), kind: dreist_gui::app::MouseKind::Down, x: 5.0, y: 5.0 };
    ed.handle(&ev);
    ed.animate();
    assert_eq!(lit(&ed), vec!["1"]);
}

#[test]
fn dragging_a_pattern_key_highlights_the_target_key() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    fn key(c: &Ctrl, name: &str, path: &mut Vec<usize>) -> Option<Vec<usize>> {
        if c.overload == "PatternSelector" && c.name == name {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = key(k, name, path) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let (Some(k0), Some(k1)) = (key(&ed.app.root, "Key0", &mut Vec::new()), key(&ed.app.root, "Key1", &mut Vec::new())) else { return };
    ed.handle(&UiEvent::DragOver { from: k0.clone(), to: k1.clone(), x: 0.0, y: 0.0 });
    assert_eq!(ed.app.ctrl(&k1).unwrap().attr("dragAlpha"), Some("0"));
    std::thread::sleep(std::time::Duration::from_millis(40));
    ed.animate();
    // One 1/30 s tick (+25), or more on a slow machine.
    assert!(matches!(ed.app.ctrl(&k1).unwrap().attr("dragAlpha"), Some("25" | "50" | "75" | "100")));
    // Back over the source: no highlight.
    ed.handle(&UiEvent::DragOver { from: k0.clone(), to: k0.clone(), x: 0.0, y: 0.0 });
    assert!(ed.app.ctrl(&k1).unwrap().attr("dragAlpha").is_none());
}

#[test]
fn utility_splitter_drags_and_collapses() {
    use dreist_gui::app::MouseButton;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(sp) = ed.app.root.path_of("UtilitySplitter") else { return };
    let (ox, oy) = ed.app.origin(&sp);
    let c = ed.app.ctrl(&sp).unwrap().clone();
    let bar = dreist_gui::splitter::bar(&c);
    let start = dreist_gui::splitter::position(&c);
    let (bx, by) = ((ox + bar.x + bar.w / 2) as f32, (oy + bar.y + bar.h / 2) as f32);
    ed.app.mouse_move(bx, by);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::LeftRight);
    ed.app.mouse_down(MouseButton::Left);
    let evs = ed.app.mouse_move(bx + 50.0, by);
    assert!(matches!(evs.as_slice(), [UiEvent::Split { pos, end: false, .. }] if *pos == start + 50));
    let evs = ed.app.mouse_up(MouseButton::Left);
    for e in &evs {
        ed.handle(e);
    }
    let c = ed.app.ctrl(&sp).unwrap();
    let p0 = c.children.iter().find(|k| k.display_name == "SplitterPanel0").unwrap();
    assert_eq!(p0.rect.w, start + 50);
    // Clamped at intMaxSize0 = 600.
    std::thread::sleep(std::time::Duration::from_millis(450));
    let bx = bx + 50.0;
    ed.app.mouse_move(bx, by);
    ed.app.mouse_down(MouseButton::Left);
    ed.app.mouse_move(bx + 900.0, by);
    ed.app.mouse_up(MouseButton::Left);
    assert_eq!(dreist_gui::splitter::position(ed.app.ctrl(&sp).unwrap()), 600);
    // A double click collapses panel 0 (optDbClkCollapse1 false), animated.
    std::thread::sleep(std::time::Duration::from_millis(450));
    let bx = (ox + 600 + bar.w / 2) as f32;
    ed.app.mouse_move(bx, by);
    for _ in 0..2 {
        ed.app.mouse_down(MouseButton::Left);
        ed.app.mouse_up(MouseButton::Left);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    ed.animate();
    assert_eq!(dreist_gui::splitter::position(ed.app.ctrl(&sp).unwrap()), 0);
}

#[test]
fn window_sizer_resizes_the_root_within_the_minimum() {
    use dreist_gui::app::MouseButton;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let (w, h) = ed.app.size();
    let (x, y) = ((w - 7) as f32, (h - 7) as f32);
    ed.app.mouse_move(x, y);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::ResizeNwse);
    ed.app.mouse_down(MouseButton::Left);
    ed.app.mouse_move(x + 100.0, y + 40.0);
    assert_eq!(ed.app.size(), (w + 100, h + 40));
    assert!(ed.app.resize_request && ed.app.live_resizing());
    // The sizer follows the corner.
    let sz = ed.app.root.children.iter().find(|c| c.class == "CtrlWindowSizer").unwrap();
    assert_eq!((sz.rect.x, sz.rect.y), (w + 100 - 15, h + 40 - 15));
    // Below 1150 × 650 an axis keeps its size.
    ed.app.mouse_move(x - 2000.0, y + 60.0);
    assert_eq!(ed.app.size(), (w + 100, h + 60));
    ed.app.mouse_up(MouseButton::Left);
    ed.animate();
    assert_eq!(ed.modals.options.string("LastEditorSize", ""), format!("{},{}", w + 100, h + 60));
}

#[test]
fn ui_scale_applies_at_once_and_no_restores_it() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.modals.show(&mut ed.app, modal::Kind::Preferences);
    let Some(p) = ed.modals.prefs_path() else { return };
    let Some(ui) = ed.app.ctrl(&p).and_then(|c| c.path_of("UIScaling")).map(|q| [p.clone(), q].concat()) else { return };
    ed.handle(&UiEvent::MenuSelected { anchor: ui.clone(), name: "UIScaling".into(), index: 5 });
    assert!((ed.app.ui_scale - 1.5).abs() < 1e-6);
    std::thread::sleep(std::time::Duration::from_millis(2050));
    ed.animate();
    assert_eq!(ed.app.menu.as_ref().map(|m| m.items[0].clone()), Some(modal::KEEP_SCALE.to_string()));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: ui, name: "UIScaling".into(), index: 3 });
    assert!((ed.app.ui_scale - 1.0).abs() < 1e-6);
    assert_eq!(ed.modals.options.int("LastEditorScaleFactor", 0), 100);
}

#[test]
fn ui_style_applies_at_once() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.modals.show(&mut ed.app, modal::Kind::Preferences);
    let Some(p) = ed.modals.prefs_path() else { return };
    let Some(ui) = ed.app.ctrl(&p).and_then(|c| c.path_of("UIStyle")).map(|q| [p.clone(), q].concat()) else { return };
    ed.handle(&UiEvent::Clicked { path: ui.clone(), name: "UIStyle".into(), id: 0, on: false });
    let items = ed.app.menu.as_ref().map(|m| m.items.clone()).unwrap_or_default();
    let i = items.iter().position(|t| t == "Ember").expect("style menu");
    let before = ed.app.skin.styles.colours.clone();
    ed.handle(&UiEvent::MenuSelected { anchor: ui, name: "UIStyle".into(), index: i });
    assert_eq!(ed.app.skin.theme, "Ember");
    assert_ne!(ed.app.skin.styles.colours, before);
    assert_eq!(ed.modals.options.string("StyleSheet", ""), "Ember");
}

#[test]
fn documents_path_is_written_at_once_with_one_restart_alert() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.modals.show(&mut ed.app, modal::Kind::Preferences);
    let Some(p) = ed.modals.prefs_path() else { return };
    let find = |ed: &Editor, n: &str| ed.app.ctrl(&p).and_then(|c| c.path_of(n)).map(|q| [p.clone(), q].concat()).unwrap();
    let tb = find(&ed, "DocumentsPath");
    // Not a folder: reverted, nothing written.
    let before = ed.modals.documents_path();
    ed.handle(&UiEvent::TextCommitted { path: tb.clone(), name: "DocumentsPath".into(), text: "/no/such/folder".into() });
    assert_eq!(ed.app.ctrl(&tb).unwrap().text, before);
    assert!(ed.app.menu.is_none());
    // An existing folder: written straight away, the alert once.
    let dir = std::env::temp_dir().to_string_lossy().trim_end_matches('/').to_string();
    ed.handle(&UiEvent::TextCommitted { path: tb.clone(), name: "DocumentsPath".into(), text: dir.clone() });
    assert_eq!(ed.modals.options.string("LocalDocumentsPath", ""), dir);
    assert_eq!(ed.app.menu.as_ref().map(|m| m.items[0].clone()), Some(modal::docs_restart()));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: tb.clone(), name: "DocumentsPath".into(), index: 0 });
    // Reset asks; Yes writes the default without another alert.
    let rb = find(&ed, "ResetPathButton");
    ed.handle(&UiEvent::Clicked { path: rb.clone(), name: "ResetPathButton".into(), id: 0, on: false });
    assert!(ed.app.menu.as_ref().is_some_and(|m| m.items[0].starts_with("Are you sure you want to reset the Documents path to ")));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: rb, name: "ResetPathButton".into(), index: 2 });
    assert_eq!(ed.modals.options.string("LocalDocumentsPath", ""), modal::default_documents());
    assert!(ed.app.menu.is_none());
    // Browse hands the folder dialog to the editor.
    let bb = find(&ed, "BrowseButton");
    ed.handle(&UiEvent::Clicked { path: bb, name: "BrowseButton".into(), id: 0, on: false });
    assert!(!ed.modals.browse_requested, "taken by the editor (no dialog runner in tests)");
}

#[test]
fn export_panel_shows_progress_asks_and_closes() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    ed.modals.show(&mut ed.app, modal::Kind::Export);
    let job = command::ExportProgress::default();
    ed.modals.export_job = Some(job.clone());
    job.lock().unwrap().progress = 0.5;
    ed.animate();
    let ov = ed.app.root.path_of("ExportOverlay").unwrap();
    let bar = [ov.clone(), (ed.app.ctrl(&ov).unwrap()).path_of("ProgressBar").unwrap()].concat();
    let c = ed.app.ctrl(&bar).unwrap();
    assert!(c.visible && (c.value - 0.5).abs() < 1e-6);
    job.lock().unwrap().prompt = Some("clipped?".into());
    ed.animate();
    assert_eq!(ed.app.menu.as_ref().map(|m| m.items[0].clone()), Some("clipped?".into()));
    ed.app.menu = None;
    ed.handle(&UiEvent::MenuSelected { anchor: bar.clone(), name: "ProgressBar".into(), index: 2 });
    assert_eq!(job.lock().unwrap().answer, Some(true));
    job.lock().unwrap().result = Some(Ok(Vec::new()));
    ed.animate();
    assert!(ed.modals.open.is_none() && ed.modals.export_job.is_none());
}

#[test]
fn rex_loops_slice_with_their_own_points_and_load_truncated_on_pads() {
    let home = std::env::var("HOME").unwrap_or_default();
    let f = std::path::PathBuf::from(home).join(dreist_formats::legacy::LITE_FACTORY).join("Loops/Breaks/130 Whitekit 03 KS.rx2");
    if !f.exists() || dreist_engine::rex::load(&f).is_none() {
        return;
    }
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    assert!(ed.slicer.start(&ed.model, &f));
    assert_eq!(ed.slicer.slices.len(), 14);
    // On a pad the layer refers to the ".truncated" composite.
    assert!(browser::load_sample_into_layer(&mut ed.model, 0, 0, 0, &f));
    let name = ed.model.layer_node(0, 0, 0).and_then(|n| n.attr("AudioFileName")).unwrap().to_string();
    assert!(name.ends_with(".rx2.truncated"));
    assert_eq!(ed.model.pad_name(0, 0).as_deref(), Some("P1 130 Whitekit 03 KS"));
}

#[test]
fn layer_toggles_reach_the_document() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    for (name, attr, want) in [("Phase", "PhasePosition", "-1"), ("Reverse", "IsReversed", "true"), ("EnvMode", "EnvTracksPitch", "true"), ("LayerMode", "SynthesisMode", "VintageSampler")] {
        let Some(b) = ed.app.root.path_of(name) else { continue };
        ed.handle(&UiEvent::Clicked { path: b.clone(), name: name.into(), id: 0, on: true });
        assert_eq!(ed.model.layer_node(0, 0, 0).and_then(|n| n.attr(attr)), Some(want), "{name}");
        assert!(ed.app.ctrl(&b).unwrap().on, "{name}");
    }
    assert!(ed.take_commands().iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_LAYER_PHASE_POSITION)));
}

#[test]
fn browser_menu_acts_on_the_selection() {
    use dreist_gui::app::{Mods, MouseKind};
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let dir = std::env::temp_dir().join(format!("dreist-bmenu-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for f in ["a.wav", "b.wav"] {
        let _ = std::fs::write(dir.join(f), b"");
    }
    let Some(b) = ed.browser.as_mut() else { return };
    b.browse(dir.clone());
    let Some(page) = ed.browser.as_ref().map(|b| b.page().to_vec()) else { return };
    let list = (ed.app.ctrl(&page).unwrap()).path_of("FileList").map(|p| [page.clone(), p].concat());
    fn by(c: &Ctrl, path: &mut Vec<usize>) -> Option<Vec<usize>> {
        if c.overload == "BrowserFileList" {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = by(k, path) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let list = list.filter(|p| ed.app.ctrl(p).is_some_and(|c| c.overload == "BrowserFileList")).or_else(|| by(&ed.app.root, &mut Vec::new())).unwrap();
    let ev = |kind, row: f32| UiEvent::Mouse { path: list.clone(), overload: "BrowserFileList".into(), kind, x: 5.0, y: row * 18.0 + 2.0 };
    ed.handle(&ev(MouseKind::Down, 0.0));
    ed.app.mods = Mods { cmd: true, ..Mods::default() };
    ed.handle(&ev(MouseKind::Down, 1.0));
    ed.app.mods = Mods::default();
    ed.handle(&ev(MouseKind::RightDown, 1.0));
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert!(items.iter().any(|t| t == "Reveal selected items in Finder"), "{items:?}");
    assert!(items.iter().any(|t| t.starts_with("Add selected to shortlist")), "{items:?}");
    // Favourites take folders only: disabled for files.
    assert!(items.iter().any(|t| t == &dreist_gui::menu::item("Add selected items to favourites", false)), "{items:?}");
    // The shortlist sub menu is enabled for files (not chosen here: it
    // would write the user's shortlists).
    let k = items.iter().position(|t| t.starts_with("Add selected to shortlist")).unwrap();
    let anchor = ed.app.menu.as_ref().unwrap().anchor.clone();
    ed.handle(&UiEvent::MenuSelected { anchor, name: String::new(), index: k });
    let sub = ed.app.menu.as_ref().unwrap().items.clone();
    assert!(sub.iter().any(|t| t == "Add to new shortlist"), "{sub:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn browser_source_list_item_menu() {
    use dreist_gui::app::MouseKind;
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let dir = std::env::temp_dir().join(format!("dreist-srcmenu-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("snare.wav");
    let _ = std::fs::write(&f, b"x");
    let Some(b) = ed.browser.as_mut() else { return };
    let name = b.test_shortlist(vec![f.clone()]);
    fn by(c: &Ctrl, path: &mut Vec<usize>) -> Option<Vec<usize>> {
        if c.overload == "BrowserSourceList" {
            return Some(path.clone());
        }
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if let Some(p) = by(k, path) {
                return Some(p);
            }
            path.pop();
        }
        None
    }
    let list = by(&ed.app.root, &mut Vec::new()).unwrap();
    let row = |ed: &Editor, label: &str| ed.browser.as_ref().unwrap().source_row(label);
    let menu = |ed: &mut Editor, r: usize, label: &str| {
        ed.handle(&UiEvent::Mouse { path: list.clone(), overload: "BrowserSourceList".into(), kind: MouseKind::RightDown, x: 5.0, y: r as f32 * 25.0 + 3.0 });
        let items = ed.app.menu.as_ref().expect("menu").items.clone();
        let i = items.iter().position(|s| s.ends_with(label)).unwrap_or_else(|| panic!("{label} in {items:?}"));
        ed.handle(&UiEvent::MenuSelected { anchor: list.clone(), name: String::new(), index: i });
        items
    };
    let (r, _) = row(&ed, &name).unwrap();
    let items = menu(&mut ed, r, "Set colour label... ▸");
    assert_eq!(
        items,
        vec![
            format!("Remove \"{name}\""),
            String::new(),
            "Set name...".into(),
            "Set colour label... ▸".into(),
            String::new(),
            "Export to folder".into(),
            "Export to folder and add as favourite".into()
        ]
    );
    ed.handle(&UiEvent::MenuSelected { anchor: list.clone(), name: String::new(), index: 5 });
    assert_eq!(row(&ed, &name).map(|r| r.1), Some(5));
    // Rename: the shortlist takes the name and keeps its colour.
    menu(&mut ed, r, "Set name...");
    let edit = ed.app.root.path_of(browser::SOURCE_RENAME_EDIT).expect("edit box");
    let new = format!("Drums {}", std::process::id());
    ed.handle(&UiEvent::TextCommitted { path: edit, name: browser::SOURCE_RENAME_EDIT.into(), text: new.clone() });
    assert_eq!(row(&ed, &new).map(|r| r.1), Some(5));
    // Export copies the files into <dir>/<name>.
    let out = ed.browser.as_mut().unwrap().export_shortlist(&new, &dir, false).unwrap();
    assert!(out.join("snare.wav").exists());
    // Remove asks first.
    let (r, _) = row(&ed, &new).unwrap();
    menu(&mut ed, r, &format!("Remove \"{new}\""));
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert!(items[0].contains("Are you sure you want to remove"), "{items:?}");
    let yes = items.iter().position(|s| s == "Yes").unwrap();
    ed.handle(&UiEvent::MenuSelected { anchor: list.clone(), name: String::new(), index: yes });
    assert!(row(&ed, &new).is_none());
    // Places rows offer Reveal only.
    let (r, _) = row(&ed, "Home").unwrap();
    ed.handle(&UiEvent::Mouse { path: list.clone(), overload: "BrowserSourceList".into(), kind: MouseKind::RightDown, x: 5.0, y: r as f32 * 25.0 + 3.0 });
    assert_eq!(ed.app.menu.as_ref().unwrap().items, vec!["Reveal \"Home\" in Finder".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tooltips_follow_the_pointer() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let hover = |ed: &mut Editor, name: &str| -> Option<String> {
        let path = ed.app.root.path_of(name).unwrap();
        let (x, y) = ed.app.origin(&path);
        let c = ed.app.ctrl(&path).unwrap();
        let (cx, cy) = (x as f32 + c.rect.w as f32 / 2.0, y as f32 + c.rect.h as f32 / 2.0);
        for ev in ed.app.mouse_move(cx, cy) {
            ed.handle(&ev);
        }
        let t = ed.app.tooltip_target();
        ed.handle(&UiEvent::ToolTip { path: t });
        ed.app.tooltip.as_ref().map(|t| t.text.clone())
    };
    assert!(hover(&mut ed, "Cutoff").is_some_and(|t| t.starts_with("Filter Cutoff: ") && (t.ends_with("Hz") || t.ends_with("KHz"))));
    assert_eq!(hover(&mut ed, "GlobalGain").map(|t| t.starts_with("Master Level: ")), Some(true));
    assert_eq!(hover(&mut ed, "StepLengthCombo").as_deref(), Some("Length of time each step lasts"));
    // A shown transmod source: two lines.
    ed.model.source = 3;
    assert!(hover(&mut ed, "Cutoff").is_some_and(|t| t.contains(" -> Filter Cutoff:\n")), "{:?}", hover(&mut ed, "Cutoff"));
    // Leaving the window hides it.
    ed.app.mouse_left();
    assert!(ed.app.tooltip.is_none());
}

#[test]
fn layer_meta_data_editor_saves_the_gsa() {
    let dir = std::env::temp_dir().join(format!("dreist-meta-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    // A 2 s mono file: GuessKeyData gives 120 BPM over 1 bar.
    let wav = dir.join("loop 97.wav");
    let silence = vec![0.0f32; 88200];
    dreist_engine::write_wav(&wav, 44100, &silence, &silence).unwrap();
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let e = ed.model.engine;
    ed.model.layer_mut(e, 0, 0).set("AudioFileName", wav.to_string_lossy());
    ed.model.samples = Some(dir.clone());
    ed.sync();
    let row = ed
        .app
        .root
        .path_of("LayerRow")
        .or_else(|| {
            fn find(c: &Ctrl, path: &mut Vec<usize>) -> bool {
                for (i, k) in c.children.iter().enumerate() {
                    path.push(i);
                    if k.overload == "LayerRow" && k.attr("layer") == Some("0") {
                        return true;
                    }
                    if find(k, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
            let mut p = Vec::new();
            find(&ed.app.root, &mut p).then_some(p)
        })
        .unwrap();
    ed.handle(&UiEvent::Mouse { path: row.clone(), overload: "LayerRow".into(), kind: dreist_gui::app::MouseKind::RightDown, x: 5.0, y: 5.0 });
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    let i = items.iter().position(|s| s == "Edit meta data").unwrap_or_else(|| panic!("{items:?}"));
    ed.handle(&UiEvent::MenuSelected { anchor: row, name: String::new(), index: i });
    let ov = ed.app.root.path_of("MetaDataOverlay").unwrap();
    let inside = |ed: &Editor, n: &str| [ov.clone(), (ed.app.ctrl(&ov).unwrap()).path_of(n).unwrap()].concat();
    let bpm = inside(&ed, "BPMBox");
    assert_eq!(ed.app.ctrl(&bpm).unwrap().text, "120.00");
    ed.handle(&UiEvent::TextCommitted { path: bpm.clone(), name: "BPMBox".into(), text: "5000".into() });
    assert_eq!(ed.app.ctrl(&bpm).unwrap().text, "1000.00");
    let author = inside(&ed, "Author");
    ed.handle(&UiEvent::TextCommitted { path: author, name: "Author".into(), text: "me".into() });
    let save = inside(&ed, "SaveButton");
    ed.handle(&UiEvent::Clicked { path: save, name: "SaveButton".into(), id: 0, on: false });
    let f = dreist_engine::gsa::SliceFile::load(&wav.with_extension("gsa")).unwrap();
    assert_eq!((f.bpm, f.author.as_str()), (1000.0, "me"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mouse_drag_between_pads_emits_a_drop() {
    use dreist_gui::app::MouseButton;
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    fn find(c: &dreist_gui::Ctrl, idx: &str, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if (k.overload == "EditorPad" && k.attr("padIndex") == Some(idx)) || find(k, idx, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let centre = |ed: &Editor, idx: &str| {
        let mut v = Vec::new();
        assert!(find(&ed.app.root, idx, &mut v));
        let (x, y) = ed.app.origin(&v);
        let r = ed.app.ctrl(&v).unwrap().rect;
        (v, x as f32 + r.w as f32 / 2.0, y as f32 + r.h as f32 / 2.0)
    };
    let (a, ax, ay) = centre(&ed, "1");
    let (b, bx, by) = centre(&ed, "2");
    ed.app.mouse_move(ax, ay);
    ed.app.mouse_down(MouseButton::Left);
    let over = ed.app.mouse_move(bx, by);
    let drag = over.iter().find(|e| matches!(e, UiEvent::DragOver { .. })).cloned().expect("drag over");
    ed.handle(&drag);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::image("DragPad"));
    let evs = ed.app.mouse_up(MouseButton::Left);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::Arrow);
    assert!(matches!(&evs[0], UiEvent::Dropped { from, to, .. } if *from == a && to.starts_with(&b)), "{evs:?}");
    // A click without travel is not a drop.
    ed.app.mouse_move(ax, ay);
    ed.app.mouse_down(MouseButton::Left);
    let evs = ed.app.mouse_up(MouseButton::Left);
    assert!(!evs.iter().any(|e| matches!(e, UiEvent::Dropped { .. })));
}

#[test]
fn song_body_hover_cursors() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    fn find(c: &dreist_gui::Ctrl, path: &mut Vec<usize>) -> bool {
        for (i, k) in c.children.iter().enumerate() {
            path.push(i);
            if k.overload == "SongBody" || find(k, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut body = Vec::new();
    if !find(&ed.app.root, &mut body) {
        return;
    }
    let ev = UiEvent::Hover { path: body.clone(), overload: "SongBody".into(), x: 5000.0, y: 30.0 };
    ed.handle(&ev);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::image("Pen"));
    ed.app.mods.shift = true;
    ed.handle(&ev);
    assert_eq!(ed.app.cursor, dreist_gui::cursor::Cursor::image("Selection"));
}

#[test]
fn save_layer_sample_crops_to_markers() {
    let p = demo("Presets/Hollow.geist");
    let Ok(proj) = dreist_formats::Project::load(&p) else { return };
    let mut model = Model::new(proj.raw);
    model.samples = Some(dreist_formats::model::samples_dir(&p));
    let e = model.engine;
    let Some((pad, l)) = (0..64).flat_map(|p| (0..8).map(move |l| (p, l))).find(|&(p, l)| model.layer_sample(e, p, l).is_some_and(|f| f.exists())) else { return };
    let out = std::env::temp_dir().join(format!("dreist-save-{}.wav", std::process::id()));
    padmenu::save_layer_sample(&model, e, pad, l, &out).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let ch = u16::from_le_bytes([bytes[22], bytes[23]]) as u32;
    let data = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
    let mut cache = dreist_engine::audio::SampleCache::default();
    let src = cache.get(&model.layer_sample(e, pad, l).unwrap()).unwrap();
    let m = inspector::wave_markers(&model, e, pad, l, src.left.len() as u32);
    assert_eq!(data, (m.end.min(src.left.len() as u32) - m.start) * 3 * ch);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn pads_strip_erase_repeat_and_timing() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    let cp = ed.app.root.path_of("PadControl").unwrap();
    let child = |ed: &Editor, n: &str| {
        let c = ed.app.ctrl(&cp).unwrap();
        [cp.clone(), vec![c.children.iter().position(|k| k.name == n).unwrap()]].concat()
    };
    let er = child(&ed, "EraseButton");
    ed.handle(&UiEvent::Clicked { path: er.clone(), name: "EraseButton".into(), id: 0, on: true });
    let e = ed.model.engine;
    let st = |ed: &Editor, k: &str| ed.model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "EngineState")).and_then(|s| s.attr(k).map(str::to_string));
    assert_eq!(st(&ed, "NoteEraseEnabled").as_deref(), Some("true"));
    assert!(ed.app.ctrl(&er).unwrap().on);
    let rc = child(&ed, "RepeatCount");
    ed.handle(&UiEvent::Clicked { path: rc.clone(), name: "RepeatCount".into(), id: 0, on: false });
    let items = ed.app.menu.as_ref().unwrap().items.clone();
    assert_eq!(items[8], "✓ 1/16");
    assert_eq!(items.last().unwrap(), "✓ Sync to next division");
    let cmds = ed.take_commands();
    let _ = cmds;
    ed.handle(&UiEvent::MenuSelected { anchor: rc.clone(), name: "RepeatCount".into(), index: 10 });
    assert_eq!(ed.app.ctrl(&rc).unwrap().text, "1/32");
    let cmds = ed.take_commands();
    assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_NOTE_REPEAT_BASE && (m.value[0] * 13.0).round() as i32 == 10)));
    // At the default size Full Level does not fit: it moves into the
    // Options menu.
    let op = child(&ed, "Options");
    if ed.app.ctrl(&op).unwrap().visible {
        assert!(!ed.app.ctrl(&child(&ed, "FullLevelKnob")).unwrap().visible);
        ed.handle(&UiEvent::Clicked { path: op.clone(), name: "Options".into(), id: 0, on: false });
        let items = ed.app.menu.as_ref().unwrap().items.clone();
        assert!(items.iter().any(|t| t.ends_with("Full Level")), "{items:?}");
        // The slider row sets the level and keeps the menu open.
        let i = items.iter().position(|t| dreist_gui::menu::slider_value(t).is_some()).expect("slider row");
        ed.outbox.clear();
        ed.handle(&UiEvent::MenuSlider { anchor: op.clone(), index: i, value: 0.5 });
        let cmds = std::mem::take(&mut ed.outbox);
        assert!(cmds.iter().any(|c| matches!(c, Command::Message(m) if m.kind == dreist_engine::binding::id::SET_FULL_LEVEL_LEVEL && (m.value[0] - 0.5).abs() < 1e-6)));
        assert!(ed.app.menu.is_some());
    }
}

/// Scene names come from the document (the engine keeps none).
#[test]
fn scene_names_come_from_the_document() {
    let Some(mut ed) = demo_editor("Presets/Hollow.geist") else { return };
    assert_eq!(ed.model.scene_name(0), "Drums");
    ed.model.set_scene_name(5, "Fill");
    assert_eq!(ed.model.scene_name(5), "Fill");
    assert_eq!(ed.model.scene_name(95), "");
}

/// The running tempo (host / tap) reaches the document and the header,
/// without an undo step.
#[test]
fn engine_tempo_reaches_document_and_header() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let undo = ed.history.undo.len();
    ed.engine_tempo(133.25);
    assert_eq!(ed.model.tempo(), 133.25);
    let bpm = ed.app.root.path_of("BPMTextBox").and_then(|p| ed.app.ctrl(&p)).map(|c| c.text.clone());
    assert_eq!(bpm.as_deref(), Some("133.25"));
    assert_eq!(ed.history.undo.len(), undo);
}

/// "nan" / "inf" typed into a value box are not numbers: nothing is sent
/// and the document keeps its value.
#[test]
fn typed_non_numbers_are_ignored() {
    let Ok(mut ed) = Editor::new(dreist_gui::skin::default_theme(), Model::new(project::new_document())) else { return };
    let Some(p) = ed.app.root.path_of("BPMTextBox") else { return };
    ed.take_commands();
    for text in ["nan", "inf", "-infinity", "NaN"] {
        ed.handle(&UiEvent::TextCommitted { path: p.clone(), name: "BPMTextBox".into(), text: text.into() });
        let cmds = ed.take_commands();
        assert!(cmds.is_empty(), "{text}: {cmds:?}");
        assert_eq!(ed.model.tempo(), 120.0, "{text}");
    }
    ed.handle(&UiEvent::TextCommitted { path: p.clone(), name: "BPMTextBox".into(), text: " 97.5 ".into() });
    assert_eq!(ed.model.tempo(), 97.5);
    assert_eq!(crate::conv::number("1e99"), None, "overflows to infinity");
}
