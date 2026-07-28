// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(USB_CUSTOM_PBX_H)
#define USB_CUSTOM_PBX_H

#include <stdint.h>

// ---------------------------------------------------------------------------
// One ROM picobootx protocol extensions
// ---------------------------------------------------------------------------

#define ONEROM_PICOBOOTX_MAGIC  ('O' | ('N' << 8) | ('E' << 16) | ('R' << 24))

// Version of the One ROM picobootx extension - the wire surface described by
// this header - reported by ONEROM_CMD_GET_CAPS.  It is deliberately separate
// from the USB plugin's own version: the plugin version tracks the plugin, this
// tracks the protocol, and a host cares about the latter.
//
// 1.0 is the first version that can report itself at all.  A device that
// predates ONEROM_CMD_GET_CAPS has no extension version, which is exactly what
// its refusal of the capability probe tells the host.  Bump the minor for an
// additive change (a new command, or a new field within a struct's reserved
// space gated by a ONEROM_FEAT_* bit) and the major only for one that breaks an
// existing host - which should not happen.
#define ONEROM_PBX_EXT_MAJOR  1u
#define ONEROM_PBX_EXT_MINOR  0u

// All One ROM custom commands carry all 16 argument bytes.
#define ONEROM_CMD_ARGS_LEN  16u

// Largest data-IN response this extension will produce, in bytes.
//
// picoboot's own transfer-length convention - the one PB_CMD_GET_INFO enforces
// directly - is at most 256 bytes, and both responses defined here fit inside
// it with room to spare (32 for the capabilities, 192 for a whole RP2350B's
// worth of GPIO entries).  Bounding it here means a host asking for an absurd
// transfer_len is rejected up front rather than being streamed to.
#define ONEROM_MAX_TRANSFER_LEN  256u

// Command IDs.
//
// ONEROM_CMD_GET_CAPS and ONEROM_CMD_GPIO_QUERY return data to the host, so
// they are sent with picobootx's PICOBOOT_DIR_IN (0x80) bit set in cmd_id -
// 0x82 and 0x84 as they appear on the wire.  ONEROM_CMD_SET_LED and
// ONEROM_CMD_GPIO_SET have no data phase and travel with the bit clear.
typedef enum {
    ONEROM_CMD_SET_LED    = 0x01,
    ONEROM_CMD_GET_CAPS   = 0x02,
    ONEROM_CMD_GPIO_SET   = 0x03,
    ONEROM_CMD_GPIO_QUERY = 0x04,
} onerom_cmd_id_t;

// Every reserved field below is zero on send and ignored on receive.  That is
// deliberate, and it has a consequence worth stating: because a reserved field
// is ignored rather than rejected, a stale host's garbage is indistinguishable
// from a new host's deliberate value, so neither side can ever infer intent
// from one.  Any future use of reserved space must therefore be gated by a
// ONEROM_FEAT_* capability bit, never by sniffing the field itself.

// ---------------------------------------------------------------------------
// ONEROM_CMD_SET_LED
// ---------------------------------------------------------------------------

typedef enum {
    ONEROM_LED_OFF    = 0x00,
    ONEROM_LED_ON     = 0x01,
    ONEROM_LED_BEACON = 0x02,
    ONEROM_LED_FLAME  = 0x03,
} onerom_led_subcmd_t;

typedef struct __attribute__((packed)) {
    uint8_t  led_id;
    uint8_t  sub_cmd;    // onerom_led_subcmd_t
    uint8_t  reserved[2];
    uint32_t p0;
    uint32_t p1;
    uint32_t p2;
} onerom_set_led_args_t;
_Static_assert(sizeof(onerom_set_led_args_t) == 16, "onerom_set_led_args_t size mismatch");

// ---------------------------------------------------------------------------
// ONEROM_CMD_GET_CAPS - what this device's picobootx extension supports
//
// IN, transfer_len ONEROM_CAPS_LEN.  The host always asks for exactly
// ONEROM_CAPS_LEN bytes and the device zero-fills all of them.  struct_len says
// how many of those bytes are meaningful, which is what lets this structure
// grow without a protocol change: a host must accept any struct_len and ignore
// everything beyond it, and must never require it to equal ONEROM_CAPS_LEN.
// ---------------------------------------------------------------------------

#define ONEROM_CAPS_LEN  32u

typedef struct __attribute__((packed)) {
    // Bytes of this response that are meaningful.  Never assume this equals
    // ONEROM_CAPS_LEN.
    uint16_t struct_len;

    // One ROM picobootx extension version, independent of the plugin's own.
    uint8_t  ext_major;
    uint8_t  ext_minor;

    // ONEROM_FEAT_* bitmap.  Computed at plugin init from which ORA lookups
    // succeeded, so a new plugin on older firmware reports the GPIO bits clear
    // and the host never sends the commands.
    uint32_t features;

    // GPIOs this device has: the running variant's MAX_GPIOS, 30 on an RP2350A
    // and 48 on an RP2350B.  It is not a constant and neither side may assume
    // one; the host sizes its ONEROM_CMD_GPIO_QUERY request from it.
    uint8_t  num_gpios;

    uint8_t  reserved0[3];

    // Longest bounded hold ONEROM_CMD_GPIO_SET will accept, in milliseconds.
    uint32_t max_hold_ms;

    uint8_t  reserved1[16];
} onerom_caps_t;
_Static_assert(sizeof(onerom_caps_t) == ONEROM_CAPS_LEN, "onerom_caps_t size mismatch");

