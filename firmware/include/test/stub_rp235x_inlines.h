// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef RP235X_INLINES_H
#define RP235X_INLINES_H

#include <stdint.h>
#include "test/stub.h"

// Make inline
static inline void main_loop_gpio_init() {
    STUB_LOG("main_loop_gpio_init");
}

static inline void status_led_on(uint8_t pin) {
    (void)pin;
    STUB_LOG("status_led_on");
}

static inline void status_led_off(uint8_t pin) {
    (void)pin;
    STUB_LOG("status_led_off");
}

static inline void status_led_disable(uint8_t pin) {
    (void)pin;
    STUB_LOG("status_led_disable");
}

#endif // RP235X_INLINES_H