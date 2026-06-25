// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Provides ROM image functionality 

#if !defined(USB_ROM_H)
#define USB_ROM_H

#include "usb_plugin.h"
#include "usb_picobootx.h"
#include "include.h"

uint32_t app_get_active_rom_size(const usb_plugin_context_t *ctx);
pb_status_t app_get_logical_byte_from_logical_addr(
    uint32_t                    logical_addr,
    uint32_t                   *value_out,
    const usb_plugin_context_t *ctx
);
pb_status_t app_set_logical_byte_at_logical_addr(
    uint32_t                    logical_addr,
    uint8_t                     logical_value,
    const usb_plugin_context_t *ctx
);

#endif // USB_ROM_H