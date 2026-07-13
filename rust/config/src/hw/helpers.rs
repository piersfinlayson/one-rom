// Copyright (C) 20256 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Hand-written helper methods for `Board`, supplementing the generated
//! data accessors in `generated.rs`.

use super::generated::Board;
use crate::chip::ChipType;

impl Board {
    /// Get the MCU GPIO(s) connected to a given ROM socket pin
    ///
    /// Returns an empty slice if the pin isn't in `socket_pin_map()`
    /// (e.g. it's a non-signal pin, or this board doesn't define the map).
    pub fn gpios_for_socket_pin(&self, socket_pin: u8) -> &'static [u8] {
        self.socket_pin_map()
            .iter()
            .find(|(pin, _)| *pin == socket_pin)
            .map(|(_, gpios)| *gpios)
            .unwrap_or(&[])
    }

    /// Get the ROM socket pin connected to a given MCU GPIO, if any
    pub fn socket_pin_for_gpio(&self, gpio: u8) -> Option<u8> {
        self.socket_pin_map()
            .iter()
            .find(|(_, gpios)| gpios.contains(&gpio))
            .map(|(pin, _)| *pin)
    }

    /// Get the MCU GPIO(s) connected to a given X header pin
    pub fn gpios_for_x_pin(&self, x_pin: u8) -> &'static [u8] {
        self.x_pin_map()
            .iter()
            .find(|(pin, _)| *pin == x_pin)
            .map(|(_, gpios)| *gpios)
            .unwrap_or(&[])
    }

    /// Get the X header pin connected to a given MCU GPIO, if any
    pub fn x_pin_for_gpio(&self, gpio: u8) -> Option<u8> {
        self.x_pin_map()
            .iter()
            .find(|(_, gpios)| gpios.contains(&gpio))
            .map(|(pin, _)| *pin)
    }

    /// Whether this board permits `chip_type`, either as a natively supported
    /// type or via its extra chip type set.
    ///
    /// This is the combined test used to gate ROM chip types against a board:
    /// the CLI applies it when parsing `--slot` arguments, and the V1 firmware
    /// builder applies it (unconditionally) at build time. Plugin chip types
    /// are not covered here — callers handle those separately.
    pub fn allows_chip_type(&self, chip_type: ChipType) -> bool {
        self.supports_chip_type(chip_type) || self.extra_chip_types().contains(&chip_type)
    }
}
