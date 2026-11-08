// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM user plugin to blink the status LED

#include "plugin.h"

ORA_USER_PLUGIN(
    blink_setup,
    blink_loop,
    0, 1, 1, 0,     // Plugin version
    0, 7, 2         // Minimum One ROM firmware version required
);

static ora_set_status_led_fn_t set_status_led;
static uint8_t on;

void blink_setup(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the status LED control function.
    set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
}

void blink_loop(void) {
    if (set_status_led == NULL) {
        return;
    }

    // Blink the LED
    set_status_led(on);
    on = !on;
    ora_delay_ms(500);
}
