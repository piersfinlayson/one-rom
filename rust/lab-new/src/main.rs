// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use embassy_executor::Spawner;
use embassy_executor::main as embassy_main;
use embassy_rp::{clocks::ClockConfig, config::Config};
use embassy_time::Timer;

use embedded_alloc::LlffHeap as Heap;
use panic_rtt_target as _;

use onerom_config::hw::Board;
use onerom_config::pin_map::BoardPinMap;

mod cli;
mod error;
mod hw;
mod logs;
mod output;
mod rom;
mod usb;

use rom::CsPolarities;

pub const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Build-time configuration via environment variables
// ---------------------------------------------------------------------------

const BOARD_STR: Option<&str> = option_env!("BOARD");

const CS1_STR: Option<&str> = option_env!("CS1");
const CS2_STR: Option<&str> = option_env!("CS2");
const CS3_STR: Option<&str> = option_env!("CS3");

// ---------------------------------------------------------------------------

#[global_allocator]
static HEAP: Heap = Heap::empty();

#[embassy_main]
async fn main(spawner: Spawner) -> ! {
    // Heap
    {
        use core::mem::MaybeUninit;
        const HEAP_SIZE: usize = 4096;
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
    let p = embassy_rp::init(config);
    debug!("Clocks configured to 150MHz");

    let usb_device = usb::Usb::new(p.USB);
    usb::run(spawner, usb_device);

    // Build the physical-pin → MCU GPIO map for this board
    let board = BOARD_STR
        .map(|s| Board::try_from_str(s).unwrap_or_else(|| panic!("Unknown board '{}'", s)));
    if let Some(board) = board {
        debug!("Board: {}", board.name());
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
    } else {
        debug!("Board: (not set)");
    }

    debug!("-----");

    let mut state = cli::SessionState::new(board);
    cli::run(&mut state).await
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

pub fn cs_polarities() -> CsPolarities {
    CsPolarities {
        cs1: CS1_STR.map(|s| parse_active_level(s, "CS1")),
        cs2: CS2_STR.map(|s| parse_active_level(s, "CS2")),
        cs3: CS3_STR.map(|s| parse_active_level(s, "CS3")),
    }
}
