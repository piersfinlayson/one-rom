// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_CLAIM_DMA_CHANNEL`, `ORA_ID_CLAIM_UART`,
//! `ORA_ID_CLAIM_GPIO` and `ORA_ID_CLAIM_PIO`.
//!
//! A claim lasts until One ROM reboots and can't be released, so each test
//! restores the claims it found.

use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, GpioInfo, OraResult, ffi};
use onerom_metadata::onerom_hw_claim_t as HwClaim;
use onerom_metadata::onerom_pio_block_claim_t as PioClaim;
use onerom_metadata::onerom_rgb_led_pio_t as RgbLedPio;

use super::gpio::{first_gpio_with_use, max_gpios, use_name};
use super::led::with_engine;

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

const PIO_BLOCKS: u32 = 3;
const PIO_SMS: u32 = 4;
const PIO_INSTRS: u32 = 32;
const BLOCK_ANY: u32 = ffi::ORA_PIO_BLOCK_ANY;
const SM_ANY: u32 = ffi::ORA_PIO_SM_ANY;
const INSTR_ANY: u32 = ffi::ORA_PIO_INSTR_ANY;
const GPIOBASE_0: ffi::ora_pio_gpiobase_t = ffi::ora_pio_gpiobase_t_ORA_PIO_GPIOBASE_0;
const GPIOBASE_16: ffi::ora_pio_gpiobase_t = ffi::ora_pio_gpiobase_t_ORA_PIO_GPIOBASE_16;

/// The apio emulation's record of how the firmware set up the PIO blocks.
fn apio() -> ffi::_apio_emulated_pio_t {
    // SAFETY: `_apio_emulated_pio` is a plain C global, and the emulator is
    // single threaded.
    unsafe { core::ptr::read(core::ptr::addr_of!(ffi::_apio_emulated_pio)) }
}

fn set_apio_gpio_base(gpio_base: [u32; 3]) {
    // SAFETY: as for `apio`.
    unsafe { (*core::ptr::addr_of_mut!(ffi::_apio_emulated_pio)).gpio_base = gpio_base };
}

fn block_sms(claims: u16, block: u32) -> u16 {
    (claims >> (block * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT)) & PioClaim::PIO_CLAIM_BLOCK_0_SMS
}

fn block_is_16(claims: u16, block: u32) -> bool {
    claims & (PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16 << block) != 0
}

fn run(first: u32, num: u32) -> u32 {
    if num >= PIO_INSTRS {
        u32::MAX
    } else {
        ((1u32 << num) - 1) << first
    }
}

type PioClaims = ([u32; 3], u16);

fn pio_claims(emu: &Emulator) -> PioClaims {
    (emu.pio_instr_claims(), emu.pio_block_claims())
}

fn check_pio_claims(emu: &Emulator, what: &str, want: PioClaims) -> Result<(), String> {
    let got = pio_claims(emu);
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: claims {got:x?}, want {want:x?}"))
    }
}

/// The PIO claims for serving match the length of serving's programs in the
/// emulated PIO, and the RGB LED's claims match `rgb_led_pio`.
fn pio_boot_claims(emu: &Emulator, has_rgb: bool) -> Result<(), String> {
    let (instrs, claims) = pio_claims(emu);
    let apio = apio();
    let rgb = emu.rgb_led_pio();
    let rgb_block =
        u32::from(rgb & RgbLedPio::RGB_LED_PIO_BLOCK) >> RgbLedPio::RGB_LED_PIO_BLOCK_SHIFT;
    let rgb_sm = u32::from(rgb & RgbLedPio::RGB_LED_PIO_SM) >> RgbLedPio::RGB_LED_PIO_SM_SHIFT;

    for block in 0..PIO_BLOCKS {
        let led_here = has_rgb && (rgb_block == block);
        let led_first = u32::from(apio.first_instr[block as usize][rgb_sm as usize]);
        // Serving's programs end where the emulated PIO's record of the block
        // ends.  In a block shared with the RGB LED they end where the LED's
        // program starts.
        let serving = if led_here {
            led_first
        } else {
            u32::from(apio.max_offset[block as usize])
        };
        let (mut want_instrs, mut want_sms) = match block {
            // The address block runs one state machine, and the CS/data block
            // two.
            1 => (run(0, serving), 0b0001),
            2 => (run(0, serving), 0b0011),
            _ => (0, 0),
        };
        if led_here {
            want_instrs |= run(led_first, 4);
            want_sms |= 1 << rgb_sm;
        }
        if instrs[block as usize] != want_instrs {
            return Err(format!(
                "block {block}: instruction claims {:#010x}, want {want_instrs:#010x}",
                instrs[block as usize]
            ));
        }
        if block_sms(claims, block) != want_sms {
            return Err(format!(
                "block {block}: state machine claims {:#x}, want {want_sms:#x}",
                block_sms(claims, block)
            ));
        }
        if (want_sms != 0) && (block_is_16(claims, block) != (apio.gpio_base[block as usize] == 16))
        {
            return Err(format!(
                "block {block}: GPIOBASE 16 claim bit {}, emulated GPIOBASE {}",
                block_is_16(claims, block),
                apio.gpio_base[block as usize]
            ));
        }
    }

    if !has_rgb {
        if rgb != onerom_metadata::RGB_LED_PIO_NONE {
            return Err(format!("no RGB LED, but rgb_led_pio is {rgb:#04x}"));
        }
    } else if (rgb_block >= PIO_BLOCKS)
        || (apio.enabled_sms[rgb_block as usize] & (1 << rgb_sm) == 0)
    {
        return Err(format!("RGB LED state machine {rgb:#04x} isn't running"));
    }
    Ok(())
}

