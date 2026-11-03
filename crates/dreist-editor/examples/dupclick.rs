//! Click every interactive control whose name occurs more than once and
//! (buttons clicked, knobs / sliders moved to 0.37) print what each instance does (audit aid for handlers that match names
//! without checking where the click came from).
use dreist_editor::{Command, Editor, Model};
use dreist_gui::app::UiEvent;

fn walk(c: &dreist_gui::Ctrl, path: &mut Vec<usize>, names: &mut Vec<String>, out: &mut Vec<(String, Vec<usize>, String)>) {
    let interactive = matches!(c.class.as_str(), "CtrlButton" | "CtrlPopupButton" | "CtrlDisclosureButton" | "CtrlKnob" | "CtrlTransmodKnob" | "CtrlSlider" | "CtrlTransmodSlider");
    if interactive && !c.name.is_empty() {
        out.push((c.name.clone(), path.clone(), names.join("/")));
    }
    for (i, k) in c.children.iter().enumerate() {
        path.push(i);
        names.push(k.name.clone());
        walk(k, path, names, out);
        names.pop();
        path.pop();
    }
}

fn main() {
    let p = std::env::args().nth(1).expect("project");
    let doc = dreist_formats::Project::load(std::path::Path::new(&p)).expect("project").raw;
    let ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(doc.clone())).expect("editor");
    let mut all = Vec::new();
    walk(&ed.app.root, &mut Vec::new(), &mut Vec::new(), &mut all);
    let mut count = std::collections::BTreeMap::<String, usize>::new();
    for (n, _, _) in &all {
        *count.entry(n.clone()).or_default() += 1;
    }
    // DEAD=1: every control, printing only those whose event changed
    // nothing (no command, menu, document, model or control tree change).
    // RIGHT=1: right clicks instead (with DEAD, every control).
    let dead = std::env::var("DEAD").is_ok();
    for (name, path, anc) in all.iter().filter(|(n, _, _)| dead || count[n] > 1) {
        let mut ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(doc.clone())).expect("editor");
        let on = !ed.app.ctrl(path).map(|c| c.on).unwrap_or(false);
        let before = ed.model.doc.clone();
        let tabs = (ed.model.editor_tab, ed.model.utility_tab);
        let snap = |ed: &Editor| if dead { format!("{:?}{:?}", ed.app.root, ed.model) } else { String::new() };
        let s0 = snap(&ed);
        let knob = ed.app.ctrl(path).is_some_and(|c| !c.class.ends_with("Button"));
        // Startup commands are not the click's.
        ed.take_commands();
        if std::env::var("RIGHT").is_ok() {
            ed.handle(&UiEvent::RightClicked { path: path.clone(), name: name.clone() });
        } else if knob {
            ed.handle(&UiEvent::ValueChanged { path: path.clone(), name: name.clone(), id: 0, value: 0.37 });
        } else {
            ed.handle(&UiEvent::Clicked { path: path.clone(), name: name.clone(), id: 0, on });
        }
        let cmds: Vec<String> = ed
            .take_commands()
            .iter()
            .map(|c| match c {
                Command::Message(m) => format!("{:#x}", m.kind),
                c => format!("{c:?}").chars().take(24).collect(),
            })
            .collect();
        let menu = ed.app.menu.as_ref().and_then(|m| m.items.first().cloned()).unwrap_or_default();
        let doc = if ed.model.doc != before { "doc" } else { "" };
        let tab = if (ed.model.editor_tab, ed.model.utility_tab) != tabs { "tab" } else { "" };
        if dead && std::env::var("RIGHT").is_ok() {
            println!("{name}\t{}\t{menu:?}\t{}", anc.rsplit('/').take(3).collect::<Vec<_>>().join("<"), cmds.len());
            continue;
        }
        if dead {
            if cmds.is_empty() && menu.is_empty() && snap(&ed) == s0 {
                println!("{name}\t{anc}");
            }
            continue;
        }
        let short: String = anc.rsplit('/').take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
        println!("{name}\t{short}\t{cmds:?}\tmenu={menu:?}\t{doc}\t{tab}");
    }
}
