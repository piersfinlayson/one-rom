// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ora_set_standby` and for the RAM slot calls in standby.
//!
//! A test that turns standby on turns it off again, pass or fail, so the tests
//! after it run against a serving One ROM.

use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_fw_tester::driver;
use onerom_fw_tester::jumpers::Jumpers;
use onerom_fw_tester::oracle;
use onerom_fw_tester::pin_cache::PinCache;
use onerom_fw_tester::runner::{addr_before_cs_cycles, cs_to_data_cycles, mode_pins, run_undriven};
use onerom_fw_tester::timing;
use onerom_gen::Config;
use onerom_metadata::{OverrideState, onerom_firmware_flag_t, onerom_override_states_t};

use super::reprogram::{
    boots_in_standby, chip_config_at, force_16_bit_for, make_active, pio_verify, random_pattern,
};
use crate::setup::setup;

const STANDBY: u8 = onerom_firmware_flag_t::FIRMWARE_FLAG_STANDBY;

/// Addresses read per undriven check.  A stopped state machine leaves them
/// undriven whatever the address.
const UNDRIVEN_ADDRS: usize = 512;

/// A slot's override states with only standby set, to `state`.
fn standby_override(state: OverrideState) -> u8 {
    (state as u8) << onerom_override_states_t::OVERRIDE_STANDBY_SHIFT
}

/// Check runtime info's standby flag matches `standby` and the `FIRMWARE_FLAGS`
/// key matches runtime info.
fn expect_standby(emu: &Emulator, standby: bool, when: &str) -> Result<(), String> {
    let runtime = emu.firmware_flags();
    let key = match emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_FIRMWARE_FLAGS) {
        (OraResult::Ok, Some(value)) => value,
        (result, _) => return Err(format!("{when}: FIRMWARE_FLAGS got {result:?}")),
    };
    if key != u32::from(runtime) {
        return Err(format!(
            "{when}: FIRMWARE_FLAGS is {key:#x} and runtime info holds {runtime:#x}"
        ));
    }
    if (runtime & STANDBY != 0) != standby {
        return Err(format!(
            "{when}: FIRMWARE_FLAGS is {key:#x}, want FIRMWARE_FLAG_STANDBY {}",
            if standby { "set" } else { "clear" }
        ));
    }
    Ok(())
}

fn set_standby(emu: &Emulator, standby: u8, flags: u32) -> Result<(), String> {
    let result = emu.set_standby(standby, flags);
    if result == OraResult::Ok {
        Ok(())
    } else {
        Err(format!("set_standby({standby}, {flags:#x}) got {result:?}"))
    }
}

/// Turn standby off.  Returns `result`, or the turn-off's error where `result`
/// is `Ok`.
fn leave_serving(emu: &Emulator, result: Result<(), String>) -> Result<(), String> {
    let off = set_standby(emu, 0, 0);
    result.and(off)
}

/// Read the slot in each mode it serves and fail on a driven data pin.
/// Returns how many times the pins were checked.
fn expect_undriven(
    emu: &Emulator,
    config: &Config,
    board: Board,
    set_idx: usize,
    when: &str,
) -> Result<u64, String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let cache = PinCache::build(chip_type, chip_config, board);
    let force_16_bit = force_16_bit_for(config, set_idx);

    let mut checks = 0;
    for &mode in chip_type.bit_modes() {
        if force_16_bit && mode != 16 {
            continue;
        }
        let (mode_checks, violations) = run_undriven(
            emu,
            &cache,
            oracle::served_size(chip_type),
            mode,
            addr_before_cs_cycles(chip_type),
            cs_to_data_cycles(chip_type, mode),
            (0, 0),
            UNDRIVEN_ADDRS,
        );
        if violations != 0 {
            return Err(format!(
                "{when}: data pins driven at {violations} of {mode_checks} checks in \
                 {mode}-bit mode"
            ));
        }
        checks += mode_checks;
    }
    Ok(checks)
}

fn expect_active(emu: &Emulator, slot: u8, when: &str) -> Result<(), String> {
    match emu.get_active_ram_slot() {
        (OraResult::Ok, Some(active)) if active == slot => Ok(()),
        other => Err(format!(
            "{when}: get_active_ram_slot got {other:?}, want slot {slot}"
        )),
    }
}

