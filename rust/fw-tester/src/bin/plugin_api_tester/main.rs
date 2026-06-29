// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Plugin API Tester
//!
//! Tests elements of the One ROM "ORA" plugin API, in particular focusing on
//! the RAM slot reading, reprogramming and switching, validating the slot
//! contents via both the APIs and the PIO serving.
//!
//! # Environment variables
//!
//! | Variable     | Required | Description                                     |
//! |--------------|----------|-------------------------------------------------|
//! | `BOARD`      | yes      | Board name, e.g. `fire-24-a`                   |
//! | `CONFIG`     | yes      | Path to the firmware config JSON file           |
//! | `BASE_DIR`   | no       | Project root for resolving relative paths       |
//! | `ONEROM_LOG` | no       | Set to `1` to enable firmware logging to stdout |
//! | `RUST_LOG`   | no       | Tester log level (default: `warn`)              |
//!
//! Exits 0 on all tests passed, 1 on any failure or boot error.
//!
//! # Per-slot operation
//!
//! Every `Single` flash slot in the config is exercised in turn.  A flash slot
//! is indexed directly by the image-select value, so for each Single slot the
//! firmware is rebooted with `sel_image` set to that slot's index, making it
//! the active boot image, and the full test suite is run against it.
//! Multi/Banked slots are skipped (their serving is covered by the separate
//! pio-tester).

use std::process;

use onerom_config::hw::Board;
use onerom_fw_tester::timing;
use onerom_gen::{ChipSetType, Config};

mod report;
mod setup;
mod tests;

use report::ApiReport;
use setup::setup;

// ── RAM slot choreography ─────────────────────────────────────────────────────
//
// These are RAM serving slots (distinct from flash slots).  The flash slot
// under test is the per-iteration `set_idx`, so it needs no constant.

/// The RAM slot populated and served at boot — active on entry to each slot's
/// suite, regardless of which image was selected.
const BOOT_SLOT: u8 = 0;

/// A non-active scratch RAM slot used to exercise reprogram and copy into a
/// slot that is not currently being served.
const SCRATCH_SLOT: u8 = 1;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let board_str = std::env::var("BOARD").expect("BOARD env var must be set (e.g. fire-24-a)");
    let board =
        Board::try_from_str(&board_str).unwrap_or_else(|| panic!("Unknown board '{}'", board_str));
    let log_enabled = std::env::var("ONEROM_LOG")
        .map(|v| v == "1")
        .unwrap_or(false);

    let config_path = std::env::var("CONFIG")
        .expect("CONFIG env var must be set to the firmware config JSON path");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{}': {}", base_dir_str, e));
    let config_file = base_dir.join(&config_path);
    let config_json = std::fs::read_to_string(&config_file)
        .unwrap_or_else(|e| panic!("Failed to read config '{}': {}", config_file.display(), e));
    let config: Config = serde_json::from_str(&config_json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{}': {}", config_path, e));

    let mut report = ApiReport::new(&board_str, &config_path);

    for (idx, chip_set) in config.chip_sets.iter().enumerate() {
        let sel = idx as u8;
        match chip_set.set_type {
            ChipSetType::Single => {
                let label = chip_set
                    .chips
                    .first()
                    .map(|c| c.chip_type.name().to_string())
                    .unwrap_or_else(|| "<no chip>".to_string());
                report.begin_slot(idx, sel, &label);
                run_slot(&mut report, board, &config, &base_dir, log_enabled, idx);
            }
            ChipSetType::Multi => report.skip_slot(idx, sel, "Multi"),
            ChipSetType::Banked => report.skip_slot(idx, sel, "Banked"),
        }
    }

    report.print();
    process::exit(if report.all_passed() { 0 } else { 1 });
}

