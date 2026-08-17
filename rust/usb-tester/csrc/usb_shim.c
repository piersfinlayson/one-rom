// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Host-side shim standing in for the parts of the USB plugin's device
// environment that do not exist when the plugin is compiled natively and run
// against the firmware emulator.
//
// Three kinds of thing live here:
//
// 1. tinyusb.  The plugin calls ten of its functions and tinyusb calls back
//    into the plugin; nothing below that boundary — the device controller, the
//    USB bus itself — is modelled, because none of it is One ROM's code.  What
//    is modelled is the CDC IN endpoint, in enough detail for the log drain's
//    behaviour to be a response to something.
//
// 2. picoboot.  picobootx has its own conformance suite in its own repository,
//    so it is not compiled here.  The shim keeps what the plugin registered and
//    stands in for the default command implementations, which the plugin's ops
//    table names and so must exist at link time.
//
// 3. The entry point the harness calls to start the plugin.
//
// This file is specific to one plugin: it names usb_main, and two plugins
// cannot share a binary in any case — each defines its own ora_plugin_header
// and its own file-scope state.

#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <plugin.h>

#include "tusb.h"
#include "picobootx.h"
#include "usb_custom_pbx.h"
#include "usb_plugin.h"
#include "usb_shim.h"

// ---------------------------------------------------------------------------
// Firmware entry points we link against (from libonerom-test.a)
// ---------------------------------------------------------------------------

// firmware/src/plugin.c — the plugin API lookup the firmware hands a plugin.
void *ora_fn_lookup(api_id_t id);

// firmware/test — gives every log channel back.  Declared here rather than
// included from test/stub.h, which needs firmware internals this file does not
// have.  See the comment where it is called.
void ora_log_reset_claims(void);

// ---------------------------------------------------------------------------
// The plugin under test
// ---------------------------------------------------------------------------

void usb_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
);

// The plugin's own header, as the firmware would read it before launching the
// plugin.  Useful to the harness for asserting the plugin's declared version
// and min_fw_version.
extern const ora_plugin_header_t ora_plugin_header;

// ---------------------------------------------------------------------------
// ORA host-test seams
// ---------------------------------------------------------------------------

// ORA_TEST_YIELD.  Installed by the harness thread that runs the plugin; the
// plugin calls this at the end of every pass of its main loop, and the hook
// hands control back so the emulation can be advanced.  A NULL hook means
// nothing is driving the emulation, so the wait can never end: say so rather
// than spinning forever.
static void (*s_yield_hook)(void);

void ora_host_test_set_yield_hook(void (*hook)(void)) {
    s_yield_hook = hook;
}

void ora_host_test_yield(void) {
    if (s_yield_hook == NULL) {
        fprintf(stderr,
                "onerom usb harness: plugin yielded with no hook installed — "
                "nothing can advance the emulation, so this wait would never "
                "end.  Install the hook on the thread that runs the plugin.\n");
        abort();
    }
    s_yield_hook();
}

// ---------------------------------------------------------------------------
// The CDC endpoint
// ---------------------------------------------------------------------------

// The endpoint holds what has been written to it until the host reads it, which
// is what makes it fill.  A flush hands the FIFO to the endpoint; the room it
// occupies comes back only when the harness takes the bytes, standing in for a
// host that has read them.  A model whose flush freed the room immediately
// would be a host that reads infinitely fast, and nothing the plugin does when
// the endpoint is full would ever be reached.
static uint32_t s_tx_capacity = USB_SHIM_TX_CAPACITY;
static uint32_t s_tx_inflight;
static uint8_t  s_tx_fifo[USB_SHIM_TX_CAPACITY];
static uint32_t s_tx_fifo_len;
static uint8_t  s_connected = 1;

// Flushed bytes accumulate here.  Sized for a whole banner and several log
// lines, so a scenario can run many passes before reading.
#define USB_SHIM_TX_LOG_SIZE 4096u
static uint8_t  s_tx_log[USB_SHIM_TX_LOG_SIZE];
static uint32_t s_tx_log_len;

void usb_host_test_set_tx_capacity(uint32_t capacity) {
    s_tx_capacity = (capacity > USB_SHIM_TX_CAPACITY) ? USB_SHIM_TX_CAPACITY : capacity;
}

void usb_host_test_set_connected(uint8_t connected) {
    s_connected = connected ? 1 : 0;
}

