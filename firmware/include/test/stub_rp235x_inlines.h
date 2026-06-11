// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef RP235X_INLINES_H
#define RP235X_INLINES_H

#include "test/stub.h"

// Make inline
static inline void main_loop_gpio_init() {
    STUB_LOG("main_loop_gpio_init");
}

static inline void set_x_pulls(
    const sdrr_pins_t *pins,
    uint8_t x1_pull,
    uint8_t x2_pull
) {
    (void)pins;
    (void)x1_pull;
    (void)x2_pull;
    STUB_LOG("set_x_pulls");
}

static inline void setup_data_masks(
    const sdrr_info_t *info,
    const sdrr_runtime_info_t *runtime_info,
    uint32_t *data_output_mask_val,
    uint32_t *data_input_mask_val
) {
    (void)info;
    (void)runtime_info;
    (void)data_output_mask_val;
    (void)data_input_mask_val;
    STUB_LOG("setup_data_masks");
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