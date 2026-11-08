// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// A minimal One ROM plugin example.  This plugin blinks the status LED.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.  ora/plugin.mk defines
// PLUGIN_IS_SYSTEM to 1 or 0 for every build.
#ifndef PLUGIN_IS_SYSTEM
#error "PLUGIN_IS_SYSTEM is not defined - build this with ora/plugin.mk"
#endif
#if PLUGIN_IS_SYSTEM
ORA_SYSTEM_PLUGIN(plugin_setup, plugin_loop, 0, 1, 0, 0, 0, 7, 2);
#else // User plugin
ORA_USER_PLUGIN(plugin_setup, plugin_loop, 0, 1, 0, 0, 0, 7, 2);
#endif // Plugin type check

static ora_set_status_led_fn_t set_status_led;
static uint8_t on;

void plugin_setup(
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

void plugin_loop(void) {
    if (set_status_led != NULL) {
        set_status_led(on);
        on = !on;
    }
    ora_delay_ms(500);
}
