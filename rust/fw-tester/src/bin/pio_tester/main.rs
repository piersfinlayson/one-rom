// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Firmware Tester
//!
//! Drives the PIO/DMA emulator against a compiled firmware image and verifies
//! that every byte served matches the JSON-config ground truth.
//!
//! # Environment variables
//!
//! | Variable     | Required | Description                                          |
//! |--------------|----------|------------------------------------------------------|
//! | `CONFIG`     | yes      | Path to the firmware config JSON file                |
//! | `BOARD`      | yes      | Board name, e.g. `fire-24-e`                         |
//! | `BASE_DIR`   | no       | Project root for resolving relative paths (def: CWD) |
//! | `ONEROM_LOG` | no       | Set to `1` to enable firmware logging to stdout      |
//! | `RUST_LOG`   | no       | Tester log level, e.g. `info`, `debug` (def: `warn`) |
//!
//! Exits 0 on pass, 1 on any failure or boot error.

use std::process;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_gen::Config;

mod report;
mod runner;

use report::TestReport;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let config_path = std::env::var("CONFIG")
        .expect("CONFIG env var must be set to the firmware config JSON path");
    let board_str = std::env::var("BOARD")
        .expect("BOARD env var must be set to the board name (e.g. fire-24-e)");
    let log_enabled = std::env::var("ONEROM_LOG")
        .map(|v| v == "1")
        .unwrap_or(false);

    // BASE_DIR is the root used to resolve relative ROM image file paths from
    // the config.  Defaults to "." (CWD) so the shell-script orchestrator
    // works without change when invoking the binary from the project root.
    // Set explicitly when running via cargo from a different directory
    // (e.g. BASE_DIR=$(realpath ../..) from fw-tester/).
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{}': {}", base_dir_str, e));

    let config_file = base_dir.join(&config_path);
    let json = std::fs::read_to_string(&config_file)
        .unwrap_or_else(|e| panic!("Failed to read config '{}': {}", config_file.display(), e));
    let config: Config = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{}': {}", config_path, e));

    let board =
        Board::try_from_str(&board_str).unwrap_or_else(|| panic!("Unknown board '{}'", board_str));

    let board_display = match board.rp_variant() {
        Some(v) => format!("{board_str} ({v})"),
        None => board_str.clone(),
    };

    // Firmware logging is a global setting that persists across re-boots.
    Emulator::set_logging(log_enabled);

    let mut report = TestReport::new(&config_path, &board_display);
    runner::run_all(board, &config, &base_dir, &mut report);

    report.print();
    process::exit(if report.all_passed() { 0 } else { 1 });
}
