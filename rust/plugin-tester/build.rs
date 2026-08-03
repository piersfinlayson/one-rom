// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Builds the plugin under test natively and archives it with the host shim.
//!
//! The plugin object comes from the plugin's own Makefile (`make host`), so the
//! flags that must match the firmware's native test build live next to the ARM
//! build rather than being restated here.  The shim is compiled here with the
//! same flag set — see `SHIM_FLAGS`.

use std::{env, path::PathBuf, process::Command};

/// Plugin directory, relative to the project root, and its Makefile target.
const PLUGIN_DIR: &str = "plugins/user/host-control";
const PLUGIN_OBJS: &[&str] = &["build-host/host_control_main.o", "build-host/flash_erase.o"];

/// Flags the shim is compiled with.  These must stay in step with the plugin's
/// `host` target and with `firmware/test.mk`: all three objects link together,
/// so `-fshort-enums` and `-DTEST_BUILD=1` have to agree across every one of
/// them or the C types they exchange differ in width or layout.
const SHIM_FLAGS: &[&str] = &[
    "-DORA_HOST_TEST=1",
    "-DTEST_BUILD=1",
    "-fshort-enums",
    "-O1",
    "-g",
    "-Wall",
    "-Wextra",
    "-Werror",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let project_root = manifest_dir
        .parent()
        .expect("missing rust/ parent")
        .parent()
        .expect("missing project root")
        .to_path_buf();

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let plugin_dir = project_root.join(PLUGIN_DIR);
    let firmware = project_root.join("firmware");

    println!(
        "cargo:rerun-if-changed={}",
        plugin_dir.join("src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        plugin_dir.join("Makefile").display()
    );
    println!("cargo:rerun-if-changed={}", firmware.join("ora").display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("csrc/host_shim.c").display()
    );

    // ── Plugin object ────────────────────────────────────────────────────────

    let status = Command::new("make")
        .arg("-C")
        .arg(&plugin_dir)
        .arg("host")
        .status()
        .expect("could not run make — is it on PATH?");
    assert!(
        status.success(),
        "make host failed in {}",
        plugin_dir.display()
    );
    let plugin_objs: Vec<PathBuf> = PLUGIN_OBJS.iter().map(|o| plugin_dir.join(o)).collect();

    // ── Shim object ──────────────────────────────────────────────────────────

    let shim_obj = out_dir.join("host_shim.o");
    let cc = env::var("HOST_CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .args(SHIM_FLAGS)
        .arg(format!("-I{}", firmware.join("include").display()))
        .arg(format!("-I{}", firmware.join("generated").display()))
        .arg(format!("-I{}", firmware.join("ora").display()))
        // The shim calls the plugin's own erase routine and shares its
        // bootrom function-pointer types.
        .arg(format!("-I{}", plugin_dir.join("src").display()))
        .arg("-c")
        .arg(manifest_dir.join("csrc/host_shim.c"))
        .arg("-o")
        .arg(&shim_obj)
        .status()
        .expect("could not run the host C compiler");
    assert!(status.success(), "compiling host_shim.c failed");

    // ── Archive ──────────────────────────────────────────────────────────────

    let archive = out_dir.join("libonerom-plugin-host.a");
    let _ = std::fs::remove_file(&archive);
    let status = if cfg!(target_os = "macos") {
        // Same split as firmware/test.mk: the macOS ar cannot produce an
        // archive rustc will accept here, so use libtool.
        Command::new("libtool")
            .arg("-static")
            .arg("-o")
            .arg(&archive)
            .args(&plugin_objs)
            .arg(&shim_obj)
            .status()
    } else {
        Command::new("ar")
            .arg("rcs")
            .arg(&archive)
            .args(&plugin_objs)
            .arg(&shim_obj)
            .status()
    }
    .expect("could not run the archiver");
    assert!(status.success(), "archiving the plugin object failed");

    // ── Linking ──────────────────────────────────────────────────────────────

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=onerom-plugin-host");

    // The plugin and shim call into the firmware test library, so it must
    // appear *after* this archive on the link line — GNU ld only pulls archive
    // members that resolve an already-pending reference.  onerom-fw-emulator
    // emits it too, but as a dependency its flags land first, which is the
    // wrong order.  Naming it again here is harmless (repeat -l of the same
    // archive is fine) and makes the order explicit rather than incidental.
    println!(
        "cargo:rustc-link-search=native={}",
        firmware.join("build-test").display()
    );
    println!("cargo:rustc-link-lib=static=onerom-test");
}
