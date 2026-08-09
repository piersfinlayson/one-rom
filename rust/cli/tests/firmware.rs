// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use onerom_config::chip::ChipType;
use onerom_config::hw::BOARDS;

mod common;
use common::{
    FIXED_VERSION, FirmwareVersion, V1_VERSION, V2_VERSION, build_config_test,
    build_slots_at_version, fails, onerom, project_root, representative_board, slot, slot_fails,
    slot_succeeds, succeeds,
};

#[test]
fn verbose_with_chips_all_succeeds() {
    succeeds(onerom().args(["--verbose", "firmware", "chips", "--all"]));
}

#[test]
fn log_level_debug_with_chips_all_succeeds() {
    succeeds(onerom().args(["--log-level", "debug", "firmware", "chips", "--all"]));
}

#[test]
fn unrecognised_with_chips_all_succeeds() {
    succeeds(onerom().args(["--unrecognised", "firmware", "chips", "--all"]));
}

#[test]
fn firmware_releases_board_and_all_fails() {
    fails(onerom().args([
        "firmware",
        "releases",
        "--board",
        representative_board(24),
        "--all",
    ]));
}

#[test]
fn firmware_releases_all_succeeds() {
    succeeds(onerom().args(["firmware", "releases", "--all"]));
}

#[test]
fn firmware_download_known_board_succeeds() {
    succeeds(onerom().args(["firmware", "download", "--board", representative_board(24)]));
}

#[test]
fn firmware_help_succeeds() {
    succeeds(onerom().args(["firmware", "--help"]));
}

// firmware subcommand help
#[test]
fn firmware_chips_help_succeeds() {
    succeeds(onerom().args(["firmware", "chips", "--help"]));
}

// clap conflict: --board and --all are mutually exclusive
#[test]
fn firmware_chips_board_and_all_fails() {
    fails(onerom().args([
        "firmware",
        "chips",
        "--board",
        representative_board(24),
        "--all",
    ]));
}

// if chips --all is purely local, test it actually succeeds
#[test]
fn firmware_chips_all_succeeds() {
    succeeds(onerom().args(["firmware", "chips", "--all"]));
}

// clap conflict: --output and --path
#[test]
fn firmware_build_output_and_path_fails() {
    fails(onerom().args(["firmware", "build", "--output", "a.bin", "--path", "/tmp"]));
}

// clap conflict: --config-file and --slot
#[test]
fn firmware_build_config_file_and_slot_fails() {
    fails(onerom().args([
        "firmware",
        "build",
        "--config-file",
        "c64.json",
        "--slot",
        "file=k.bin,type=2364,cs1=active_low",
    ]));
}

// clap conflict: --firmware and --board on inspect
#[test]
fn firmware_inspect_firmware_and_board_fails() {
    fails(onerom().args([
        "firmware",
        "inspect",
        "--firmware",
        "fw.bin",
        "--board",
        representative_board(24),
    ]));
}

#[test]
fn firmware_chips_known_board_succeeds() {
    succeeds(onerom().args(["firmware", "chips", "--board", representative_board(24)]));
}

#[test]
fn firmware_chips_unknown_board_fails() {
    fails(onerom().args(["firmware", "chips", "--board", "not-a-board"]));
}

#[test]
fn firmware_build_with_config_produces_output() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("firmware.bin");
    succeeds(onerom().current_dir(project_root()).args([
        "firmware",
        "build",
        "--board",
        representative_board(24),
        "--config-file",
        "onerom-config/test/24-random-27xx.json",
        "--output",
        out.to_str().unwrap(),
    ]));
    assert!(out.exists());
    assert!(out.metadata().unwrap().len() > 0);
}

#[test]
fn firmware_build_then_inspect_succeeds() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("firmware.bin");
    succeeds(onerom().current_dir(project_root()).args([
        "firmware",
        "build",
        "--board",
        representative_board(24),
        "--config-file",
        "onerom-config/test/24-random-27xx.json",
        "--output",
        out.to_str().unwrap(),
    ]));
    succeeds(onerom().args(["firmware", "inspect", "--firmware", out.to_str().unwrap()]));
}

