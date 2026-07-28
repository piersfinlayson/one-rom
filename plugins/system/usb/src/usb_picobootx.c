// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// The USB plugin's picobootx implementation.  This
// - plumbs in picobootx to the plugin itself
// - provides USB plugin specific picoboot protocol handling.

#define USB_PICOBOOTX_IMPL

#include "usb_plugin.h"
#include "picobootx.h"
#include "picobootx_impl.h"
#include "usb_picobootx.h"
#include "usb_rom.h"
#include "usb_custom_pbx.h"

// Picboot state block.  Statically allocated here, but it is possible to
// allocate it at initialization dynamically if needed.
static uint32_t picoboot_state_buf[PICOBOOT_STATE_SIZE / 4];
#define picoboot_state ((pb_state_block_t *)picoboot_state_buf)

// Callbacks from the picoboot tinyusb vendor driver, which need to have
// picoboot_state passed in.  They just forward to the picoboot library
// callbacks.
void app_picoboot_rx_cb(uint32_t available_bytes) {
    picoboot_rx_cb(picoboot_state, available_bytes);
}
void app_picoboot_tx_cb(uint32_t sent_bytes) {
    picoboot_tx_cb(picoboot_state, sent_bytes);
}

// This callback is tud_vendor_control_xfer_cb, which is implemented by the
// application, and also needs to call into picoboot.
bool app_picoboot_control_xfer_cb(
    uint8_t rhport,
    uint8_t stage,
    tusb_control_request_t const *request
) {
    return picoboot_control_xfer_cb(picoboot_state, rhport, stage, request);
}

// Picoboot callback operations to provide implementations for the picoboot
// APIs.  We mostly use defaults.
static const picoboot_ops_t picoboot_ops = {
    .exclusive_access = picoboot_default_exclusive_access,
    .exit_xip = picoboot_default_exit_xip,
    .enter_xip = picoboot_default_enter_xip,
    .reboot2_prepare = picoboot_default_reboot2_prepare,
    .reboot2_execute = picoboot_default_reboot2_execute,
    .read_prepare = app_picoboot_read_prepare, 
    .read = app_picoboot_read,
    .write_prepare = app_picoboot_write_prepare,
    .flash_page_write = picoboot_default_flash_page_write,
    .flash_erase_prepare = app_picoboot_flash_erase_prepare,
    .flash_erase = picoboot_default_flash_erase,
    .write = app_picoboot_write,
    .otp_read = picoboot_default_otp_read,
    .otp_write = picoboot_default_otp_write,
    .get_info_sys = picoboot_default_get_info_sys,
};

// One ROM picoboot protocol extenson handler
static pb_status_t onerom_picobootx_dispatch(
    const picoboot_cmd_t *cmd,
    uint8_t *buf,
    uint32_t buf_len,
    uint32_t *bytes_written,
    void *ctx
);
static pb_status_t onerom_picobootx_fill(
    const picoboot_cmd_t *cmd,
    uint8_t *buf,
    uint32_t max_len,
    uint32_t *bytes_written,
    bool *done,
    void *ctx
);
static const picoboot_custom_ops_t onerom_picobootx_ops = {
    .magic    = ONEROM_PICOBOOTX_MAGIC,
    .dispatch = onerom_picobootx_dispatch,
    .fill     = onerom_picobootx_fill,
};

// A flash write buffer required by picoboot to batch flash writes so it has
// 256 bytes to write at a time - the flash page size.  It must be 4 byte
// aligned.
static uint8_t write_buf[256] __attribute__((aligned(4)));

// Initialize picoboot
void usb_picoboot_init(uint8_t ep_out, uint8_t ep_in) {
    picoboot_init(
        picoboot_state,
        &picoboot_ops,
        &onerom_picobootx_ops,
        write_buf,
        0,
        ep_out,
        ep_in, 
        &context
    );
}

void usb_picoboot_task(void) {
    picoboot_task(picoboot_state);
}

// ---------------------------------------------------------------------------
// Custom range handlers: logical ROM
//
// Base address APP_RANGE_LOGICAL_ROM_BASE.  Size is the logical ROM size of
// the ROM currently being served, retrieved from ctx.
// ---------------------------------------------------------------------------

