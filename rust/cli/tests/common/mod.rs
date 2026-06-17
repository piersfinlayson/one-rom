// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub fn onerom() -> Command {
    Command::new(env!("CARGO_BIN_EXE_onerom"))
}

pub fn succeeds(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn fails(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(!out.status.success(), "expected failure but exited 0");
}

pub fn project_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

pub fn representative_board(pins: u8) -> &'static str {
    match pins {
        24 => "fire-24-e",
        28 => "fire-28-c",
        32 => "fire-32-a",
        40 => "fire-40-a",
        _ => panic!("no representative board for {pins}-pin"),
    }
}

pub const FIXED_VERSION: &str = "v0.6.13";

pub enum FirmwareVersion {
    Fixed(&'static str),
    Current,
}

// Parsed version + path to built base firmware.  Initialised once on first
// use of FirmwareVersion::Current; shared across all parallel test threads.
static CURRENT_FIRMWARE: OnceLock<(String, PathBuf)> = OnceLock::new();

fn current_firmware() -> &'static (String, PathBuf) {
    CURRENT_FIRMWARE.get_or_init(|| {
        // Parse VERSION_MAJOR / VERSION_MINOR / VERSION_PATCH from the
        // Makefile at the repo root (== project_root()).
        let root = project_root();
        let makefile = root.join("Makefile");
        let content = std::fs::read_to_string(&makefile)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", makefile.display()));

        let mut major: Option<String> = None;
        let mut minor: Option<String> = None;
        let mut patch: Option<String> = None;

        for line in content.lines() {
            if let Some(v) = line.strip_prefix("VERSION_MAJOR :=") {
                major = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("VERSION_MINOR :=") {
                minor = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("VERSION_PATCH :=") {
                patch = Some(v.trim().to_string());
            }
        }

        let version = format!(
            "v{}.{}.{}",
            major.expect("VERSION_MAJOR not found in Makefile"),
            minor.expect("VERSION_MINOR not found in Makefile"),
            patch.expect("VERSION_PATCH not found in Makefile"),
        );

        // Build the base firmware.
        let status = Command::new("make")
            .arg("firmware")
            .current_dir(&root)
            .status()
            .expect("failed to spawn make firmware");
        assert!(status.success(), "make firmware failed");

        let fw_path = root.join("firmware/build/onerom-rp235x.bin");
        assert!(
            fw_path.exists(),
            "base firmware not found at {} after make firmware",
            fw_path.display()
        );

        (version, fw_path)
    })
}

pub fn build_config_test(config: &str, pins: u8, version: FirmwareVersion) {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("firmware.bin");
    let board = representative_board(pins);

    // version_label is only used in assertion messages.
    // For Fixed: pass --version to the CLI.
    // For Current: pass --base-firmware only; version is auto-parsed from the binary.
    let (version_label, base_fw): (String, Option<&PathBuf>) = match version {
        FirmwareVersion::Fixed(v) => (v.to_string(), None),
        FirmwareVersion::Current => {
            let (v, path) = current_firmware();
            (format!("current ({})", v), Some(path))
        }
    };

    let mut cmd = onerom();
    cmd.current_dir(project_root()).args([
        "firmware",
        "build",
        "--board",
        board,
        "--config-file",
        config,
        "--output",
        out.to_str().unwrap(),
    ]);
    if let Some(fw) = base_fw {
        cmd.args(["--base-firmware", fw.to_str().unwrap()]);
    } else {
        cmd.args(["--version", &version_label]);
    }

    let result = cmd.output().unwrap();
    assert!(
        result.status.success(),
        "build failed for {config} on {board} ({version_label}): {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.exists(), "no output file for {config}");
    assert!(
        out.metadata().unwrap().len() > 0,
        "empty output for {config}"
    );

    let inspect = onerom()
        .args(["firmware", "inspect", "--firmware", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "inspect failed for firmware built from {config} ({version_label}): {}",
        String::from_utf8_lossy(&inspect.stderr)
    );
}

pub fn slot(
    file: &str,
    chip_type: &str,
    cs: &[(&str, &str)],
    size_handling: Option<&str>,
) -> String {
    let mut spec = format!("file=images/test/{file},type={chip_type}");
    for (name, polarity) in cs {
        spec.push_str(&format!(",{name}={polarity}"));
    }
    if let Some(sh) = size_handling {
        spec.push_str(&format!(",size_handling={sh}"));
    }
    spec
}

pub fn build_slots(board: &str, slots: &[String]) -> std::process::Output {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("firmware.bin");
    let mut cmd = onerom();
    cmd.current_dir(project_root())
        .args(["firmware", "build", "--board", board]);
    for s in slots {
        cmd.args(["--slot", s.as_str()]);
    }
    cmd.args(["--output", out.to_str().unwrap()]);
    cmd.output().unwrap()
}

pub fn slot_succeeds(board: &str, slots: &[String]) {
    let out = build_slots(board, slots);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn slot_fails(board: &str, slots: &[String]) {
    let out = build_slots(board, slots);
    assert!(!out.status.success());
}
