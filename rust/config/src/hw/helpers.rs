// Copyright (C) 20256 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Hand-written helper methods for `Board`, supplementing the generated
//! data accessors in `generated.rs`.

use super::generated::Board;
use crate::chip::ChipType;
use crate::mcu::PinTolerance;

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
    /// This is the **V1** (pre-0.7.0 firmware) gate, and the V1 firmware
    /// builder is now its only consumer: V1 serves a fixed set of chip types
    /// per board, so a type outside it genuinely cannot be served. V2 derives
    /// what a board can serve from the address and CS/data layouts instead
    /// (`onerom_gen::compat::check_chip_set_on_board`), which admits every
    /// overhang and fly-lead combination `docs/COMPATIBILITY.md` documents —
    /// so do not use this to gate a V2 build. Plugin chip types are not covered
    /// here either; callers handle those separately.
    pub fn allows_chip_type(&self, chip_type: ChipType) -> bool {
        self.supports_chip_type(chip_type) || self.extra_chip_types().contains(&chip_type)
    }

    /// The over-voltage tolerance of an MCU GPIO on this board.
    ///
    /// Returns `None` when it cannot be determined: only the RP2350 ADC-pin
    /// exception is modelled, so STM32F4 (Ice) boards — whose relevant GPIOs are
    /// all 5V-tolerant by design, but are not characterised pin-by-pin here —
    /// yield `None`. On RP2350 (Fire) boards the tolerance comes from the
    /// board's [`RpVariant`](crate::mcu::RpVariant) via
    /// [`RpVariant::gpio_tolerance`](crate::mcu::RpVariant::gpio_tolerance).
    pub fn gpio_tolerance(&self, gpio: u8) -> Option<PinTolerance> {
        self.rp_variant().map(|v| v.gpio_tolerance(gpio))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::BOARDS;

    /// Every board reports GPIO tolerance consistently: RP2350 (Fire) boards
    /// flag their ADC pins 3.3V-only and everything else 5V-tolerant; STM32
    /// (Ice) boards are uncharacterised and return `None`.
    #[test]
    fn gpio_tolerance_only_characterised_on_rp2350_boards() {
        for board in BOARDS {
            match board.rp_variant() {
                Some(variant) => {
                    for &adc in variant.adc_gpios() {
                        assert_eq!(
                            board.gpio_tolerance(adc),
                            Some(PinTolerance::ThreeVolt3),
                            "{board:?} GPIO{adc}"
                        );
                    }
                    // A non-ADC GPIO (0 is never an ADC pin on either package).
                    assert_eq!(board.gpio_tolerance(0), Some(PinTolerance::FiveVolt));
                }
                None => {
                    assert_eq!(board.gpio_tolerance(0), None, "{board:?}");
                    assert_eq!(board.gpio_tolerance(26), None, "{board:?}");
                }
            }
        }
    }
}
