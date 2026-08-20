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

// These drive the pad model, so a host test sees the status LED move the way a
// device's pin does.
//
// The wiring is active low: the pin is driven low to light the LED.

static inline void status_led_on(uint8_t pin) {
    STUB_LOG("status_led_on");
    stub_gpio_set(pin, ORA_GPIO_STATE_LOW);
}

static inline void status_led_off(uint8_t pin) {
    STUB_LOG("status_led_off");
    stub_gpio_set(pin, ORA_GPIO_STATE_HIGH);
}

// A PIO block has taken the pin, or given it back.
//
// On a device the pin's function select decides where a write lands, and the
// hardware needs no telling.  Here the model is told, so an SIO write while a
// block holds the pin changes nothing - as it would not on a device.
static inline void gpio_pio_claim(uint8_t pin) {
    STUB_LOG("gpio_pio_claim");
    stub_gpio_set_pio_owned(pin, 1);
}

static inline void gpio_pio_release(uint8_t pin) {
    STUB_LOG("gpio_pio_release");
    stub_gpio_set_pio_owned(pin, 0);
}

static inline void status_led_disable(uint8_t pin) {
    STUB_LOG("status_led_disable");
    stub_gpio_set(pin, ORA_GPIO_STATE_INPUT);
}

#endif // RP235X_INLINES_H