uint32_t usb_host_test_tx_pending(void) {
    return s_tx_log_len;
}

uint32_t usb_host_test_take_tx(uint8_t *buf, uint32_t max_len) {
    uint32_t n = (s_tx_log_len < max_len) ? s_tx_log_len : max_len;
    if (buf != NULL && n > 0) {
        memcpy(buf, s_tx_log, n);
    }
    // Whatever was not taken stays, so a harness with a small buffer reads the
    // rest on its next call rather than losing it.
    memmove(s_tx_log, s_tx_log + n, s_tx_log_len - n);
    s_tx_log_len -= n;

    // The host has read them, so the endpoint has room again.
    s_tx_inflight = (s_tx_inflight > n) ? (s_tx_inflight - n) : 0u;
    return n;
}

uint32_t tud_cdc_n_write_available(uint8_t itf) {
    (void)itf;
    uint32_t used = s_tx_inflight + s_tx_fifo_len;
    return (s_tx_capacity > used) ? (s_tx_capacity - used) : 0u;
}

uint32_t tud_cdc_n_write(uint8_t itf, const void *buffer, uint32_t bufsize) {
    (void)itf;
    uint32_t room = tud_cdc_n_write_available(itf);
    uint32_t n = (bufsize < room) ? bufsize : room;
    memcpy(&s_tx_fifo[s_tx_fifo_len], buffer, n);
    s_tx_fifo_len += n;
    return n;
}

uint32_t tud_cdc_n_write_flush(uint8_t itf) {
    (void)itf;
    uint32_t n = s_tx_fifo_len;
    if (s_tx_log_len + n > USB_SHIM_TX_LOG_SIZE) {
        // Dropping silently would make a scenario that produced more than the
        // harness read look like one that produced less.
        fprintf(stderr,
                "onerom usb harness: the flushed-byte log is full.  Read it "
                "with usb_host_test_take_tx before running further passes.\n");
        abort();
    }
    memcpy(&s_tx_log[s_tx_log_len], s_tx_fifo, n);
    s_tx_log_len += n;
    s_tx_inflight += n;
    s_tx_fifo_len = 0;
    return n;
}

bool tud_cdc_n_write_clear(uint8_t itf) {
    (void)itf;
    s_tx_fifo_len = 0;
    return true;
}

bool tud_cdc_n_connected(uint8_t itf) {
    (void)itf;
    return s_connected != 0;
}

uint32_t tud_cdc_n_read(uint8_t itf, void *buffer, uint32_t bufsize) {
    (void)itf; (void)buffer; (void)bufsize;
    // Nothing is sent to the device over CDC.  The plugin logs and drops what
    // arrives, so there is nothing here for a scenario to assert.
    return 0;
}

void usb_host_test_set_dtr(uint8_t dtr) {
    // Through the plugin's own callback, so the transition is delivered exactly
    // as tinyusb delivers it — which is what the plugin keys a new session off.
    tud_cdc_line_state_cb(0, dtr != 0, false);
}

// ---------------------------------------------------------------------------
// The rest of tinyusb
// ---------------------------------------------------------------------------

bool tusb_rhport_init(uint8_t rhport, const tusb_rhport_init_t *rh_init) {
    (void)rhport; (void)rh_init;
    return true;
}

void tud_task_ext(uint32_t timeout_ms, bool in_isr) {
    (void)timeout_ms; (void)in_isr;
    // Without a device controller there is no bus traffic to service.  What
    // tud_task would deliver — the line state callback, a control transfer — a
    // scenario raises directly.
}

// What the plugin last offered to send, and how many times it has offered
// anything.  On a device tinyusb would put these bytes on the wire, which is
// the only place the answer to a control request is visible - so the shim keeps
// them instead, and a scenario reads them as the host would have.
//
// The buffer is copied rather than kept by pointer: the plugin hands over a
// pointer to its own const data, and a test that dereferenced it later would be
// asserting on the descriptor rather than on what was sent.
static uint8_t  s_control_xfer[512];
static uint32_t s_control_xfer_len;
static uint32_t s_control_xfer_count;

bool tud_control_xfer(uint8_t rhport, const tusb_control_request_t *request,
                      void *buffer, uint16_t len) {
    (void)rhport; (void)request;

    uint32_t copy = len;
    if (copy > sizeof(s_control_xfer)) {
        copy = sizeof(s_control_xfer);
    }
    if (buffer != NULL && copy > 0u) {
        memcpy(s_control_xfer, buffer, copy);
    }
    s_control_xfer_len = len;
    s_control_xfer_count++;

    return true;
}

