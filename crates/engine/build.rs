//! Copies the vendored FFmpeg DLLs next to the build outputs.
//!
//! FFmpeg is linked as libraries, so the resulting binaries need the matching
//! DLLs at runtime. Windows searches the executable's own directory first, so
//! we place them beside the binaries (and in `examples/` + `deps/`) instead of
//! relying on PATH.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let prefix = env::var("FFMPEG_DIR")
        .or_else(|_| env::var("FFMPEG_PREFIX"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("../../third_party/ffmpeg"));
    let bin = prefix.join("bin");

    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-env-changed=FFMPEG_PREFIX");
    println!("cargo:rerun-if-changed={}", bin.display());

    if !bin.is_dir() {
        println!(
            "cargo:warning=FFmpeg not found at {} — run scripts/fetch-ffmpeg.ps1",
            bin.display()
        );
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    // OUT_DIR = <target>/<profile>/build/<pkg>-<hash>/out
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };

    for dest in [
        profile_dir.to_path_buf(),
        profile_dir.join("examples"),
        profile_dir.join("deps"),
    ] {
        copy_dlls(&bin, &dest);
    }
}

fn copy_dlls(from: &Path, to: &Path) {
    let Ok(entries) = fs::read_dir(from) else {
        return;
    };
    if fs::create_dir_all(to).is_err() {
        return;
    }

    for entry in entries.flatten() {
        let path = entry.path();
        let is_dll = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"));
        if !is_dll {
            continue;
        }
        if let Some(name) = path.file_name() {
            let _ = fs::copy(&path, to.join(name));
        }
    }
}
