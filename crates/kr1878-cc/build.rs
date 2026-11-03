// Builds the vendored chibicc frontend plus the TESEI backend into a static
// library that the Rust wrapper links against.
//
// The C compiler is the *host* compiler: the resulting binary runs on the
// host and emits TESEI assembly, so no cross-compilation flags are needed.
// A C toolchain is therefore required to build this crate — it is present on
// macOS (clang) and on the CI images (gcc, via build-essential).

use std::env;
use std::path::PathBuf;
use std::process::Command;

const C_SOURCES: &[&str] = &[
    "tokenize.c",
    "preprocess.c",
    "parse.c",
    "type.c",
    "unicode.c",
    "hashmap.c",
    "strings.c",
    "tesei_codegen.c",
    "tesei_api.c",
];

/// Finds a tool by probing it with `--version` (works for cc, gcc and clang).
fn find_tool(names: &[&str]) -> String {
    if let Ok(cc) = env::var("CC") {
        if !cc.is_empty() {
            return cc;
        }
    }
    for name in names {
        if Command::new(name)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return (*name).to_string();
        }
    }
    panic!("no C compiler found; tried CC, {names:?}");
}

/// The archiver: BSD `ar` has no `--version`, so it is not probed — the
/// archiving step reports a failure itself if the tool is missing.
fn find_ar() -> String {
    env::var("AR").unwrap_or_else(|_| "ar".to_string())
}

fn main() {
    let vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/chibicc");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    for src in C_SOURCES {
        println!("cargo:rerun-if-changed={}", vendor.join(src).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("chibicc.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("tesei_hook.h").display()
    );

    let cc = find_tool(&["cc", "gcc", "clang"]);
    let ar = find_ar();

    let mut objects = Vec::new();
    for src in C_SOURCES {
        let obj = out_dir.join(format!("{}.o", src.trim_end_matches(".c")));
        let status = Command::new(&cc)
            .args(["-c", "-O2", "-std=gnu11", "-fno-strict-aliasing"])
            .args(["-Wno-unused-function", "-Wno-unused-variable"])
            .arg("-I")
            .arg(&vendor)
            .arg(vendor.join(src))
            .arg("-o")
            .arg(&obj)
            .status()
            .unwrap_or_else(|e| panic!("failed to run {cc}: {e}"));
        assert!(status.success(), "failed to compile {src} with {cc}");
        objects.push(obj);
    }

    let lib = out_dir.join("libkr1878cc.a");
    let _ = std::fs::remove_file(&lib);
    let status = Command::new(&ar)
        .arg("rcs")
        .arg(&lib)
        .args(&objects)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {ar}: {e}"));
    assert!(status.success(), "failed to archive the objects with {ar}");

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=kr1878cc");
}
