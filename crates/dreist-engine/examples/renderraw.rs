//! Render a project (effects stripped with NOFX=1) to raw f32: `renderraw in.geist out.raw [secs]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = std::path::Path::new(&a[1]);
    let mut doc = dreist_formats::Project::load(p).unwrap().raw;
    if std::env::var("NOFX").is_ok() {
        fn strip(n: &mut dreist_formats::Node) {
            n.children.retain(|c| c.class != "EffectChain");
            n.children.iter_mut().for_each(strip);
        }
        strip(&mut doc);
    }
    let secs: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut cache = dreist_engine::audio::SampleCache::default();
    let mut g = dreist_engine::Dreist::from_document(&doc, &dreist_formats::model::samples_dir(p), 44100.0, dreist_engine::RenderOptions::default(), &mut cache).unwrap();
    g.start();
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    let mut out = Vec::new();
    for _ in 0..secs * 44100 / 512 {
        g.process(&mut l, &mut r);
        for i in 0..512 {
            out.extend_from_slice(&l[i].to_le_bytes());
            out.extend_from_slice(&r[i].to_le_bytes());
        }
    }
    std::fs::write(&a[2], out).unwrap();
}
