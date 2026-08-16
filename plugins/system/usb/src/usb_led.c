// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "usb_plugin.h"

static void led_set(uint8_t state) {
    context.set_status_led(state);
    context.led_status.led_state = state;
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
            context.led_status.pre_beacon_state = context.led_status.led_state;
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