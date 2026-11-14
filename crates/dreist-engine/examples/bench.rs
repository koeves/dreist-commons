//! Render a project offline and report how much of real time the engine
//! needs: overall and for the worst one-second window.
//!
//! `cargo run --release -p dreist-engine --example bench -- song.geist [secs] [rate] [block]`
//! (defaults: 180 s, 48000 Hz, 512-sample blocks). Below 100 % fits in real
//! time on one core; the worst window is what a song's busiest part needs.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let Some(path) = a.get(1) else {
        eprintln!("usage: bench song.geist [secs] [rate] [block]");
        std::process::exit(2);
    };
    let p = std::path::Path::new(path);
    let secs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(180);
    let rate: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(48000);
    let block: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    let doc = dreist_formats::Project::load(p).expect("project").raw;
    let mut cache = dreist_engine::audio::SampleCache::default();
    let mut g = dreist_engine::Dreist::from_document(&doc, &dreist_formats::model::samples_dir(p), rate as f32, dreist_engine::RenderOptions::default(), &mut cache).expect("engine");
    g.start();
    let (mut l, mut r) = (vec![0.0f32; block], vec![0.0f32; block]);
    let blocks_per_sec = rate / block;
    let (mut total, mut worst, mut worst_at) = (0.0f64, 0.0f64, 0usize);
    for s in 0..secs {
        let t = std::time::Instant::now();
        for _ in 0..blocks_per_sec {
            g.process(&mut l, &mut r);
        }
        let used = t.elapsed().as_secs_f64() * rate as f64 / (blocks_per_sec * block) as f64;
        total += used;
        if used > worst {
            (worst, worst_at) = (used, s);
        }
    }
    println!("{}: {secs} s at {rate} Hz, {block}-sample blocks", p.file_name().unwrap_or_default().to_string_lossy());
    println!("overall: {:.1} % of real time (one core)", total * 100.0 / secs as f64);
    println!("worst second: {:.1} % at {worst_at} s", worst * 100.0);
}
