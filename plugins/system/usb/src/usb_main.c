// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM system plugin implementing USB

#include "include.h"
#include "usb_plugin.h"
#include "tusb.h"
#include "usb_descriptors.h"
#include "usb_picobootx.h"

// Optimisations:
// - Move timer handler to library and see if it can be better optimised
// - Add IRQ prioritisation a la SDK

// Define this plugin's attribues
void usb_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
);
ORA_SECTION(".plugin_header")
const ora_plugin_header_t ora_plugin_header = {
    .magic    = ORA_PLUGIN_MAGIC,
    .api_version  = ORA_PLUGIN_VERSION_1,
    .major_version = MAJOR_VERSION,
    .minor_version = MINOR_VERSION,
    .patch_version = PATCH_VERSION,
    .build_version = BUILD_VERSION,
    .entry  = usb_main,
    .plugin_type = ORA_PLUGIN_TYPE_SYSTEM,
    .sam_usage = 255,
    .overrides1 = ORA_OVERRIDE1_DISABLE_VBUS_DETECT,
    .properties1 = ORA_PROPERTY1_SUPPORTS_USB_RUNNING | ORA_PROPERTY1_SUPPORTS_YIELD,
    .min_fw_major_version = 0,
    .min_fw_minor_version = 7,
    .min_fw_patch_version = 0,
    .reserved = {0},
};

// Plugin context, stored in .bss
usb_plugin_context_t context;

void init_data_bss(void) {
    extern uint32_t __ramfunc_start;
    extern uint32_t __ramfunc_end;
    extern uint32_t __ramfunc_load;
    extern uint32_t __data_start;
    extern uint32_t __data_end;
    extern uint32_t __data_load;
    extern uint32_t __bss_start;
    extern uint32_t __bss_end;

    // Copy .ramfunc from LMA (flash) to VMA (RAM)
    uint32_t *src = &__ramfunc_load;
    uint32_t *dst = &__ramfunc_start;
    while (dst < &__ramfunc_end) {
        *dst++ = *src++;
    }

    // Copy .data from LMA (flash) to VMA (RAM)
    src = &__data_load;
    dst = &__data_start;
    while (dst < &__data_end) {
        *dst++ = *src++;
    }

    // Zero .bss
    dst = &__bss_start;
    while (dst < &__bss_end) {
        *dst++ = 0;
    }
}

// Timer0 IRQ handler to increment the timer_ms field in our plugin context
void timer0_irq_0_handler(void) {
    TIMER0_INTR = (1 << 0);
    TIMER0_ALARM0 = TIMER0_TIMELR + 1000;
    context.timer_ms++;
}

// Implement a function to get the current time in milliseconds, which the
// USB stack can use for timing.
uint32_t board_millis(void) {
    return context.timer_ms;
}

// tinyusb's name for it
uint32_t tusb_time_millis_api(void) {
    return board_millis();
}

void setup_timer0(uint32_t clkref_mhz) {
    // Release TIMER0 from reset
    RESET_RESET &= ~RESET_TIMER0;
    while (!(RESET_DONE & RESET_TIMER0));

    // Set up TICKS
    TICKS_TIMER0_CYCLES = clkref_mhz;
    TICKS_TIMER0_CTRL = 1; 

    // Enable alarm 0 interrupt
    // ORA_IRQ_TIMER0_IRQ_0 corresponds to bit 0 in TIMER0_INTE
    TIMER0_INTE |= (1 << (ORA_IRQ_TIMER0_IRQ_0 % 4));

    // Fire first alarm 1ms from now
    TIMER0_ALARM0 = TIMER0_TIMELR + 1000;
}

void usb_plugin_task(void) {
    // Handle incoming pending command
    if (context.pending.cmd != ONEROM_PENDING_NONE) {
        switch (context.pending.cmd) {
            case ONEROM_PENDING_SET_LED:
                led_handle_pending_set();
                break;

            default:
                LOG("usb_plugin_task: unhandled pending cmd %u", context.pending.cmd);
                break;
        }
        context.pending.cmd = ONEROM_PENDING_NONE;
    }

    led_handle_ongoing_led_modes();

    // ONEROM_CMD_GPIO_SET is applied in the dispatch handler; only the timed
    // release of a bounded hold is deferred to here, where the millisecond
    // timer can be checked.
    gpio_handle_pending_releases();
}

