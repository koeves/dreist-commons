//! Save → load round trips: `roundtrip_check file.geist...`.
//!
//! Each project is saved through the editor's save path into a temp folder
//! and loaded back; the documents (sample paths aside) and 8 s renders of
//! both must match.

fn strip_paths(n: &mut dreist_formats::Node) {
    for (k, v) in &mut n.attrs {
        if k == "AudioFileName" {
            *v = v.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        }
    }
    for c in &mut n.children {
        strip_paths(c);
    }
}

fn render(doc: &dreist_formats::Node, samples: &std::path::Path) -> Vec<f32> {
    let mut doc = doc.clone();
    if std::env::var("NOFX").is_ok() {
        fn strip(n: &mut dreist_formats::Node) {
            n.children.retain(|c| c.class != "EffectChain");
            n.children.iter_mut().for_each(strip);
        }
        strip(&mut doc);
    }
    let doc = &doc;
    let mut cache = dreist_engine::audio::SampleCache::default();
    let mut g = dreist_engine::Dreist::from_document(doc, samples, 44100.0, dreist_engine::RenderOptions::default(), &mut cache).expect("build");
    g.start();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    let mut out = Vec::new();
    for _ in 0..(8 * 44100 / 512) {
        g.process(&mut l, &mut r);
        out.extend_from_slice(&l);
        out.extend_from_slice(&r);
    }
    out
}

fn main() {
    let tmp = std::env::temp_dir().join(format!("dreist-rt-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    for f in std::env::args().skip(1) {
        let p = std::path::Path::new(&f);
        let doc = dreist_formats::Project::load(p).expect("load").raw;
        let src = dreist_formats::model::samples_dir(p);
        let out = tmp.join(p.file_name().unwrap());
        let (saved, errors) = dreist_editor::project::save(&doc, Some(&src), &out).expect("save");
        let back = dreist_formats::Project::load(&out).expect("reload").raw;
        let (mut a, mut b) = (doc.clone(), back.clone());
        strip_paths(&mut a);
        strip_paths(&mut b);
        let same_doc = a == b;
        let _ = saved;
        let ra = render(&doc, &src);
        let rb = render(&back, &dreist_formats::model::samples_dir(&out));
        let ra2 = render(&doc, &src);
        let self_diff = ra.iter().zip(&ra2).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        let first_bad = ra.iter().zip(&rb).position(|(x, y)| (x - y).abs() > 1e-4);
        println!("  self diff {self_diff:.2e}, first differing sample {first_bad:?}");
        let diff = ra.iter().zip(&rb).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        let peak = ra.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        println!("{}: doc {} copy-errors {} render peak {peak:.3} max diff {diff:.2e}", p.display(), if same_doc { "same" } else { "DIFFERENT" }, errors.len());
        if !same_doc {
            // First difference.
            fn first(a: &dreist_formats::Node, b: &dreist_formats::Node, path: &str) -> Option<String> {
                if a.class != b.class || a.attrs != b.attrs {
                    let only_a: Vec<_> = a.attrs.iter().filter(|x| !b.attrs.contains(x)).collect();
                    let only_b: Vec<_> = b.attrs.iter().filter(|x| !a.attrs.contains(x)).collect();
                    return Some(format!("{path}/{}: {only_a:?} vs {only_b:?}", a.class));
                }
                if a.children.len() != b.children.len() {
                    return Some(format!("{path}/{}: {} vs {} children", a.class, a.children.len(), b.children.len()));
                }
                a.children.iter().zip(&b.children).find_map(|(x, y)| first(x, y, &format!("{path}/{}", a.class)))
            }
            println!("  {}", first(&a, &b, "").unwrap_or_default());
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