uint32_t usb_host_test_control_xfer_count(void) { return s_control_xfer_count; }

uint32_t usb_host_test_take_control_xfer(uint8_t *buf, uint32_t max_len) {
    uint32_t copy = s_control_xfer_len;
    if (copy > max_len) {
        copy = max_len;
    }
    if (copy > sizeof(s_control_xfer)) {
        copy = sizeof(s_control_xfer);
    }
    if (buf != NULL && copy > 0u) {
        memcpy(buf, s_control_xfer, copy);
    }
    return s_control_xfer_len;
}

bool usb_host_test_vendor_control(uint8_t stage, uint8_t bm_request_type,
                                  uint8_t b_request, uint16_t w_index) {
    tusb_control_request_t request = {0};
    request.bmRequestType = bm_request_type;
    request.bRequest      = b_request;
    request.wValue        = 0;
    request.wIndex        = w_index;
    request.wLength       = 0;

    return tud_vendor_control_xfer_cb(0, stage, &request);
}

// ---------------------------------------------------------------------------
// picoboot
// ---------------------------------------------------------------------------

static const picoboot_ops_t        *s_picoboot_ops;
static const picoboot_custom_ops_t *s_custom_ops;
static void                        *s_picoboot_ctx;

const void *usb_host_test_picoboot_ops(void) { return s_picoboot_ops; }
const void *usb_host_test_custom_ops(void)   { return s_custom_ops; }
void       *usb_host_test_picoboot_ctx(void) { return s_picoboot_ctx; }

void picoboot_init(pb_state_block_t *state, const picoboot_ops_t *ops,
                   const picoboot_custom_ops_t *custom, uint8_t *flash_write_buf,
                   uint8_t rhport, uint8_t ep_out, uint8_t ep_in, void *ctx) {
    (void)state; (void)flash_write_buf; (void)rhport; (void)ep_out; (void)ep_in;
    s_picoboot_ops = ops;
    s_custom_ops = custom;
    s_picoboot_ctx = ctx;
}

void picoboot_task(pb_state_block_t *state) { (void)state; }
void picoboot_rx_cb(pb_state_block_t *state, uint32_t count) { (void)state; (void)count; }
void picoboot_tx_cb(pb_state_block_t *state, uint32_t sent_bytes) { (void)state; (void)sent_bytes; }

static uint8_t s_picoboot_claims_control;

void usb_host_test_set_picoboot_claims_control(uint8_t claims) {
    s_picoboot_claims_control = claims;
}

bool picoboot_control_xfer_cb(pb_state_block_t *state, uint8_t rhport,
                              uint8_t stage, const tusb_control_request_t *req) {
    (void)state; (void)rhport; (void)stage; (void)req;
    // Not picoboot's by default: the plugin's own handler runs next, which is
    // the one under test.  A scenario turns this on to check the plugin stops
    // there rather than answering a request picoboot has already taken.
    return s_picoboot_claims_control != 0u;
}

// The chip ID a device reads out of OTP, as the UTF-16 hex string
// picoboot_get_serial produces.  Fixed, so a scenario can assert the serial the
// plugin derives from it.
size_t picoboot_get_serial(uint16_t *buffer, size_t buf_size) {
    static const char hex[] = "0123456789ABCDEF";
    if (buf_size < 17) {
        return 0;
    }
    uint64_t id = USB_SHIM_CHIP_ID;
    for (int i = 0; i < 16; i++) {
        buffer[i] = (uint16_t)hex[(id >> ((15 - i) * 4)) & 0xf];
    }
    buffer[16] = 0;
    return 16;
}

// The default command implementations.  The plugin's ops table names them, so
// they must exist, and app_picoboot_read and app_picoboot_write fall through to
// them for any address no custom range claims.  Reaching one means the plugin
// routed past its own ranges, which is what a scenario asserts — so they record
// nothing and refuse, rather than modelling a device this suite does not test.
#define PB_DEFAULT_REFUSED PB_STATUS_NOT_PERMITTED