fn pio_refusals(emu: &Emulator) -> Result<(), String> {
    let before = pio_claims(emu);
    for (what, block, sm, first, num, base, flags) in [
        (
            "block NULL",
            None,
            Some(SM_ANY),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "sm NULL",
            Some(BLOCK_ANY),
            None,
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "first_instr NULL",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            None,
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "no instructions",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            Some(INSTR_ANY),
            0,
            GPIOBASE_0,
            0,
        ),
        (
            "33 instructions",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            Some(INSTR_ANY),
            33,
            GPIOBASE_0,
            0,
        ),
        (
            "block 3",
            Some(PIO_BLOCKS),
            Some(SM_ANY),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "SM 4",
            Some(0),
            Some(PIO_SMS),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "instruction 32",
            Some(0),
            Some(SM_ANY),
            Some(PIO_INSTRS),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "run past the last instruction",
            Some(0),
            Some(SM_ANY),
            Some(29),
            4,
            GPIOBASE_0,
            0,
        ),
        (
            "GPIOBASE 1",
            Some(0),
            Some(SM_ANY),
            Some(INSTR_ANY),
            1,
            1,
            0,
        ),
        (
            "specific SM in any block",
            Some(BLOCK_ANY),
            Some(0),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "specific instruction in any block",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            Some(0),
            1,
            GPIOBASE_0,
            0,
        ),
        (
            "flags bit 0",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            1,
        ),
        (
            "flags bit 31",
            Some(BLOCK_ANY),
            Some(SM_ANY),
            Some(INSTR_ANY),
            1,
            GPIOBASE_0,
            1 << 31,
        ),
    ] {
        let (r, b, s, f) = emu.claim_pio(block, sm, first, num, base, flags);
        check(what, r, OraResult::InvalidArg)?;
        if (b, s, f) != (block, sm, first) {
            return Err(format!("{what}: arguments changed to {b:?}/{s:?}/{f:?}"));
        }
        check_pio_claims(emu, what, before)?;
    }
    Ok(())
}

/// Specific claims in each block.  Each block is emptied before its own test.
fn pio_specific_claims(emu: &Emulator) -> Result<(), String> {
    (0..PIO_BLOCKS).try_for_each(|block| pio_specific_claims_in(emu, block))
}

