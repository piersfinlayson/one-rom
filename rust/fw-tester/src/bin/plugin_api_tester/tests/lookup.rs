// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the plugin API lookup table.
//!
//! Verifies that every active API ID resolves to a non-null function pointer,
//! and that deprecated/invalid IDs correctly return null.

use onerom_fw_emulator::{Emulator, ffi};

pub fn test_lookup_coverage(emu: &Emulator) -> Result<(), String> {
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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(", "))
    }
}
