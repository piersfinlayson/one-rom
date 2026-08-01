// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration tests for the standalone image transforms:
//! `onerom image swap-bytes` and `onerom image deinterleave`.
//!
//! These subcommands and the `--slot transform=` key share a single
//! implementation in `onerom-gen`, so what is tested here is the plumbing —
//! argument handling, file I/O and error reporting — rather than the transform
//! arithmetic, which is covered by that crate's unit tests.

mod common;
use common::{fails, onerom};

use std::path::Path;

/// Two 32-bit groups' worth of pattern, repeated: every byte of a group is
/// distinguishable, so any mis-selection is visible.
fn interleaved_32bit(groups: usize) -> Vec<u8> {
    [0xA0u8, 0xA1, 0xB0, 0xB1]
        .iter()
        .cycle()
        .take(groups * 4)
        .copied()
        .collect()
}

fn swap_bytes(input: &Path, output: &Path) {
    let out = onerom()
        .args([
            "image",
            "swap-bytes",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "swap-bytes failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn deinterleave(input: &Path, output: &Path, offset: &str, stride: &str, bytes: Option<&str>) {
    let mut cmd = onerom();
    cmd.args([
        "image",
        "deinterleave",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--offset",
        offset,
        "--stride",
        stride,
    ]);
    if let Some(bytes) = bytes {
        cmd.args(["--bytes", bytes]);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "deinterleave failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn swap_bytes_reverses_pairs_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let data = interleaved_32bit(256);
    let src = dir.path().join("rom.bin");
    let swapped = dir.path().join("swapped.bin");
    let back = dir.path().join("back.bin");
    std::fs::write(&src, &data).unwrap();

    swap_bytes(&src, &swapped);
    let got = std::fs::read(&swapped).unwrap();
    assert_eq!(got.len(), data.len());
    assert_eq!(&got[..4], &[0xA1, 0xA0, 0xB1, 0xB0]);

    // Swapping twice is the identity, so the original file comes back.
    swap_bytes(&swapped, &back);
    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn deinterleave_extracts_a_byte_lane() {
    let dir = tempfile::tempdir().unwrap();
    let data = interleaved_32bit(256);
    let src = dir.path().join("rom32.bin");
    let out = dir.path().join("lane.bin");
    std::fs::write(&src, &data).unwrap();

    // Byte 2 of every 4 is always 0xB0.
    deinterleave(&src, &out, "2", "4", None);
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), data.len() / 4);
    assert!(got.iter().all(|&b| b == 0xB0), "unexpected lane contents");
}

#[test]
fn deinterleave_extracts_a_16_bit_half() {
    let dir = tempfile::tempdir().unwrap();
    let data = interleaved_32bit(256);
    let src = dir.path().join("rom32.bin");
    let out = dir.path().join("half.bin");
    std::fs::write(&src, &data).unwrap();

    // The upper 16-bit lane of each 32-bit group is [0xB0, 0xB1].
    deinterleave(&src, &out, "1", "2", Some("2"));
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), data.len() / 2);
    assert_eq!(&got[..4], &[0xB0, 0xB1, 0xB0, 0xB1]);
}

#[test]
fn deinterleave_accepts_the_unit_alias_for_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let data = interleaved_32bit(16);
    let src = dir.path().join("rom32.bin");
    std::fs::write(&src, &data).unwrap();

    // `--unit` was the original spelling; it stays accepted so existing
    // invocations keep working, and must mean exactly what `--bytes` means.
    let mut with_bytes = dir.path().join("bytes.bin");
    deinterleave(&src, &with_bytes, "1", "2", Some("2"));
    let expected = std::fs::read(&with_bytes).unwrap();

    with_bytes = dir.path().join("unit.bin");
    let out = onerom()
        .args([
            "image",
            "deinterleave",
            "--input",
            src.to_str().unwrap(),
            "--output",
            with_bytes.to_str().unwrap(),
            "--offset",
            "1",
            "--stride",
            "2",
            "--unit",
            "2",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "--unit alias rejected: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&with_bytes).unwrap(), expected);
}

#[test]
fn deinterleave_then_swap_bytes_composes() {
    let dir = tempfile::tempdir().unwrap();
    let data = interleaved_32bit(256);
    let src = dir.path().join("rom32.bin");
    let half = dir.path().join("half.bin");
    let out = dir.path().join("out.bin");
    std::fs::write(&src, &data).unwrap();

    // The same recipe as `transform=deinterleave:1/2/2+swap_bytes`, run as two
    // standalone steps.
    deinterleave(&src, &half, "1", "2", Some("2"));
    swap_bytes(&half, &out);

    let got = std::fs::read(&out).unwrap();
    assert_eq!(got.len(), data.len() / 2);
    assert_eq!(&got[..4], &[0xB1, 0xB0, 0xB1, 0xB0]);
}

#[test]
fn swap_bytes_rejects_an_odd_length_image() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("odd.bin");
    std::fs::write(&src, [0u8; 5]).unwrap();

    fails(onerom().args([
        "image",
        "swap-bytes",
        "--input",
        src.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
    ]));
}

#[test]
fn deinterleave_rejects_bad_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("rom.bin");
    std::fs::write(&src, interleaved_32bit(16)).unwrap();
    let out = dir.path().join("out.bin");

    // stride below 2 selects everything, and an offset outside the stride
    // selects nothing; both are rejected before the file is even read.
    for (offset, stride) in [("0", "1"), ("4", "4")] {
        fails(onerom().args([
            "image",
            "deinterleave",
            "--input",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--offset",
            offset,
            "--stride",
            stride,
        ]));
    }
}

#[test]
fn deinterleave_rejects_a_ragged_image() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("ragged.bin");
    // 10 bytes is not a whole number of 4-byte groups.
    std::fs::write(&src, [0u8; 10]).unwrap();

    fails(onerom().args([
        "image",
        "deinterleave",
        "--input",
        src.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
        "--offset",
        "0",
        "--stride",
        "4",
    ]));
}
