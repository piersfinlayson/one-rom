// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Resetting the host system a One ROM is installed in.
//!
//! A wire from a One ROM pad to the host's reset line lets the device restart
//! the machine it lives in - after programming a new image, or on demand. Two
//! halves live here: deciding whether a pin can carry that wire, which is a
//! question about board and chip metadata and needs no device, and sending the
//! pulse, which needs nothing but a device.
//!
//! What the user is told about either is the caller's business.

use crate::gpio;
use crate::usb::{Caps, GpioSetArgs, GpioState, gpio_set};
use crate::{Device, Error};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::PinTolerance;

/// A reason a GPIO cannot carry a reset wire, or should not without a word to
/// the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinObjection {
    /// One ROM itself uses the pin, for the functions named. The device refuses
    /// to give up such a pin, so this is fatal unless the user overrides it with
    /// `control pin --force`, which breaks serving.
    InUse(Vec<String>),

    /// The pad is not 5V-tolerant. Whether that matters depends on what the wire
    /// reaches, which nothing here knows, so this one is for the user to weigh.
    NotFiveVoltTolerant,
}

/// Everything standing between `gpio_num` and a reset wire, on this board, for
/// an image that can serve `chips`.
///
/// Answered from static metadata alone, so it holds before the image is flashed.
/// That is the point of it: a device can only be asked about the image it is
/// already running.
///
/// `chips` is every chip type the image can serve, because which pins One ROM
/// uses depends on the chip it is emulating, and an image may hold several. An
/// empty list is an image that serves nothing.
pub fn vet_pin(board: &Board, chips: &[ChipType], gpio_num: u8) -> Vec<PinObjection> {
    let mut objections = Vec::new();

    let mut uses: Vec<String> = Vec::new();
    for chip in chips {
        if let Some(function) = gpio::rom_function(board, *chip, gpio_num)
            && !uses.contains(&function)
        {
            uses.push(function);
        }
    }
    for function in gpio::system_functions(board, gpio_num) {
        uses.push(function.to_string());
    }
    if !uses.is_empty() {
        objections.push(PinObjection::InUse(uses));
    }

    // Static board metadata, not a measurement: the RP2350's ADC pins are the
    // only pads that are not 5V-tolerant. Nothing here knows or asks what the
    // pad is wired to.
    if board.gpio_tolerance(gpio_num) == Some(PinTolerance::ThreeVolt3) {
        objections.push(PinObjection::NotFiveVoltTolerant);
    }

    objections
}

/// Pulse a reset line low for `hold_ms` milliseconds.
///
/// The pin is driven low and then released to high impedance, never driven high:
/// a reset net has its own pull-up and may have other drivers on it.
///
/// The device times the pulse, so it completes even if the host that asked for
/// it does not. `hold_ms` must not be zero, which reaches the device as "latch
/// until something changes it" and on a reset line means holding the host down
/// for ever. The caller is expected to have refused that already.
pub async fn pulse(device: &Device, caps: &Caps, gpio_num: u8, hold_ms: u32) -> Result<(), Error> {
    debug_assert!(hold_ms > 0, "a reset pulse with no end is not a reset");
    gpio_set(
        device,
        caps,
        GpioSetArgs {
            gpio: gpio_num,
            state: GpioState::Low,
            after_state: GpioState::Input,
            flags: 0,
            duration_ms: hold_ms,
        },
    )
    .await
}
