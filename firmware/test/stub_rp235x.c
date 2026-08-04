// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Stubs out RP235X specific routines
//
// Specifically target routines accessing hardware registers.

#include "include.h"
#include "test/stub.h"

#define APIO_LOG_IMPL
#define APIO_LOG_ENABLE(fmt, ...) printf(fmt "\n", ##__VA_ARGS__)

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

// Defined below, alongside stub_set_rp_variant().
extern uint8_t stub_rp235x_is_b;

// Maximum GPIO number for the RP235x variant under test.
//
// The firmware's MAX_GPIOS is max_gpios[RUNTIME->rp235x], and RUNTIME->rp235x
// is only populated once firmware_main() has run - it cold-boots as RP235XA.
// Stubs called before boot (stub_set_sel_image) must not read it: doing so
// judged a B-variant board's sel pins (38-41) out of range, drove no pins, and
// silently selected image 0 on the first boot of a process.  The variant is
// the test's own choice, supplied by stub_set_rp_variant(), so use that - it
// is correct both before and during boot.  (The plugin API cannot serve this:
// it only exists once the firmware is running, and exposes no variant or GPIO
// count in any case.)
static uint8_t stub_max_gpios(void) {
    return max_gpios[stub_rp235x_is_b ? RP235XB : RP235XA];
}

// Sel pin stub state
static uint64_t stub_gpio_sel_value;
static uint8_t stub_sel_image;

uint8_t stub_set_sel_image(uint8_t image_index) {
    uint8_t valid_bits = 0;
    uint8_t gpio_limit = stub_max_gpios();
    stub_gpio_sel_value = 0;
    for (int ii = 0; ii < MAX_IMG_SEL_PINS; ii++) {
        uint8_t pin = HW->gpio_sel[ii];
        if (pin < gpio_limit) {
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
    uint8_t gpio_limit = stub_max_gpios();
    for (int ii = 0; ii < MAX_IMG_SEL_PINS; ii++) {
        uint8_t pin = HW->gpio_sel[ii];
        if (pin < gpio_limit) {
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

void disable_swd(void) {
    STUB_LOG("disable_swd");
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

uint8_t logging_enabled = 1;

void stub_log_v(const char* msg, va_list args) {
    if (logging_enabled) {
        vprintf(msg, args);
        printf("\n");
    }
}

void stub_log(const char* msg, ...) {
    va_list args;
    va_start(args, msg);
    stub_log_v(msg, args);
    va_end(args);
}

// As stub_log_v, with a prefix.  The prefix is inside the logging_enabled
// check so a disabled log emits nothing at all, rather than a bare prefix.
void stub_log_prefix_v(const char* prefix, const char* msg, va_list args) {
    if (logging_enabled) {
        printf("%s", prefix);
        vprintf(msg, args);
        printf("\n");
    }
}

void err_log(const char* msg, ...) {
    printf("ERROR: ");
    va_list args;
    va_start(args, msg);
    stub_log_v(msg, args);
    va_end(args);
}

// Allocate twice the required RAM ROM table size, so it can be aligned to
// 512KB (done in preload_rom_image).
uint32_t test_ram_rom_image_table[RAM_ROM_TABLE_SIZE*2/4] = {0};
uint64_t *get_ram_rom_image_table_aligned(void) {
    uint64_t address = (uint64_t)(uintptr_t)test_ram_rom_image_table;
    address += RAM_ROM_TABLE_SIZE-1;
    address /= RAM_ROM_TABLE_SIZE;
    address = address * RAM_ROM_TABLE_SIZE;
    return (uint64_t *)(uintptr_t)address;
}

limp_mode_pattern_t limp_mode_value = LIMP_MODE_NONE;
void limp_mode(limp_mode_pattern_t pattern) {
    limp_mode_value = pattern;
}

SEGGER_RTT_CB _SEGGER_RTT = {0};

uint8_t stub_rp235x_is_b = 0;

void stub_set_rp_variant(uint8_t is_b) {
    stub_rp235x_is_b = is_b;
}