/// Boot the slot with its standby override on, then off.
///
/// On, One ROM boots into standby with RAM slot 0 active.  Off, it boots as a
/// slot without the override does.
///
/// Each boot replaces the firmware's state, so this runs ahead of the boot the
/// rest of a slot's suite uses.
pub fn test_standby_boot(
    board: Board,
    jumpers: &Jumpers,
    log_enabled: bool,
    set_idx: usize,
    sel_image: u8,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for (state, standby) in [
        (OverrideState::OverrideStateOn, true),
        (OverrideState::OverrideStateOff, false),
    ] {
        Emulator::set_rom_slot_override_states(set_idx as u8, standby_override(state));
        let (emu, _) = setup(board, jumpers, log_enabled, sel_image);
        Emulator::restore_rom_slots();

        let when = format!("booted with standby {state}");
        if let Err(e) = expect_standby(&emu, standby, &when) {
            errors.push(e);
        }
        if let Err(e) = expect_active(&emu, 0, &when) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        println!("  standby on boots into standby with RAM slot 0 active, standby off doesn't");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// The standby flag at boot matches the set's config.
///
/// A set that boots into standby has its data pins undriven through bus
/// cycles.  Standby is then turned off, so the set is served for the tests
/// after this one.
pub fn test_standby_from_config(
    emu: &Emulator,
    config: &Config,
    board: Board,
    set_idx: usize,
) -> Result<(), String> {
    if !boots_in_standby(config, set_idx) {
        expect_standby(emu, false, "after boot")?;
        println!("  booted serving");
        return Ok(());
    }

    let result = (|| {
        expect_standby(emu, true, "after boot")?;
        let checks = expect_undriven(emu, config, board, set_idx, "after boot")?;
        set_standby(emu, 0, 0)?;
        expect_standby(emu, false, "after set_standby(0, 0)")?;
        println!("  booted into standby, data pins undriven at {checks} checks, then turned off");
        Ok(())
    })();
    leave_serving(emu, result)
}

/// A standby value other than 0 or 1 is rejected and leaves standby as it was,
/// with standby off and with it on.
pub fn test_standby_invalid_arg(emu: &Emulator) -> Result<(), String> {
    let result = (|| {
        for state in [0u8, 1] {
            set_standby(emu, state, 0)?;
            for standby in [2u8, 0x80, 0xFF] {
                let before = emu.firmware_flags() & STANDBY != 0;
                let got = emu.set_standby(standby, 0);
                if got != OraResult::InvalidArg {
                    return Err(format!(
                        "set_standby({standby}, 0) got {got:?}, want InvalidArg"
                    ));
                }
                expect_standby(emu, before, &format!("after set_standby({standby}, 0)"))?;
            }
        }
        println!("  2, 0x80 and 0xFF rejected with standby off and on");
        Ok(())
    })();
    leave_serving(emu, result)
}

/// Each state set while One ROM is already in it succeeds, and the reserved
/// flag bits are ignored.
pub fn test_standby_same_state(emu: &Emulator) -> Result<(), String> {
    let result = (|| {
        for (standby, flags) in [(0, 0), (1, 0), (1, 0), (0, u32::MAX)] {
            set_standby(emu, standby, flags)?;
            expect_standby(
                emu,
                standby == 1,
                &format!("after set_standby({standby}, {flags:#x})"),
            )?;
        }
        println!("  each state set twice, reserved flag bits ignored");
        Ok(())
    })();
    leave_serving(emu, result)
}

/// In standby the data pins stay undriven through bus cycles.  Once standby is
/// off the slot is served again.
pub fn test_standby_undriven_then_serves(
    emu: &Emulator,
    config: &Config,
    board: Board,
    set_idx: usize,
    boot_image: &[u8],
) -> Result<(), String> {
    let result = (|| {
        set_standby(emu, 1, 0)?;
        let checks = expect_undriven(emu, config, board, set_idx, "in standby")?;
        set_standby(emu, 0, 0)?;

        let chip_config = chip_config_at(config, set_idx)?;
        let chip_type = chip_config.chip_type.resolved();
        let cache = PinCache::build(chip_type, chip_config, board);
        let served = pio_verify(
            emu,
            &cache,
            boot_image,
            chip_type,
            force_16_bit_for(config, set_idx),
        )?;
        println!("  data pins undriven at {checks} checks in standby, {served} bytes served after");
        Ok(())
    })();
    leave_serving(emu, result)
}

/// Standby turned on and off with chip select held asserted.
///
/// Standby stops the CS state machine part way through a read.  Once standby
/// is off the data pins are driven with the byte for the address.
pub fn test_standby_with_cs_held(
    emu: &Emulator,
    config: &Config,
    board: Board,
    set_idx: usize,
    boot_image: &[u8],
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let cache = PinCache::build(chip_type, chip_config, board);
    let mode = if force_16_bit_for(config, set_idx) {
        16
    } else {
        chip_type.bit_modes()[0]
    };
    let pins = mode_pins(&cache, mode);
    let data_mask = pins.data_gpios.iter().fold(0u64, |m, &g| m | (1u64 << g));
    let cycles_cs_to_data = cs_to_data_cycles(chip_type, mode);

    // An address with both levels on its low address lines, within the chip.
    let word = usize::from(mode / 8);
    let addr = 0x55 % (boot_image.len() / word);
    let expected = &boot_image[addr * word..(addr + 1) * word];

    let addr_mask = driver::addr_mask(addr, pins.addr_gpios);
    let drive = |active: bool| {
        let ctrl = driver::ctrl_mask(&cache.control_lines, active);
        let levels = driver::merge(driver::merge(addr_mask, ctrl), pins.byte_mask);
        emu.drive_gpios(levels.0, levels.1);
    };
    let read = |when: &str, driven_want: bool| -> Result<(), String> {
        let driven = emu.read_driven_pins() & data_mask;
        if driven_want && driven != data_mask {
            return Err(format!(
                "{when}: data pins {:#018x} not driven",
                data_mask & !driven
            ));
        }
        if !driven_want && driven != 0 {
            return Err(format!("{when}: data pins {driven:#018x} driven"));
        }
        if driven_want {
            let states = emu.read_pin_states();
            let got: Vec<u8> = pins
                .data_gpios
                .chunks(8)
                .map(|lane| driver::extract_byte(states, lane))
                .collect();
            if got != expected {
                return Err(format!(
                    "{when}: address {addr:#x} served {got:02x?}, want {expected:02x?}"
                ));
            }
        }
        Ok(())
    };

    drive(false);
    emu.step_cycles(addr_before_cs_cycles(chip_type));
    drive(true);
    emu.step_cycles(cycles_cs_to_data);

    // epio puts every pin back to its pull when the firmware's PIO changes are
    // applied, so the bus is driven again after each call.
    let result = (|| {
        read("serving", true)?;
        set_standby(emu, 1, 0)?;
        drive(true);
        emu.step_cycles(cycles_cs_to_data);
        read("standby turned on", false)?;
        set_standby(emu, 0, 0)?;
        drive(true);
        emu.step_cycles(cycles_cs_to_data);
        read("standby turned off", true)?;
        println!("  address {addr:#x} served again with chip select held throughout");
        Ok(())
    })();

    drive(false);
    emu.step_cycles(timing::CYCLES_AFTER_READ);
    leave_serving(emu, result)
}

/// The RAM slot calls in standby.
///
/// The active slot is the one active before standby, and it can still only be
/// written with allow_active.  Switching the active slot leaves One ROM in
/// standby, and the slot switched to is served once standby is off.
pub fn test_standby_ram_slots(
    emu: &Emulator,
    config: &Config,
    board: Board,
    set_idx: usize,
    boot_slot: u8,
    scratch_slot: u8,
) -> Result<(), String> {
    make_active(emu, boot_slot)?;
    let result = (|| {
        set_standby(emu, 1, 0)?;
        expect_active(emu, boot_slot, "in standby")?;
        let got = emu.reprogram_ram_rom_slot(boot_slot, 0, &[0], false);
        if got != OraResult::SlotActive {
            return Err(format!(
                "reprogram of active slot {boot_slot} in standby without allow_active got \
                 {got:?}, want SlotActive"
            ));
        }

        if emu.get_ram_slot_count() <= scratch_slot {
            println!("  active slot kept and protected (one RAM slot, so the switch is skipped)");
            return Ok(());
        }

        let chip_config = chip_config_at(config, set_idx)?;
        let chip_type = chip_config.chip_type.resolved();
        let pattern = random_pattern(oracle::served_size(chip_type));
        let got = emu.reprogram_ram_rom_slot(scratch_slot, 0, &pattern, false);
        if got != OraResult::Ok {
            return Err(format!(
                "reprogram of slot {scratch_slot} in standby got {got:?}"
            ));
        }
        make_active(emu, scratch_slot)?;
        expect_standby(emu, true, "after set_active_ram_slot in standby")?;
        expect_active(emu, scratch_slot, "after set_active_ram_slot in standby")?;
        expect_undriven(
            emu,
            config,
            board,
            set_idx,
            "after set_active_ram_slot in standby",
        )?;

        set_standby(emu, 0, 0)?;
        let cache = PinCache::build(chip_type, chip_config, board);
        let served = pio_verify(
            emu,
            &cache,
            &pattern,
            chip_type,
            force_16_bit_for(config, set_idx),
        )?;
        println!(
            "  active slot kept and protected, slot {scratch_slot} switched to in standby and \
             {served} bytes of it served after"
        );
        Ok(())
    })();
    let result = leave_serving(emu, result);
    result.and(make_active(emu, boot_slot))
}
