// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the plugin API lookup table.
//!
//! Verifies that every active API ID resolves to a non-null function pointer,
//! and that deprecated/invalid IDs correctly return null.
//!
//! # Why the header is parsed
//!
//! The two lists below are written by hand, so an ID added to the API and to
//! neither list is covered by nothing and reported by nothing — which is how
//! `ORA_ID_LED_SET` and `ORA_ID_LED_GET` arrived without a test.  So the
//! `api_id_t` enum in `firmware/ora/api.h` is read and every name in it must
//! appear in one list or the other.  That is the API's own declaration of what
//! it offers, rather than a second copy of these lists, so a new ID fails here
//! until someone says which it is.
//!
//! Classifying an ID is the floor, not the job: a new call also needs a test
//! that exercises what it does.

use std::path::Path;

use onerom_fw_emulator::{Emulator, ffi};

/// Every `ORA_ID_*` the `api_id_t` enum declares, in declaration order.
///
/// The enum body is the only place in the header where a line starts with
/// `ORA_ID_` and assigns a value — a doc block's references to an ID are inside
/// a comment, so they start with `*`.
fn api_ids_in_header(base_dir: &Path) -> Result<Vec<String>, String> {
    let path = base_dir.join("firmware/ora/api.h");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let ids: Vec<String> = text
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("ORA_ID_") && line.contains('='))
        .map(|line| {
            line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect();

    // A header that parsed to nothing would pass every check below silently.
    if ids.is_empty() {
        return Err(format!("no ORA_ID_* names found in {}", path.display()));
    }

    Ok(ids)
}

pub fn test_lookup_coverage(emu: &Emulator, base_dir: &Path) -> Result<(), String> {
    // Active IDs — must resolve to non-null.
    let active_ids: &[(ffi::api_id_t, &str)] = &[
        (ffi::api_id_t_ORA_ID_REBOOT_BOOTSEL, "ORA_ID_REBOOT_BOOTSEL"),
        (ffi::api_id_t_ORA_ID_ALLOC, "ORA_ID_ALLOC"),
        (ffi::api_id_t_ORA_ID_LOG, "ORA_ID_LOG"),
        (ffi::api_id_t_ORA_ID_ERR_LOG, "ORA_ID_ERR_LOG"),
        (ffi::api_id_t_ORA_ID_DEBUG_LOG, "ORA_ID_DEBUG_LOG"),
        (ffi::api_id_t_ORA_ID_GET_FREE_MEM, "ORA_ID_GET_FREE_MEM"),
        (ffi::api_id_t_ORA_ID_SET_STATUS_LED, "ORA_ID_SET_STATUS_LED"),
        (ffi::api_id_t_ORA_ID_SETUP_USB, "ORA_ID_SETUP_USB"),
        (ffi::api_id_t_ORA_ID_SETUP_ADC, "ORA_ID_SETUP_ADC"),
        (ffi::api_id_t_ORA_ID_REGISTER_IRQ, "ORA_ID_REGISTER_IRQ"),
        (
            ffi::api_id_t_ORA_ID_SET_PLUGIN_CONTEXT,
            "ORA_ID_SET_PLUGIN_CONTEXT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_PLUGIN_CONTEXT,
            "ORA_ID_GET_PLUGIN_CONTEXT",
        ),
        (ffi::api_id_t_ORA_ID_GET_SYSCLK_MHZ, "ORA_ID_GET_SYSCLK_MHZ"),
        (ffi::api_id_t_ORA_ID_ENABLE_IRQ, "ORA_ID_ENABLE_IRQ"),
        (ffi::api_id_t_ORA_ID_GET_CLKREF_MHZ, "ORA_ID_GET_CLKREF_MHZ"),
        (
            ffi::api_id_t_ORA_ID_GET_CHIP_SIZE_FROM_TYPE,
            "ORA_ID_GET_CHIP_SIZE_FROM_TYPE",
        ),
        (ffi::api_id_t_ORA_ID_IS_PIN_OUTPUT, "ORA_ID_IS_PIN_OUTPUT"),
        (
            ffi::api_id_t_ORA_ID_GET_DATA_PIN_NUMS,
            "ORA_ID_GET_DATA_PIN_NUMS",
        ),
        (
            ffi::api_id_t_ORA_ID_SETUP_ADDRESS_MONITOR,
            "ORA_ID_SETUP_ADDRESS_MONITOR",
        ),
        (
            ffi::api_id_t_ORA_ID_MAP_ADDR_TO_PHYS,
            "ORA_ID_MAP_ADDR_TO_PHYS",
        ),
        (
            ffi::api_id_t_ORA_ID_MAP_DATA_TO_PHYS,
            "ORA_ID_MAP_DATA_TO_PHYS",
        ),
        (ffi::api_id_t_ORA_ID_DEMANGLE_ADDR, "ORA_ID_DEMANGLE_ADDR"),
        (ffi::api_id_t_ORA_ID_INIT_KNOCK, "ORA_ID_INIT_KNOCK"),
        (ffi::api_id_t_ORA_ID_WAIT_FOR_KNOCK, "ORA_ID_WAIT_FOR_KNOCK"),
        (
            ffi::api_id_t_ORA_ID_REPROGRAM_RAM_ROM_SLOT,
            "ORA_ID_REPROGRAM_RAM_ROM_SLOT",
        ),
        (
            ffi::api_id_t_ORA_ID_START_ADDRESS_MONITOR,
            "ORA_ID_START_ADDRESS_MONITOR",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS,
            "ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_RAM_SLOT_COUNT,
            "ORA_ID_GET_RAM_SLOT_COUNT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_RAM_SLOT_INFO,
            "ORA_ID_GET_RAM_SLOT_INFO",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_ACTIVE_RAM_SLOT,
            "ORA_ID_GET_ACTIVE_RAM_SLOT",
        ),
        (
            ffi::api_id_t_ORA_ID_SET_ACTIVE_RAM_SLOT,
            "ORA_ID_SET_ACTIVE_RAM_SLOT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_COUNT,
            "ORA_ID_GET_FLASH_SLOT_COUNT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_INFO,
            "ORA_ID_GET_FLASH_SLOT_INFO",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_FLASH_SLOT_EXT_INFO,
            "ORA_ID_GET_FLASH_SLOT_EXT_INFO",
        ),
        (
            ffi::api_id_t_ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT,
            "ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_DEVICE_VERSION,
            "ORA_ID_GET_DEVICE_VERSION",
        ),
        (ffi::api_id_t_ORA_ID_DEMANGLE_DATA, "ORA_ID_DEMANGLE_DATA"),
        (
            ffi::api_id_t_ORA_ID_ENTER_EXCLUSIVE_MODE,
            "ORA_ID_ENTER_EXCLUSIVE_MODE",
        ),
        (
            ffi::api_id_t_ORA_ID_EXIT_EXCLUSIVE_MODE,
            "ORA_ID_EXIT_EXCLUSIVE_MODE",
        ),
        (ffi::api_id_t_ORA_ID_YIELD, "ORA_ID_YIELD"),
        (
            ffi::api_id_t_ORA_ID_READ_RAM_ROM_SLOT,
            "ORA_ID_READ_RAM_ROM_SLOT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_METADATA_STR,
            "ORA_ID_GET_METADATA_STR",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_METADATA_UINT,
            "ORA_ID_GET_METADATA_UINT",
        ),
        (
            ffi::api_id_t_ORA_ID_DEMANGLE_OBSERVED_ADDR,
            "ORA_ID_DEMANGLE_OBSERVED_ADDR",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_UNOBSERVED_ADDR_BITS,
            "ORA_ID_GET_UNOBSERVED_ADDR_BITS",
        ),
        (ffi::api_id_t_ORA_ID_GPIO_SET, "ORA_ID_GPIO_SET"),
        (ffi::api_id_t_ORA_ID_GPIO_QUERY, "ORA_ID_GPIO_QUERY"),
        (ffi::api_id_t_ORA_ID_LOG_OPEN_WRITE, "ORA_ID_LOG_OPEN_WRITE"),
        (ffi::api_id_t_ORA_ID_LOG_WRITE, "ORA_ID_LOG_WRITE"),
        (
            ffi::api_id_t_ORA_ID_LOG_CLOSE_WRITE,
            "ORA_ID_LOG_CLOSE_WRITE",
        ),
        (ffi::api_id_t_ORA_ID_LOG_OPEN_READ, "ORA_ID_LOG_OPEN_READ"),
        (ffi::api_id_t_ORA_ID_LOG_READ, "ORA_ID_LOG_READ"),
        (ffi::api_id_t_ORA_ID_LOG_CLOSE_READ, "ORA_ID_LOG_CLOSE_READ"),
        (ffi::api_id_t_ORA_ID_LOG_QUERY, "ORA_ID_LOG_QUERY"),
        (
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_UINT,
            "ORA_ID_GET_COMPILE_OPTION_UINT",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_COMPILE_OPTION_STR,
            "ORA_ID_GET_COMPILE_OPTION_STR",
        ),
        (
            ffi::api_id_t_ORA_ID_LOG_CATEGORY_ENABLED,
            "ORA_ID_LOG_CATEGORY_ENABLED",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_PLUGIN_UPTIME_MS,
            "ORA_ID_GET_PLUGIN_UPTIME_MS",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_METADATA_UINT_AT,
            "ORA_ID_GET_METADATA_UINT_AT",
        ),
        (ffi::api_id_t_ORA_ID_LED_SET, "ORA_ID_LED_SET"),
        (ffi::api_id_t_ORA_ID_LED_GET, "ORA_ID_LED_GET"),
    ];

    // Deprecated/invalid IDs — must resolve to null.
    let null_ids: &[(ffi::api_id_t, &str)] = &[
        (
            ffi::api_id_t_ORA_ID_GET_FIRMWARE_INFO,
            "ORA_ID_GET_FIRMWARE_INFO",
        ),
        (
            ffi::api_id_t_ORA_ID_GET_RUNTIME_INFO,
            "ORA_ID_GET_RUNTIME_INFO",
        ),
        (ffi::api_id_t_ORA_ID_INVALID, "ORA_ID_INVALID"),
    ];

    let mut errors = Vec::new();

    for (id, name) in active_ids {
        if !emu.plugin_lookup_valid(*id) {
            errors.push(format!("{} returned NULL", name));
        }
    }

    for (id, name) in null_ids {
        if emu.plugin_lookup_valid(*id) {
            errors.push(format!("{} returned non-NULL (expected NULL)", name));
        }
    }

    // Every ID the API declares is classified above, and everything classified
    // above is still an ID the API declares.
    let declared = api_ids_in_header(base_dir)?;
    let classified: Vec<&str> = active_ids
        .iter()
        .chain(null_ids.iter())
        .map(|(_, name)| *name)
        .collect();

    for name in &declared {
        if !classified.contains(&name.as_str()) {
            errors.push(format!(
                "{name} is in api.h and in neither list here — add it to \
                 active_ids with a test that exercises it, or to null_ids"
            ));
        }
    }

    for name in &classified {
        if !declared.iter().any(|d| d == name) {
            errors.push(format!("{name} is listed here but no longer in api.h"));
        }
    }

    if errors.is_empty() {
        println!("  {} API IDs declared, all classified", declared.len());
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
