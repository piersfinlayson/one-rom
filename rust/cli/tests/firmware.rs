// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use onerom_config::hw::BOARDS;

mod common;
use common::{
    build_config_test, build_slots, fails, onerom, project_root, representative_board, slot,
    slot_fails, slot_succeeds, succeeds,
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

#[test]
fn firmware_build_24pin_config() {
    build_config_test("onerom-config/test/24-random-27xx.json", 24);
}

#[test]
fn firmware_build_28pin_config() {
    build_config_test("onerom-config/test/28-random-27xxx.json", 28);
}

#[test]
fn firmware_build_32pin_config_27c0x0() {
    build_config_test("onerom-config/test/32-random-27c0x0.json", 32);
}

#[test]
fn firmware_build_32pin_config_27c301() {
    build_config_test("onerom-config/test/32-random-27c301.json", 32);
}

#[test]
fn firmware_build_40pin_config_27c400() {
    build_config_test("onerom-config/test/40-random-27c400.json", 40);
}

#[test]
fn firmware_build_slot_2364_single() {
    slot_succeeds(
        representative_board(24),
        &[slot(
            "rand_8KB.rom",
            "2364",
            &[("cs1", "active_low")],
            None,
        )],
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

#[test]
fn firmware_build_slot_sram() {
    slot_succeeds(
        representative_board(24),
        &[slot("0_63_2048.rom", "6116", &[], None)],
    );
}

#[test]
fn firmware_build_slot_sram_no_image() {
    let slot = "type=6116";
    let output = build_slots(representative_board(24), &[slot.to_string()]);
    println!("Output: {}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "SRAM slot without image should succeed"
    );
}
