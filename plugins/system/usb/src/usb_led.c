// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "usb_plugin.h"

// Which ORA LED a wire channel names, or 0 if this plugin does not know the
// channel.  The two enumerations are separate deliberately - one is One ROM's
// wire and the other the plugin API - so the mapping is written down rather
// than assumed, and a channel outside it is refused rather than falling through
// onto an LED the host did not name.
static uint8_t led_ora_id(uint8_t led_id, uint8_t *ora_led_out) {
    switch (led_id) {
        case ONEROM_LED_ID_STATUS:
            *ora_led_out = ORA_LED_STATUS;
            return 1;
        case ONEROM_LED_ID_RGB:
            *ora_led_out = ORA_LED_RGB;
            return 1;
        default:
            return 0;
    }
}

void led_init_caps(void) {
    uint32_t neopixel = GPIO_NONE;
    ora_get_metadata_uint_fn_t get_metadata_uint =
        context.ora_lookup_fn(ORA_ID_GET_METADATA_UINT);

    context.led_set = context.ora_lookup_fn(ORA_ID_LED_SET);
    context.led_get = context.ora_lookup_fn(ORA_ID_LED_GET);

    if (context.led_set == NULL) {
        LOG("Firmware has no LED engine; RGB control unavailable");
        return;
    }

    // The capability bit says the command reaches an engine, not that this
    // board has the LED.  A board without one refuses the command, which is a
    // different answer from a device that cannot be asked at all.
    context.features |= ONEROM_FEAT_LED_ARGS;

    if ((get_metadata_uint != NULL) &&
        (get_metadata_uint(ORA_METADATA_KEY_GPIO_NEOPIXEL, &neopixel) ==
         ORA_RESULT_OK) && (neopixel != GPIO_NONE)) {
        DEBUG("RGB LED on GPIO %u", (unsigned)neopixel);
    } else {
        DEBUG("Board has no RGB LED");
    }
}

pb_status_t led_handle_set(const onerom_set_led_args_t *args) {
    ora_led_request_t req = {0};
    ora_result_t result;
    uint8_t ora_led;

    if (context.led_set == NULL) {
        return PB_STATUS_UNKNOWN_CMD;
    }

    if (!led_ora_id(args->led_id, &ora_led)) {
        return PB_STATUS_NOT_FOUND;
    }

    if (args->hold_ms > ONEROM_LED_MAX_HOLD_MS) {
        return PB_STATUS_INVALID_ARG;
    }

    req.size       = sizeof(req);
    req.led        = ora_led;
    req.mode       = args->sub_cmd;
    req.brightness = args->brightness;
    req.r          = args->r;
    req.g          = args->g;
    req.b          = args->b;
    req.period_ms  = args->period_ms;
    req.hold_ms    = args->hold_ms;

    // Applied here rather than deferred to a later pass of the task loop: the
    // firmware's engine holds the mode and keeps it going, including a beacon
    // and a flame, so this call does not outlive the command and its answer is
    // one the host wants to hear.
    result = context.led_set(&req);

    switch (result) {
        case ORA_RESULT_OK:
            return PB_STATUS_OK;
        case ORA_RESULT_NOT_SUPPORTED:
            return PB_STATUS_NOT_FOUND;
        default:
            return PB_STATUS_INVALID_ARG;
    }
}

uint8_t led_can_query(void) {
    return context.led_get != NULL;
}

pb_status_t led_fill_state(uint8_t led_id, onerom_led_state_t *out) {
    ora_led_state_t state = {0};
    ora_result_t result;
    uint8_t ora_led;

    if (!led_ora_id(led_id, &ora_led)) {
        return PB_STATUS_NOT_FOUND;
    }

    // The firmware writes at most this many bytes and reports how many it did,
    // so a plugin built against a different version of the structure than the
    // running firmware reads only what that firmware wrote.
    state.size = sizeof(state);

    result = context.led_get(ora_led, &state);
    if (result != ORA_RESULT_OK) {
        return PB_STATUS_INVALID_ARG;
    }

    out->struct_len  = (uint16_t)sizeof(*out);
    out->led_id      = led_id;
    out->present     = state.present;
    out->mode        = state.mode;
    out->brightness  = state.brightness;
    out->r           = state.r;
    out->g           = state.g;
    out->b           = state.b;
    out->gpio        = state.gpio;
    out->shared_gpio = state.shared_gpio;
    out->period_ms   = state.period_ms;

    return PB_STATUS_OK;
}
