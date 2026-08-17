// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Builds the USB plugin natively and archives it with the host shim.
//!
//! The plugin objects come from the plugin's own Makefile (`make host`), so the
//! flags that must match the firmware's native test build live next to the ARM
//! build rather than being restated here.  The shim is compiled here with the
//! same flag set — see `SHIM_FLAGS`.

use std::{env, path::PathBuf, process::Command};

/// Plugin directory, relative to the project root, and the objects `make host`
/// leaves behind.
const PLUGIN_DIR: &str = "plugins/system/usb";
const PLUGIN_OBJS: &[&str] = &[
    "build-host/usb_descriptors.o",
    "build-host/usb_picobootx.o",
    "build-host/usb_rom.o",
    "build-host/usb_led.o",
    "build-host/usb_gpio.o",
    "build-host/usb_log.o",
    "build-host/usb_main.o",
];

/// Flags the shim is compiled with.  These must stay in step with the plugin's
/// `host` target and with `firmware/test.mk`: all the objects link together, so
/// `-fshort-enums` and `-DTEST_BUILD=1` have to agree across every one of them
/// or the C types they exchange differ in width or layout.
const SHIM_FLAGS: &[&str] = &[
    "-DORA_HOST_TEST=1",
    "-DTEST_BUILD=1",
    "-fshort-enums",
    "-O1",
    "-g",
    "-Wall",
    "-Wextra",
    "-Werror",
    "-std=c11",
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
        plugin_dir.join("include").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        plugin_dir.join("Makefile").display()
    );
    println!("cargo:rerun-if-changed={}", firmware.join("ora").display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("csrc/usb_shim.c").display()
    );

    // ── Plugin objects ───────────────────────────────────────────────────────

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
    //
    // The shim stands in for tinyusb and picobootx, so it needs their headers
    // for the types it exchanges with the plugin.  Both are cloned by the
    // plugin's Makefile, which `make host` above has already run.

    let shim_obj = out_dir.join("usb_shim.o");
    let cc = env::var("HOST_CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .args(SHIM_FLAGS)
        .arg(format!("-I{}", firmware.join("include").display()))
        .arg(format!("-I{}", firmware.join("generated").display()))
        .arg(format!("-I{}", firmware.join("ora").display()))
        .arg(format!("-I{}", plugin_dir.display()))
        .arg(format!("-I{}", plugin_dir.join("include").display()))
        .arg(format!("-I{}", plugin_dir.join("src").display()))
        .arg(format!(
            "-I{}",
            plugin_dir.join("picobootx/include").display()
        ))
        .arg(format!("-I{}", plugin_dir.join("tinyusb/src").display()))
        .arg(format!(
            "-I{}",
            plugin_dir.join("tinyusb/src/common").display()
        ))
        .arg(format!(
            "-I{}",
            plugin_dir.join("tinyusb/src/device").display()
        ))
        .arg(format!(
            "-I{}",
            plugin_dir.join("tinyusb/src/class/cdc").display()
        ))
        .arg("-c")
        .arg(manifest_dir.join("csrc/usb_shim.c"))
        .arg("-o")
        .arg(&shim_obj)
        .status()
        .expect("could not run the host C compiler");
    assert!(status.success(), "compiling usb_shim.c failed");

    // ── Archive ──────────────────────────────────────────────────────────────

    let archive = out_dir.join("libonerom-usb-host.a");
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
    assert!(status.success(), "archiving the plugin objects failed");

    // ── Linking ──────────────────────────────────────────────────────────────

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=onerom-usb-host");

    // The plugin and shim call into the firmware test library, so it must
    // appear *after* this archive on the link line — GNU ld only pulls archive
    // members that resolve an already-pending reference.  onerom-fw-emulator
    // emits it too, but as a dependency its flags land first, which is the
    // wrong order.
    println!(
        "cargo:rustc-link-search=native={}",
        firmware.join("build-test").display()
    );
    println!("cargo:rustc-link-lib=static=onerom-test");
}
