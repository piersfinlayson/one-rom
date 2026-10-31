// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_CLAIM_DMA_CHANNEL` and `ORA_ID_CLAIM_UART`.
//!
//! A claim lasts until One ROM reboots and can't be released, so each test
//! restores the claims it found.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::onerom_hw_claim_t as HwClaim;

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
