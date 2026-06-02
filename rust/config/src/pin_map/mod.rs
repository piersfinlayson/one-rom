// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Physical socket pin to MCU GPIO mapping.
//!
//! [`BoardPinMap`] takes a [`Board`] and produces a complete, immutable map
//! of every physical socket pin to the MCU GPIO it is wired to on that board.
//!
//! The map is independent of which chip is in the socket.  The ROM reader
//! uses [`ChipType`] separately to determine which physical pins to query.
//!
//! # Construction
//!
//! The mapping is derived using the canonical reference chip for each socket
//! size, chosen because it occupies every GPIO-connected pin of that socket
//! across its address, data, and control lines:
//!
//! | Socket | Reference chip |
//! |--------|----------------|
//! | 24-pin | 2364           |
//! | 28-pin | 27512          |
//! | 32-pin | 27C040         |
//! | 40-pin | 27C400         |
//!
//! `board.addr_pins()[n]` gives the MCU GPIO for address line An as laid out
//! by the reference chip, and `reference_chip.address_pins()[n]` gives the
//! physical socket pin for An.  The same relationship holds for data lines.
//! Control line physical pins are taken from the reference chip's
//! `control_lines()` and resolved to GPIOs via the board's per-signal
//! methods.

use crate::chip::ChipType;
use crate::hw::Board;

/// Immutable map of every physical socket pin to the MCU GPIO it is
/// connected to on a specific One ROM board variant.
///
/// Physical pin numbers are 1-based (as printed in chip datasheets).
/// Pins not connected to an MCU GPIO (VCC, GND) return `None` from
/// [`gpio_for_chip_pin`](BoardPinMap::gpio_for_chip_pin).
#[derive(Clone, Copy, Debug)]
pub struct BoardPinMap {
    /// Physical pin → MCU GPIO.  Index is `physical_pin - 1` (0-based).
    /// Sentinel 255 = not connected to an MCU GPIO.
    pins: [u8; 40],

    /// Number of physical pins on the socket (24, 28, 32, or 40).
    chip_pins: u8,

    /// MCU GPIO for the board's status LED; 255 if absent.
    led_gpio: u8,

    /// The board this map was built for, for reference.
    _board: Board,
}

impl BoardPinMap {
    /// Build the physical-pin → MCU-GPIO map for `board`.
    pub fn new(board: Board) -> Self {
        let ref_chip = reference_chip(board.chip_pins());
        let mut pins = [255u8; 40];

        // Address lines
        let chip_addr_phys = ref_chip.address_pins();
        let board_addr_gpios = board.addr_pins();
        debug_assert!(
            chip_addr_phys.len() <= board_addr_gpios.len(),
            "board addr_pins shorter than reference chip address line count"
        );
        for n in 0..chip_addr_phys.len() {
            set_pin(&mut pins, chip_addr_phys[n], board_addr_gpios[n]);
        }

        // Data lines
        let chip_data_phys = ref_chip.data_pins();
        let board_data_gpios = board.data_pins();
        debug_assert!(
            chip_data_phys.len() <= board_data_gpios.len(),
            "board data_pins shorter than reference chip data line count"
        );
        for n in 0..chip_data_phys.len() {
            set_pin(&mut pins, chip_data_phys[n], board_data_gpios[n]);
        }

        // Control lines
        for ctrl in ref_chip.control_lines() {
            let gpio = board_gpio_for_control(board, ref_chip, ctrl.name);
            if gpio != 255 {
                set_pin(&mut pins, ctrl.pin, gpio);
            }
        }

        // Programming pins (VPP, PGM etc.)
        if let Some(prog_pins) = ref_chip.programming_pins() {
            for spec in prog_pins {
                if let Some(gpio) = board.alt_pin(&ref_chip, spec.name) {
                    set_pin(&mut pins, spec.pin, gpio);
                }
            }
        }

        Self {
            pins,
            chip_pins: board.chip_pins(),
            led_gpio: board.pin_status(),
            _board: board,
        }
    }

    /// Return the MCU GPIO connected to `physical_pin` (1-based), or `None`
    /// if that pin is not wired to an MCU GPIO (e.g. VCC or GND).
    pub fn gpio_for_chip_pin(&self, physical_pin: u8) -> Option<u8> {
        if physical_pin == 0 || physical_pin as usize > self.chip_pins as usize {
            return None;
        }
        match self.pins[physical_pin as usize - 1] {
            255 => None,
            gpio => Some(gpio),
        }
    }

    /// Number of physical pins on the socket.
    pub fn chip_pins(&self) -> u8 {
        self.chip_pins
    }

    /// MCU GPIO for the board's status LED, or `None` if absent.
    pub fn led_gpio(&self) -> Option<u8> {
        match self.led_gpio {
            255 => None,
            gpio => Some(gpio),
        }
    }

    /// Iterate over all connected (physical_pin, mcu_gpio) pairs in
    /// ascending pin order.  Unconnected pins (VCC, GND) are skipped.
    /// `physical_pin` is 1-based.
    pub fn iter(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.pins[..self.chip_pins as usize]
            .iter()
            .enumerate()
            .filter_map(|(i, &gpio)| {
                if gpio != 255 {
                    Some((i as u8 + 1, gpio))
                } else {
                    None
                }
            })
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// The canonical reference chip for a given socket pin count.
///
/// Panics on unknown counts — all valid [`Board`] variants return one of
/// 24, 28, 32, or 40 from [`Board::chip_pins`].
fn reference_chip(chip_pins: u8) -> ChipType {
    match chip_pins {
        24 => ChipType::Chip2364,
        28 => ChipType::Chip27512,
        32 => ChipType::Chip27C040,
        40 => ChipType::Chip27C400,
        n  => panic!("no reference chip defined for {}-pin socket", n),
    }
}

/// Resolve the MCU GPIO for a named control signal using the board's
/// per-signal accessor.  Returns 255 if the signal name is not recognised.
fn board_gpio_for_control(board: Board, chip: ChipType, name: &str) -> u8 {
    match name {
        "ce"   => board.pin_ce(chip),
        "oe"   => board.pin_oe(chip),
        "cs1"  => board.pin_cs1(chip),
        "cs2"  => board.pin_cs2(chip),
        "cs3"  => board.pin_cs3(chip),
        "byte" => board.pin_byte(),
        _      => 255,
    }
}

/// Write `gpio` into `pins` at the 0-based index for 1-based `physical_pin`.
/// Silently ignores out-of-range values.
#[inline]
fn set_pin(pins: &mut [u8; 40], physical_pin: u8, gpio: u8) {
    if (1..=40).contains(&physical_pin) {
        pins[physical_pin as usize - 1] = gpio;
    }
}