// Resolve the USB serial override from device metadata and widen it into the
// UTF-16 descriptor buffer.  Returns the number of code units written, or 0
// when there is no override to apply - either the running firmware predates the
// metadata getter, or no override is set - so the caller falls back to the
// chip-ID serial.
//
// The override string lives in flash and is read on demand (zero-copy); nothing
// is cached, and the metadata getter is only looked up here, at descriptor
// time.  An override longer than max_chars is truncated, so the descriptor
// never overruns.  min_fw is unaffected: absence of the getter is handled, not
// required.
size_t usb_get_serial(uint16_t *desc_str, size_t max_chars) {
    ora_get_metadata_str_fn_t get_metadata_str =
        context.ora_lookup_fn(ORA_ID_GET_METADATA_STR);
    if (get_metadata_str == NULL) {
        return 0;
    }

    const char *serial = NULL;
    if (get_metadata_str(ORA_METADATA_KEY_SERIAL_OVERRIDE, &serial) != ORA_RESULT_OK
        || serial == NULL) {
        return 0;
    }

    size_t len = 0;
    while (serial[len] != '\0' && len < max_chars) {
        desc_str[len] = (uint16_t)(uint8_t)serial[len];
        len++;
    }
    return len;
}

void usb_init(ora_lookup_fn_t ora_lookup_fn) {
    // Look up the required functions from the API.
    context.ora_lookup_fn = ora_lookup_fn;
    context.log = ora_lookup_fn(ORA_ID_LOG);
    context.debug = ora_lookup_fn(ORA_ID_DEBUG_LOG);
    context.err_log = ora_lookup_fn(ORA_ID_ERR_LOG);
    ora_register_irq_fn_t register_irq = ora_lookup_fn(ORA_ID_REGISTER_IRQ);
    ora_setup_usb_fn_t setup_usb = ora_lookup_fn(ORA_ID_SETUP_USB);
    ora_enable_irq_fn_t enable_irq = ora_lookup_fn(ORA_ID_ENABLE_IRQ);
    ora_get_clkref_mhz_fn_t get_clkref_mhz = ora_lookup_fn(ORA_ID_GET_CLKREF_MHZ);
    context.set_status_led = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    context.get_active_ram_slot = ora_lookup_fn(ORA_ID_GET_ACTIVE_RAM_SLOT);
    context.get_ram_slot_info = ora_lookup_fn(ORA_ID_GET_RAM_SLOT_INFO);
    context.read_ram_rom_slot = ora_lookup_fn(ORA_ID_READ_RAM_ROM_SLOT);
    context.reprogram_ram_rom_slot = ora_lookup_fn(ORA_ID_REPROGRAM_RAM_ROM_SLOT);
    // Can't log until we have the log functions
    DEBUG("USB plugin started");

    // Resolve the GPIO API and decide what the GPIO commands can offer.  Done
    // once, here, because none of it can change while the plugin runs - and
    // because probing it per request would put ORA lookups on the command path.
    gpio_init_caps();

    // Set up USB.  tinyusb will register its own IRQ handler, using the API
    // functions we provide.
    setup_usb();

    // Set up timer0
    register_irq(ORA_IRQ_TIMER0_IRQ_0, timer0_irq_0_handler);
    uint32_t clkref_mhz = get_clkref_mhz();
    setup_timer0(clkref_mhz);
    enable_irq(ORA_IRQ_TIMER0_IRQ_0, 1);

    usb_picoboot_init(EPNUM_VENDOR_OUT, EPNUM_VENDOR_IN);

    tusb_rhport_init_t dev_init = {
        .role = TUSB_ROLE_DEVICE,
        .speed = TUSB_SPEED_AUTO
    };
    tusb_init(BOARD_TUD_RHPORT, &dev_init);

    DEBUG("USB plugin setup complete");
}

// Main plugin entry point
void usb_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Unused variables
    (void)plugin_type;
    (void)entry_args;

    // Initialize .ram_func, .data and .bss.  Do up-front to avoid
    // accidentally using it first
    init_data_bss();

    // Initialize USB and related functionality
    usb_init(ora_lookup_fn);
    ora_yield_fn_t yield = ora_lookup_fn(ORA_ID_YIELD);

    while (1) {
        tud_task();
        usb_picoboot_task();
        usb_plugin_task();
        yield(NULL);
    }

    ERR("USB plugin exiting");
    return;
}