pb_status_t picoboot_default_exclusive_access(uint8_t excl, void *ctx) {
    (void)excl; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_exit_xip(void *ctx) { (void)ctx; return PB_DEFAULT_REFUSED; }
pb_status_t picoboot_default_enter_xip(void *ctx) { (void)ctx; return PB_DEFAULT_REFUSED; }
pb_status_t picoboot_default_reboot2_prepare(const pb_reboot2_args_t *args, void *ctx) {
    (void)args; (void)ctx; return PB_DEFAULT_REFUSED;
}
void picoboot_default_reboot2_execute(void *ctx) { (void)ctx; }
pb_status_t picoboot_default_read_prepare(uint32_t addr, uint32_t size, void *ctx) {
    (void)addr; (void)size; (void)ctx; return PB_STATUS_NOT_FOUND;
}
pb_status_t picoboot_default_read(uint32_t addr, uint8_t *buf, uint32_t len, void *ctx) {
    (void)addr; (void)buf; (void)len; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_write_prepare(uint32_t addr, uint32_t size, bool *is_flash,
                                           void *ctx) {
    (void)addr; (void)size; (void)is_flash; (void)ctx; return PB_STATUS_NOT_FOUND;
}
pb_status_t picoboot_default_write(uint32_t addr, const uint8_t *buf, uint32_t len,
                                   void *ctx) {
    (void)addr; (void)buf; (void)len; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_flash_page_write(uint32_t addr, const uint8_t *buf, void *ctx) {
    (void)addr; (void)buf; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_flash_erase_prepare(const pb_addr_size_args_t *args, void *ctx) {
    (void)args; (void)ctx; return PB_STATUS_OK;
}
pb_status_t picoboot_default_flash_erase(const pb_addr_size_args_t *args, void *ctx) {
    (void)args; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_otp_read(uint16_t row, uint8_t ecc, uint8_t *buf, uint32_t len,
                                      void *ctx) {
    (void)row; (void)ecc; (void)buf; (void)len; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_otp_write(uint16_t row, uint8_t ecc, const uint8_t *buf,
                                       uint32_t len, void *ctx) {
    (void)row; (void)ecc; (void)buf; (void)len; (void)ctx; return PB_DEFAULT_REFUSED;
}
pb_status_t picoboot_default_get_info_sys(const pb_get_info_args_t *args, uint8_t *buf,
                                          uint32_t max_len, uint32_t *written, void *ctx) {
    (void)args; (void)buf; (void)max_len; (void)ctx;
    if (written != NULL) {
        *written = 0;
    }
    return PB_DEFAULT_REFUSED;
}

// ---------------------------------------------------------------------------
// Driving the One ROM commands
//
// A scenario stands where picoboot's core stands: it builds the command packet
// and calls the handlers through the table the plugin registered.  Building the
// packet here rather than in the harness keeps the wire layout in the one place
// that has the headers for it.
// ---------------------------------------------------------------------------

// Fill a command packet as it would arrive on the wire.
static void shim_make_cmd(picoboot_cmd_t *cmd, uint8_t cmd_id, uint8_t cmd_size,
                          uint32_t transfer_len, const uint8_t *args) {
    memset(cmd, 0, sizeof(*cmd));
    cmd->magic = ONEROM_PICOBOOTX_MAGIC;
    cmd->token = 1;
    cmd->cmd_id = cmd_id;
    cmd->cmd_size = cmd_size;
    cmd->transfer_len = transfer_len;
    if (args != NULL) {
        memcpy(cmd->args, args, PICOBOOT_ARGS_LEN);
    }
}

// The command the last dispatch described, kept so fill is given the same one
// the core would preserve for the duration of the transfer.
static picoboot_cmd_t s_last_cmd;

int32_t usb_host_test_dispatch(uint8_t cmd_id, uint8_t cmd_size, uint32_t transfer_len,
                               const uint8_t *args) {
    if (s_custom_ops == NULL || s_custom_ops->dispatch == NULL) {
        return -1;
    }
    shim_make_cmd(&s_last_cmd, cmd_id, cmd_size, transfer_len, args);

    // dispatch is handed the buffer picoboot would offer for a response with no
    // data phase, which these commands do not use.
    uint32_t written = 0;
    return (int32_t)s_custom_ops->dispatch(&s_last_cmd, NULL, 0, &written, s_picoboot_ctx);
}

int32_t usb_host_test_fill(uint8_t *buf, uint32_t max_len, uint32_t *written, uint8_t *done) {
    if (s_custom_ops == NULL || s_custom_ops->fill == NULL) {
        return -1;
    }
    bool is_done = false;
    uint32_t n = 0;
    pb_status_t st = s_custom_ops->fill(&s_last_cmd, buf, max_len, &n, &is_done,
                                        s_picoboot_ctx);
    if (written != NULL) {
        *written = n;
    }
    if (done != NULL) {
        *done = is_done ? 1u : 0u;
    }
    return (int32_t)st;
}

// The magic the plugin registered its commands under.
uint32_t usb_host_test_custom_magic(void) {
    return (s_custom_ops != NULL) ? s_custom_ops->magic : 0u;
}

// Whether the plugin registered both handlers.  A custom command with a data
// phase is stalled if fill is absent, so its presence is part of what the
// registration promises.
uint8_t usb_host_test_custom_handlers(void) {
    if (s_custom_ops == NULL) {
        return 0;
    }
    return (uint8_t)((s_custom_ops->dispatch != NULL ? 1u : 0u)
                   | (s_custom_ops->fill != NULL ? 2u : 0u));
}

// Route a read through the ops table the plugin registered, as picoboot would:
// prepare first, and read only if it was allowed.
int32_t usb_host_test_read(uint32_t addr, uint8_t *buf, uint32_t len) {
    if (s_picoboot_ops == NULL || s_picoboot_ops->read_prepare == NULL) {
        return -1;
    }
    pb_status_t st = s_picoboot_ops->read_prepare(addr, len, s_picoboot_ctx);
    if (st != PB_STATUS_OK) {
        return (int32_t)st;
    }
    return (int32_t)s_picoboot_ops->read(addr, buf, len, s_picoboot_ctx);
}

// The same for a write.
int32_t usb_host_test_write(uint32_t addr, const uint8_t *buf, uint32_t len) {
    if (s_picoboot_ops == NULL || s_picoboot_ops->write_prepare == NULL) {
        return -1;
    }
    bool is_flash = false;
    pb_status_t st = s_picoboot_ops->write_prepare(addr, len, &is_flash, s_picoboot_ctx);
    if (st != PB_STATUS_OK) {
        return (int32_t)st;
    }
    return (int32_t)s_picoboot_ops->write(addr, buf, len, s_picoboot_ctx);
}

// ---------------------------------------------------------------------------
// Plugin entry
// ---------------------------------------------------------------------------

// Start the plugin.  Does not return: the plugin's main loop is infinite by
// design, so the harness runs this on a thread it is willing to abandon.
//
// The entry arguments describe the plugin's static RAM and stack on a device.
// This plugin voids both that and its plugin-type argument, and on a host its
// data and stack are the host's, so a zeroed struct is passed rather than an
// invented layout that nothing would honour.
void ora_host_test_run_plugin(void) {
    static const ora_entry_args_t args = {0};
    usb_main(ora_fn_lookup, ORA_PLUGIN_TYPE_SYSTEM, &args);
}

// Put the plugin back to how a device hands it to itself.
//
// On a device the plugin's .bss is zero when it is entered, and init_data_bss()
// puts it back there on every launch.  That has nothing to act on in this
// process — the plugin's data is the host's, placed once before main — so a
// second scenario would otherwise enter the plugin with the first one's release
// slots still claimed and its LED mode still running.
//
// The firmware's record of what it granted goes with it.  A plugin records
// which log channels it holds itself, so clearing one half alone leaves the
// plugin asking for a channel and being told it is already taken, by itself.
//
// Called between scenarios, before the plugin starts and before a scenario
// arranges anything the plugin will find at startup.
void usb_host_test_reset_plugin(void) {
    memset(&context, 0, sizeof(context));
    ora_log_reset_claims();

    // The shim's own record of the last control transfer goes too, or a
    // scenario asserting that nothing was offered would see the previous
    // scenario's answer.
    s_control_xfer_len = 0;
    s_control_xfer_count = 0;
    s_picoboot_claims_control = 0;
}

// Version fields from the plugin's own header, so the harness can report which
// build of the plugin it exercised.
uint32_t ora_host_test_plugin_version(void) {
    return ((uint32_t)ora_plugin_header.major_version << 24)
         | ((uint32_t)ora_plugin_header.minor_version << 16)
         | ((uint32_t)ora_plugin_header.patch_version << 8)
         | (uint32_t)ora_plugin_header.build_version;
}