/// Boot the firmware with `set_idx` as the selected image, then run the full
/// test suite against that flash slot.
///
/// The `Emulator` (and its epio handle) is created here and dropped when this
/// function returns, before the next slot boots.  `set_host_sram_ptr` is left
/// pointing at the freed epio buffer in the gap between drop and the next
/// `setup_epio`; this is safe only because boot populates the firmware's real
/// SRAM directly and nothing calls `sram_to_host` before the next `setup_epio`
/// re-points it.
fn run_slot(
    report: &mut ApiReport,
    board: Board,
    config: &Config,
    base_dir: &std::path::Path,
    log_enabled: bool,
    set_idx: usize,
) {
    let (mut emulator, fw_version) = setup(board, log_enabled, set_idx as u8);

    // Info
    report.add(
        "device_version",
        tests::info::test_device_version(&emulator, &fw_version),
    );

    // Lookup
    report.add(
        "lookup_coverage",
        tests::lookup::test_lookup_coverage(&emulator),
    );

    // Mapping
    report.add(
        "chip_size",
        tests::mapping::test_chip_size(&emulator, config, set_idx),
    );
    report.add(
        "addr_mapping",
        tests::mapping::test_addr_mapping(&emulator, config, set_idx),
    );
    report.add(
        "data_mapping",
        tests::mapping::test_data_mapping(&emulator, config),
    );

    // Slots
    report.add(
        "flash_slot_count",
        tests::slots::test_flash_slot_count(&emulator, config),
    );
    report.add(
        "flash_slot_info",
        tests::slots::test_flash_slot_info(&emulator, config),
    );
    report.add(
        "ram_slot_count",
        tests::slots::test_ram_slot_count(&emulator, config, board, fw_version, base_dir, set_idx),
    );
    report.add(
        "ram_slot_info",
        tests::slots::test_ram_slot_info(&emulator, config, board, fw_version, base_dir, set_idx),
    );
    report.add(
        "active_ram_slot",
        tests::slots::test_active_ram_slot(&emulator, BOOT_SLOT),
    );
    report.add(
        "read_initial_slot",
        tests::slots::test_read_initial_slot(&emulator, config, base_dir, BOOT_SLOT, set_idx),
    );

    // Set up epio before any reprogram so that the PIO path can be verified
    // against the unmodified oracle image in the boot slot.  epio_from_apio()
    // is called here while the apio state is clean (no pending EXEC
    // instructions from pio_switch_rom_region).
    let word_size = config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .map(|c| {
            if c.chip_type.bit_modes().contains(&16) {
                16u8
            } else {
                8u8
            }
        })
        .unwrap_or(8);
    emulator.setup_epio(word_size);
    emulator.step_cycles(timing::CYCLES_BEFORE_START);

    // PIO baseline: verify the PIO serves the oracle image correctly before
    // any reprogram or slot switch.  If this fails the PIO path itself is
    // broken; subsequent PIO verify failures are not caused by our changes.
    report.add(
        "initial_pio_verify",
        tests::reprogram::test_initial_pio_verify(&emulator, config, board, base_dir, set_idx),
    );
    report.add(
        "noop_switch_pio_verify",
        tests::reprogram::test_noop_switch_pio_verify(
            &emulator, config, board, base_dir, BOOT_SLOT, set_idx,
        ),
    );

    // Reprogram / copy (no epio sync needed — sram_to_host writes directly into
    // epio's buffer via set_host_sram_ptr, so epio stays in sync).  The flash
    // slot copied is the booted image (set_idx), so its bytes match the RAM
    // region's chip type.
    report.add(
        "reprogram_reject_active",
        tests::reprogram::test_reprogram_reject_active(&emulator, BOOT_SLOT),
    );

    // reprogram_active_round_trip targets the active slot, which always exists,
    // so it runs regardless of slot count.  The remaining three need a second
    // RAM slot (SCRATCH_SLOT); when the region is large enough that only one
    // slot fits (e.g. a 512KB served region → 1 slot), they are skipped rather
    // than failing on a slot that legitimately does not exist.
    report.add(
        "reprogram_active_round_trip",
        tests::reprogram::test_reprogram_active_round_trip(&emulator, config, BOOT_SLOT, set_idx),
    );

    if emulator.get_ram_slot_count() > SCRATCH_SLOT {
        report.add(
            "reprogram_round_trip",
            tests::reprogram::test_reprogram_round_trip(&emulator, config, SCRATCH_SLOT, set_idx),
        );
        report.add(
            "copy_flash_to_ram",
            tests::reprogram::test_copy_flash_to_ram(
                &emulator,
                config,
                base_dir,
                set_idx,
                SCRATCH_SLOT,
            ),
        );
        report.add(
            "switch_active_slot",
            tests::reprogram::test_switch_active_slot(&emulator, SCRATCH_SLOT),
        );
    } else {
        let reason = "requires a second RAM slot (region size leaves only one)";
        report.skip("reprogram_round_trip", reason);
        report.skip("copy_flash_to_ram", reason);
        report.skip("switch_active_slot", reason);
    }

    // PIO verification after reprogram / copy.  Each test sets its own target
    // slot active before serving, so these are order-independent.
    report.add(
        "reprogram_pio_verify",
        tests::reprogram::test_reprogram_pio_verify(
            &emulator, config, board, base_dir, BOOT_SLOT, set_idx,
        ),
    );
    report.add(
        "copy_flash_pio_verify",
        tests::reprogram::test_copy_flash_pio_verify(
            &emulator, config, board, base_dir, set_idx, BOOT_SLOT,
        ),
    );

    // `emulator` dropped here; Drop impl frees the epio handle before the next
    // slot boots.
}
