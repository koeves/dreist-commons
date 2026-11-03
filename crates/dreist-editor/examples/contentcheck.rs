//! Load every factory preset and kit the way the browser does and report
//! layers whose samples cannot be found: `contentcheck [content dir]`.

fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() && p.extension().is_none_or(|x| x != "samples") {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "geist" || x == "geistkit" || x == "geistengine" || x == "geistbank") {
            out.push(p);
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).map(std::path::PathBuf::from).unwrap_or_else(dreist_editor::browser::content_dir);
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut bad = 0;
    for f in &files {
        let mut model = if f.extension().is_some_and(|x| x == dreist_formats::profile::LEGACY.project) {
            let doc = dreist_formats::Project::load(f).expect("project").raw;
            let mut m = dreist_editor::Model::new(doc);
            m.samples = Some(dreist_formats::model::samples_dir(f));
            m
        } else if f.extension().is_some_and(|x| x == "geistbank") {
            let mut m = dreist_editor::Model::new(dreist_editor::project::new_document());
            if !dreist_editor::project::load_part(&mut m.doc, None, dreist_editor::project::Part::Bank(0), f) {
                println!("{}: bank did not load", f.display());
                bad += 1;
                continue;
            }
            let on: usize = (0..24).map(|k| m.pattern(0, k).tracks.iter().map(|t| t.steps.iter().filter(|s| s.on).count()).sum::<usize>()).sum();
            println!("{}: {on} steps on", f.file_name().unwrap().to_string_lossy());
            if on == 0 {
                bad += 1;
            }
            continue;
        } else if f.extension().is_some_and(|x| x == "geistengine") {
            let mut m = dreist_editor::Model::new(dreist_editor::project::new_document());
            if !dreist_editor::project::load_part(&mut m.doc, None, dreist_editor::project::Part::Engine(0), f) {
                println!("{}: engine did not load", f.display());
                bad += 1;
                continue;
            }
            m
        } else {
            let mut m = dreist_editor::Model::new(dreist_editor::project::new_document());
            if !dreist_editor::browser::load_kit_into_doc(&mut m, f) {
                println!("{}: kit did not load", f.display());
                bad += 1;
                continue;
            }
            m
        };
        let miss = dreist_editor::filefind::missing_files(&model);
        let layers: usize = (0..8).map(|e| (0..64).map(|p| (0..8).filter(|&l| model.layer(e, p, l).valid).count()).sum::<usize>()).sum();
        println!("{}: {layers} layers, {} missing", f.file_name().unwrap().to_string_lossy(), miss.len());
        for m in miss.iter().take(3) {
            println!("    {}", m.path.display());
        }
        bad += miss.len();
        let _ = &mut model;
    }
    println!("{} files, {bad} problems", files.len());
}