static pb_status_t app_range_logical_rom_read_prepare(
    uint32_t addr,
    uint32_t len,
    void *ctx
) {
    const usb_plugin_context_t *uctx = (const usb_plugin_context_t *)ctx;
    uint32_t rom_size = app_get_active_rom_size(uctx);

    if (addr < APP_RANGE_LOGICAL_ROM_BASE ||
        (addr + len) > (APP_RANGE_LOGICAL_ROM_BASE + rom_size)) {
        return PB_STATUS_NOT_FOUND;
    }

    return PB_STATUS_OK;
}


static pb_status_t app_range_logical_rom_read(
    uint32_t addr,
    uint8_t  *buf,
    uint32_t len,
    void *ctx
) {
    addr &= 0x0FFFFFFF;
 
    for (uint32_t i = 0; i < len; i++) {
        uint32_t    value;
        pb_status_t st = app_get_logical_byte_from_logical_addr(
            addr + i,
            &value,
            (const usb_plugin_context_t *)ctx
        );
        if (st != PB_STATUS_OK) {
            return st;
        }
        buf[i] = (uint8_t)value;
    }
    return PB_STATUS_OK;
}

static pb_status_t app_range_logical_rom_write_prepare(
    uint32_t addr,
    uint32_t len,
    bool *is_flash,
    void *ctx
) {
    const usb_plugin_context_t *uctx = (const usb_plugin_context_t *)ctx;
    uint32_t rom_size = app_get_active_rom_size(uctx);

    if (addr < APP_RANGE_LOGICAL_ROM_BASE ||
        (addr + len) > (APP_RANGE_LOGICAL_ROM_BASE + rom_size)) {
        return PB_STATUS_NOT_FOUND;
    }

    *is_flash = false;
    return PB_STATUS_OK;
}


static pb_status_t app_range_logical_rom_write(
    uint32_t addr,
    const uint8_t *buf,
    uint32_t len,
    void *ctx
) {
    const usb_plugin_context_t *uctx = (const usb_plugin_context_t *)ctx;
 
    addr &= 0x0FFFFFFF;
 
    for (uint32_t i = 0u; i < len; i++) {
        pb_status_t st = app_set_logical_byte_at_logical_addr(addr + i, buf[i], uctx);
        if (st != PB_STATUS_OK) {
            return st;
        }
    }
    return PB_STATUS_OK;
}

// ---------------------------------------------------------------------------
// Custom range dispatch helpers
// ---------------------------------------------------------------------------

// Walk the custom prepare handlers.  Returns PB_STATUS_OK if a handler owns
// the range, PB_STATUS_NOT_FOUND if no handler matches, or another status to
// reject immediately.
static pb_status_t app_custom_prepare(uint32_t addr, uint32_t len, void *ctx) {
    for (uint32_t i = 0u; i < APP_CUSTOM_RANGE_COUNT; i++) {
        pb_status_t st = k_custom_read_ranges[i].prepare(addr, len, ctx);
        if (st != PB_STATUS_NOT_FOUND) {
            return st;
        }
    }
    return PB_STATUS_NOT_FOUND;
}

// Walk the custom read handlers.  Calls prepare first to identify the owning
// handler, then calls its read.  Returns PB_STATUS_NOT_FOUND if no handler
// matches.
static pb_status_t app_custom_read(
    uint32_t addr,
    uint8_t *buf,
    uint32_t len,
    void *ctx
) {
    for (uint32_t i = 0u; i < APP_CUSTOM_RANGE_COUNT; i++) {
        pb_status_t st = k_custom_read_ranges[i].prepare(addr, len, ctx);
        if (st == PB_STATUS_OK) {
            return k_custom_read_ranges[i].read(addr, buf, len, ctx);
        }
        if (st != PB_STATUS_NOT_FOUND) {
            return st;
        }
    }
    return PB_STATUS_NOT_FOUND;
}

// Similar functions for write range handling. 

static pb_status_t app_custom_write_prepare(uint32_t addr, uint32_t len, bool *is_flash, void *ctx) {
    for (uint32_t i = 0u; i < APP_CUSTOM_RANGE_COUNT; i++) {
        pb_status_t st = k_custom_write_ranges[i].prepare(addr, len, is_flash, ctx);
        if (st != PB_STATUS_NOT_FOUND) {
            return st;
        }
    }
    return PB_STATUS_NOT_FOUND;
}

