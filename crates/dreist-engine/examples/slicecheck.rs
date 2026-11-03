//! Compare `dreist_dsp::slice::analyze` with a sample's saved `.gsa` slices.
use std::path::Path;

fn main() {
    let wav = std::env::args().nth(1).expect("sample path");
    let s = dreist_engine::audio::load(Path::new(&wav)).expect("load");
    let ours = dreist_dsp::slice::analyze(&s.left, s.right.as_deref());
    let gsa = std::fs::read_to_string(Path::new(&wav).with_extension("gsa")).unwrap_or_default();
    let root = dreist_formats::Node::parse(&gsa).ok();
    let mut theirs = Vec::new();
    fn walk(n: &dreist_formats::Node, out: &mut Vec<(u32, f32, f32, f32)>) {
        if n.class == "SlicePoint" {
            out.push((n.u32("Start").unwrap_or(0), n.f32("Strength").unwrap_or(0.0), n.f32("DynSense").unwrap_or(0.0), n.f32("Mean").unwrap_or(0.0)));
        }
        n.children.iter().for_each(|c| walk(c, out));
    }
    if let Some(r) = &root {
        walk(r, &mut theirs);
    }
    println!("ours {} theirs {}", ours.len(), theirs.len());
    for i in 0..ours.len().max(theirs.len()).min(20) {
        let o = ours.get(i).map(|p| format!("{:>7} s{:.4} d{:.4} m{:.4}", p.start, p.strength, p.dyn_sense, p.mean)).unwrap_or_default();
        let t = theirs.get(i).map(|p| format!("{:>7} s{:.4} d{:.4} m{:.4}", p.0, p.1, p.2, p.3)).unwrap_or_default();
        println!("{o:40} | {t}");
    }
}