// Feature bits in onerom_caps_t.features.
#define ONEROM_FEAT_GPIO_SET    (1u << 0)   // ONEROM_CMD_GPIO_SET is available
#define ONEROM_FEAT_GPIO_QUERY  (1u << 1)   // ONEROM_CMD_GPIO_QUERY is available
#define ONEROM_FEAT_GPIO_HOLD   (1u << 2)   // duration_ms/after_state are honoured
// Bits 3 upwards are reserved for later: pulls, drive strength, slew, pulse
// trains, named pins.

// ---------------------------------------------------------------------------
// ONEROM_CMD_GPIO_SET - drive a GPIO, optionally for a bounded period
//
// No data phase; everything travels in the 16 argument bytes.
// ---------------------------------------------------------------------------

// State to place a GPIO in.  Mirrors ora_gpio_state_t value for value.
typedef enum {
    ONEROM_GPIO_STATE_LOW   = 0,   // Drive low
    ONEROM_GPIO_STATE_HIGH  = 1,   // Drive high
    ONEROM_GPIO_STATE_INPUT = 2,   // Release - output driver off, high impedance
} onerom_gpio_state_t;

// Drive the GPIO even though One ROM is using it.  Mirrors ORA_GPIO_FLAG_FORCE.
#define ONEROM_GPIO_FLAG_FORCE  (1u << 0)

typedef struct __attribute__((packed)) {
    uint8_t  gpio;
    uint8_t  state;        // onerom_gpio_state_t
    uint8_t  after_state;  // onerom_gpio_state_t to revert to when duration_ms expires
    uint8_t  flags;        // ONEROM_GPIO_FLAG_*
    uint32_t duration_ms;  // 0 = latch indefinitely, after_state unused
    uint32_t reserved0;
    uint32_t reserved1;
} onerom_gpio_set_args_t;
_Static_assert(sizeof(onerom_gpio_set_args_t) == 16, "onerom_gpio_set_args_t size mismatch");

// ---------------------------------------------------------------------------
// ONEROM_CMD_GPIO_QUERY - what One ROM is using a run of GPIOs for
//
// IN, transfer_len = count * sizeof(onerom_gpio_entry_t).  The host sizes the
// run from the caps num_gpios, never from a constant.
//
// picoboot's transfer-length convention - the one PB_CMD_GET_INFO enforces
// directly - is a multiple of 4, at most 256 bytes.  An entry is 4 bytes, so
// the whole device fits in one command on either variant: 48 * 4 = 192 on an
// RP2350B, 30 * 4 = 120 on an RP2350A.  That headroom is the budget for any
// future growth of onerom_gpio_entry_t: at 5 bytes per entry the multiple-of-4
// rule breaks, and at 6 the RP2350B no longer fits.  Grow the entry and a host
// must start splitting the sweep.
// ---------------------------------------------------------------------------

typedef struct __attribute__((packed)) {
    uint8_t  first_gpio;
    uint8_t  count;        // 1..num_gpios; first_gpio + count must be <= num_gpios
    uint8_t  reserved[14];
} onerom_gpio_query_args_t;
_Static_assert(sizeof(onerom_gpio_query_args_t) == 16, "onerom_gpio_query_args_t size mismatch");

typedef struct __attribute__((packed)) {
    uint8_t use;        // ora_gpio_use_t: 0 free, 1 serving reads, 2 serving
                        // drives, 3 system pin
    uint8_t level;      // Level currently on the pad, 0 or 1
    uint8_t is_output;  // 1 if the output driver is enabled, 0 if not
    uint8_t reserved;
} onerom_gpio_entry_t;
_Static_assert(sizeof(onerom_gpio_entry_t) == 4, "onerom_gpio_entry_t size mismatch");

// ---------------------------------------------------------------------------
// Internal types to handle One ROM picoboot protocol extensions
// ---------------------------------------------------------------------------
typedef enum {
    ONEROM_PENDING_NONE = 0,
    ONEROM_PENDING_SET_LED = 1,
} onerom_pending_cmd_t;
_Static_assert(sizeof(onerom_pending_cmd_t) == 1, "onerom_pending_cmd_t size mismatch");

typedef struct {
    onerom_pending_cmd_t cmd;
    union {
        struct {
            uint8_t led_id;
            onerom_led_subcmd_t sub_cmd;
        } set_led;
    } args;
} onerom_pending_t;

// State of an in-flight device -> host data phase for a custom command.
//
// picobootx keeps no cursor on our behalf: it calls the fill callback
// repeatedly until the callback says it is done, and the callback is
// responsible for remembering where it got to.  This is that memory.  Only one
// custom transfer can be in flight at a time - picoboot is a strictly
// sequential protocol, one command in flight per connection - so a single
// instance in the plugin context suffices.  dispatch sets it up; fill consumes
// it.
typedef struct {
    // Bytes already handed to picobootx for this transfer.
    uint32_t offset;

    // Bytes this transfer will produce in total, which is the command's
    // transfer_len: the host reads exactly that many, so the fill callback must
    // produce exactly that many, padding with zeros if the response is shorter.
    uint32_t total;

    // ONEROM_CMD_GPIO_QUERY only: the first GPIO of the run being reported.
    uint8_t first_gpio;
} onerom_in_xfer_t;

#endif // USB_CUSTOM_PBX_H