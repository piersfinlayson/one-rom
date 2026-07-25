// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration tests for `onerom image convert` (binary <-> Intel HEX).

mod common;
use common::{fails, onerom};

use std::path::Path;

/// A deterministic, non-trivial test image.
fn sample(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(37) ^ 0x5A) as u8)
        .collect()
}

fn convert(from: &str, to: &str, input: &Path, output: &Path, load_address: Option<&str>) {
    let mut cmd = onerom();
    cmd.args([
        "image",
        "convert",
        "--from",
        from,
        "--to",
        to,
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    if let Some(la) = load_address {
        cmd.args(["--load-address", la]);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "convert {from}->{to} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn binary_ihex_binary_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(8192);
    let bin = dir.path().join("rom.bin");
    let hex = dir.path().join("rom.hex");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("binary", "ihex", &bin, &hex, None);
    convert("ihex", "binary", &hex, &back, None);

    assert_eq!(std::fs::read(&back).unwrap(), data, "round-trip mismatch");
}

#[test]
fn round_trips_with_load_address() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(4096);
    let bin = dir.path().join("rom.bin");
    let hex = dir.path().join("rom.hex");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    // Emit Intel HEX addressed at 0xE000, then read it back with the same
    // load address; the offset must cancel out to the original image.
    convert("binary", "ihex", &bin, &hex, Some("$E000"));
    convert("ihex", "binary", &hex, &back, Some("0xE000"));

    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn format_aliases_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(64);
    let bin = dir.path().join("rom.bin");
    let hex = dir.path().join("rom.hex");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("raw", "intel-hex", &bin, &hex, None);
    convert("ihex", "bin", &hex, &back, None);

    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn load_address_without_ihex_fails() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("rom.bin");
    std::fs::write(&bin, sample(16)).unwrap();

    fails(onerom().args([
        "image",
        "convert",
        "--from",
        "binary",
        "--to",
        "binary",
        "--input",
        bin.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
        "--load-address",
        "0x10",
    ]));
}

#[test]
fn decoding_non_ihex_fails() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("rom.bin");
    std::fs::write(&bin, sample(16)).unwrap();

    fails(onerom().args([
        "image",
        "convert",
        "--from",
        "ihex",
        "--to",
        "binary",
        "--input",
        bin.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
    ]));
}
