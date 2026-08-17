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

// ---------------------------------------------------------------------------
// GPIO pad model
//
// Mirrors what ora_gpio_query reads out of GPIOx_STATUS on a device: the output
// enable, the level the output drives, and the level an input reads.  See
// test/stub.h for what is deliberately not modelled.
// ---------------------------------------------------------------------------

// Pins the model covers.  The B variant has the most, at 48 - see max_gpios[]
// in constants.c.  Sizing to the larger variant means the model covers whatever
// stub_set_rp_variant() selected, and the firmware's own MAX_GPIOS is what
// bounds the pin numbers that reach here.
#define STUB_MAX_GPIOS 48u

static uint8_t stub_gpio_oe[STUB_MAX_GPIOS];
static uint8_t stub_gpio_out[STUB_MAX_GPIOS];
static uint8_t stub_gpio_in[STUB_MAX_GPIOS];

void stub_gpio_set(uint8_t gpio, uint8_t state) {
    if (gpio >= STUB_MAX_GPIOS) {
        return;
    }
    if (state == ORA_GPIO_STATE_INPUT) {
        stub_gpio_oe[gpio] = 0;
    } else {
        stub_gpio_oe[gpio] = 1;
        stub_gpio_out[gpio] = (state == ORA_GPIO_STATE_HIGH) ? 1 : 0;
    }
}

void stub_set_gpio_input(uint8_t gpio, uint8_t level) {
    if (gpio < STUB_MAX_GPIOS) {
        stub_gpio_in[gpio] = level ? 1 : 0;
    }
}

uint8_t stub_gpio_is_output(uint8_t gpio) {
    return (gpio < STUB_MAX_GPIOS) ? stub_gpio_oe[gpio] : 0;
}

// An output reports what it drives, an input what it reads - the same rule
// ora_gpio_query applies to GPIOx_STATUS on a device.
uint8_t stub_gpio_level(uint8_t gpio) {
    if (gpio >= STUB_MAX_GPIOS) {
        return 0;
    }
    return stub_gpio_oe[gpio] ? stub_gpio_out[gpio] : stub_gpio_in[gpio];
}

// A device's pads come out of reset with their drivers off.
void stub_gpio_reset(void) {
    for (uint32_t ii = 0; ii < STUB_MAX_GPIOS; ii++) {
        stub_gpio_oe[ii] = 0;
        stub_gpio_out[ii] = 0;
        stub_gpio_in[ii] = 0;
    }
}

void setup_gpio(void) {
    STUB_LOG("setup_gpio");
}

// Put back what a reset puts back.
//
// A test build runs many boots in one process, and the firmware's own statics
// are ordinary host objects there: nothing restores them the way a device's
// reset restores RAM from flash.  The emulator calls this on every boot.
//
// Only firmware state with no counterpart in a plugin belongs here.  The log
// channel claims do not: a plugin records which channels it holds in its own
// statics, and those are not cleared unless that plugin's harness clears them,
// so resetting one half alone leaves the two disagreeing.  A harness that
// clears its plugin's state calls ora_log_reset_claims() alongside it.
//
// Runtime info is not here either — the emulator restores that itself, from a
// snapshot taken before the first boot.
void onerom_test_reset(void) {
    stub_gpio_reset();
    stub_timer_reset();
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

// There is no TIMER0 to start.  Under a test build the counter behind
// ora_get_plugin_uptime_ms() is the scripted sequence further down this file,
// which a test drives directly.
void setup_timer0(void) {
    STUB_LOG("setup_timer0");
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
    va_list args;
    va_start(args, msg);
    stub_log_prefix_v("ERROR: ", msg, args);
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

uint8_t stub_rp235x_is_b = 0;

void stub_set_rp_variant(uint8_t is_b) {
    stub_rp235x_is_b = is_b;
}

// The microsecond count ora_get_plugin_uptime_ms() reads under a test build.  Starts
// at zero, as a device's counter does when the firmware releases TIMER0, and
// moves only when a test moves it - nothing here advances with wall time, so a
// run is repeatable.
//
// Held as a sequence of successive counter values rather than one number, with
// each read of either half consuming a step.  That is what lets a test place a
// high-half change between the firmware's two reads of it.
static uint64_t stub_timer_script[STUB_TIMER_SCRIPT_MAX] = {0};
static uint32_t stub_timer_script_len = 1;
static uint32_t stub_timer_script_pos = 0;

// The value the next half-read sees.  Holds at the final entry once the script
// is spent, so a script shorter than the read sequence settles rather than
// running off the end.
static uint64_t stub_timer_current(void) {
    uint32_t pos = stub_timer_script_pos;
    if (pos >= stub_timer_script_len) {
        pos = stub_timer_script_len - 1;
    }
    return stub_timer_script[pos];
}

static void stub_timer_step(void) {
    if (stub_timer_script_pos < stub_timer_script_len) {
        stub_timer_script_pos++;
    }
}

uint32_t stub_timer_raw_hi(void) {
    uint64_t value = stub_timer_current();
    stub_timer_step();
    return (uint32_t)(value >> 32);
}

uint32_t stub_timer_raw_lo(void) {
    uint64_t value = stub_timer_current();
    stub_timer_step();
    return (uint32_t)value;
}

void stub_set_timer_us(uint64_t us) {
    stub_timer_script[0] = us;
    stub_timer_script_len = 1;
    stub_timer_script_pos = 0;
}

void stub_advance_timer_us(uint64_t delta_us) {
    stub_set_timer_us(stub_timer_script[stub_timer_script_len - 1] + delta_us);
}

void stub_timer_reset(void) {
    stub_set_timer_us(0);
}

void stub_set_timer_raw_script(const uint64_t *values, uint32_t count) {
    STUB_ASSERT(
        values != NULL && count >= 1 && count <= STUB_TIMER_SCRIPT_MAX,
        "stub_set_timer_raw_script: count %u out of range", count
    );
    for (uint32_t i = 0; i < count; i++) {
        stub_timer_script[i] = values[i];
    }
    stub_timer_script_len = count;
    stub_timer_script_pos = 0;
}