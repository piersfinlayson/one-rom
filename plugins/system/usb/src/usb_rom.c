// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Provides ROM image functionality
//
// This file handles USB access to live ROM images using the v2 plugin API.
// The v1 per-chip snowflake logic has been removed entirely.

#include "usb_plugin.h"
#include "usb_rom.h"

// ---------------------------------------------------------------------------
// Access ROM set information
// ---------------------------------------------------------------------------

uint32_t app_get_active_rom_size(const usb_plugin_context_t *ctx) {
    uint8_t slot;
    if (ctx->get_active_ram_slot(&slot) != ORA_RESULT_OK) {
        return 0u;
    }
    uint32_t addr, size;
    ctx->get_ram_slot_info(slot, &addr, &size, NULL);
    return size;
}

// ---------------------------------------------------------------------------
// Logical address and data mapping
// ---------------------------------------------------------------------------

pb_status_t app_get_logical_byte_from_logical_addr(
    uint32_t logical_addr,
    uint32_t *value_out,
    const usb_plugin_context_t *ctx
) {
    uint8_t slot;
    if (ctx->get_active_ram_slot(&slot) != ORA_RESULT_OK) {
        return PB_STATUS_PRECONDITION_NOT_MET;
    }
    uint8_t      byte;
    ora_result_t r = ctx->read_ram_rom_slot(slot, logical_addr, &byte, 1);
    if (r == ORA_RESULT_OK) {
        *value_out = byte;
        return PB_STATUS_OK;
    }
    return (r == ORA_RESULT_INVALID_ARG) ? PB_STATUS_INVALID_ADDRESS : PB_STATUS_UNKNOWN_ERROR;
}

pb_status_t app_set_logical_byte_at_logical_addr(
    uint32_t logical_addr,
    uint8_t logical_value,
    const usb_plugin_context_t *ctx
) {
    uint8_t slot;
    if (ctx->get_active_ram_slot(&slot) != ORA_RESULT_OK) {
        return PB_STATUS_PRECONDITION_NOT_MET;
    }
    ora_result_t r = ctx->reprogram_ram_rom_slot(
        slot,
        logical_addr,
        &logical_value,
        1,
        1
    );
    if (r == ORA_RESULT_OK) {
        return PB_STATUS_OK;
    }
    return (r == ORA_RESULT_INVALID_ARG) ? PB_STATUS_INVALID_ADDRESS : PB_STATUS_UNKNOWN_ERROR;
}