// Invoked when device is mounted
void tud_mount_cb(void) {
    LOG("USB mounted");
}

// Invoked when device is unmounted
void tud_umount_cb(void) {
    LOG("USB unmounted");
}

void tud_suspend_cb(bool remote_wakeup_en) {
    LOG("USB bus suspended, remote wakeup %s", remote_wakeup_en ? "enabled" : "disabled");
}

void tud_resume_cb(void) {
    LOG("USB bus resumed");
}

// Invoked when CDC data is received
void tud_cdc_rx_cb(uint8_t itf) {
    uint8_t buf[64];
    uint32_t count = tud_cdc_n_read(itf, buf, sizeof(buf));
    LOG("CDC received %u bytes on interface %u", count, itf);
}

// Invoked when a control transfer is received on vendor interface
// Used to respond to MS OS 2.0 descriptor request from Windows
bool tud_vendor_control_xfer_cb(
    uint8_t rhport,
    uint8_t stage,
    tusb_control_request_t const *request
) {
    // Try PICOBOOT first
    if (app_picoboot_control_xfer_cb(rhport, stage, request)) {
        return true;
    }

    // Handle MS OS 2.0 descriptor request, for WCID on Windoows 8.1+.  Avoids
    // the need for Zadig to setup WinUSB on Windows.
    if ((request->bRequest == VENDOR_REQUEST_MICROSOFT) &&
        (request->bmRequestType_bit.type == TUSB_REQ_TYPE_VENDOR)) {
        if (stage == CONTROL_STAGE_SETUP) {
            if (request->wIndex == 7) {
                // Return MS OS 2.0 descriptor
                return tud_control_xfer(rhport, request, (void *)desc_ms_os_20, MS_OS_20_DESC_LEN);
            }

            // Unsupported wIndex
            return false;
        } else {
            // Return true for ACK and DATA stages.
            return true;
        }
    }

    return false;
}

#include <sys/stat.h>

void _exit(int status) { (void)status; while(1); }
int _close(int fd) { (void)fd; return -1; }
int _fstat(int fd, struct stat *st) { (void)fd; (void)st; return -1; }
int _isatty(int fd) { (void)fd; return 1; }
int _lseek(int fd, int offset, int whence) { (void)fd; (void)offset; (void)whence; return -1; }
int _read(int fd, char *buf, int len) { (void)fd; (void)buf; (void)len; return -1; }
int _write(int fd, char *buf, int len) { (void)fd; (void)buf; (void)len; return -1; }
int _sbrk(int incr) { (void)incr; return -1; }
int _kill(int pid, int sig) { (void)pid; (void)sig; return -1; }
int _getpid(void) { return 1; }

#include <stdint.h>
#include <stdarg.h>

// panic - called by dcd_rp2040 on unrecoverable error
void panic(const char *fmt, ...) {
    (void)fmt;
    while (1);
}

// IRQ functions - dcd_rp2040 calls these but the plugin framework
// owns IRQ registration. The USB IRQ is already registered before
// tusb_init is called, so these can be no-ops.
void irq_add_shared_handler(uint32_t num, ora_irq_handler_t handler, uint8_t order) {
    (void)order;
    ora_register_irq_fn_t register_irq = context.ora_lookup_fn(ORA_ID_REGISTER_IRQ);

    // Pico SDK declares handlers as void* but we store them as function
    // pointers.  This cast is safe because tinyusb always passes genuine
    // function pointers here.
    register_irq(num, handler);
}

void irq_remove_handler(uint32_t num, ora_irq_handler_t handler) {
    (void)handler;
    ora_register_irq_fn_t register_irq = context.ora_lookup_fn(ORA_ID_REGISTER_IRQ);
    register_irq(num, NULL);
}

void irq_set_enabled(uint32_t num, bool enabled) {
    ora_enable_irq_fn_t enable_irq = context.ora_lookup_fn(ORA_ID_ENABLE_IRQ);
    enable_irq(num, enabled ? 1 : 0);
}

void __assert_func(const char *file, int line, const char *func, const char *expr) {
    ERR("Assertion failed: %s, at %s:%d in function %s", expr, file, line, func);
    while (1);
}
