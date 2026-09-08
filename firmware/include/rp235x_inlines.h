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

// Say that a PIO block has taken the pin, and given it back.
//
// Nothing to do here: the pad follows the pin's function select, which the
// caller has already written, and an SIO write to a pin a block holds changes
// nothing of its own accord.  A host build has no such hardware, so its
// counterpart records the fact - see test/stub_rp235x_inlines.h.
static inline void __attribute__((always_inline)) gpio_pio_claim(uint8_t pin) {
    (void)pin;
}

static inline void __attribute__((always_inline)) gpio_pio_release(uint8_t pin) {
    (void)pin;
}

static inline void __attribute__((always_inline)) status_led_disable(uint8_t pin) {
    // Disable the status LED by disabling output
    GPIO_PAD(pin) |= PAD_OUTPUT_DISABLE;
}

#endif // RP235X_INLINES_H
