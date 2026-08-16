// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "usb_plugin.h"

static void led_set(uint8_t state) {
    context.set_status_led(state);
    context.led_status.led_state = state;
}

// The status LED's live state, which the firmware holds - any plugin can drive
// the LED through ora_set_status_led.  led_state is only what this plugin last
// set, and is the fallback where the firmware cannot answer.
static uint8_t led_live_state(void) {
    ora_get_metadata_uint_fn_t get_metadata_uint =
        context.ora_lookup_fn(ORA_ID_GET_METADATA_UINT);
    uint32_t state = 0;

    if ((get_metadata_uint != NULL) &&
        (get_metadata_uint(ORA_METADATA_KEY_STATUS_LED_STATE, &state) ==
         ORA_RESULT_OK)) {
        return state ? 1u : 0u;
    }

    return context.led_status.led_state;
}

void led_handle_pending_set(void) {
    onerom_led_subcmd_t sub_cmd = context.pending.args.set_led.sub_cmd;
    switch (sub_cmd) {
        case ONEROM_LED_OFF:
            context.led_status.mode = ONEROM_LED_OFF;
            led_set(0);
            break;

        case ONEROM_LED_ON:
            context.led_status.mode = ONEROM_LED_ON;
            led_set(1);
            break;

        case ONEROM_LED_BEACON:
            // Captured on the way into beacon mode only.  A beacon arriving
            // while one is running restarts it, and the state to restore is
            // still the one from before the first - not the blink phase the
            // LED happens to be in now.
            if (context.led_status.mode != ONEROM_LED_BEACON) {
                context.led_status.pre_beacon_state = led_live_state();
            }
            uint32_t start = context.get_plugin_uptime_ms();
            context.led_status.beacon_start_ms  = start;
            context.led_status.last_toggle_ms   = start;
            context.led_status.mode             = ONEROM_LED_BEACON;
            led_set(1);
            break;

        case ONEROM_LED_FLAME:
            context.led_status.flame_index = 0;
            context.led_status.last_toggle_ms = context.get_plugin_uptime_ms();
            context.led_status.mode = ONEROM_LED_FLAME;
            led_set(flame_table[0].state);
            break;

        default:
            LOG("usb_plugin_task: unhandled SET_LED sub_cmd %u", sub_cmd);
            break;
    }
}

void led_handle_ongoing_led_modes(void) {
    // Drive ongoing LED modes
    switch (context.led_status.mode) {
        case ONEROM_LED_BEACON:
            ;
            uint32_t now = context.get_plugin_uptime_ms();
            if (now - context.led_status.beacon_start_ms >= ONEROM_BEACON_DURATION_MS) {
                // Beacon done, restore prior state
                led_set(context.led_status.pre_beacon_state);
                context.led_status.mode = context.led_status.pre_beacon_state
                                          ? ONEROM_LED_ON : ONEROM_LED_OFF;
            } else if (now - context.led_status.last_toggle_ms >= ONEROM_BEACON_TOGGLE_MS) {
                led_set(context.led_status.led_state ^ 1u);
                context.led_status.last_toggle_ms = now;
            }
            break;

        case ONEROM_LED_FLAME:
            ;
            uint32_t now2 = context.get_plugin_uptime_ms();
            uint8_t idx = context.led_status.flame_index;
            if (now2 - context.led_status.last_toggle_ms >= flame_table[idx].ms) {
                idx = (idx + 1) % FLAME_TABLE_LEN;
                context.led_status.flame_index = idx;
                context.led_status.last_toggle_ms = now2;
                led_set(flame_table[idx].state);
            }
            break;

        default:
            break;
    }
}