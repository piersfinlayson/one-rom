// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the plugin context store (`ORA_ID_SET_PLUGIN_CONTEXT` and
//! `ORA_ID_GET_PLUGIN_CONTEXT`).
//!
//! # Why this exists
//!
//! Each plugin type has its own slot in the runtime info block, and `api.h`
//! publishes the addresses of both as `ORA_GET_PLUGIN_CONTEXT_SYSTEM` and
//! `ORA_GET_PLUGIN_CONTEXT_USER` so an IRQ handler can read one without an ORA
//! lookup. A context stored through the API therefore has to land in the slot
//! the matching macro reads, or a plugin that sets its context and reads it
//! back from an interrupt gets NULL for ever.
//!
//! The harness calls through the `api.h` typedef rather than the C symbol, so
//! an implementation whose signature disagrees with the declaration fails here
//! rather than silently storing the wrong argument.

use onerom_fw_emulator::{Emulator, ffi};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

/// Values chosen so that neither can be confused with a plugin type, and so
/// that a swap between the two slots is visible.
const SYSTEM_CONTEXT: usize = 0x2008_0100;
const USER_CONTEXT: usize = 0x2008_0200;

pub fn test_plugin_context(emu: &Emulator) -> Result<(), String> {
    // Arm: both slots start empty, so a later read cannot pass on a leftover.
    if emu.get_plugin_context(SYSTEM) != 0 {
        return Err("system context is not NULL before anything set it".to_string());
    }
    if emu.get_plugin_context(USER) != 0 {
        return Err("user context is not NULL before anything set it".to_string());
    }

    // Stimulate: store one type's context and check the other is untouched.
    // Doing this before the second store is what catches an implementation
    // that ignores the plugin argument, since with both stored a single shared
    // slot would still return the right value for whichever was written last.
    emu.set_plugin_context(SYSTEM, SYSTEM_CONTEXT);
    let got = emu.get_plugin_context(SYSTEM);
    if got != SYSTEM_CONTEXT {
        return Err(format!(
            "system context read back {got:#x}, want {SYSTEM_CONTEXT:#x}"
        ));
    }
    let got = emu.get_plugin_context(USER);
    if got != 0 {
        return Err(format!(
            "storing the system context also set the user context to {got:#x}"
        ));
    }

    // Fence: the two slots hold their own values at the same time.
    emu.set_plugin_context(USER, USER_CONTEXT);
    let sys = emu.get_plugin_context(SYSTEM);
    let usr = emu.get_plugin_context(USER);
    if sys != SYSTEM_CONTEXT || usr != USER_CONTEXT {
        return Err(format!(
            "after both stores: system {sys:#x} (want {SYSTEM_CONTEXT:#x}), \
             user {usr:#x} (want {USER_CONTEXT:#x})"
        ));
    }

    // The two published macro addresses are the two slots, so what the API
    // stored must be what an IRQ handler would read at those addresses.
    let (sys_addr, usr_addr) = emu.plugin_context_addrs();
    if sys_addr != SYSTEM_CONTEXT || usr_addr != USER_CONTEXT {
        return Err(format!(
            "runtime info holds system {sys_addr:#x}, user {usr_addr:#x} - \
             the API stored somewhere the ORA_GET_PLUGIN_CONTEXT_* macros do \
             not read"
        ));
    }

    Ok(())
}
