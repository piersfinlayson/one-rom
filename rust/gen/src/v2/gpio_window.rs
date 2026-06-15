// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! PIO GPIO-window constraint, shared by address-range
//! (`addr_layout::derive_addr_layout`) and CS/data-range
//! (`cs_data_layout::derive_cs_data_layout`) layout derivation.
//!
//! Each RP2350 PIO block can only access a 32-GPIO window: GPIOs
//! `[0, 32)` or `[16, 48)` (the two `GPIO_BASE` options; RP2350B has 48
//! GPIOs). A resolved layout's GPIO range - `[gpio_base, gpio_base +
//! span)` - must fit entirely within one of these windows, or the PIO
//! simply cannot reach some of the GPIOs in that range. This isn't
//! captured by either layout's own contiguity/span logic (a range can be
//! contiguous, or meet `MIN_ADDR_PINS`, while still being unreachable by
//! any single PIO), so both derivations check it independently via this
//! shared helper - on the same combo-rejection path as their other
//! per-combo checks (contiguity, select-line shape, etc).

/// Number of GPIOs accessible in a single PIO window.
const PIO_WINDOW_SIZE: u8 = 32;

/// The two possible PIO `GPIO_BASE` values on RP2350B.
const PIO_GPIO_BASES: [u8; 2] = [0, 16];

/// Returns `true` if `[gpio_base, gpio_base + span)` fits entirely within
/// one of the PIO's 32-GPIO windows (`[0,32)` or `[16,48)`).
pub(crate) fn fits_pio_window(gpio_base: u8, span: u8) -> bool {
    let end = gpio_base as u16 + span as u16;
    PIO_GPIO_BASES
        .iter()
        .any(|&pio_base| gpio_base >= pio_base && end <= pio_base as u16 + PIO_WINDOW_SIZE as u16)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_within_first_window() {
        assert!(fits_pio_window(0, 32));
        assert!(fits_pio_window(13, 11));
        assert!(!fits_pio_window(0, 33));
    }

    #[test]
    fn fits_within_second_window() {
        assert!(fits_pio_window(16, 32));
        assert!(fits_pio_window(19, 18));
        assert!(!fits_pio_window(15, 32));
    }

    /// Fire40A's D15/A-1 dual bond (GPIO 15 or GPIO 37), against a
    /// 0-based 16-GPIO data range (D0..=D15).
    #[test]
    fn fire40a_d15_dual_bond() {
        // D0-D14 at GPIO0-14, D15 at GPIO15 -> [0,16): fits.
        assert!(fits_pio_window(0, 16));
        // D0-D14 at GPIO0-14, D15 at GPIO37 -> [0,38): fits neither
        // window (37 >= 32, and gpio_base=0 < 16).
        assert!(!fits_pio_window(0, 38));
    }
}
