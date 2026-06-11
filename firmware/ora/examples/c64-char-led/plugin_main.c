// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Watches activity on a C64 character ROM.  If CHAR_LED_ON is detected,
// turns on the status LED, and if CHAR_LED_OFF is detected, turns it off.
//
// Note that the VIC-II is constantly retrieving character data from the ROM,
// even on cycles nothing is drawn on screen, and this causes spurious address
// captures.  CHAR_LED_ON and CHAR_LED_OFF are chosen to be uncommonly
// spuriously generated.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.
ORA_DEFINE_USER_PLUGIN(
    c64_char_led_main,
    0, 1, 1, 0,     // Plugin version
    0, 6, 9         // Minimum One ROM firmware version required
);

// Ring buffer for captured addresses from PIO, written by DMA.  Must be
// aligned to its size, and be a power of 2 in length.  The macro takes care
// of the alignment and size requirements.
#define RING_ENTRIES_LOG2   6   // 2^6 = 64 entries
ORA_RING_BUF_DECLARE_32BIT(ring_buf, RING_ENTRIES_LOG2);
#define RING_ENTRIES_NUM    (1u << RING_ENTRIES_LOG2)

// Character values to monitor for.
#define CHAR_LED_ON         0x3E    // >
#define CHAR_LED_OFF        0x3F    // ?

void c64_char_led_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    (void)plugin_type;
    (void)entry_args;

    // Get plugin API functions we need
    ora_set_status_led_fn_t set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    ora_log_fn_t log = ora_lookup_fn(ORA_ID_LOG);
    ora_setup_address_monitor_fn_t setup_address_monitor = ora_lookup_fn(ORA_ID_SETUP_ADDRESS_MONITOR);
    ora_start_address_monitor_fn_t start_address_monitor = ora_lookup_fn(ORA_ID_START_ADDRESS_MONITOR);
    ora_map_addr_to_phys_fn_t remap_addr = ora_lookup_fn(ORA_ID_MAP_ADDR_TO_PHYS);
    ora_get_address_monitor_ring_write_pos_fn_t get_ring_write_pos = ora_lookup_fn(ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS);

    log("One ROM - C64 character ROM monitor example plugin");

    // Start with status LED off
    set_status_led(0);

    // Initialize the PIO state machines and DMA channel to capture addresses
    // when CS is active.
    if (setup_address_monitor(ring_buf, RING_ENTRIES_LOG2, ORA_MONITOR_MODE_OBSERVE, 32, NULL) != ORA_RESULT_OK) {
        log("Failed to set up address monitor");
        return;
    }

    // Precalculate the addresses to match on for the different characters.
    // Shift them left 3 bits first, as the lower 3 bits are used by the
    // VIC-II chip to select the row.  For the mask we want everything except
    // the row bits.  remap_addr() takes care of avoiding the mask including
    // any higher address bits than the current ROM type supports.
    uint32_t led_on_match = remap_addr(CHAR_LED_ON << 3);
    uint32_t led_off_match = remap_addr(CHAR_LED_OFF << 3);
    //uint32_t char_mask = remap_addr(~7u);
    uint32_t char_mask = 0;
    for (int i = 3; i < 12; i++) {
        char_mask |= remap_addr(1u << i);
    }
    log("LED on match: 0x%04x off match: 0x%04x, mask: 0x%04x", led_on_match, led_off_match, char_mask);

    start_address_monitor();

    // Get a pointer to the current write position in the ring buffer, which
    // the DMA engine updates as it writes captures.  We'll use this to
    // determine when new captures are available.
    volatile uint32_t * volatile *ring_write_pos = get_ring_write_pos();
    
    // Set the read pointer to the last DMA write address (which will be the
    // start of the ring buffer if it hasn't done anything yet).  This ensures
    // we throw away anything captured up to this point, which should just be
    // garbage due to the VIC-II starting before the CPU and One ROM.
    log("Starting main loop");
    volatile uint32_t *read_ptr = *ring_write_pos;

    // Now go into an infinite loop, watching for new captures and updating
    // the LED
    while (1) {
        // Get the current write position of the address monitor
        while (read_ptr != *ring_write_pos) {
            // A new address has been captured - get it
            uint32_t addr = *read_ptr;

            // Figure out if it's a character we care about, and update the
            // LED accordingly
            if ((addr & char_mask) == led_on_match) {
                log("Status LED on");
                set_status_led(1);
            } else if ((addr & char_mask) == led_off_match) {
                log("Status LED off");
                set_status_led(0);
            }

            // Advance the read pointer, wrapping if necessary
            if (++read_ptr >= ring_buf + RING_ENTRIES_NUM) {
                read_ptr = ring_buf;
            }
        }
    }
}