#[test]
fn firmware_chips_all_output_contains_known_chips() {
    let out = onerom()
        .args(["firmware", "chips", "--all"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for name in onerom_config::chip::CHIP_TYPE_NAMES_24_PIN {
        assert!(stdout.contains(name), "missing 24-pin chip: {name}");
    }
    for name in onerom_config::chip::CHIP_TYPE_NAMES_28_PIN {
        assert!(stdout.contains(name), "missing 28-pin chip: {name}");
    }
    for name in onerom_config::chip::CHIP_TYPE_NAMES_32_PIN {
        assert!(stdout.contains(name), "missing 32-pin chip: {name}");
    }
    for name in onerom_config::chip::CHIP_TYPE_NAMES_40_PIN {
        assert!(stdout.contains(name), "missing 40-pin chip: {name}");
    }
    for name in onerom_config::chip::CHIP_TYPE_NAMES_PLUGINS {
        assert!(stdout.contains(name), "missing plugin: {name}");
    }
}

// Verify chips --board succeeds for every known board and output contains
// the expected chip names for that board's pin count
#[test]
fn firmware_chips_all_boards_succeed() {
    for board in &BOARDS {
        let out = onerom()
            .args(["firmware", "chips", "--board", board.name()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "board {} failed: {}",
            board.name(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        for name in board.supported_chip_type_names() {
            assert!(
                stdout.contains(name),
                "board {} missing chip {} in output",
                board.name(),
                name
            );
        }
    }
}

// --- 24-pin random configs: fixed + current ---

#[test]
fn firmware_build_24pin_config_23xx() {
    build_config_test(
        "onerom-config/test/24-random-23xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_23xx_current() {
    build_config_test(
        "onerom-config/test/24-random-23xx.json",
        24,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_24pin_config_27xx() {
    build_config_test(
        "onerom-config/test/24-random-27xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_27xx_current() {
    build_config_test(
        "onerom-config/test/24-random-27xx.json",
        24,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_24pin_config_28xx() {
    build_config_test(
        "onerom-config/test/24-random-28xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_28xx_current() {
    build_config_test(
        "onerom-config/test/24-random-28xx.json",
        24,
        FirmwareVersion::Current,
    );
}

// --- 28-pin random configs: fixed + current ---

#[test]
fn firmware_build_28pin_config_23xxx() {
    build_config_test(
        "onerom-config/test/28-random-23xxx.json",
        28,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_28pin_config_23xxx_current() {
    build_config_test(
        "onerom-config/test/28-random-23xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_23qlxxx() {
    build_config_test(
        "onerom-config/test/28-random-23qlxxx.json",
        28,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_28pin_config_23qlxxx_current() {
    build_config_test(
        "onerom-config/test/28-random-23qlxxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_27xxx() {
    build_config_test(
        "onerom-config/test/28-random-27xxx.json",
        28,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_28pin_config_27xxx_current() {
    build_config_test(
        "onerom-config/test/28-random-27xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_28xxx() {
    build_config_test(
        "onerom-config/test/28-random-28xxx.json",
        28,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_28pin_config_28xxx_current() {
    build_config_test(
        "onerom-config/test/28-random-28xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

// --- 32-pin random configs: fixed + current ---

#[test]
fn firmware_build_32pin_config_27c0x0() {
    build_config_test(
        "onerom-config/test/32-random-27c0x0.json",
        32,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_32pin_config_27c0x0_current() {
    build_config_test(
        "onerom-config/test/32-random-27c0x0.json",
        32,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_32pin_config_27c301() {
    build_config_test(
        "onerom-config/test/32-random-27c301.json",
        32,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_32pin_config_27c301_current() {
    build_config_test(
        "onerom-config/test/32-random-27c301.json",
        32,
        FirmwareVersion::Current,
    );
}

// --- 40-pin random configs: fixed + current ---

#[test]
fn firmware_build_40pin_config() {
    build_config_test(
        "onerom-config/test/40-random.json",
        40,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_40pin_config_current() {
    build_config_test(
        "onerom-config/test/40-random.json",
        40,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_40pin_config_force_16bit() {
    build_config_test(
        "onerom-config/test/40-random-force-16bit.json",
        40,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_40pin_config_force_16bit_current() {
    build_config_test(
        "onerom-config/test/40-random-force-16bit.json",
        40,
        FirmwareVersion::Current,
    );
}

// --- 24-pin bank configs: fixed + current ---

#[test]
fn firmware_build_24pin_config_bank_23xx() {
    build_config_test(
        "onerom-config/test/24-bank-23xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_bank_23xx_current() {
    build_config_test(
        "onerom-config/test/24-bank-23xx.json",
        24,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_24pin_config_bank_27xx() {
    build_config_test(
        "onerom-config/test/24-bank-27xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_bank_27xx_current() {
    build_config_test(
        "onerom-config/test/24-bank-27xx.json",
        24,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_24pin_config_bank_28xx() {
    build_config_test(
        "onerom-config/test/24-bank-28xx.json",
        24,
        FirmwareVersion::Fixed(FIXED_VERSION),
    );
}

#[test]
fn firmware_build_24pin_config_bank_28xx_current() {
    build_config_test(
        "onerom-config/test/24-bank-28xx.json",
        24,
        FirmwareVersion::Current,
    );
}

// --- 28-pin bank configs: current only ---

#[test]
fn firmware_build_28pin_config_bank_23xxx_current() {
    build_config_test(
        "onerom-config/test/28-bank-23xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_bank_23qlxxx_current() {
    build_config_test(
        "onerom-config/test/28-bank-23qlxxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_bank_27xxx_current() {
    build_config_test(
        "onerom-config/test/28-bank-27xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_28pin_config_bank_28xxx_current() {
    build_config_test(
        "onerom-config/test/28-bank-28xxx.json",
        28,
        FirmwareVersion::Current,
    );
}

#[test]
fn firmware_build_slot_2364_single() {
    slot_succeeds(
        representative_board(24),
        &[slot("rand_8KB.rom", "2364", &[("cs1", "active_low")], None)],
    );
}

#[test]
fn firmware_build_slot_27512_no_cs() {
    slot_succeeds(
        representative_board(28),
        &[slot("rand_64KB.rom", "27512", &[], None)],
    );
}

#[test]
fn firmware_build_slot_27c010_no_cs() {
    slot_succeeds(
        representative_board(32),
        &[slot("rand_128KB.rom", "27C010", &[], None)],
    );
}

#[test]
fn firmware_build_slot_27c400_no_cs() {
    slot_succeeds(
        representative_board(40),
        &[slot("rand_512KB.rom", "27C400", &[], None)],
    );
}

#[test]
fn firmware_build_slot_2316_three_cs() {
    slot_succeeds(
        representative_board(24),
        &[slot(
            "0_63_2048.rom",
            "2316",
            &[
                ("cs1", "active_low"),
                ("cs2", "active_high"),
                ("cs3", "active_low"),
            ],
            None,
        )],
    );
}

// Error cases
#[test]
fn firmware_build_slot_malformed_spec_fails() {
    slot_fails(representative_board(24), &["notavalidspec".to_string()]);
}

#[test]
fn firmware_build_slot_2364_missing_cs_fails() {
    slot_fails(
        representative_board(24),
        &[slot("rand_8KB.rom", "2364", &[], None)],
    );
}

#[test]
fn firmware_build_slot_27512_spurious_cs_fails() {
    slot_fails(
        representative_board(28),
        &[slot(
            "rand_64KB.rom",
            "27512",
            &[("cs1", "active_low")],
            None,
        )],
    );
}

#[test]
fn firmware_build_slot_size_handling_duplicate() {
    // 4KB ROM into 8KB slot
    slot_succeeds(
        representative_board(24),
        &[slot(
            "0_63_4096.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("duplicate"),
        )],
    );
}

#[test]
fn firmware_build_slot_size_handling_pad() {
    slot_succeeds(
        representative_board(24),
        &[slot(
            "0_63_4096.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("pad"),
        )],
    );
}

#[test]
fn firmware_build_slot_size_handling_truncate() {
    // 128KB ROM truncated to 64KB slot
    slot_succeeds(
        representative_board(28),
        &[slot("rand_128KB.rom", "27512", &[], Some("truncate"))],
    );
}

#[test]
fn firmware_build_slot_size_handling_none_wrong_size_fails() {
    // Explicit none with wrong size should fail
    slot_fails(
        representative_board(24),
        &[slot(
            "0_63_4096.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("none"),
        )],
    );
}

#[test]
fn firmware_build_slot_wrong_size_no_handling_fails() {
    // Wrong size with no size_handling specified should also fail
    slot_fails(
        representative_board(24),
        &[slot(
            "0_63_4096.rom",
            "2364",
            &[("cs1", "active_low")],
            None,
        )],
    );
}

// Exact size match with size_handling specified is an error
#[test]
fn firmware_build_slot_size_handling_on_exact_size_fails() {
    slot_fails(
        representative_board(24),
        &[slot(
            "rand_8KB.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("duplicate"),
        )],
    );
}

// Aliases
#[test]
fn firmware_build_slot_size_handling_dup_alias() {
    slot_succeeds(
        representative_board(24),
        &[slot(
            "0_63_4096.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("dup"),
        )],
    );
}

#[test]
fn firmware_build_slot_size_handling_trunc_alias() {
    slot_succeeds(
        representative_board(28),
        &[slot("rand_128KB.rom", "27512", &[], Some("trunc"))],
    );
}

// Invalid value
#[test]
fn firmware_build_slot_size_handling_invalid_fails() {
    slot_fails(
        representative_board(24),
        &[slot(
            "rand_8KB.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("notavalue"),
        )],
    );
}

#[test]
fn firmware_build_slot_size_handling_duplicate_indivisible_fails() {
    slot_fails(
        representative_board(24),
        &[slot(
            "zero3.rom",
            "2364",
            &[("cs1", "active_low")],
            Some("duplicate"),
        )],
    );
}

#[test]
fn firmware_build_slot_28pin_dual() {
    slot_succeeds(
        representative_board(28),
        &[
            slot("rand_64KB.rom", "27512", &[], None),
            slot("rand_64KB.rom", "27512", &[], None),
        ],
    );
}

#[test]
fn firmware_build_slot_32pin_dual() {
    slot_succeeds(
        representative_board(32),
        &[
            slot("rand_128KB.rom", "27C010", &[], None),
            slot("rand_128KB.rom", "27C010", &[], None),
        ],
    );
}

#[test]
fn firmware_build_slot_40pin_single() {
    slot_succeeds(
        representative_board(40),
        &[slot("rand_512KB.rom", "27C400", &[], None)],
    );
}

#[test]
fn firmware_build_slot_40pin_dual() {
    slot_succeeds(
        representative_board(40),
        &[
            slot("rand_512KB.rom", "27C400", &[], None),
            slot("rand_512KB_alt.rom", "27C400", &[], None),
        ],
    );
}

/// SRAM builds against V1 firmware, with and without an image, and must keep
/// doing so: `6116` is in the V1 per-board chip type set, so anyone building
/// for 0.6.x can use it.
#[test]
fn firmware_build_slot_sram_v1() {
    for spec in [slot("0_63_2048.rom", "6116", &[], None), "type=6116".into()] {
        let out = build_slots_at_version(representative_board(24), &[spec], Some(V1_VERSION));
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Against V2 firmware, whether SRAM builds is whatever the V2 builder says:
/// no V2 firmware serves `6116` at the time of writing, so it is refused.
///
/// The expectation is read from `SUPPORTED_CHIP_TYPES_V2` rather than written
/// down, so this test follows the builder: it starts requiring success the day
/// `Chip6116` joins that list, and fails if the firmware gains SRAM without it.
#[test]
fn firmware_build_slot_sram_tracks_the_v2_chip_list() {
    let servable = onerom_gen::SUPPORTED_CHIP_TYPES_V2.contains(&ChipType::Chip6116);
    let out = build_slots_at_version(
        representative_board(24),
        &[slot("0_63_2048.rom", "6116", &[], None)],
        Some(V2_VERSION),
    );
    assert_eq!(
        out.status.success(),
        servable,
        "SRAM build vs SUPPORTED_CHIP_TYPES_V2\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A chip type only V2 serves must name the firmware version it needs when
/// built against V1, rather than reporting that the tool does not know it.
///
/// `23C1001` is the case in hand: V1 firmware has never served one, so a plain
/// `firmware build` - which targets the latest release, still a 0.6.x - reached
/// the tool-support error and sent the user looking for a missing chip type
/// instead of a newer firmware.  The minimum is read from the chip type, so the
/// test follows `chip-types.json` rather than restating it.
#[test]
fn firmware_build_slot_v2_only_chip_on_v1_names_the_minimum_version() {
    let minimum = ChipType::Chip23C1001
        .min_supported_firmware_version()
        .expect("23C1001 declares a minimum firmware version");
    let spec = slot(
        "rand_128KB.rom",
        "23C1001",
        &[("cs1", "active-low"), ("cs2", "ignore")],
        None,
    );

    let out = build_slots_at_version("fire-32-b", std::slice::from_ref(&spec), Some(V1_VERSION));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "V1 built a 23C1001: {stderr}");
    assert!(
        stderr.contains(&minimum.to_string()),
        "V1 failure does not name the {minimum} minimum: {stderr}"
    );

    // The same slot against the firmware that does serve it, so the V1 failure
    // is known to be about the firmware version and nothing else.
    let out = build_slots_at_version("fire-32-b", &[spec], Some(V2_VERSION));
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A firmware built from the same image in each supported format must be
/// byte-identical.
///
/// The generator's own tests cover the decoders; this covers the CLI wiring
/// between `--slot format=`/`load-address=` and them, which is what would
/// silently pass the wrong load address or the wrong decoder.  `label=` is set
/// so the filename recorded in the metadata is the same for all three and the
/// comparison is about image content alone.
#[test]
fn binary_ihex_and_srec_slots_build_identical_firmware() {
    let dir = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..8192u32)
        .map(|i| (i.wrapping_mul(37) ^ 0x5A) as u8)
        .collect();
    let bin = dir.path().join("rom.bin");
    std::fs::write(&bin, &data).unwrap();

    // The images are assembled at 0xE000, so each record format also has to
    // apply the load address to land back at ROM offset 0.
    for (to, name) in [("ihex", "rom.hex"), ("srec", "rom.s19")] {
        succeeds(onerom().args([
            "image",
            "convert",
            "--from",
            "binary",
            "--to",
            to,
            "--input",
            bin.to_str().unwrap(),
            "--output",
            dir.path().join(name).to_str().unwrap(),
            "--load-address",
            "0xE000",
        ]));
    }

    let build = |file: &str, format: Option<&str>| -> Vec<u8> {
        let out = dir.path().join(format!("fw-{file}.bin"));
        let mut spec = format!(
            "file={},label=rom,type=2364,cs1=active-low",
            dir.path().join(file).to_str().unwrap()
        );
        if let Some(format) = format {
            spec.push_str(&format!(",format={format},load-address=0xE000"));
        }
        let result = onerom()
            .args([
                "firmware",
                "build",
                "--board",
                representative_board(24),
                "--slot",
                &spec,
                "--output",
                out.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "build from {file} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::read(&out).unwrap()
    };

    let from_binary = build("rom.bin", None);
    assert!(!from_binary.is_empty(), "build produced no firmware");
    assert_eq!(build("rom.hex", Some("ihex")), from_binary);
    assert_eq!(build("rom.s19", Some("srec")), from_binary);
}

/// Omitting the load address for an image assembled high in the address space
/// makes its extent overshoot the chip — the clean error that catches the
/// mistake, rather than a silently misplaced image.
#[test]
fn srec_slot_without_its_load_address_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("rom.bin");
    let srec = dir.path().join("rom.s19");
    std::fs::write(&bin, vec![0x5Au8; 8192]).unwrap();
    succeeds(onerom().args([
        "image",
        "convert",
        "--from",
        "binary",
        "--to",
        "srec",
        "--input",
        bin.to_str().unwrap(),
        "--output",
        srec.to_str().unwrap(),
        "--load-address",
        "0xE000",
    ]));

    let spec = |with_load_address: bool| {
        let mut s = format!(
            "file={},type=2364,cs1=active-low,format=srec",
            srec.to_str().unwrap()
        );
        if with_load_address {
            s.push_str(",load-address=0xE000");
        }
        s
    };

    // Control: with the load address it builds.
    slot_succeeds(representative_board(24), &[spec(true)]);
    slot_fails(representative_board(24), &[spec(false)]);
}
