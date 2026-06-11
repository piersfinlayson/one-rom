// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// A minimal One ROM plugin example.  This plugin blinks the status LED.

#include "plugin.h"

// Logic to allow this plugin to be built as either a system or user plugin,
// based on the PLUGIN_TYPE passed in on make.
#if defined(PLUGIN_TYPE_NUM) && (PLUGIN_TYPE_NUM == ORA_PLUGIN_TYPE_SYSTEM)
ORA_DEFINE_SYSTEM_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 6, 7);
#else // User plugin
ORA_DEFINE_USER_PLUGIN(plugin_main, 0, 1, 0, 0, 0, 6, 7);
#endif // Plugin type check

void plugin_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the status LED control function.
    ora_set_status_led_fn_t set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);

    uint8_t on = 0;
    while (1) {
        // Blink the LED
        set_status_led(on);
        on = !on;
        for (volatile int i = 0; i < 10000000; i++);
    }
}