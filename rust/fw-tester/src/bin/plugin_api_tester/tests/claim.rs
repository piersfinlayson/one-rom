// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_CLAIM_DMA_CHANNEL`, `ORA_ID_CLAIM_UART` and
//! `ORA_ID_CLAIM_GPIO`.
//!
//! A claim lasts until One ROM reboots and can't be released, so each test
//! restores the claims it found.

use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, GpioInfo, OraResult, ffi};
use onerom_metadata::onerom_hw_claim_t as HwClaim;

use super::gpio::{first_gpio_with_use, max_gpios, use_name};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

const ANY: u32 = ffi::ORA_DMA_CHANNEL_ANY;
const DMA_CHANNELS: u32 = 16;
const ALL_DMA_CHANNELS: u16 = 0xFFFF;

/// Channels 0 to 3 for serving, which include the address monitor's, and 15
/// for copying ROM images.
const FIRMWARE_DMA_CHANNELS: u16 = 0x800F;

fn check(what: &str, got: OraResult, want: OraResult) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

fn check_dma_claims(emu: &Emulator, what: &str, want: u16) -> Result<(), String> {
    let got = emu.dma_channel_claims();
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: claims {got:#06x}, want {want:#06x}"))
    }
}

fn check_channel(what: &str, got: u32, want: u32) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: channel {got:#x}, want {want:#x}"))
    }
}

fn dma_refusals(emu: &Emulator) -> Result<(), String> {
    check(
        "NULL channel",
        emu.claim_dma_channel_null(),
        OraResult::InvalidArg,
    )?;
    for (what, channel, flags) in [
        ("channel past the last", DMA_CHANNELS, 0),
        ("channel just below ANY", ANY - 1, 0),
        ("flags bit 0", ANY, 1),
        ("flags bit 31", ANY, 1 << 31),
    ] {
        let (r, out) = emu.claim_dma_channel(channel, flags);
        check(what, r, OraResult::InvalidArg)?;
        check_channel(what, out, channel)?;
    }
    check_dma_claims(emu, "after refusals", FIRMWARE_DMA_CHANNELS)
}

fn dma_claims(emu: &Emulator) -> Result<(), String> {
    check_dma_claims(emu, "at boot", FIRMWARE_DMA_CHANNELS)?;
    dma_refusals(emu)?;

    for channel in (0..DMA_CHANNELS).filter(|ch| FIRMWARE_DMA_CHANNELS & (1 << ch) != 0) {
        let what = format!("firmware channel {channel}");
        let (r, out) = emu.claim_dma_channel(channel, 0);
        check(&what, r, OraResult::InUse)?;
        check_channel(&what, out, channel)?;
    }

    // The highest free channel, so a firmware that claims the lowest free
    // channel instead of the one asked for fails.
    let specific = (0..DMA_CHANNELS)
        .rev()
        .find(|ch| FIRMWARE_DMA_CHANNELS & (1 << ch) == 0)
        .ok_or("no channel is free at boot")?;
    let mut claims = FIRMWARE_DMA_CHANNELS | (1 << specific);
    emu.set_calling_plugin(SYSTEM);
    let (r, out) = emu.claim_dma_channel(specific, 0);
    check("system claims a free channel", r, OraResult::Ok)?;
    check_channel("system claims a free channel", out, specific)?;
    check_dma_claims(emu, "system claims a free channel", claims)?;
    emu.set_calling_plugin(USER);
    let (r, out) = emu.claim_dma_channel(specific, 0);
    check("user claims the system's channel", r, OraResult::InUse)?;
    check_channel("user claims the system's channel", out, specific)?;
    check_dma_claims(emu, "user claims the system's channel", claims)?;

    // Alternate the plugins until every channel is claimed.
    let mut plugin = SYSTEM;
    while claims != ALL_DMA_CHANNELS {
        emu.set_calling_plugin(plugin);
        let (r, out) = emu.claim_dma_channel(ANY, 0);
        check("claim any channel", r, OraResult::Ok)?;
        if (out >= DMA_CHANNELS) || (claims & (1 << out) != 0) {
            return Err(format!(
                "claim any channel: got channel {out:#x} with claims {claims:#06x}"
            ));
        }
        claims |= 1 << out;
        check_dma_claims(emu, "claim any channel", claims)?;
        plugin = if plugin == SYSTEM { USER } else { SYSTEM };
    }
    let (r, out) = emu.claim_dma_channel(ANY, 0);
    check("claim any channel with none free", r, OraResult::InUse)?;
    check_channel("claim any channel with none free", out, ANY)?;
    check_dma_claims(emu, "claim any channel with none free", ALL_DMA_CHANNELS)
}

