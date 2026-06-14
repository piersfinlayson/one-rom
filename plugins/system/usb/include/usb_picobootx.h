// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(USB_PICOBOOTX_H)
#define USB_PICOBOOTX_H

#include "picobootx.h"
#include "picobootx_impl.h"

#if defined(USB_PICOBOOTX_IMPL)


// ---------------------------------------------------------------------------
// Flash handling
//
// Protect the first 128KB for now - firmware, metadata and system plugin
// slot.
// ---------------------------------------------------------------------------
#define FLASH_PROTECTED_START   RP2350_FLASH_BASE
#define FLASH_PROTECTED_END     (RP2350_FLASH_BASE + 128u * 1024u)

// ---------------------------------------------------------------------------
// Synthetic address range bases
//
// These are protocol-level constants and must not change without corresponding
// changes to host tooling.
// ---------------------------------------------------------------------------

// 0x90000000: logical ROM read range.  Returns the original (un-mangled)
// byte at the given logical ROM address, regardless of how the image is
// stored in RAM.  Size is dynamic: determined by the ROM type currently
// being served, retrieved via ctx at call time.
#define APP_RANGE_LOGICAL_ROM_BASE  0x90000000u

// ---------------------------------------------------------------------------
// Range handler function types
//
// prepare: range ownership check only.  Returns PB_STATUS_OK if the range
//   owns addr..addr+len, PB_STATUS_NOT_FOUND to pass to the next handler,
//   or another pb_status_t to reject immediately with that error.
//   Uses ctx for dynamic sizing.  Must not touch buf.
//
// read: performs the actual read into buf.  Only called if the corresponding
//   prepare returned PB_STATUS_OK.  Returns PB_STATUS_NOT_FOUND to pass to
//   next handler, or another pb_status_t on error.
// ---------------------------------------------------------------------------

typedef pb_status_t (*app_range_read_prepare_fn_t)(
    uint32_t addr,
    uint32_t len,
    void *ctx
);

typedef pb_status_t (*app_range_read_fn_t)(
    uint32_t addr,
    uint8_t *buf,
    uint32_t len,
    void *ctx
);

typedef pb_status_t (*app_range_write_prepare_fn_t)(
    uint32_t addr,
    uint32_t len,
    bool *is_flash,
    void *ctx
);

typedef pb_status_t (*app_range_write_fn_t)(
    uint32_t addr,
    const uint8_t *buf,
    uint32_t len,
    void *ctx
);

typedef struct {
    app_range_read_prepare_fn_t prepare;
    app_range_read_fn_t read;
} app_read_range_entry_t;

typedef struct {
    app_range_write_prepare_fn_t prepare;
    app_range_write_fn_t write;
} app_write_range_entry_t;

// ---------------------------------------------------------------------------
// Range handler table (definition gated to one translation unit)
// ---------------------------------------------------------------------------

// Forward declarations of handlers
static pb_status_t app_range_logical_rom_read_prepare(uint32_t addr, uint32_t len, void *ctx);
static pb_status_t app_range_logical_rom_read(uint32_t addr, uint8_t *buf, uint32_t len, void *ctx);
static pb_status_t app_range_logical_rom_write_prepare(uint32_t addr, uint32_t len, bool *is_flash, void *ctx);
static pb_status_t app_range_logical_rom_write(uint32_t addr, const uint8_t *buf, uint32_t len, void *ctx);

// Handlers tried in order.  Each prepare returns PB_STATUS_OK if it owns
// the range, PB_STATUS_NOT_FOUND to continue, or another error to reject.
static const app_read_range_entry_t k_custom_read_ranges[] = {
    { app_range_logical_rom_read_prepare, app_range_logical_rom_read },
};
static const app_write_range_entry_t k_custom_write_ranges[] = {
    { app_range_logical_rom_write_prepare, app_range_logical_rom_write },
};

#define APP_CUSTOM_RANGE_COUNT \
    (sizeof(k_custom_read_ranges) / sizeof(k_custom_read_ranges[0]))

_Static_assert(
    sizeof(k_custom_read_ranges) / sizeof(k_custom_read_ranges[0]) ==
    sizeof(k_custom_write_ranges) / sizeof(k_custom_write_ranges[0]),
    "read and write range tables must be the same size"
);

#endif // USB_PICOBOOTX_IMPL

// ---------------------------------------------------------------------------
// Public function prototypes
// ---------------------------------------------------------------------------

bool app_picoboot_control_xfer_cb(
    uint8_t rhport,
    uint8_t stage,
    tusb_control_request_t const *request
);

pb_status_t app_picoboot_read_prepare(uint32_t addr, uint32_t size, void *ctx);
pb_status_t app_picoboot_read(uint32_t addr, uint8_t *buf, uint32_t len, void *ctx);
pb_status_t app_picoboot_write_prepare(uint32_t addr, uint32_t size, bool *is_flash, void *ctx);
pb_status_t app_picoboot_write(uint32_t addr, const uint8_t *buf, uint32_t len, void *ctx);
pb_status_t app_picoboot_flash_erase_prepare(const pb_addr_size_args_t *args, void *ctx);
const onerom_rom_slot_t *app_get_active_rom_set(const usb_plugin_context_t *ctx);

#endif // USB_PICOBOOTX_H