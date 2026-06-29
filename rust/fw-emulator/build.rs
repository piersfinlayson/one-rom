// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use std::{env, path::PathBuf, process::Command};

// Not defaults — used in error messages to show the caller what to set.
const EXAMPLE_CONFIG: &str = "onerom-config/test-0.json";
const EXAMPLE_BOARD: &str = "fire-24-a";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Structure: project/rust/onerom-fw-emulator  →  two parents  →  project/
    let project_root = manifest_dir
        .parent()
        .expect("missing rust/ parent")
        .parent()
        .expect("missing project root")
        .to_path_buf();

    let c_root = project_root.join("firmware");

    let config = env::var("CONFIG")
        .unwrap_or_else(|_| panic!("CONFIG must be set (e.g. CONFIG={EXAMPLE_CONFIG})"));
    let board = env::var("BOARD")
        .unwrap_or_else(|_| panic!("BOARD must be set (e.g. BOARD={EXAMPLE_BOARD})"));

    // BASE_DIR is the project root used to resolve relative CONFIG paths and
    // ROM image files.  Defaults to the computed project_root so the Makefile
    // shell-script build works without change; set it explicitly when invoking
    // cargo from a different directory (e.g. BASE_DIR=$(realpath ../..) from
    // fw-tester/).
    let base_dir = if let Ok(bd) = env::var("BASE_DIR") {
        std::fs::canonicalize(&bd)
            .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{}': {}", bd, e))
    } else {
        project_root.clone()
    };

    // Resolve CONFIG to a canonical absolute path relative to base_dir.
    // Absolute paths are used as-is.
    let config_abs = {
        let p = PathBuf::from(&config);
        if p.is_absolute() {
            std::fs::canonicalize(&p)
                .unwrap_or_else(|e| panic!("Cannot find CONFIG '{}': {}", config, e))
        } else {
            std::fs::canonicalize(base_dir.join(&p)).unwrap_or_else(|e| {
                panic!(
                    "Cannot find CONFIG '{}' relative to BASE_DIR '{}': {}",
                    config,
                    base_dir.display(),
                    e
                )
            })
        }
    };

    // Re-run build.rs if these env vars change.
    println!("cargo:rerun-if-env-changed=CONFIG");
    println!("cargo:rerun-if-env-changed=BOARD");
    println!("cargo:rerun-if-env-changed=BASE_DIR");

    // ── C build ──────────────────────────────────────────────────────────────

    println!(
        "cargo:rerun-if-changed={}",
        project_root.join("Makefile").display()
    );
    println!("cargo:rerun-if-changed={}", c_root.display());
    println!("cargo:rerun-if-changed={}", config_abs.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/wrapper.h").display()
    );

    // Clean the C library if CONFIG or BOARD has changed since the last build.
    // The Makefile has no visibility into these variables, so we track them
    // ourselves via a stamp file in the build output directory.
    let stamp_path = c_root.join("build-test/.build-config");
    let stamp = format!("CONFIG={}\nBOARD={board}\n", config_abs.display());
    let needs_clean = std::fs::read_to_string(&stamp_path)
        .map(|s| s != stamp)
        .unwrap_or(true);

    if needs_clean {
        let _ = Command::new("make")
            .arg("-C")
            .arg(&project_root)
            .arg("clean-libonerom-test")
            .env("CONFIG", &config_abs)
            .env("BOARD", &board)
            .status()
            .expect("could not run make clean-libonerom-test");
    }

    let status = Command::new("make")
        .arg("-C")
        .arg(&project_root)
        .arg("libonerom-test")
        .env("CONFIG", &config_abs)
        .env("BOARD", &board)
        .status()
        .expect("could not run make — is it on PATH?");
    assert!(
        status.success(),
        "make libonerom-test failed (CONFIG={} BOARD={board})",
        config_abs.display()
    );

    std::fs::write(&stamp_path, &stamp).expect("could not write build stamp");

    // ── Linking ──────────────────────────────────────────────────────────────

    println!(
        "cargo:rustc-link-search=native={}",
        c_root.join("build-test").display()
    );
    println!("cargo:rustc-link-lib=static=onerom-test");
    println!("cargo:rustc-link-lib=m");

    // ── bindgen ──────────────────────────────────────────────────────────────

    let bindings = bindgen::Builder::default()
        .header(manifest_dir.join("src/wrapper.h").to_str().unwrap())
        .clang_arg(format!("-I{}", c_root.join("include").display()))
        .clang_arg(format!("-I{}", c_root.join("generated").display()))
        .clang_arg(format!("-I{}", c_root.join("include/test").display()))
        .clang_arg(format!("-I{}", c_root.join("apio/include").display()))
        .clang_arg(format!("-I{}", c_root.join("epio/include").display()))
        .clang_arg(format!("-I{}", c_root.join("ora").display()))
        .clang_arg("-DTEST_BUILD=1".to_string())
        .clang_arg("-DDEBUG_LOGGING=1".to_string())
        .allowlist_function("firmware_main")
        .allowlist_function("epio_from_apio")
        .allowlist_function("epio_get_sram_ptr")
        .allowlist_function("epio_update_from_apio")
        .allowlist_function("epio_drive_gpios_ext")
        .allowlist_function("epio_read_pin_states")
        .allowlist_function("epio_step_cycles")
        .allowlist_function("epio_free")
        .allowlist_function("epio_read_driven_pins")
        .allowlist_function("epio_read_pull_up_pins")
        .allowlist_function("epio_read_pull_down_pins")
        .allowlist_function("set_host_sram_ptr")
        .allowlist_function("stub_set_sel_image")
        .allowlist_function("stub_set_rp_variant")
        .allowlist_function("ffi_limp_mode")
        .allowlist_function("ffi_pios_enabled")
        .allowlist_function("ffi_epio_setup_sram")
        .allowlist_function("ffi_epio_setup_dma_chain")
        .allowlist_function("ffi_set_logging")
        .allowlist_function("ora_fn_lookup")
        .allowlist_type("ora_result_t")
        .allowlist_type("ora_.*_fn_t")
        .allowlist_function("ffi_runtime_info_ptr")
        .allowlist_function("ffi_runtime_info_size")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindgen failed to generate bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("could not write bindings.rs");
}
