// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM STM32F4 Specific Routines

#ifndef RP235X_INLINES_H
#define RP235X_INLINES_H

// Inlined as may be used by main_loop (which may be in RAM)
static inline void __attribute__((always_inline)) status_led_on(uint8_t pin) {
    // Set to 0 to turn on
    SIO_GPIO_OUT_CLR_PIN(pin);
}

// Inlined as may be used by main_loop (which may be in RAM)
static inline void __attribute__((always_inline)) status_led_off(uint8_t pin) {
    // Set to 1 to turn on
    SIO_GPIO_OUT_SET_PIN(pin);
}

static inline void __attribute__((always_inline)) status_led_disable(uint8_t pin) {
    // Disable the status LED by disabling output
    GPIO_PAD(pin) |= PAD_OUTPUT_DISABLE;
}

#endif // RP235X_INLINES_H
