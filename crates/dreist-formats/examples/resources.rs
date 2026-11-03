//! Print where the bundled resources resolve to (diagnostics).
fn main() {
    println!("{}", dreist_formats::paths::resources_dir().display());
}
