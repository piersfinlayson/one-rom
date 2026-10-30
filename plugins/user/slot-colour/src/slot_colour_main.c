// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM user plugin to identify the booted slot by using the RGB LED

#include "plugin.h"

ORA_DEFINE_USER_PLUGIN(
    slot_colour_main,
    0, 1, 0, 0,     // Plugin version
    0, 7, 2         // Minimum One ROM firmware version required
);

#define BRIGHTNESS_PERCENT  50u

static const uint8_t COLOURS[][3] = {
    { 255,   0,   0 },  // Red
    {   0, 255,   0 },  // Green
    {   0,   0, 255 },  // Blue
    { 255, 255,   0 },  // Yellow
    {   0, 255, 255 },  // Cyan
    { 255,   0, 255 },  // Magenta
    { 255, 128,   0 },  // Orange
    { 255, 255, 255 },  // White
};
#define NUM_COLOURS (sizeof(COLOURS) / sizeof(COLOURS[0]))

static void show_slot_colour(ora_lookup_fn_t ora_lookup_fn) {
    ora_log_fn_t log = ora_lookup_fn(ORA_ID_LOG);
    ora_get_metadata_uint_fn_t get_metadata_uint =
        ora_lookup_fn(ORA_ID_GET_METADATA_UINT);
    ora_get_flash_slot_count_fn_t get_flash_slot_count =
        ora_lookup_fn(ORA_ID_GET_FLASH_SLOT_COUNT);
    ora_led_set_fn_t led_set = ora_lookup_fn(ORA_ID_LED_SET);

    // ROM_SLOT_INDEX includes the plugin slots.  0xFF means no slot.
    uint32_t rom_slot_index;
    if (get_metadata_uint(ORA_METADATA_KEY_ROM_SLOT_INDEX,
                          &rom_slot_index) != ORA_RESULT_OK ||
        rom_slot_index >= get_flash_slot_count(0u)) {
        log("Slot colour: booted slot unknown");
        return;
    }
    uint32_t plugin_slots = get_flash_slot_count(0u) -
        get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);
    if (rom_slot_index < plugin_slots) {
        log("Slot colour: booted slot %u is a plugin slot",
            (unsigned)rom_slot_index);
        return;
    }
    uint32_t slot = rom_slot_index - plugin_slots;
    const uint8_t *colour = COLOURS[slot % NUM_COLOURS];

    ora_led_request_t req = {
        .size       = (uint8_t)sizeof(req),
        .led        = ORA_LED_RGB,
        .mode       = ORA_LED_MODE_ON,
        .brightness = BRIGHTNESS_PERCENT,
        .red        = colour[0],
        .green      = colour[1],
        .blue       = colour[2],
    };
    ora_result_t result = led_set(&req);
    if (result != ORA_RESULT_OK) {
        log("Slot colour: setting the RGB LED failed: %d", (int)result);
        return;
    }
    log("Slot colour: slot %u", (unsigned)slot);
}

void slot_colour_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    (void)plugin_type;
    (void)entry_args;

    show_slot_colour(ora_lookup_fn);

    // Don't return as that causes the firmware to log an error.
    while (1) {
        __asm volatile ("wfi");
    }
}