fn pio_specific_claims_in(emu: &Emulator, block: u32) -> Result<(), String> {
    let at = |what: &str| format!("block {block}: {what}");
    let sm_shift = block * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT;
    let base_16 = PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16 << block;
    let (mut instrs, mut claims) = pio_claims(emu);
    instrs[block as usize] = 0;
    claims &= !((PioClaim::PIO_CLAIM_BLOCK_0_SMS << sm_shift) | base_16);
    emu.set_pio_instr_claims(instrs);
    emu.set_pio_block_claims(claims);

    // Claim at the top of the block, so the test fails if the lowest free
    // state machine or run is claimed instead of the one requested.
    emu.set_calling_plugin(SYSTEM);
    let (r, b, s, f) = emu.claim_pio(Some(block), Some(3), Some(28), 4, GPIOBASE_16, 0);
    check(&at("specific claim"), r, OraResult::Ok)?;
    if (b, s, f) != (Some(block), Some(3), Some(28)) {
        return Err(at(&format!("specific claim: claimed {b:?}/{s:?}/{f:?}")));
    }
    instrs[block as usize] = run(28, 4);
    claims |= ((1 << 3) << sm_shift) | base_16;
    check_pio_claims(emu, &at("specific claim"), (instrs, claims))?;
    if apio().gpio_base[block as usize] != 16 {
        return Err(at(&format!(
            "specific claim: emulated GPIOBASE {}, want 16",
            apio().gpio_base[block as usize]
        )));
    }

    emu.set_calling_plugin(USER);
    for (what, sm, first, base, want) in [
        (
            "the same state machine",
            3,
            INSTR_ANY,
            GPIOBASE_16,
            OraResult::InUse,
        ),
        (
            "a claimed instruction",
            SM_ANY,
            31,
            GPIOBASE_16,
            OraResult::InUse,
        ),
        (
            "the other GPIOBASE",
            SM_ANY,
            INSTR_ANY,
            GPIOBASE_0,
            OraResult::Conflict,
        ),
    ] {
        let (r, b, s, f) = emu.claim_pio(Some(block), Some(sm), Some(first), 1, base, 0);
        check(&at(what), r, want)?;
        if (b, s, f) != (Some(block), Some(sm), Some(first)) {
            return Err(at(&format!(
                "{what}: arguments changed to {b:?}/{s:?}/{f:?}"
            )));
        }
        check_pio_claims(emu, &at(what), (instrs, claims))?;
    }
    Ok(())
}

/// Claims in any block, alternating the plugins, until nothing is left.
fn pio_any_claims(emu: &Emulator, base: ffi::ora_pio_gpiobase_t) -> Result<(), String> {
    let want_16 = base == GPIOBASE_16;
    let mut plugin = SYSTEM;
    loop {
        let (instrs, claims) = pio_claims(emu);
        emu.set_calling_plugin(plugin);
        let (r, b, s, f) =
            emu.claim_pio(Some(BLOCK_ANY), Some(SM_ANY), Some(INSTR_ANY), 2, base, 0);
        if r == OraResult::InUse {
            check_pio_claims(emu, "claim with none free", (instrs, claims))?;
            for block in 0..PIO_BLOCKS {
                let free_sms = !block_sms(claims, block) & PioClaim::PIO_CLAIM_BLOCK_0_SMS;
                let usable = (block_sms(claims, block) == 0 && instrs[block as usize] == 0)
                    || (block_is_16(claims, block) == want_16);
                let room = (0..=PIO_INSTRS - 2).any(|i| instrs[block as usize] & run(i, 2) == 0);
                if usable && (free_sms != 0) && room {
                    return Err(format!("claim with none free: block {block} has room"));
                }
            }
            return Ok(());
        }
        check("claim any", r, OraResult::Ok)?;
        let (b, s, f) = (
            b.unwrap_or(u32::MAX),
            s.unwrap_or(u32::MAX),
            f.unwrap_or(u32::MAX),
        );
        if (b >= PIO_BLOCKS) || (s >= PIO_SMS) || (f > PIO_INSTRS - 2) {
            return Err(format!("claim any: claimed {b}/{s}/{f}"));
        }
        let in_use = block_sms(claims, b) != 0 || instrs[b as usize] != 0;
        if in_use && (block_is_16(claims, b) != want_16) {
            return Err(format!("claim any: block {b} has the other GPIOBASE"));
        }
        if (block_sms(claims, b) & (1 << s) != 0) || (instrs[b as usize] & run(f, 2) != 0) {
            return Err(format!("claim any: {b}/{s}/{f} was already claimed"));
        }
        let mut want_instrs = instrs;
        want_instrs[b as usize] |= run(f, 2);
        let mut want_claims = claims | ((1 << s) << (b * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT));
        if want_16 {
            want_claims |= PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16 << b;
        }
        check_pio_claims(emu, "claim any", (want_instrs, want_claims))?;
        plugin = if plugin == SYSTEM { USER } else { SYSTEM };
    }
}

pub fn test_pio_claims(emu: &Emulator, has_rgb: bool) -> Result<(), String> {
    let saved = pio_claims(emu);
    let saved_gpio_base = apio().gpio_base;
    let restore = || {
        emu.set_pio_instr_claims(saved.0);
        emu.set_pio_block_claims(saved.1);
        set_apio_gpio_base(saved_gpio_base);
        emu.set_calling_plugin(SYSTEM);
    };

    let mut result = pio_boot_claims(emu, has_rgb)
        .and_then(|()| pio_refusals(emu))
        .and_then(|()| pio_specific_claims(emu));
    restore();
    for base in [GPIOBASE_0, GPIOBASE_16] {
        if result.is_ok() {
            result = pio_any_claims(emu, base);
            restore();
        }
    }
    result
}

