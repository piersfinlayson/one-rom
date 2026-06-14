// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Provides ROM image functionality 

#if !defined(USB_ROM_H)
#define USB_ROM_H

#include "usb_plugin.h"
#include "usb_picobootx.h"
#include "include.h"

typedef struct {
    uint8_t addr_gpio[32];   // GPIO for each logical address bit
    uint8_t num_addr_bits;
    uint8_t min_addr_pin;
    uint8_t data_gpio[16];   // GPIO for each logical data bit
    uint8_t num_data_bits;
    uint8_t min_data_pin;
    uint8_t x1;
    uint8_t x2;
    uint8_t cs1;
    uint8_t cs2;
    uint8_t cs3;
    onerom_rom_type_t rom_type;
    uint8_t chip_pins;
    uint8_t chip_addr_bits;
    uint8_t chip_data_bits;
    onerom_cs_state_t cs1_state;
    onerom_cs_state_t cs2_state;
    onerom_cs_state_t cs3_state;
} rom_pin_layout_t;

const onerom_rom_slot_t *app_get_active_rom_set(const usb_plugin_context_t *ctx);
uint32_t app_get_active_rom_size(const usb_plugin_context_t *ctx);
pb_status_t app_retrieve_pin_layout(
    const usb_plugin_context_t *ctx,
    rom_pin_layout_t           *layout
);
pb_status_t app_get_logical_byte_from_logical_addr(
    uint32_t logical_addr,
    uint32_t  *value_out,
    const rom_pin_layout_t *layout,
    const usb_plugin_context_t *ctx
);
pb_status_t app_set_logical_byte_at_logical_addr(
    uint32_t                    logical_addr,
    uint8_t                     logical_value,
    const rom_pin_layout_t     *layout,
    const usb_plugin_context_t *ctx
);

#endif // USB_ROM_H

