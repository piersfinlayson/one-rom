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
use onerom_metadata::{
    ALLOC_REQUEST_MIN_SIZE, USB_DPRAM_ACCESS_CYCLES, USB_DPRAM_BASE_ADDR, USB_DPRAM_SIZE,
    onerom_firmware_flag_t,
};

/// A handler address to hand `ora_register_irq`.  The firmware stores the
/// pointer and nothing in this process ever dispatches to it.
unsafe extern "C" fn dummy_irq_handler() {}

/// The deprecated `ora_alloc` and `plugin_get_free_mem` hand out no memory.
pub fn test_no_allocator(emu: &Emulator) -> Result<(), String> {
    let free = emu.get_free_mem();
    let mut errors = Vec::new();

    if free != 0 {
        errors.push(format!("get_free_mem reports {free} bytes free"));
    }

    for size in [0usize, 1, 4, 1024, 512 * 1024, usize::MAX] {
        let p = emu.alloc(size);
        if p != 0 {
            errors.push(format!(
                "alloc({size}) returned 0x{p:X} from a {free} byte pool"
            ));
        }
    }

    if errors.is_empty() {
        println!("  deprecated allocator: free_mem 0, alloc always NULL");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

const ALL_LIMITATIONS: u32 =
    ffi::ORA_MEM_ACCEPT_ALIGNED_ONLY | ffi::ORA_MEM_ACCEPT_NO_EXEC | ffi::ORA_MEM_ACCEPT_SLOWER;

fn alloc_request(bytes: u32) -> ffi::ora_alloc_request_t {
    ffi::ora_alloc_request_t {
        size: ALLOC_REQUEST_MIN_SIZE,
        reserved: [0],
        max_access_cycles: USB_DPRAM_ACCESS_CYCLES,
        accept: ALL_LIMITATIONS,
        bytes,
    }
}

/// Allocate with `req` and check the result and the address where it succeeded.
fn check_alloc(
    emu: &Emulator,
    label: &str,
    req: &ffi::ora_alloc_request_t,
    expected: Result<u32, OraResult>,
) -> Result<(), String> {
    let (r, addr) = emu.alloc_mem(req);
    let found = if r == OraResult::Ok { Ok(addr) } else { Err(r) };
    if found != expected {
        return Err(format!("{label}: {found:x?}, not {expected:x?}"));
    }
    Ok(())
}

/// `ORA_ID_ALLOC_MEM` with neither plugin reserving USB controller DPRAM, so
/// the pool starts at offset 0.
pub fn test_alloc_mem(emu: &Emulator) -> Result<(), String> {
    const LOG_IN_DPRAM: u8 = onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1;
    let dpram_size = USB_DPRAM_SIZE as u32;
    let full = OraResult::InsufficientFreeMem;
    let flags = emu.firmware_flags() & !LOG_IN_DPRAM;
    emu.set_firmware_flags(flags);

    check_alloc(emu, "before the launch", &alloc_request(4), Err(full))?;
    emu.launch_usb_dpram(0);

    let req = alloc_request(4);
    for (label, null_req, null_addr) in [
        ("a NULL request", true, false),
        ("a NULL address", false, true),
    ] {
        let r = emu.alloc_mem_null(&req, null_req, null_addr);
        if r != OraResult::InvalidArg {
            return Err(format!("{label}: {r:?}, not InvalidArg"));
        }
    }

    let mut refused = Vec::new();
    for size in [0u8, ALLOC_REQUEST_MIN_SIZE - 1] {
        refused.push((
            format!("size {size}"),
            ffi::ora_alloc_request_t { size, ..req },
            OraResult::InvalidSize,
        ));
    }
    refused.push((
        "0 bytes".to_string(),
        alloc_request(0),
        OraResult::InvalidArg,
    ));
    for bit in [
        ffi::ORA_MEM_ACCEPT_ALIGNED_ONLY,
        ffi::ORA_MEM_ACCEPT_NO_EXEC,
        ffi::ORA_MEM_ACCEPT_SLOWER,
    ] {
        refused.push((
            format!("accept without {bit:#x}"),
            ffi::ora_alloc_request_t {
                accept: ALL_LIMITATIONS & !bit,
                ..req
            },
            full,
        ));
    }
    refused.push((
        "an access time below USB DPRAM's".to_string(),
        ffi::ora_alloc_request_t {
            max_access_cycles: USB_DPRAM_ACCESS_CYCLES - 1,
            ..req
        },
        full,
    ));
    for (label, req, result) in &refused {
        check_alloc(emu, label, req, Err(*result))?;
    }

    let base = USB_DPRAM_BASE_ADDR;
    let larger = ffi::ora_alloc_request_t {
        size: u8::MAX,
        accept: ALL_LIMITATIONS | (1 << 31),
        bytes: 1,
        ..req
    };
    check_alloc(
        emu,
        "a larger request with an unknown bit",
        &larger,
        Ok(base),
    )?;
    check_alloc(emu, "after a 1-byte allocation", &req, Ok(base + 4))?;

    let left = dpram_size - 8;
    check_alloc(
        emu,
        "one byte too many",
        &alloc_request(left + 1),
        Err(full),
    )?;
    check_alloc(emu, "u32::MAX bytes", &alloc_request(u32::MAX), Err(full))?;
    check_alloc(emu, "the rest", &alloc_request(left), Ok(base + 8))?;
    let offset = emu.usb_dpram_pool_offset();
    if u32::from(offset) != dpram_size {
        return Err(format!(
            "the pool offset is {offset:#x} after the last byte"
        ));
    }
    check_alloc(emu, "an empty pool", &alloc_request(1), Err(full))?;

    // One ROM's log at the top of USB DPRAM ends the pool.
    emu.set_firmware_flags(flags | LOG_IN_DPRAM);
    emu.launch_usb_dpram(0);
    let below_log = dpram_size - ffi::USB_DPRAM_LOG_SIZE;
    let result = check_alloc(
        emu,
        "past the log",
        &alloc_request(below_log + 1),
        Err(full),
    )
    .and_then(|_| check_alloc(emu, "up to the log", &alloc_request(below_log), Ok(base)));
    emu.set_firmware_flags(flags);
    result?;

    println!("  alloc_mem: refusals, alignment, exhaustion and the log's limit");
    Ok(())
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
