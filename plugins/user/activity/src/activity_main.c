// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM user plugin to blink the status LED if there's ROM activity

#include "plugin.h"

ORA_USER_PLUGIN(
    activity_setup,
    activity_loop,
    0, 1, 0, 0,     // Plugin version
    0, 7, 2         // Minimum One ROM firmware version required
);

static ora_set_status_led_fn_t set_status_led;
static ora_is_pin_output_fn_t is_pin_output;
static uint8_t data_pins[16];
static uint8_t num_data_pins;
static uint8_t on;

void activity_setup(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Look up the status LED control function.
    set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    is_pin_output = ora_lookup_fn(ORA_ID_IS_PIN_OUTPUT);
    ora_get_data_pin_nums_fn_t get_data_pin_nums = ora_lookup_fn(ORA_ID_GET_DATA_PIN_NUMS);
    if (set_status_led == NULL || is_pin_output == NULL || get_data_pin_nums == NULL) {
        return;
    }

    // Get the data pins.  One ROM 40 supports 16 data pins, so try to get
    // that many.  If this isn't a One ROM 40, the function will return 8.
    num_data_pins = get_data_pin_nums(data_pins, 16);

    // Start with status LED off
    set_status_led(on);
}

void activity_loop(void) {
    // num_data_pins is 0 if setup failed.
    if (num_data_pins == 0) {
        return;
    }

    // Brief pause
    ora_delay_ms(50);

    // Check for One ROM activity - are any data lines set to outputs?
    uint8_t activity = 0;
    for (int ii = 0; ii < num_data_pins; ii++) {
        if (is_pin_output(data_pins[ii])) {
            activity = 1;
            break;
        }
    }

    // If there is activity, toggle the LED
    if (activity) {
        on = !on;
        set_status_led(on);
    }
}
