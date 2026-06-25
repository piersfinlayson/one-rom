// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Build `OneromHardwareInfo`: static per-board metadata, read directly off
//! `Board`'s methods.

use alloc::string::ToString;

use onerom_config::hw::Board;
use onerom_config::mcu::RpVariant;

use onerom_metadata::{
    GPIO_NONE, MAX_IMG_SEL_PINS, MAX_PHYS_PINS, MAX_X_PIN_GPIOS, OneromHardwareInfo, Rp235xVariant,
};

/// Build `OneromHardwareInfo` for `board`.
pub fn build_hardware_info(board: Board) -> OneromHardwareInfo {
    let hw_rev = board.name().to_string();

    // RpVariant's discriminants (Rp235xA=1/Rp235xB=0) match Rp235xVariant's
    // (Rp235xa=1/Rp235xb=0) exactly - a rename, not a remap. Fire boards are
    // guaranteed Some.
    let rp235x = match board
        .rp_variant()
        .expect("Fire boards always have an RP variant")
    {
        RpVariant::Rp235xA => Rp235xVariant::Rp235xa,
        RpVariant::Rp235xB => Rp235xVariant::Rp235xb,
    };

    let num_phys_pins = board.chip_pins();
    let usb_capable = board.has_usb() as u8;
    let gpio_vbus = board.usb_vbus_pin().unwrap_or(GPIO_NONE);
    let gpio_ext_flash_cs = board.external_flash_cs_pin().unwrap_or(GPIO_NONE);
    let gpio_status = board.pin_status();
    let gpio_neopixel = board.pin_neo().unwrap_or(GPIO_NONE);
    let gpio_swdio = board.swdio_sel_pin();
    let gpio_swclk = board.swclk_sel_pin();

    let mut gpio_sel = [GPIO_NONE; MAX_IMG_SEL_PINS];
    for (i, &gpio) in board.sel_pins().iter().enumerate() {
        gpio_sel[i] = gpio;
    }

    // Bitfield: LSB = SEL0's pull direction, bit 1 = SEL1's, etc. (1=pull
    // high, 0=pull low). Pulls can differ per SEL pin, hence the bitfield
    // rather than a single value.
    let sel_jumper_pull = board
        .sel_jumper_pulls()
        .iter()
        .enumerate()
        .fold(0u8, |acc, (i, &pull)| acc | ((pull & 1) << i));

    // gpio_from_phys_pin[phys_pin - 1] = [gpio0, gpio1] (GPIO_NONE for
    // unused slots, including pins not in socket_pin_map() at all - e.g.
    // non-signal GND/VCC pins, or pins beyond this board's chip_pins()).
    let mut gpio_from_phys_pin = [[GPIO_NONE; 2]; MAX_PHYS_PINS];
    for &(phys_pin, gpios) in board.socket_pin_map() {
        let idx = (phys_pin - 1) as usize;
        for (j, &gpio) in gpios.iter().enumerate() {
            gpio_from_phys_pin[idx][j] = gpio;
        }
    }

    // x1[0]/x1[1]: GPIOs for X1 expansion pin. x1[0] is GPIO_NONE if X1 is
    // absent; x1[1] is GPIO_NONE if X1 connects to only one GPIO.
    // Likewise for x2.
    let mut gpio_x1 = [GPIO_NONE; MAX_X_PIN_GPIOS];
    let mut gpio_x2 = [GPIO_NONE; MAX_X_PIN_GPIOS];
    for &(x_pin, gpios) in board.x_pin_map() {
        let target = match x_pin {
            1 => &mut gpio_x1,
            2 => &mut gpio_x2,
            _ => continue,
        };
        for (j, &gpio) in gpios.iter().enumerate().take(MAX_X_PIN_GPIOS) {
            target[j] = gpio;
        }
    }

    OneromHardwareInfo {
        hw_rev,
        rp235x,
        num_phys_pins,
        usb_capable,
        gpio_vbus,
        gpio_ext_flash_cs,
        gpio_status,
        gpio_neopixel,
        gpio_swdio,
        gpio_swclk,
        gpio_sel,
        sel_jumper_pull,
        gpio_from_phys_pin,
        gpio_x1,
        gpio_x2,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Fire24A: full assertions, from its hardware config (no USB, no
    /// external flash, no neopixel, no SWD - status LED on GPIO26, SEL
    /// jumpers on GPIO27-29 all pulled low when closed).
    /// X pin map: X1 → GPIO9, X2 → GPIO8 (single GPIO per pin).
    #[test]
    fn fire24a() {
        let hw = build_hardware_info(Board::Fire24A);

        assert_eq!(hw.hw_rev, Board::Fire24A.name());
        assert_eq!(hw.num_phys_pins, 24);
        assert!(matches!(
            hw.rp235x,
            Rp235xVariant::Rp235xa | Rp235xVariant::Rp235xb
        ));

        assert_eq!(hw.usb_capable, 0);
        assert_eq!(hw.gpio_vbus, GPIO_NONE);
        assert_eq!(hw.gpio_ext_flash_cs, GPIO_NONE);
        assert_eq!(hw.gpio_neopixel, GPIO_NONE);
        assert_eq!(hw.gpio_swdio, GPIO_NONE);
        assert_eq!(hw.gpio_swclk, GPIO_NONE);
        assert_eq!(hw.gpio_status, 26);
        assert_eq!(
            hw.gpio_sel,
            [27, 28, 29, GPIO_NONE, GPIO_NONE, GPIO_NONE, GPIO_NONE]
        );
        assert_eq!(hw.sel_jumper_pull, 0b000);

        // X1 → GPIO9 only; X2 → GPIO8 only.
        assert_eq!(hw.gpio_x1, [9, GPIO_NONE]);
        assert_eq!(hw.gpio_x2, [8, GPIO_NONE]);

        // gpio_from_phys_pin, from Fire24A's socket_pin_map(): pins 1-11,
        // 13-23 each map to a single GPIO; pins 12 and 24 (non-signal) and
        // 25-40 (beyond this 24-pin board) are [GPIO_NONE, GPIO_NONE].
        let expected_pairs: [(u8, u8); 22] = [
            (1, 0),
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 4),
            (6, 5),
            (7, 6),
            (8, 7),
            (9, 16),
            (10, 17),
            (11, 18),
            (13, 19),
            (14, 20),
            (15, 21),
            (16, 22),
            (17, 23),
            (18, 15),
            (19, 14),
            (20, 13),
            (21, 12),
            (22, 11),
            (23, 10),
        ];
        for &(phys_pin, gpio) in &expected_pairs {
            assert_eq!(
                hw.gpio_from_phys_pin[(phys_pin - 1) as usize],
                [gpio, GPIO_NONE],
                "phys pin {phys_pin}"
            );
        }
        for phys_pin in [12u8, 24] {
            assert_eq!(
                hw.gpio_from_phys_pin[(phys_pin - 1) as usize],
                [GPIO_NONE, GPIO_NONE],
                "phys pin {phys_pin} (non-signal)"
            );
        }
        for idx in 24..MAX_PHYS_PINS {
            assert_eq!(
                hw.gpio_from_phys_pin[idx],
                [GPIO_NONE, GPIO_NONE],
                "phys pin {}",
                idx + 1
            );
        }
    }

    /// Fire28C: X1 is dual-bonded [GPIO9, GPIO28], X2 is dual-bonded
    /// [GPIO8, GPIO29].
    #[test]
    fn fire28c_x_pins() {
        let hw = build_hardware_info(Board::Fire28C);
        assert_eq!(hw.gpio_x1, [9, 28]);
        assert_eq!(hw.gpio_x2, [8, 29]);
    }

    /// Fire28A: no X pins.
    #[test]
    fn fire28a_no_x_pins() {
        let hw = build_hardware_info(Board::Fire28A);
        assert_eq!(hw.gpio_x1, [GPIO_NONE, GPIO_NONE]);
        assert_eq!(hw.gpio_x2, [GPIO_NONE, GPIO_NONE]);
    }
}
