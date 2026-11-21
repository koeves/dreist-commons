//! Builds the x86_64 REX helper (`rex/rex_helper.c`) with the system
//! clang; it is embedded in the engine and extracted at run time. Without
//! clang the helper is empty and REX files do not load. It targets the
//! apps' oldest macOS (11.0, or `MACOSX_DEPLOYMENT_TARGET`), not the build
//! machine's.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("dreist-rex-helper");
    println!("cargo:rerun-if-changed=rex/rex_helper.c");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let min = std::env::var("MACOSX_DEPLOYMENT_TARGET").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "11.0".into());
    let ok = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && Command::new("clang")
            .args(["-arch", "x86_64", &format!("-mmacosx-version-min={min}"), "-O2", "-framework", "CoreFoundation", "rex/rex_helper.c", "-o"])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        std::fs::write(&out, b"").unwrap();
    }
}