/// The RGB LED's PIO program length, in instructions.
const LED_INSTRS: u32 = 4;

/// The block the RGB LED's state machine is claimed in, given the claims made
/// before it.  A block in use with the LED's GPIOBASE is preferred.  Otherwise
/// it is the first block with room that is unused or has that GPIOBASE.
fn led_block_choice(instrs: [u32; 3], claims: u16, want_16: bool) -> Option<u32> {
    let in_use = |b: u32| block_sms(claims, b) != 0 || instrs[b as usize] != 0;
    let room = |b: u32| {
        block_sms(claims, b) != PioClaim::PIO_CLAIM_BLOCK_0_SMS
            && (0..=PIO_INSTRS - LED_INSTRS).any(|i| instrs[b as usize] & run(i, LED_INSTRS) == 0)
    };
    let preferred = (0..PIO_BLOCKS).find(|&b| in_use(b) && block_is_16(claims, b) == want_16);
    if let Some(b) = preferred.filter(|&b| room(b)) {
        return Some(b);
    }
    (0..PIO_BLOCKS).find(|&b| (!in_use(b) || block_is_16(claims, b) == want_16) && room(b))
}

/// The RGB LED's PIO block and state machine, or `None` if none is claimed.
fn rgb_location(emu: &Emulator) -> Option<(u32, u32)> {
    let rgb = emu.rgb_led_pio();
    (rgb != onerom_metadata::RGB_LED_PIO_NONE).then(|| {
        (
            u32::from(rgb & RgbLedPio::RGB_LED_PIO_BLOCK) >> RgbLedPio::RGB_LED_PIO_BLOCK_SHIFT,
            u32::from(rgb & RgbLedPio::RGB_LED_PIO_SM) >> RgbLedPio::RGB_LED_PIO_SM_SHIFT,
        )
    })
}

fn led_gpio(emu: &Emulator) -> Result<u32, String> {
    let (r, gpio) = emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_NEOPIXEL);
    match (r, gpio) {
        (OraResult::Ok, Some(gpio)) => Ok(gpio),
        _ => Err(format!("RGB LED GPIO metadata: {r:?}/{gpio:?}")),
    }
}

/// The RGB LED's claim made at plugin launch is in the expected block, and the
/// LED's program fits it.
fn led_boot_claim(emu: &Emulator, want_16: bool) -> Result<(), String> {
    let (block, sm) = rgb_location(emu).ok_or("the RGB LED has no PIO claim")?;
    let apio = apio();
    let first = u32::from(apio.first_instr[block as usize][sm as usize]);

    // The LED's program is the last one written into its block at boot, so it
    // ends where the emulated PIO's record of the block ends.
    let length = u32::from(apio.max_offset[block as usize]) - first;
    if length != LED_INSTRS {
        return Err(format!(
            "the RGB LED's program is {length} instructions, and {LED_INSTRS} are claimed"
        ));
    }

    let (mut instrs, mut claims) = pio_claims(emu);
    instrs[block as usize] &= !run(first, LED_INSTRS);
    claims &= !((1 << sm) << (block * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT));
    let want = led_block_choice(instrs, claims, want_16);
    if want != Some(block) {
        return Err(format!(
            "the RGB LED's state machine is in block {block}, want {want:?}"
        ));
    }
    Ok(())
}

/// Run the RGB LED's launch claim again, with claims set up by the test.
fn led_relaunch(emu: &Emulator, claims: PioClaims) -> Option<(u32, u32)> {
    emu.set_pio_instr_claims(claims.0);
    emu.set_pio_block_claims(claims.1);
    emu.set_rgb_led_pio(onerom_metadata::RGB_LED_PIO_NONE);
    emu.pio_led_launch();
    rgb_location(emu)
}

/// Block 0 is in use with the LED's GPIOBASE and full, block 1 is in use with
/// the other GPIOBASE, and block 2 is unused.  The LED's state machine is
/// claimed in block 2.
fn led_fallback(emu: &Emulator, want_16: bool) -> Result<(), String> {
    let (this, other) = (
        PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16,
        PioClaim::PIO_CLAIM_BLOCK_1_GPIOBASE_16,
    );
    let base = if want_16 { this } else { other };
    let claims = ([u32::MAX, 1, 0], 0x0011 | base);

    let got = led_relaunch(emu, claims);
    if got != Some((2, 0)) {
        return Err(format!(
            "fallback: RGB LED claim {got:?}, want block 2 state machine 0"
        ));
    }
    let mut want = claims;
    want.0[2] = run(0, LED_INSTRS);
    want.1 |= 1 << (2 * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT);
    if want_16 {
        want.1 |= PioClaim::PIO_CLAIM_BLOCK_2_GPIOBASE_16;
    }
    check_pio_claims(emu, "fallback", want)
}