static pb_status_t app_custom_write(
    uint32_t addr,
    const uint8_t *buf,
    uint32_t len,
    void *ctx
) {
    // prepare is called here only for routing — is_flash was already
    // communicated to the picoboot core via app_picoboot_write_prepare.  Hence
    // it is discarded in this function.
    bool is_flash;
    for (uint32_t i = 0u; i < APP_CUSTOM_RANGE_COUNT; i++) {
        pb_status_t st = k_custom_write_ranges[i].prepare(addr, len, &is_flash, ctx);
        if (st == PB_STATUS_OK) {
            return k_custom_write_ranges[i].write(addr, buf, len, ctx);
        }
        if (st != PB_STATUS_NOT_FOUND) {
            return st;
        }
    }
    return PB_STATUS_NOT_FOUND;
}

// ---------------------------------------------------------------------------
// Public read_prepare and read callbacks (wired into picoboot_ops_t)
// ---------------------------------------------------------------------------

pb_status_t app_picoboot_read_prepare(uint32_t addr, uint32_t size, void *ctx) {
    pb_status_t st = picoboot_default_read_prepare(addr, size, ctx);
    if (st == PB_STATUS_OK) {
        return PB_STATUS_OK;
    }

    st = app_custom_prepare(addr, size, ctx);
    if (st == PB_STATUS_NOT_FOUND) {
        DEBUG("read_prepare: no handler for addr=0x%08x size=%u", addr, size);
        return PB_STATUS_INVALID_ADDRESS;
    }
    return st;
}

pb_status_t app_picoboot_read(
    uint32_t  addr,
    uint8_t  *buf,
    uint32_t  len,
    void     *ctx
) {
    // Optimise the for the custom range cases.
    pb_status_t st = app_custom_read(addr, buf, len, ctx);
    if (st != PB_STATUS_NOT_FOUND) {
        return st;
    }

    // No custom handler matched; must be a default range, already validated
    // in read_prepare.
    return picoboot_default_read(addr, buf, len, ctx);
}


// ---------------------------------------------------------------------------
// Public write_prepare and write callbacks (wired into picoboot_ops_t)
// ---------------------------------------------------------------------------

pb_status_t app_picoboot_write_prepare(
    uint32_t  addr,
    uint32_t  size,
    bool     *is_flash,
    void     *ctx
) {
    pb_status_t st = app_custom_write_prepare(addr, size, is_flash, ctx);
    if (st == PB_STATUS_OK) {
        *is_flash = false;
        return PB_STATUS_OK;
    }
    if (st != PB_STATUS_NOT_FOUND) {
        return st;
    }

    if (addr < FLASH_PROTECTED_END &&
        (addr + size) > RP2350_FLASH_BASE) {
        LOG("write_prepare: address in protected flash region: addr=0x%08x size=%u", addr, size);
        return PB_STATUS_NOT_PERMITTED;
    }

    // No custom handler matched; check if it's a default range.
    return picoboot_default_write_prepare(addr, size, is_flash, ctx);
}

pb_status_t app_picoboot_write(
    uint32_t        addr,
    const uint8_t  *buf,
    uint32_t        len,
    void           *ctx
) {
    pb_status_t st = app_custom_write(addr, buf, len, ctx);
    if (st != PB_STATUS_NOT_FOUND) {
        return st;
    }

    // No custom handler matched; must be a default range, already validated
    // in write_prepare.
    return picoboot_default_write(addr, buf, len, ctx);
}

// ---------------------------------------------------------------------------
// Public flash_erase_prepare callback (wired into picoboot_ops_t)
// ---------------------------------------------------------------------------

pb_status_t app_picoboot_flash_erase_prepare(
    const pb_addr_size_args_t *args,
    void *ctx
) {
    pb_status_t st = picoboot_default_flash_erase_prepare(args, ctx);
    if (st != PB_STATUS_OK) {
        return st;
    }

#if 0
    if (args->addr < FLASH_PROTECTED_END &&
        (args->addr + args->size) > RP2350_FLASH_BASE) {
        ERR("flash_erase_prepare: address in protected flash region: addr=0x%08x size=%u", args->addr, args->size);
        return PB_STATUS_NOT_PERMITTED;
    }
#endif

    return PB_STATUS_OK;
}

// ---------------------------------------------------------------------------
// One ROM picoboot protocol extension handling
// ---------------------------------------------------------------------------

