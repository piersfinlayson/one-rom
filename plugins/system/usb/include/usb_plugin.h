// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef USB_PLUGIN_H
#define USB_PLUGIN_H

#include <stdint.h>
#include <stdbool.h>
#include "plugin.h"
#include "tusb.h"
#include "include.h"
#include "usb_custom_pbx.h"
#include "usb_led.h"
#include "usb_gpio.h"

// Context structure for our plugin
typedef struct {
    ora_lookup_fn_t ora_lookup_fn;
    ora_log_fn_t log;
    ora_debug_log_fn_t debug;
    ora_err_log_fn_t err_log;
    ora_set_status_led_fn_t set_status_led;
    ora_get_active_ram_slot_fn_t get_active_ram_slot;
    ora_get_ram_slot_info_fn_t get_ram_slot_info;
    ora_read_ram_rom_slot_fn_t read_ram_rom_slot;
    ora_reprogram_ram_rom_slot_fn_t reprogram_ram_rom_slot;

    // GPIO control.  Both are NULL on firmware that predates the GPIO plugin
    // API (added in firmware 0.7.1); see gpio_init_caps().
    ora_gpio_set_fn_t gpio_set;
    ora_gpio_query_fn_t gpio_query;

    // What ONEROM_CMD_GET_CAPS reports, decided once at init.
    //
    // features is a ONEROM_FEAT_* bitmap, and num_gpios the running RP2350
    // variant's GPIO count.  Both are zero when the running firmware cannot
    // support GPIO control, which is what keeps the plugin's min_fw_version at
    // 0.7.0: the host is told the commands are unavailable rather than the
    // plugin refusing to load.
    uint32_t features;
    uint8_t num_gpios;

    uint32_t timer_ms;
    onerom_pending_t pending;
    onerom_in_xfer_t in_xfer;
    led_status_t led_status;
    gpio_status_t gpio_status;
} usb_plugin_context_t;

// Forward declaration of the context, which we define in usb_main.c
extern usb_plugin_context_t context;

// Forward declaration of plugin's Picoboot functions, from usb_picoboot.c
void usb_picoboot_init(uint8_t ep_out, uint8_t ep_in);
bool usb_picoboot_control_xfer_cb(
    uint8_t rhport,
    uint8_t stage,
    tusb_control_request_t const *request
);
void usb_picoboot_tx_cb(uint8_t idx, uint32_t sent_bytes);
void usb_picoboot_rx_cb(uint8_t idx, uint8_t const *buf, uint32_t count);
void usb_picoboot_task(void);

// Resolve a configured USB serial override from device metadata, defined in
// usb_main.c.  Widens the override into desc_str and returns the number of
// code units, or 0 when no override applies (so the caller falls back to the
// chip-ID serial).
size_t usb_get_serial(uint16_t *desc_str, size_t max_chars);

// Logging macros
#if defined(DEBUG)
#undef DEBUG
#endif
#define DEBUG(...) do { \
    if (context.debug) { \
        context.debug(__VA_ARGS__); \
    } \
} while (0)

#if defined(LOG)
#undef LOG
#endif
#define LOG(...) do { \
    if (context.log) { \
        context.log(__VA_ARGS__); \
    } \
} while (0)

#if defined(ERR)
#undef ERR
#endif
#define ERR(...) do { \
    if (context.err_log) { \
        context.err_log(__VA_ARGS__); \
    } \
} while (0)

//--------------------------------------------------------------------+
// TIMER0 peripheral
//--------------------------------------------------------------------+
#define TIMER0_BASE         0x400b0000

// TIMER0 Registers
#define TIMER0_TIMELR       (*((volatile uint32_t *)(TIMER0_BASE + 0x0C)))
#define TIMER0_ALARM0       (*((volatile uint32_t *)(TIMER0_BASE + 0x10)))
#define TIMER0_INTE         (*((volatile uint32_t *)(TIMER0_BASE + 0x40)))
#define TIMER0_INTR         (*((volatile uint32_t *)(TIMER0_BASE + 0x3C)))

//--------------------------------------------------------------------+
// TICKS peripheral
//--------------------------------------------------------------------+
#define TICKS_BASE          0x40108000

// TICKS Registers
#define TICKS_TIMER0_CTRL   (*((volatile uint32_t *)(TICKS_BASE + 0x18)))
#define TICKS_TIMER0_CYCLES (*((volatile uint32_t *)(TICKS_BASE + 0x1C)))

//--------------------------------------------------------------------+
// RESET peripheral
//--------------------------------------------------------------------+
#define RESETS_BASE         0x40020000

#define RESET_RESET     (*((volatile uint32_t *)(RESETS_BASE + 0x00)))
#define RESET_DONE      (*((volatile uint32_t *)(RESETS_BASE + 0x08)))
#define RESET_TIMER0        (1 << 23)

#endif // USB_PLUGIN_H