pub fn test_dma_channel_claims(emu: &Emulator) -> Result<(), String> {
    let saved = emu.dma_channel_claims();
    let result = dma_claims(emu);
    emu.set_dma_channel_claims(saved);
    emu.set_calling_plugin(SYSTEM);
    result
}

fn check_hw_claims(emu: &Emulator, what: &str, want: u8) -> Result<(), String> {
    let got = emu.hw_claims();
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: claims {got:#04x}, want {want:#04x}"))
    }
}

fn uart_claims(emu: &Emulator) -> Result<(), String> {
    check_hw_claims(emu, "at boot", 0)?;

    for (what, uart, flags) in [
        ("UART 2", 2, 0),
        ("UART u32::MAX", u32::MAX, 0),
        ("flags bit 0", 0, 1),
        ("flags bit 31", 0, 1 << 31),
    ] {
        check(what, emu.claim_uart(uart, flags), OraResult::InvalidArg)?;
    }
    check_hw_claims(emu, "after refusals", 0)?;

    emu.set_calling_plugin(SYSTEM);
    check("system claims UART 1", emu.claim_uart(1, 0), OraResult::Ok)?;
    check_hw_claims(emu, "system claims UART 1", HwClaim::HW_CLAIM_UART_1)?;
    emu.set_calling_plugin(USER);
    check(
        "user claims the system's UART",
        emu.claim_uart(1, 0),
        OraResult::InUse,
    )?;
    check("user claims UART 0", emu.claim_uart(0, 0), OraResult::Ok)?;
    let both = HwClaim::HW_CLAIM_UART_0 | HwClaim::HW_CLAIM_UART_1;
    check_hw_claims(emu, "user claims UART 0", both)?;
    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims the user's UART",
        emu.claim_uart(0, 0),
        OraResult::InUse,
    )?;
    check_hw_claims(emu, "system claims the user's UART", both)?;
    check_dma_claims(emu, "after UART claims", FIRMWARE_DMA_CHANNELS)
}

pub fn test_uart_claims(emu: &Emulator) -> Result<(), String> {
    let saved = emu.hw_claims();
    let result = uart_claims(emu);
    emu.set_hw_claims(saved);
    emu.set_calling_plugin(SYSTEM);
    result
}

const FREE: ffi::ora_gpio_use_t = ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE;
const INPUT_FORCED: ffi::ora_gpio_use_t = ffi::ora_gpio_use_t_ORA_GPIO_USE_INPUT_FORCED;
const PLUGIN: ffi::ora_gpio_use_t = ffi::ora_gpio_use_t_ORA_GPIO_USE_PLUGIN;
const HIGH: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_HIGH;
const INPUT: ffi::ora_gpio_state_t = ffi::ora_gpio_state_t_ORA_GPIO_STATE_INPUT;

/// Whether `ora_gpio_set` drives a GPIO with this use only when forced.
fn is_busy(gpio_use: u8) -> bool {
    gpio_use != FREE && gpio_use != INPUT_FORCED
}

fn check_gpio_claims(emu: &Emulator, what: &str, want: u64) -> Result<(), String> {
    let got = emu.gpio_claims();
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: claims {got:#014x}, want {want:#014x}"))
    }
}

fn query(emu: &Emulator, what: &str, gpio: u8) -> Result<GpioInfo, String> {
    match emu.gpio_query(gpio) {
        (OraResult::Ok, info) => Ok(info),
        (r, _) => Err(format!("{what}: query GPIO {gpio}: {r:?}")),
    }
}

fn check_use(emu: &Emulator, what: &str, gpio: u8, want: u8) -> Result<GpioInfo, String> {
    let info = query(emu, what, gpio)?;
    if info.gpio_use == want {
        Ok(info)
    } else {
        Err(format!(
            "{what}: GPIO {gpio} use {}, want {}",
            use_name(info.gpio_use),
            use_name(want)
        ))
    }
}

fn check_uses(emu: &Emulator, what: &str, gpio: u8, want: &[u8]) -> Result<(), String> {
    let info = query(emu, what, gpio)?;
    let want = want.iter().fold(0u32, |uses, &u| uses | (1 << u));
    if info.uses == want {
        Ok(())
    } else {
        Err(format!(
            "{what}: GPIO {gpio} uses 0x{:02X}, want 0x{want:02X}",
            info.uses
        ))
    }
}

