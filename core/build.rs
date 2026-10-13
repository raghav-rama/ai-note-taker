// build.rs — Rust build script for notetaker-core
//
// Assumes Cargo.toml declares:
//   [lib]
//   name = "notetaker"
//   crate-type = ["cdylib", "staticlib", "rlib"]
//
//   [build-dependencies]
//   cbindgen = "0.27"
//
//   [features]
//   metal  = ["whisper-rs/metal"]
//   coreml = ["whisper-rs/coreml"]
//   cuda   = ["whisper-rs/cuda"]
//   vulkan = ["whisper-rs/vulkan"]

use std::env;
use std::path::PathBuf;

fn main() {
    // ── Rebuild triggers ────────────────────────────────────────────────
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=WHISPER_LIB_DIR");

    // ── Optional prebuilt whisper library override ──────────────────────
    // Set WHISPER_LIB_DIR to skip building whisper from source and link a
    // prebuilt library instead.  Do NOT emit rustc-link-lib here — that is
    // already handled by whisper-rs-sys; adding it again causes duplicate
    // symbol errors.
    if let Ok(lib_dir) = env::var("WHISPER_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", lib_dir);
    }

    // ── Platform-specific link flags ────────────────────────────────────
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=Accelerate");
            println!("cargo:rustc-link-lib=framework=Security");

            if cfg!(feature = "metal") {
                println!("cargo:rustc-link-lib=framework=Metal");
                println!("cargo:rustc-link-lib=framework=MetalKit");
            }
            if cfg!(feature = "coreml") {
                println!("cargo:rustc-link-lib=framework=CoreML");
            }
        }
        "ios" => {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=Accelerate");
            println!("cargo:rustc-link-lib=framework=Security");

            if cfg!(feature = "metal") {
                println!("cargo:rustc-link-lib=framework=Metal");
                println!("cargo:rustc-link-lib=framework=MetalKit");
            }
            if cfg!(feature = "coreml") {
                println!("cargo:rustc-link-lib=framework=CoreML");
            }
        }
        "android" => {
            // Android requires explicit linking to the log library and
            // the C++ shared runtime.  The APK must bundle libc++_shared.so
            // alongside libnotetaker.so (cargo-ndk handles this).
            println!("cargo:rustc-link-lib=log");
            println!("cargo:rustc-link-lib=c++_shared");
        }
        "windows" => {
            // WASAPI and window affinity APIs come from these system libs.
            println!("cargo:rustc-link-lib=ole32");
            println!("cargo:rustc-link-lib=user32");
        }
        _ => {
            // Linux and others: no extra link flags needed.
            // PipeWire / PulseAudio are loaded at runtime via pkg-config
            // dependencies declared in Cargo.toml.
        }
    }

    // ── Generate C header via cbindgen ───────────────────────────────────
    generate_c_header();
}

fn generate_c_header() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(&crate_dir).join("include");

    // Attempt to create the output directory; ignore errors (may already exist).
    let _ = std::fs::create_dir_all(&out_dir);

    let config_path = PathBuf::from(&crate_dir).join("cbindgen.toml");
    let config = if config_path.exists() {
        cbindgen::Config::from_file(&config_path).unwrap_or_else(|e| {
            println!("cargo:warning=Failed to read cbindgen.toml: {e}");
            cbindgen::Config::default()
        })
    } else {
        println!("cargo:warning=cbindgen.toml not found, using defaults");
        cbindgen::Config::default()
    };

    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            bindings.write_to_file(out_dir.join("notetaker.h"));
        }
        Err(e) => {
            // Warn but do not fail — allows `cargo check` to pass even when
            // FFI signatures are temporarily broken during development.
            println!("cargo:warning=cbindgen failed to generate header: {e}");
        }
    }
}
