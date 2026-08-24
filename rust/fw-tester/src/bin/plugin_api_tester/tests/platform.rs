// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the platform calls: memory, peripherals, interrupts and the
//! cooperative yield.
//!
//! # What this build can see
//!
//! `ora_setup_adc`, `ora_enable_irq` and `ora_register_irq` all touch hardware
//! this process does not have — the ADC block, the USB PLL and the NVIC — so
//! under `TEST_BUILD` each is compiled down to a log line that goes to the
//! host's stdout.  The tests below therefore pin their entry points and the
//! argument range a plugin passes, and fence them against disturbing anything
//! the rest of the API reports.  They cannot assert the register writes, and
//! nothing in this build can.
//!
//! `ora_get_clkref_mhz` is worse off and has no test at all: it dereferences
//! the absolute address of the RP2350 clocks block, which is unmapped in this
//! process, so calling it takes the tester down with a segfault rather than
//! returning a wrong answer.  See `Emulator::clkref_mhz`.

use onerom_fw_emulator::{Emulator, OraResult, ffi};

/// A handler address to hand `ora_register_irq`.  The firmware stores the
/// pointer and nothing in this process ever dispatches to it.
unsafe extern "C" fn dummy_irq_handler() {}

/// The firmware hands out no memory, and says so consistently.
///
/// `ora_alloc` and `plugin_get_free_mem` are two halves of one answer: a pool
/// that is empty is a pool nothing can be taken from.  Both are asserted here
/// so a change to either alone shows up as the pair disagreeing rather than as
/// a plugin quietly getting a pointer into nothing.
pub fn test_no_allocator(emu: &Emulator) -> Result<(), String> {
    let free = emu.get_free_mem();
    let mut errors = Vec::new();

    if free != 0 {
        errors.push(format!("get_free_mem reports {free} bytes free"));
    }

    // Nothing larger than the free pool can be handed out, and the pool is
    // empty, so every request must come back NULL — including a zero-byte one,
    // which has no memory to point at either.
    for size in [0usize, 1, 4, 1024, 512 * 1024, usize::MAX] {
        let p = emu.alloc(size);
        if p != 0 {
            errors.push(format!(
                "alloc({size}) returned 0x{p:X} from a {free} byte pool"
            ));
        }
    }

    if errors.is_empty() {
        println!("  no pool: free_mem 0, alloc always NULL");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// The peripheral and interrupt calls take every argument a plugin passes them
/// and leave the running device alone.
///
/// Each of the three is a hardware call this build compiles out (see the module
/// docs), so what is asserted is the boundary: both declared IRQs, a handler
/// and the NULL that deregisters it, enable and disable, and the ADC setup that
/// brings up the USB PLL on a device.  The fence is what the rest of the API
/// says afterwards — the clocks, the active RAM slot and the GPIO
/// classification are all read back, because on a device these calls touch
/// clocks and the NVIC and a plugin's next call must still be answered.
pub fn test_peripheral_and_irq_calls(emu: &Emulator, max_gpios: u8) -> Result<(), String> {
    let sysclk_before = emu.sysclk_mhz();
    let (slot_result_before, slot_before) = emu.get_active_ram_slot();
    let uses_before: Vec<u8> = (0..max_gpios)
        .map(|g| emu.gpio_query(g).1.gpio_use)
        .collect();

    emu.setup_adc();

    for irq in [
        ffi::ora_irq_t_ORA_IRQ_TIMER0_IRQ_0,
        ffi::ora_irq_t_ORA_IRQ_USBCTRL_IRQ,
    ] {
        emu.register_irq(irq, Some(dummy_irq_handler));
        emu.enable_irq(irq, true);
        emu.enable_irq(irq, false);
        // A NULL handler deregisters, which on a device also disables the IRQ.
        emu.register_irq(irq, None);
    }

    let mut errors = Vec::new();
    let sysclk_after = emu.sysclk_mhz();
    if sysclk_after != sysclk_before {
        errors.push(format!(
            "sysclk moved from {sysclk_before} to {sysclk_after} MHz"
        ));
    }
    let (slot_result_after, slot_after) = emu.get_active_ram_slot();
    if (slot_result_after, slot_after) != (slot_result_before, slot_before) {
        errors.push(format!(
            "active RAM slot moved from {slot_result_before:?}/{slot_before:?} to {slot_result_after:?}/{slot_after:?}"
        ));
    }
    for gpio in 0..max_gpios {
        let after = emu.gpio_query(gpio).1.gpio_use;
        if after != uses_before[gpio as usize] {
            errors.push(format!(
                "GPIO {gpio} use moved from {} to {after}",
                uses_before[gpio as usize]
            ));
        }
    }

    if errors.is_empty() {
        println!("  ADC setup and 2 IRQs registered, enabled, disabled, released");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// `ora_yield` reports whether the core was paused, and accepts a caller that
/// does not want to know.
///
/// The out parameter is pre-filled with a value the firmware cannot legally
/// leave behind, so "reported 0" is told apart from "wrote nothing".  There is
/// no other core in this process to request exclusive mode, so the answer is
/// always that the core was not paused — which is the answer a plugin's main
/// loop gets on a device for every call but the rare one.
pub fn test_yield(emu: &Emulator) -> Result<(), String> {
    /// Neither 0 nor 1, so the firmware writing nothing is visible.
    const UNWRITTEN: u8 = 0xFF;

    let mut errors = Vec::new();

    let (result, was_paused) = emu.plugin_yield();
    if result != OraResult::Ok {
        errors.push(format!("yield: got {result:?}, want Ok"));
    }
    if was_paused == UNWRITTEN {
        errors.push("yield left was_paused_out unwritten".to_string());
    } else if was_paused != 0 {
        errors.push(format!(
            "yield reported was_paused={was_paused} with no other core to pause it"
        ));
    }

    let result = emu.plugin_yield_null_out();
    if result != OraResult::Ok {
        errors.push(format!("yield with NULL out: got {result:?}, want Ok"));
    }

    if errors.is_empty() {
        println!("  yield returned, core not paused");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
