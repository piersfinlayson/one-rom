// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM USB Plugin Tester
//!
//! Runs the USB system plugin's own C source natively against the firmware
//! emulator.  The plugin is neither reimplemented nor stubbed: this is the same
//! source that is cross-compiled for the device, linked against the same plugin
//! API.
//!
//! What is stubbed is everything below the plugin that is not One ROM's code —
//! tinyusb, and picobootx, which has a conformance suite of its own in its own
//! repository.  The shim keeps the CDC endpoint in enough detail for the log
//! drain's behaviour to be a response to something, and keeps what the plugin
//! registered with picoboot so a scenario can call its handlers through the
//! same pointers picoboot would.
//!
//! # Environment variables
//!
//! | Variable     | Required | Description                                     |
//! |--------------|----------|-------------------------------------------------|
//! | `BOARD`      | yes      | Board name, e.g. `fire-24-a`                    |
//! | `CONFIG`     | yes      | Path to the firmware config JSON file           |
//! | `BASE_DIR`   | no       | Project root for resolving relative paths       |
//! | `ONEROM_LOG` | no       | Set to `1` to enable firmware logging to stdout |
//! | `RUST_LOG`   | no       | Tester log level (default: `warn`)              |
//!
//! # Arguments
//!
//! `--suite <name>` runs one suite; `--scenario <substr>` runs only scenarios
//! whose name contains the substring.  Both default to everything.
//!
//! Exits 0 if every scenario that ran passed, 1 otherwise.

use std::path::PathBuf;

use onerom_config::hw::Board;
use onerom_config::mcu::RpVariant;
use onerom_fw_emulator::Emulator;
use onerom_gen::Config;
use onerom_plugin_tester::harness::Plugin;
use onerom_plugin_tester::run::{Filters, Outcome, Tally, suite_header};

mod device;
mod ffi;
mod suites;

use device::Device;

/// Everything a scenario needs to know about the device it is talking to,
/// beyond the [`Device`] itself.
pub struct Ctx {
    pub board: Board,
    /// GPIO count of the running RP2350 variant, which is what the plugin
    /// reports in its capabilities and sizes a GPIO query from.
    pub num_gpios: u8,
}

pub type ScenarioFn = fn(&mut Device, &Ctx) -> Result<Outcome, String>;

pub struct Scenario {
    /// Dotted name, filtered on by `--scenario`.
    pub name: &'static str,
    /// A one-line description of what the scenario asserts, printed when it
    /// fails.
    pub about: &'static str,
    pub run: ScenarioFn,
    /// Run after the firmware boots and before the plugin is entered.
    ///
    /// A plugin settles what it can do once, at entry, so a scenario that wants
    /// it to find the device in a particular state has to arrange that before
    /// it starts rather than as its own first act.  Claiming the log channel's
    /// reader is the case this exists for.
    pub before_start: Option<fn(&Emulator)>,
}

pub struct Suite {
    pub name: &'static str,
    pub blurb: &'static str,
    pub scenarios: &'static [Scenario],
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let filters = Filters::from_args();

    let board_str = std::env::var("BOARD").expect("BOARD env var must be set (e.g. fire-24-a)");
    let board =
        Board::try_from_str(&board_str).unwrap_or_else(|| panic!("Unknown board '{board_str}'"));
    let log_enabled = std::env::var("ONEROM_LOG")
        .map(|v| v == "1")
        .unwrap_or(false);
    let config_path = std::env::var("CONFIG").expect("CONFIG env var must be set");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let base_dir = std::fs::canonicalize(&base_dir_str)
        .unwrap_or_else(|e| panic!("Cannot resolve BASE_DIR '{base_dir_str}': {e}"));
    let config_json = std::fs::read_to_string(base_dir.join(&config_path))
        .unwrap_or_else(|e| panic!("Failed to read config '{config_path}': {e}"));
    let config: Config = serde_json::from_str(&config_json)
        .unwrap_or_else(|e| panic!("Failed to parse config '{config_path}': {e}"));

    let mut tally = Tally::new();

    for suite in suites::SUITES {
        if !filters.suite_matches(suite.name) {
            continue;
        }
        let selected: Vec<&Scenario> = suite
            .scenarios
            .iter()
            .filter(|s| filters.scenario_matches(s.name))
            .collect();
        if selected.is_empty() {
            continue;
        }

        suite_header(suite.name, suite.blurb);
        for sc in selected {
            let result = run_scenario(sc, board, &config, log_enabled);
            tally.record(sc.name, sc.about, result);
        }
    }

    tally.finish("usb", &format!("{board_str} / {config_path}"));
}

/// Boot the firmware, start the plugin, and run one scenario against it.
///
/// Every scenario gets a fresh boot and a fresh entry into the plugin, so no
/// scenario can be influenced by another's leftover state: the firmware's RAM is
/// restored to its cold-boot image, and re-entering the plugin re-runs its own
/// initialisation.
fn run_scenario(
    sc: &Scenario,
    board: Board,
    config: &Config,
    log_enabled: bool,
) -> Result<Outcome, String> {
    Emulator::set_logging(log_enabled);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(0);
    let mut emu = Emulator::boot();

    if emu.limp_mode() {
        return Err("firmware entered limp mode".to_string());
    }

    // Serving is already running when a plugin is launched on a device, and the
    // plugin's logical ROM range reads the slot it is serving from, so the
    // emulation is brought up the same way round here.
    emu.setup_epio(native_word_size(config));

    // The shim's endpoint model is process-global, as the plugin's own statics
    // are, so it starts each scenario as a device's does: an idle terminal, a
    // bus that can carry data, and a full packet of room.
    device::reset_endpoint();

    if let Some(before_start) = sc.before_start {
        before_start(&emu);
    }

    // SAFETY: `emu` outlives `plugin`, which is dropped at the end of this fn.
    let plugin = unsafe { Plugin::start(&emu)? };

    let ctx = Ctx {
        board,
        // Mirrors max_gpios[] in firmware/src/constants.c, which is what the
        // firmware range-checks against and what the plugin reports.
        num_gpios: match board.rp_variant() {
            Some(RpVariant::Rp235xB) => 48,
            _ => 30,
        },
    };

    let mut device = Device::new(&emu, &plugin);
    (sc.run)(&mut device, &ctx)
}

/// The width the firmware serves this set at.
///
/// A chip that can be read either way is always served by the wider data path,
/// with BYTE# selecting a half of it at run time, so this is the widest mode the
/// chip supports rather than anything the host is driving.
fn native_word_size(config: &Config) -> u8 {
    config
        .chip_sets
        .first()
        .and_then(|s| s.chips.first())
        .and_then(|c| c.chip_type.resolved().bit_modes().iter().max().copied())
        .unwrap_or(8)
}

/// Where the tester's own files live, for a scenario that needs one.
#[allow(dead_code)]
pub fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