/// `ora_gpio_set` drives claimed `gpio` only when forced, and the claim
/// outlasts the set.
fn claimed_gpio_set(emu: &Emulator, gpio: u8) -> Result<(), String> {
    let before = check_use(emu, "claimed", gpio, PLUGIN)?;
    check(
        &format!("set claimed GPIO {gpio} unforced"),
        emu.gpio_set(gpio, HIGH, false),
        OraResult::GpioInUse,
    )?;
    let after = query(emu, "refused set", gpio)?;
    if after != before {
        return Err(format!(
            "refused set moved claimed GPIO {gpio}: {before:?} -> {after:?}"
        ));
    }
    check(
        &format!("force claimed GPIO {gpio} high"),
        emu.gpio_set(gpio, HIGH, true),
        OraResult::Ok,
    )?;
    let forced = check_use(emu, "forced high", gpio, PLUGIN)?;
    if forced.is_output != 1 || forced.level != 1 {
        return Err(format!(
            "forced claimed GPIO {gpio}: is_output={} level={}, want 1/1",
            forced.is_output, forced.level
        ));
    }
    check(
        &format!("release claimed GPIO {gpio}"),
        emu.gpio_set(gpio, INPUT, true),
        OraResult::Ok,
    )?;
    let released = query(emu, "released", gpio)?;
    if released != before {
        return Err(format!(
            "released claimed GPIO {gpio}: {released:?}, want {before:?}"
        ));
    }
    Ok(())
}

fn gpio_claims(emu: &Emulator, board: Board) -> Result<(), String> {
    let max_gpios = max_gpios(board);
    check_gpio_claims(emu, "at boot", 0)?;

    // The lowest and highest free GPIOs, so a claim landing in the wrong byte
    // or bit fails.  Those on a dual-wired X pin are left to
    // test_gpio_claims_x_pin.
    let x_pin_gpios: Vec<u8> = dual_wired_x_pins(board).concat();
    let free: Vec<u8> = (0..max_gpios)
        .filter(|gpio| !x_pin_gpios.contains(gpio))
        .filter(|&gpio| emu.gpio_query(gpio).1.gpio_use == FREE)
        .collect();
    let (&low, &high) = match free[..] {
        [ref low, .., ref high] => (low, high),
        _ => return Err(format!("free GPIOs {free:?}, want two or more")),
    };

    for (what, gpio, flags) in [
        ("GPIO past the last", u32::from(max_gpios), 0),
        ("GPIO 64", 64, 0),
        ("GPIO u32::MAX", u32::MAX, 0),
        ("flags bit 0", u32::from(low), 1),
        ("flags bit 31", u32::from(low), 1 << 31),
    ] {
        check(what, emu.claim_gpio(gpio, flags), OraResult::InvalidArg)?;
    }
    check_gpio_claims(emu, "after refusals", 0)?;

    // Every GPIO One ROM uses, each of the serving, system and X pin uses.
    let mut busy = Vec::new();
    for gpio in 0..max_gpios {
        let info = query(emu, "sweep", gpio)?;
        if is_busy(info.gpio_use) {
            check(
                &format!("claim {} GPIO {gpio}", use_name(info.gpio_use)),
                emu.claim_gpio(u32::from(gpio), 0),
                OraResult::InUse,
            )?;
            busy.push(gpio);
        }
    }
    if busy.is_empty() {
        return Err("no GPIO is in use by One ROM".to_string());
    }
    check_gpio_claims(emu, "after One ROM's GPIOs", 0)?;

    let before = query(emu, "before claim", low)?;
    emu.set_calling_plugin(USER);
    check(
        &format!("user claims free GPIO {low}"),
        emu.claim_gpio(u32::from(low), 0),
        OraResult::Ok,
    )?;
    let mut claims = 1u64 << low;
    check_gpio_claims(emu, "user claims a free GPIO", claims)?;
    let after = check_use(emu, "user claims a free GPIO", low, PLUGIN)?;
    if (after.level, after.is_output) != (before.level, before.is_output) {
        return Err(format!("claim moved GPIO {low}: {before:?} -> {after:?}"));
    }
    check(
        "user claims its own GPIO again",
        emu.claim_gpio(u32::from(low), 0),
        OraResult::InUse,
    )?;
    emu.set_calling_plugin(SYSTEM);
    check(
        "system claims the user's GPIO",
        emu.claim_gpio(u32::from(low), 0),
        OraResult::InUse,
    )?;
    check(
        &format!("system claims free GPIO {high}"),
        emu.claim_gpio(u32::from(high), 0),
        OraResult::Ok,
    )?;
    claims |= 1u64 << high;
    check_gpio_claims(emu, "system claims a free GPIO", claims)?;
    check_use(emu, "system claims a free GPIO", high, PLUGIN)?;
    check_uses(emu, "system claims a free GPIO", high, &[PLUGIN])?;

    claimed_gpio_set(emu, low)?;

    // One ROM doesn't read an input forced GPIO, so it can be claimed, and is
    // then reported as claimed.
    if let Ok(forced) = first_gpio_with_use(emu, max_gpios, INPUT_FORCED) {
        check(
            &format!("claim input forced GPIO {forced}"),
            emu.claim_gpio(u32::from(forced), 0),
            OraResult::Ok,
        )?;
        claims |= 1u64 << forced;
        check_gpio_claims(emu, "claim an input forced GPIO", claims)?;
        check_use(emu, "claim an input forced GPIO", forced, PLUGIN)?;
        check_uses(
            emu,
            "claim an input forced GPIO",
            forced,
            &[INPUT_FORCED, PLUGIN],
        )?;
        println!("  input forced GPIO {forced}");
    }

    for gpio in busy {
        let info = query(emu, "after claims", gpio)?;
        if info.gpio_use == PLUGIN || info.uses & (1 << PLUGIN) != 0 {
            return Err(format!("GPIO {gpio} in use by One ROM reports PLUGIN"));
        }
    }

    println!("  claimed free GPIOs {low} and {high}");
    Ok(())
}

