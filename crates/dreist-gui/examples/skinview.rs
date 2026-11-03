//! Open the root skin in a window: `cargo run -p dreist-gui --features window --example skinview [theme]`.

use dreist_gui::{Skin, app::App};

fn main() {
    let theme = std::env::args().nth(1).unwrap_or_else(|| dreist_gui::skin::default_theme().into());
    let mut skin = Skin::load(&Skin::resources_dir(), &theme).expect("skin");
    let root = skin.load_skin("RootSkin.skin").expect("root skin");
    let app = App::new(skin, root);
    dreist_gui::window::open_blocking(
        dreist_gui::window::Hooked(
            app,
            Box::new(|_, e| {
                if let Some(e) = e {
                    println!("{e:?}")
                }
            }),
        ),
        "Skin viewer",
    );
}
