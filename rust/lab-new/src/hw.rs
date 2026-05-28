// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

use embassy_rp::gpio::Flex;
use embassy_rp::peripherals::*;

/// Steal a GPIO pin by number and wrap it as a [`Flex`] pin.
///
/// # Panics
/// Panics if `gpio_num` is outside 0–47 (the valid range for RP2350B).
///
/// # Safety
/// Uses unsafe peripheral stealing.  The caller must ensure each GPIO
/// number is only stolen once across the lifetime of the application.
pub fn steal_gpio(gpio_num: u8) -> Flex<'static> {
    match gpio_num {
        0 => Flex::new(unsafe { PIN_0::steal() }),
        1 => Flex::new(unsafe { PIN_1::steal() }),
        2 => Flex::new(unsafe { PIN_2::steal() }),
        3 => Flex::new(unsafe { PIN_3::steal() }),
        4 => Flex::new(unsafe { PIN_4::steal() }),
        5 => Flex::new(unsafe { PIN_5::steal() }),
        6 => Flex::new(unsafe { PIN_6::steal() }),
        7 => Flex::new(unsafe { PIN_7::steal() }),
        8 => Flex::new(unsafe { PIN_8::steal() }),
        9 => Flex::new(unsafe { PIN_9::steal() }),
        10 => Flex::new(unsafe { PIN_10::steal() }),
        11 => Flex::new(unsafe { PIN_11::steal() }),
        12 => Flex::new(unsafe { PIN_12::steal() }),
        13 => Flex::new(unsafe { PIN_13::steal() }),
        14 => Flex::new(unsafe { PIN_14::steal() }),
        15 => Flex::new(unsafe { PIN_15::steal() }),
        16 => Flex::new(unsafe { PIN_16::steal() }),
        17 => Flex::new(unsafe { PIN_17::steal() }),
        18 => Flex::new(unsafe { PIN_18::steal() }),
        19 => Flex::new(unsafe { PIN_19::steal() }),
        20 => Flex::new(unsafe { PIN_20::steal() }),
        21 => Flex::new(unsafe { PIN_21::steal() }),
        22 => Flex::new(unsafe { PIN_22::steal() }),
        23 => Flex::new(unsafe { PIN_23::steal() }),
        24 => Flex::new(unsafe { PIN_24::steal() }),
        25 => Flex::new(unsafe { PIN_25::steal() }),
        26 => Flex::new(unsafe { PIN_26::steal() }),
        27 => Flex::new(unsafe { PIN_27::steal() }),
        28 => Flex::new(unsafe { PIN_28::steal() }),
        29 => Flex::new(unsafe { PIN_29::steal() }),
        #[cfg(feature = "rp2350b")]
        30 => Flex::new(unsafe { PIN_30::steal() }),
        #[cfg(feature = "rp2350b")]
        31 => Flex::new(unsafe { PIN_31::steal() }),
        #[cfg(feature = "rp2350b")]
        32 => Flex::new(unsafe { PIN_32::steal() }),
        #[cfg(feature = "rp2350b")]
        33 => Flex::new(unsafe { PIN_33::steal() }),
        #[cfg(feature = "rp2350b")]
        34 => Flex::new(unsafe { PIN_34::steal() }),
        #[cfg(feature = "rp2350b")]
        35 => Flex::new(unsafe { PIN_35::steal() }),
        #[cfg(feature = "rp2350b")]
        36 => Flex::new(unsafe { PIN_36::steal() }),
        #[cfg(feature = "rp2350b")]
        37 => Flex::new(unsafe { PIN_37::steal() }),
        #[cfg(feature = "rp2350b")]
        38 => Flex::new(unsafe { PIN_38::steal() }),
        #[cfg(feature = "rp2350b")]
        39 => Flex::new(unsafe { PIN_39::steal() }),
        #[cfg(feature = "rp2350b")]
        40 => Flex::new(unsafe { PIN_40::steal() }),
        #[cfg(feature = "rp2350b")]
        41 => Flex::new(unsafe { PIN_41::steal() }),
        #[cfg(feature = "rp2350b")]
        42 => Flex::new(unsafe { PIN_42::steal() }),
        #[cfg(feature = "rp2350b")]
        43 => Flex::new(unsafe { PIN_43::steal() }),
        #[cfg(feature = "rp2350b")]
        44 => Flex::new(unsafe { PIN_44::steal() }),
        #[cfg(feature = "rp2350b")]
        45 => Flex::new(unsafe { PIN_45::steal() }),
        #[cfg(feature = "rp2350b")]
        46 => Flex::new(unsafe { PIN_46::steal() }),
        #[cfg(feature = "rp2350b")]
        47 => Flex::new(unsafe { PIN_47::steal() }),
        n => panic!("GPIO {} out of range for RP2350 (valid: 0-47)", n),
    }
}
