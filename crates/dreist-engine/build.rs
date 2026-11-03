//! Builds the x86_64 REX helper (`rex/rex_helper.c`) with the system
//! clang; it is embedded in the engine and extracted at run time. Without
//! clang the helper is empty and REX files do not load.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dreist-rex-helper");
    println!("cargo:rerun-if-changed=rex/rex_helper.c");
    let ok = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && Command::new("clang").args(["-arch", "x86_64", "-O2", "-framework", "CoreFoundation", "rex/rex_helper.c", "-o"]).arg(&out).status().is_ok_and(|s| s.success());
    if !ok {
        std::fs::write(&out, b"").unwrap();
    }
}
