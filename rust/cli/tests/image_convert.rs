// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration tests for `onerom image convert` (binary <-> Intel HEX <-> S-record).

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

#[test]
fn binary_srec_binary_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(8192);
    let bin = dir.path().join("rom.bin");
    let srec = dir.path().join("rom.s19");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("binary", "srec", &bin, &srec, None);
    convert("srec", "binary", &srec, &back, None);

    assert_eq!(std::fs::read(&back).unwrap(), data, "round-trip mismatch");
}

#[test]
fn srec_round_trips_with_load_address() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(4096);
    let bin = dir.path().join("rom.bin");
    let srec = dir.path().join("rom.s19");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("binary", "srec", &bin, &srec, Some("$E000"));
    convert("srec", "binary", &srec, &back, Some("0xE000"));

    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn ihex_and_srec_convert_between_each_other() {
    // Neither side is binary, so this exercises decode and encode of both
    // formats in a single run.
    let dir = tempfile::tempdir().unwrap();
    let data = sample(1024);
    let bin = dir.path().join("rom.bin");
    let hex = dir.path().join("rom.hex");
    let srec = dir.path().join("rom.s19");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("binary", "ihex", &bin, &hex, None);
    convert("ihex", "srec", &hex, &srec, None);
    convert("srec", "binary", &srec, &back, None);

    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn srec_format_aliases_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let data = sample(64);
    let bin = dir.path().join("rom.bin");
    let srec = dir.path().join("rom.s19");
    let back = dir.path().join("back.bin");
    std::fs::write(&bin, &data).unwrap();

    convert("raw", "s-record", &bin, &srec, None);
    convert("motorola", "bin", &srec, &back, None);

    assert_eq!(std::fs::read(&back).unwrap(), data);
}

#[test]
fn decoding_non_srec_fails() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("rom.bin");
    std::fs::write(&bin, sample(16)).unwrap();

    fails(onerom().args([
        "image",
        "convert",
        "--from",
        "srec",
        "--to",
        "binary",
        "--input",
        bin.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
    ]));
}

#[test]
fn decoding_ihex_as_srec_fails() {
    // The two formats are near neighbours; feeding one to the other's decoder
    // must be a clean error, not a misparse.
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("rom.bin");
    let hex = dir.path().join("rom.hex");
    std::fs::write(&bin, sample(64)).unwrap();
    convert("binary", "ihex", &bin, &hex, None);

    fails(onerom().args([
        "image",
        "convert",
        "--from",
        "srec",
        "--to",
        "binary",
        "--input",
        hex.to_str().unwrap(),
        "--output",
        dir.path().join("out.bin").to_str().unwrap(),
    ]));
}

/// Write `data` out as an S-record file via the CLI, returning its path.
fn make_srec(dir: &Path, data: &[u8], load_address: Option<&str>) -> std::path::PathBuf {
    let bin = dir.join("src.bin");
    let srec = dir.join("src.s19");
    std::fs::write(&bin, data).unwrap();
    convert("binary", "srec", &bin, &srec, load_address);
    srec
}

/// Decoding `srec` text that has been corrupted by `damage` must fail, naming
/// `expected`.
///
/// These are the malformed-input paths a truncated or mangled transfer
/// produces; each is a decoder policy that would otherwise only be exercised by
/// hand.
///
/// The pristine file is converted first, so a `damage` that quietly produced an
/// unreadable path — or a harness that failed for its own reasons — shows up as
/// the control failing rather than as a spurious pass.  The message is then
/// matched, so each case is pinned to the policy it is about and not merely to
/// "something went wrong".
fn corrupt_srec_is_rejected(damage: impl Fn(String) -> String, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let srec = make_srec(dir.path(), &sample(1024), None);
    let text = std::fs::read_to_string(&srec).unwrap();

    // Control: undamaged, the same conversion must succeed.
    convert(
        "srec",
        "binary",
        &srec,
        &dir.path().join("control.bin"),
        None,
    );

    let damaged = dir.path().join("damaged.s19");
    std::fs::write(&damaged, damage(text)).unwrap();

    let out = onerom()
        .args([
            "image",
            "convert",
            "--from",
            "srec",
            "--to",
            "binary",
            "--input",
            damaged.to_str().unwrap(),
            "--output",
            dir.path().join("out.bin").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected failure but exited 0");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(expected), "expected {expected:?} in: {err}");
}

#[test]
fn srec_missing_its_terminator_fails() {
    // What a truncated transfer looks like.
    corrupt_srec_is_rejected(
        |text| {
            text.lines()
                .filter(|l| !l.starts_with("S9"))
                .collect::<Vec<_>>()
                .join("\r\n")
        },
        "missing termination record",
    );
}

#[test]
fn srec_with_a_bad_checksum_fails() {
    // Flip one data digit, leaving the record's checksum stale.
    corrupt_srec_is_rejected(
        |text| {
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            let data = &mut lines[1];
            let byte = data.as_bytes()[10];
            data.replace_range(10..11, if byte == b'0' { "1" } else { "0" });
            lines.join("\r\n")
        },
        "bad checksum",
    );
}

#[test]
fn srec_with_a_wrong_record_count_fails() {
    // Overstate the S5 count, as a file with records dropped from the middle
    // would.  Rebuilds the record so its own checksum stays valid — the count
    // itself is what must be rejected.
    corrupt_srec_is_rejected(
        |text| {
            text.lines()
                .map(|l| {
                    if l.starts_with("S5") {
                        let count: u16 = u16::from_str_radix(&l[4..8], 16)
                            .expect("S5 count is four hex digits")
                            + 1;
                        let csum = !(0x03u8
                            .wrapping_add((count >> 8) as u8)
                            .wrapping_add(count as u8));
                        format!("S503{count:04X}{csum:02X}")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\r\n")
        },
        "record count declares",
    );
}
