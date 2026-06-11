// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Stubs out RP235X specific routines
//
// Specifically target routines accessing hardware registers.

#include "sdrr_config.h" 

#if defined(TEST_BUILD) && defined(RP235X)

#include "include.h"
#include "test/stub.h"

void platform_specific_init(void) {
    STUB_LOG("platform_specific_init");
}

void setup_vbus_interrupt(void) {
    STUB_LOG("setup_vbus_interrupt");
}

void vbus_connect_handler(void) {
    STUB_LOG("vbus_connect_handler");
}

void setup_gpio(void) {
    STUB_LOG("setup_gpio");
}

void setup_qmi(rp235x_clock_config_t *config) {
    (void)config;
    STUB_LOG("setup_qmi");
}

void setup_vreg(rp235x_clock_config_t *config) {
    (void)config;
    STUB_LOG("setup_vreg");
}

// Set up the PLL with the generated values
void setup_pll(rp235x_clock_config_t *config) {
    (void)config;
    STUB_LOG("setup_pll");
}

void setup_usb_pll(void) {
    STUB_LOG("setup_usb_pll");
}

void setup_adc(void) {
    STUB_LOG("setup_adc");
}

uint16_t get_temp(void) {
    STUB_LOG("get_temp");
    return 0;
}

void setup_cp(void) {
    STUB_LOG("setup_cp");
}

// Sel pin stub state
static uint64_t stub_gpio_sel_value;
static uint8_t stub_sel_image;

uint8_t stub_set_sel_image(uint8_t image_index) {
    uint8_t valid_bits = 0;
    stub_gpio_sel_value = 0;
    for (int ii = 0; ii < MAX_IMG_SEL_PINS; ii++) {
        uint8_t pin = sdrr_info.pins->sel[ii];
        if (pin < MAX_USED_GPIOS) {
            valid_bits++;
            if (image_index & (1 << ii)) {
                stub_gpio_sel_value |= (1ULL << pin);
            }
        }
    }

    stub_sel_image = image_index % (1 << valid_bits);

    return stub_sel_image;
}

uint8_t stub_get_sel_image(void) {
    return stub_sel_image;
}

uint32_t setup_sel_pins(uint64_t *sel_mask, uint64_t *flip_bits) {
    *sel_mask = 0;
    *flip_bits = 0;
    uint32_t count = 0;
    for (int ii = 0; ii < MAX_IMG_SEL_PINS; ii++) {
        uint8_t pin = sdrr_info.pins->sel[ii];
        if (pin < MAX_USED_GPIOS) {
            *sel_mask |= (1ULL << pin);
            count++;
        }
    }
    return count;
}

uint64_t get_sel_value(uint64_t sel_mask, uint64_t flip_bits) {
    (void)flip_bits;
    return stub_gpio_sel_value & sel_mask;
}

void disable_sel_pins(void) {
    STUB_LOG("disable_sel_pins");
}

// Enters bootloader mode.
void enter_bootloader(void) {
    STUB_LOG("enter_bootloader");
}

void platform_logging(void) {
    STUB_LOG("platform_logging");
}

void setup_xosc(void) {
    STUB_LOG("setup_xosc");
}
#endif // TEST_BUILD && RP235X