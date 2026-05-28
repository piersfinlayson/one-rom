// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::format;
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_executor::main as embassy_main;
use embassy_rp::{clocks::ClockConfig, config::Config};
use embassy_time::Timer;

use embedded_alloc::LlffHeap as Heap;
use panic_rtt_target as _;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::pin_map::BoardPinMap;

mod error;
mod hw;
mod logs;
mod rom;

use rom::{CsPolarities, RomReader};

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Build-time configuration via environment variables
// ---------------------------------------------------------------------------

const BOARD_STR: &str = match option_env!("BOARD") {
    Some(s) => s,
    None => "fire-24-e",
};

const CHIP_STR: &str = match option_env!("CHIP_TYPE") {
    Some(s) => s,
    None => panic!("CHIP_TYPE environment variable must be set at build time"),
};

const CS1_STR: Option<&str> = option_env!("CS1");
const CS2_STR: Option<&str> = option_env!("CS2");
const CS3_STR: Option<&str> = option_env!("CS3");

// ---------------------------------------------------------------------------

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[embassy_main]
async fn main(_spawner: Spawner) -> ! {
    // Heap
    {
        use core::mem::MaybeUninit;
        const HEAP_SIZE: usize = 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(&raw mut HEAP_MEM as usize, HEAP_SIZE) }
    }

    logs::init_rtt();

    info!("-----");
    info!("One ROM Lab v{}", PKG_VERSION);
    info!("Copyright (c) 2026 Piers Finlayson");
    info!("-----");
    debug!("RP2350 target");

    // Clocks
    let mut config = Config::default();
    config.clocks = ClockConfig::system_freq(150_000_000).expect("Failed to configure clocks");
    let _p = embassy_rp::init(config);
    debug!("Clocks configured to 150MHz");

    // Parse board and chip type
    let board =
        Board::try_from_str(BOARD_STR).unwrap_or_else(|| panic!("Unknown board '{}'", BOARD_STR));

    let chip = ChipType::try_from_str(CHIP_STR)
        .unwrap_or_else(|| panic!("Unknown chip type '{}'", CHIP_STR));

    assert!(
        board.supports_chip_type(chip),
        "Board '{}' does not support chip type '{}'",
        board.name(),
        chip.name()
    );

    info!("Board: {}", board.name());
    info!("Chip:  {}", chip.name());

    // Parse CS polarities (required for configurable lines; ignored otherwise)
    let cs_polarities = CsPolarities {
        cs1: CS1_STR.map(|s| parse_active_level(s, "CS1")),
        cs2: CS2_STR.map(|s| parse_active_level(s, "CS2")),
        cs3: CS3_STR.map(|s| parse_active_level(s, "CS3")),
    };

    // Build the physical-pin → MCU GPIO map for this board
    let pin_map = BoardPinMap::new(board);

    // Status LED (optional — some boards have none)
    let mut led = pin_map.led_gpio().map(hw::steal_gpio);
    if let Some(ref mut led) = led {
        led.set_as_output();
        for _ in 0..2 {
            led.set_high();
            Timer::after_millis(200).await;
            led.set_low();
            Timer::after_millis(200).await;
        }
    }

    // Build and initialise the ROM reader
    let mut reader = RomReader::new(&pin_map, chip, cs_polarities);
    reader.init();

    debug!("-----");

    loop {
        info!("Reading {} ...", chip.name());

        let results = reader.read();

        for r in &results {
            info!(
                "{}-bit SHA1: {} checksum: {:#010X}",
                r.mode,
                hex::encode(r.sha1),
                r.checksum,
            );
        }

        if results.len() >= 2 {
            let ok = results
                .windows(2)
                .all(|w| w[0].sha1 == w[1].sha1 && w[0].checksum == w[1].checksum);
            info!("Match: {}", ok);
        }

        let ts: Vec<_> = results
            .iter()
            .map(|r| format!("{}-bit: {}", r.mode, r.failures))
            .collect();
        info!("Tristate failures: {}", ts.join(", "));

        info!("-----");
        Timer::after_secs(1).await;
    }
}

/// Parse a CS active-level string from an environment variable.
/// Accepts "high"/"1" (active-high) and "low"/"0" (active-low).
fn parse_active_level(s: &str, var_name: &str) -> bool {
    if s.eq_ignore_ascii_case("high") || s == "1" {
        true
    } else if s.eq_ignore_ascii_case("low") || s == "0" {
        false
    } else {
        panic!(
            "{} must be 'high', '1', 'low', or '0', got '{}'",
            var_name, s
        )
    }
}