// Set up the device -> host data phase of a custom command.
//
// total is what the command's transfer_len asks for and what onerom_picobootx_
// fill must produce exactly: the host reads that many bytes and no more, so
// under-producing hangs its read and over-producing desynchronises the stream.
static pb_status_t onerom_in_xfer_begin(
    const picoboot_cmd_t *cmd,
    uint8_t first_gpio
) {
    if (cmd->transfer_len == 0u ||
        cmd->transfer_len > ONEROM_MAX_TRANSFER_LEN) {
        return PB_STATUS_INVALID_TRANSFER_LEN;
    }

    context.in_xfer.offset = 0u;
    context.in_xfer.total = cmd->transfer_len;
    context.in_xfer.first_gpio = first_gpio;

    return PB_STATUS_OK;
}

// Validate ONEROM_CMD_GET_CAPS and prepare its response.
//
// Any transfer_len within the bound is accepted rather than only
// ONEROM_CAPS_LEN, because onerom_caps_t is meant to grow: a newer host asking
// for more gets zero padding, and an older host asking for less gets a prefix,
// which is exactly what struct_len lets it make sense of.
static pb_status_t onerom_caps_prepare(const picoboot_cmd_t *cmd) {
    return onerom_in_xfer_begin(cmd, 0u);
}

// Validate ONEROM_CMD_GPIO_QUERY and prepare its response.
static pb_status_t onerom_gpio_query_prepare(const picoboot_cmd_t *cmd) {
    const onerom_gpio_query_args_t *args =
        (const onerom_gpio_query_args_t *)cmd->args;

    if (!(context.features & ONEROM_FEAT_GPIO_QUERY)) {
        return PB_STATUS_NOT_PERMITTED;
    }

    // The device is the authority on how many GPIOs it has; the host sizes its
    // run from the num_gpios this same plugin reported in its capabilities.
    if (args->count == 0u ||
        ((uint32_t)args->first_gpio + args->count) > context.num_gpios) {
        return PB_STATUS_INVALID_ARG;
    }

    // Unlike the capabilities, this response has no growth story of its own -
    // the entry size is fixed and the run length is the host's - so the two
    // sides must agree on the byte count exactly.
    if (cmd->transfer_len !=
        ((uint32_t)args->count * sizeof(onerom_gpio_entry_t))) {
        return PB_STATUS_INVALID_TRANSFER_LEN;
    }

    return onerom_in_xfer_begin(cmd, args->first_gpio);
}

static pb_status_t onerom_picobootx_dispatch(
    const picoboot_cmd_t *cmd,
    uint8_t *buf,
    uint32_t buf_len,
    uint32_t *bytes_written,
    void *ctx
) {
    (void)buf; (void)buf_len; (void)bytes_written;

    // Every One ROM command carries all 16 argument bytes.  The data phase is
    // per-command from here on: rejecting any command with a transfer_len, as
    // this did before ONEROM_CMD_GET_CAPS existed, would reject the two
    // commands that have one.  That older behaviour is load-bearing in the
    // other direction - it is how a host recognises a device that predates
    // these commands - so it must not be reproduced by a plugin that has them.
    if (cmd->cmd_size != ONEROM_CMD_ARGS_LEN) {
        return PB_STATUS_INVALID_CMD_LENGTH;
    }

    usb_plugin_context_t *uctx = (usb_plugin_context_t *)ctx;

    // PICOBOOT_DIR_IN is the host's statement that it will read data back, not
    // part of the command ID, so it has to come off before matching.  Without
    // this, ONEROM_CMD_GET_CAPS and ONEROM_CMD_GPIO_QUERY arrive as 0x82 and
    // 0x84 and fall through to the unknown-command arm.
    uint8_t cmd_id = (uint8_t)(cmd->cmd_id & ~PICOBOOT_DIR_IN);

    switch ((onerom_cmd_id_t)cmd_id) {
        case ONEROM_CMD_SET_LED: {
            if (cmd->transfer_len != 0u) {
                return PB_STATUS_INVALID_CMD_LENGTH;
            }
            // Deferred to the task loop: an LED mode is a state machine that
            // outlives the command, and nothing about it can be refused.
            const onerom_set_led_args_t *args = (const onerom_set_led_args_t *)cmd->args;
            uctx->pending.cmd = ONEROM_PENDING_SET_LED;
            uctx->pending.args.set_led.led_id = args->led_id;
            uctx->pending.args.set_led.sub_cmd = (onerom_led_subcmd_t)args->sub_cmd;
            return PB_STATUS_OK;
        }

        case ONEROM_CMD_GET_CAPS:
            return onerom_caps_prepare(cmd);

        case ONEROM_CMD_GPIO_SET:
            if (cmd->transfer_len != 0u) {
                return PB_STATUS_INVALID_CMD_LENGTH;
            }
            // Applied here and now, not deferred: see gpio_handle_set().
            return gpio_handle_set((const onerom_gpio_set_args_t *)cmd->args);

        case ONEROM_CMD_GPIO_QUERY:
            return onerom_gpio_query_prepare(cmd);

        default:
            return PB_STATUS_UNKNOWN_CMD;
    }
}