pub fn test_gpio_claims(emu: &Emulator, board: Board) -> Result<(), String> {
    let saved = emu.gpio_claims();
    let result = gpio_claims(emu, board);
    emu.set_gpio_claims(saved);
    emu.set_calling_plugin(SYSTEM);
    result
}

/// `board`'s X pins wired to more than one GPIO.
pub fn dual_wired_x_pins(board: Board) -> Vec<&'static [u8]> {
    [1, 2]
        .into_iter()
        .map(|x_pin| board.gpios_for_x_pin(x_pin))
        .filter(|gpios| gpios.len() > 1)
        .collect()
}

/// A claim on one GPIO of an X pin claims the pin.  Where One ROM uses either
/// GPIO, neither can be claimed.
fn x_pin_claims(emu: &Emulator, gpios: &[u8]) -> Result<&'static str, String> {
    let mut busy = false;
    for &gpio in gpios {
        busy |= is_busy(query(emu, "X pin", gpio)?.gpio_use);
    }

    if busy {
        for &gpio in gpios {
            check(
                &format!("claim GPIO {gpio} of a busy X pin"),
                emu.claim_gpio(u32::from(gpio), 0),
                OraResult::InUse,
            )?;
        }
        check_gpio_claims(emu, "claim a busy X pin", 0)?;
        return Ok("in use by One ROM");
    }

    let (&claimed, others) = gpios.split_first().ok_or("no GPIOs")?;
    check(
        &format!("claim X pin GPIO {claimed}"),
        emu.claim_gpio(u32::from(claimed), 0),
        OraResult::Ok,
    )?;
    check_gpio_claims(emu, "claim an X pin GPIO", 1u64 << claimed)?;
    check_use(emu, "claim an X pin GPIO", claimed, PLUGIN)?;
    for &other in others {
        check(
            &format!("claim GPIO {other} on GPIO {claimed}'s X pin"),
            emu.claim_gpio(u32::from(other), 0),
            OraResult::InUse,
        )?;
        claimed_gpio_set(emu, other)?;
    }
    check_gpio_claims(emu, "claim an X pin's other GPIO", 1u64 << claimed)?;
    Ok("free")
}

pub fn test_gpio_claims_x_pin(emu: &Emulator, board: Board) -> Result<(), String> {
    let saved = emu.gpio_claims();
    let result = dual_wired_x_pins(board).into_iter().try_for_each(|gpios| {
        emu.set_gpio_claims(saved);
        let state = x_pin_claims(emu, gpios)?;
        println!("  X pin GPIOs {gpios:?} {state}");
        Ok(())
    });
    emu.set_gpio_claims(saved);
    result
}
