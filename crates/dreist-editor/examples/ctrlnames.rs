//! Print every interactive control's class, name and ancestry (audit aid).
use dreist_editor::{Editor, Model};

fn walk(c: &dreist_gui::Ctrl, path: &mut Vec<String>) {
    let interactive = matches!(
        c.class.as_str(),
        "CtrlButton"
            | "CtrlPopupButton"
            | "CtrlKnob"
            | "CtrlTransmodKnob"
            | "CtrlSlider"
            | "CtrlTransmodSlider"
            | "CtrlTextBox"
            | "CtrlValueBox"
            | "CtrlValueStepper"
            | "CtrlSearchBox"
            | "CtrlDisclosureButton"
            | "CtrlTempoBox"
    );
    if interactive && !c.name.is_empty() {
        println!("{}\t{}\t{}", c.class, c.name, path.join("/"));
    }
    path.push(c.name.clone());
    for k in &c.children {
        walk(k, path);
    }
    path.pop();
}

fn main() {
    let ed = Editor::new(dreist_gui::skin::default_theme(), Model::new(dreist_formats::Node::new("root", None))).expect("editor");
    walk(&ed.app.root, &mut Vec::new());
}
