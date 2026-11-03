//! Render a skin file to PNG: `skinshot [skin] [out.png] [theme] [scale]`.

use dreist_gui::{Skin, text::Fonts};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rel = args.get(1).map(String::as_str).unwrap_or("RootSkin.skin");
    let out = args.get(2).map(String::as_str).unwrap_or("skin.png");
    let theme = args.get(3).map(String::as_str).unwrap_or(dreist_gui::skin::default_theme());
    let scale: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut skin = Skin::load(&Skin::resources_dir(), theme).expect("skin");
    let root = skin.load_skin(rel).expect("load skin");
    let mut fonts = Fonts::new(&skin.font_dir());
    let pm = dreist_gui::render_ctrl(&skin, &mut fonts, &root, scale);
    pm.save_png(out).expect("png");
    println!("{} {}x{}", out, pm.width(), pm.height());
}
