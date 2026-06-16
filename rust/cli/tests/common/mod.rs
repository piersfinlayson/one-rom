// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#![allow(dead_code)]

use std::process::Command;

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

pub fn project_root() -> std::path::PathBuf {
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

pub fn build_config_test(config: &str, pins: u8) {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("firmware.bin");
    let board = representative_board(pins);
    let status = onerom()
        .current_dir(project_root())
        .args([
            "firmware",
            "build",
            "--board",
            board,
            "--config-file",
            config,
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "build failed for {config} on {board}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(out.exists(), "no output file for {config}");
    assert!(
        out.metadata().unwrap().len() > 0,
        "empty output for {config}"
    );

    // inspect the built firmware
    let inspect = onerom()
        .args(["firmware", "inspect", "--firmware", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "inspect failed for firmware built from {config}: {}",
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