// Produce the bytes of an ONEROM_CMD_GET_CAPS response lying in
// [offset, offset + len), zero-padding past the end of onerom_caps_t.
static void onerom_caps_bytes(uint8_t *buf, uint32_t offset, uint32_t len) {
    onerom_caps_t caps = {0};
    caps.struct_len = (uint16_t)sizeof(caps);
    caps.ext_major = ONEROM_PBX_EXT_MAJOR;
    caps.ext_minor = ONEROM_PBX_EXT_MINOR;
    caps.features = context.features;
    caps.num_gpios = context.num_gpios;

    // A real bound, not zero: a host is entitled to treat a non-zero value as
    // authoritative and refuse a longer hold before it ever reaches the device.
    // Zero would mean "no opinion", which this plugin has never had - it always
    // enforces ONEROM_GPIO_MAX_HOLD_MS - so saying so would be a lie of
    // omission.  It is reported only alongside the feature bit that makes holds
    // meaningful.
    caps.max_hold_ms = (context.features & ONEROM_FEAT_GPIO_HOLD) ?
                       ONEROM_GPIO_MAX_HOLD_MS : 0u;

    const uint8_t *src = (const uint8_t *)&caps;
    for (uint32_t i = 0u; i < len; i++) {
        uint32_t pos = offset + i;
        buf[i] = (pos < sizeof(caps)) ? src[pos] : 0u;
    }
}

static pb_status_t onerom_picobootx_fill(
    const picoboot_cmd_t *cmd,
    uint8_t *buf,
    uint32_t max_len,
    uint32_t *bytes_written,
    bool *done,
    void *ctx
) {
    (void)ctx;

    *bytes_written = 0u;
    *done = false;

    uint32_t remaining = context.in_xfer.total - context.in_xfer.offset;
    uint32_t len = (max_len < remaining) ? max_len : remaining;

    switch ((onerom_cmd_id_t)(cmd->cmd_id & ~PICOBOOT_DIR_IN)) {
        case ONEROM_CMD_GET_CAPS:
            onerom_caps_bytes(buf, context.in_xfer.offset, len);
            break;

        case ONEROM_CMD_GPIO_QUERY: {
            // Entries are produced one at a time, straight from the firmware,
            // so nothing has to be buffered between calls; the offset is the
            // whole of the cursor.  A call with room for less than one entry
            // produces nothing and asks to be called again, which is what the
            // fill contract's zero-bytes-not-done case is for.
            len -= len % sizeof(onerom_gpio_entry_t);
            for (uint32_t i = 0u; i < len; i += sizeof(onerom_gpio_entry_t)) {
                uint32_t index =
                    (context.in_xfer.offset + i) / sizeof(onerom_gpio_entry_t);
                onerom_gpio_entry_t entry;
                pb_status_t st = gpio_fill_entry(
                    (uint8_t)(context.in_xfer.first_gpio + index),
                    &entry
                );
                if (st != PB_STATUS_OK) {
                    return st;
                }
                memcpy(&buf[i], &entry, sizeof(entry));
            }
            break;
        }

        default:
            // Unreachable: dispatch only starts a data phase for the two
            // commands above.
            ERR("Unexpected fill for cmd_id 0x%02x", cmd->cmd_id);
            return PB_STATUS_UNKNOWN_ERROR;
    }

    context.in_xfer.offset += len;
    *bytes_written = len;
    *done = (context.in_xfer.offset >= context.in_xfer.total);

    return PB_STATUS_OK;
}