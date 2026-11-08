// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - cycle delay for code running from RAM
//!
//! `cortex_m::asm::delay` calls a function in flash, whose speed depends on
//! the XIP cache.  This is the same loop inlined, so a delay in a RAM function
//! runs from RAM too.

/// Delays for at least `cycles` CPU cycles, as `cortex_m::asm::delay` does.
#[inline(always)]
pub fn cycles(cycles: u32) {
    // cortex_m::asm::delay's loop count, which assumes 2 cycles a loop.
    let loops = 1 + cycles / 2;
    unsafe {
        core::arch::asm!(
            "1:",
            "subs {}, #1",
            "bne 1b",
            inout(reg) loops => _,
            options(nomem, nostack),
        );
    }
}
