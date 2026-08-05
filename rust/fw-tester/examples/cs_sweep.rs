// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Measure the CS-to-data latency of every chip in a config, and print it
//! against what [`onerom_fw_tester::cs_timing`] expects.
//!
//! `pio-tester` asserts that latency on every run; this reports it.  Use it
//! when a CS timing assertion fails and you need the number rather than a
//! verdict, or when adding a serving algorithm and you need a figure to put in
//! `expected_cs_to_data`.
//!
//! Runs against the first chip of each `single` set — enough to characterise a
//! serving path, and it avoids having to reproduce the runner's per-set-type
//! background masks here.  Multi and Banked sets are covered by `pio-tester`.
//!
//! ```text
//! BASE_DIR=$(pwd) CONFIG=onerom-config/test/24-random-23xx.json \
//!     BOARD=fire-24-a cargo run -p onerom-fw-tester --example cs_sweep
//! ```
//!
//! | Variable    | Description                                          |
//! |-------------|------------------------------------------------------|
//! | `CONFIG`    | Config JSON path, as for `pio-tester`                |
//! | `BOARD`     | Board name, e.g. `fire-24-a`                         |
//! | `BASE_DIR`  | Project root for relative paths (default: CWD)       |
//! | `SETS`      | Comma-separated set indices (default: all)           |
//! | `MAX_CYCLES`| Highest CS-to-data delay to search (default: 32)     |

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_fw_tester::cs_timing::{self, Algs, Probe, expected_cs_to_data};
use onerom_fw_tester::driver;
use onerom_fw_tester::pin_cache::PinCache;
use onerom_fw_tester::runner::addr_before_cs_cycles;
use onerom_fw_tester::timing;
use onerom_gen::{ChipSetConfig, ChipSetType, Config, CsConfig};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let config_path = std::env::var("CONFIG").expect("CONFIG must be set");
    let board_str = std::env::var("BOARD").expect("BOARD must be set");
    let base_dir_str = std::env::var("BASE_DIR").unwrap_or_else(|_| ".".to_string());
    let max_cycles: u32 = std::env::var("MAX_CYCLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    let want: Option<Vec<usize>> = std::env::var("SETS").ok().map(|v| {
        v.split(',')
            .map(|s| s.trim().parse().expect("bad SETS entry"))
            .collect()
    });

    let base_dir = std::fs::canonicalize(&base_dir_str).expect("bad BASE_DIR");
    let json = std::fs::read_to_string(base_dir.join(&config_path)).expect("cannot read CONFIG");
    let config: Config = serde_json::from_str(&json).expect("cannot parse CONFIG");
    let board = Board::try_from_str(&board_str).expect("unknown BOARD");

    Emulator::set_logging(std::env::var("ONEROM_LOG").is_ok_and(|v| v == "1"));
    println!("config={config_path} board={board_str}");

    for (set_idx, chip_set) in config.chip_sets.iter().enumerate() {
        if want.as_ref().is_some_and(|w| !w.contains(&set_idx)) {
            continue;
        }
        if chip_set.set_type != ChipSetType::Single || chip_set.chips.is_empty() {
            println!("\nset {set_idx}: skipped (not a single-chip set)");
            continue;
        }
        sweep_set(board, chip_set, set_idx, max_cycles);
    }
}

fn sweep_set(board: Board, chip_set: &ChipSetConfig, set_idx: usize, max_cycles: u32) {
    let chip_config = &chip_set.chips[0];
    let chip_type = chip_config.chip_type.resolved();
    let force_16_bit = chip_set
        .firmware_overrides
        .as_ref()
        .and_then(|fw| fw.fire.as_ref())
        .map(|f| f.force_16_bit)
        .unwrap_or(false);

    let cs_config = CsConfig::from_chip_type(
        &chip_type,
        chip_config.cs1,
        chip_config.cs2,
        chip_config.cs3,
        chip_config.cs4,
        chip_config.ce,
        chip_config.oe,
    );
    let info = match onerom_gen::compat::serving_alg_info(
        board,
        chip_type,
        ChipSetType::Single,
        1,
        cs_config,
        None,
        force_16_bit,
    ) {
        Ok(i) => i,
        Err(e) => {
            println!(
                "\nset {set_idx} ({}): does not derive — {e}",
                chip_type.name()
            );
            return;
        }
    };

    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(set_idx as u8);
    let mut emulator = Emulator::boot();
    assert!(!emulator.limp_mode(), "set {set_idx}: limp mode");
    assert!(emulator.pios_enabled(), "set {set_idx}: PIOs not enabled");
    emulator.setup_epio(
        if matches!(chip_type, ChipType::Chip27C400 | ChipType::Chip27C200) {
            16
        } else {
            8
        },
    );
    emulator.step_cycles(timing::CYCLES_BEFORE_START);

    let cache = PinCache::build(chip_type, chip_config, board);
    let addr_before_cs = addr_before_cs_cycles(chip_type);
    let algs = Algs::from_config(&info);
    let cs_in_window = cache
        .control_lines
        .iter()
        .flat_map(|cl| cl.gpios.iter())
        .any(|&g| info.samples_gpio(g)); // diagnostic: overrides not modelled

    println!(
        "\n=== set {set_idx}: {} — {algs:?} window=[{},{}) cs_in_window={cs_in_window} \
         addr_before_cs={addr_before_cs}",
        chip_type.name(),
        info.addr_window_base,
        info.addr_window_base + info.addr_window_pins,
    );

    for &mode in chip_type.bit_modes() {
        if force_16_bit && mode != 16 {
            continue;
        }
        let byte_mask = match cache.byte_n_gpio {
            Some(g) => driver::byte_n_mask(g, mode),
            None => (0, 0),
        };
        let addr_gpios: &[Vec<u8>] = if mode == 16 {
            &cache.addr_gpios[1..]
        } else {
            &cache.addr_gpios
        };
        let span = 1usize << addr_gpios.len(); // diagnostic: full drive range

        let Some(slot) = cs_timing::resolve_slot(
            &emulator,
            &cache,
            addr_gpios,
            byte_mask,
            mode,
            addr_before_cs,
        ) else {
            println!("  mode {mode}bit: no RAM slot matches what the bus serves");
            continue;
        };
        let probe = Probe::new(
            &emulator,
            &cache,
            addr_gpios,
            byte_mask,
            addr_before_cs,
            mode,
            slot,
        );

        for i in 0..3 {
            let addr = (span / 4) * (i + 1) % span;
            let expected = expected_cs_to_data(algs, mode, cs_in_window, addr);
            match probe.measure_at(addr, span, max_cycles) {
                Some(measured) => println!(
                    "  mode {mode}bit addr={addr:#07x}: measured {measured:>2}, \
                     expected {expected:>2}{}",
                    if measured == expected {
                        ""
                    } else {
                        "   <-- differs"
                    },
                ),
                None => println!(
                    "  mode {mode}bit addr={addr:#07x}: no latency up to {max_cycles} \
                     (could not locate the served bytes)"
                ),
            }
        }
    }
}
