//! Render a project offline and report how much of real time the engine
//! needs: overall, for the worst one-second window and for single blocks
//! (a block over 100 % of its own duration is a dropout at that period).
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
    let block_secs = block as f64 / rate as f64;
    // Each block's time as a share of its own duration.
    let mut blocks: Vec<(f64, f64)> = Vec::with_capacity(secs * blocks_per_sec);
    for s in 0..secs {
        let t = std::time::Instant::now();
        for k in 0..blocks_per_sec {
            let tb = std::time::Instant::now();
            g.process(&mut l, &mut r);
            blocks.push((tb.elapsed().as_secs_f64() / block_secs, s as f64 + k as f64 * block_secs));
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
    let (wb, wb_at) = blocks.iter().copied().fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
    let mut sorted: Vec<f64> = blocks.iter().map(|b| b.0).collect();
    sorted.sort_by(f64::total_cmp);
    let pct = |q: f64| sorted.get(((sorted.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(0.0) * 100.0;
    println!("blocks: worst {:.1} % at {wb_at:.2} s, 99.9th percentile {:.1} %, 99th {:.1} %", wb * 100.0, pct(0.999), pct(0.99));
    for limit in [0.5, 0.75, 1.0] {
        println!("blocks over {:.0} %: {}", limit * 100.0, sorted.iter().filter(|&&b| b > limit).count());
    }
}