/// The RGB LED's state machine is claimed in each block, with the other two in
/// use with the other GPIOBASE.  The LED then lights from that block.
fn led_in_each_block(emu: &Emulator, gpio: u32, want_16: bool) -> Result<(), String> {
    for block in 0..PIO_BLOCKS {
        let mut claims: PioClaims = ([1; 3], 0);
        claims.0[block as usize] = 0;
        for other in (0..PIO_BLOCKS).filter(|&b| b != block) {
            claims.1 |= 1 << (other * PioClaim::PIO_CLAIM_BLOCK_1_SMS_SHIFT);
            if !want_16 {
                claims.1 |= PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16 << other;
            }
        }
        let got = led_relaunch(emu, claims);
        if got != Some((block, 0)) {
            return Err(format!("block {block}: RGB LED claim {got:?}"));
        }
        with_engine(emu, |emu| {
            let mut req = Emulator::led_request();
            req.led = ffi::ora_led_t_ORA_LED_RGB;
            req.mode = ffi::ora_led_mode_t_ORA_LED_MODE_ON;
            check(
                &format!("block {block}: RGB LED on"),
                emu.led_set(&req),
                OraResult::Ok,
            )?;
            // SAFETY: `_apio_emulated_gpios` is a plain C global, and the
            // emulator is single threaded.
            let owner = unsafe { (*core::ptr::addr_of!(ffi::_apio_emulated_gpios)).output_block }
                [gpio as usize];
            if owner != block as i8 {
                return Err(format!(
                    "block {block}: the RGB LED's GPIO is driven by block {owner}"
                ));
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// No block the LED can use has room.  Nothing is claimed for the LED, and an
/// RGB LED request fails.
fn led_no_room(emu: &Emulator, want_16: bool) -> Result<(), String> {
    let other = if want_16 {
        0
    } else {
        PioClaim::PIO_CLAIM_BLOCK_1_GPIOBASE_16 | PioClaim::PIO_CLAIM_BLOCK_2_GPIOBASE_16
    };
    let this = if want_16 {
        PioClaim::PIO_CLAIM_BLOCK_0_GPIOBASE_16
    } else {
        0
    };
    let claims = ([u32::MAX, 1, 1], 0x0111 | this | other);

    let got = led_relaunch(emu, claims);
    if got.is_some() {
        return Err(format!("no room: RGB LED claim {got:?}"));
    }
    check_pio_claims(emu, "no room", claims)?;

    let mut req = Emulator::led_request();
    req.led = ffi::ora_led_t_ORA_LED_RGB;
    req.mode = ffi::ora_led_mode_t_ORA_LED_MODE_ON;
    check(
        "RGB LED request with no claim",
        emu.led_set(&req),
        OraResult::NotSupported,
    )
}

/// Only run where the board has an RGB LED.
pub fn test_pio_led_claim(emu: &Emulator) -> Result<(), String> {
    let saved = (pio_claims(emu), emu.rgb_led_pio());
    let saved_apio = apio();

    // SAFETY: as for `apio`.
    let saved_gpios = unsafe { core::ptr::read(core::ptr::addr_of!(ffi::_apio_emulated_gpios)) };

    let result = led_gpio(emu).and_then(|gpio| {
        let want_16 = gpio >= 32;
        led_boot_claim(emu, want_16)
            .and_then(|()| led_fallback(emu, want_16))
            .and_then(|()| led_in_each_block(emu, gpio, want_16))
            .and_then(|()| led_no_room(emu, want_16))
    });

    emu.set_pio_instr_claims(saved.0.0);
    emu.set_pio_block_claims(saved.0.1);
    emu.set_rgb_led_pio(saved.1);
    // SAFETY: as for `apio`.
    unsafe {
        core::ptr::write(core::ptr::addr_of_mut!(ffi::_apio_emulated_pio), saved_apio);
        core::ptr::write(
            core::ptr::addr_of_mut!(ffi::_apio_emulated_gpios),
            saved_gpios,
        );
    }
    result
}
