// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// RBCP (ROM Bus Control Protocol) device-side plugin for One ROM.
//
// Implements the full RBCP device role, supporting both Command mode and
// Command-Response mode as defined by the RBCP specification.
//
// Knock sequence: "!RBCP!" (6 bytes, matched against A0-A7 only).
//
// In Command mode each knock initiates a single-command session with no
// back-channel.  In Command-Response mode a session spans from the knock
// through ENTER_CMD_RESP to an explicit exit command, with all responses
// written into the configured back-channel region of the active RAM slot.

// IMPORTANT NOTE: When running with PLUGIN_LOGGING enabled in the core
// firmware, the optimisation level of this plugin MUST be reduced.  It is
// though that -O2/-O3 causes more stack usage and hence blowing of the stack.

#include <stdint.h>
#include <stdbool.h>
#include "plugin.h"
#include "flash_erase.h"

#define RP235X
#define MCU_FLASH_SIZE_KB 2048
#define MCU_RAM_SIZE_KB 520
#define RP2350A
#include "onerom_metadata.h"
#include "reg-rp235x.h"

// ---------------------------------------------------------------------------
// Plugin header
// ---------------------------------------------------------------------------

// We do _not_ support yielding, as that would interfere with knock/byte detection.

// Define this plugin's attribues
void rbcp_main(
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
    .entry  = rbcp_main,
    .plugin_type = ORA_PLUGIN_TYPE_USER,
    .sam_usage = 255,
    .overrides1 = 0,
    .properties1 = 0,
    .min_fw_major_version = 0,
    .min_fw_minor_version = 7,
    .min_fw_patch_version = 1,
    .reserved = {0},
};

// ---------------------------------------------------------------------------
// Ring buffer
//
// To change between 8, 16 and 32 bit ring buffer entries change:
// - RING_DATA_SIZE
// - RING_BUF_TYPE
// - ORA_RING_BUF_DECLARE_*BIT to match RING_DATA_SIZE
// ---------------------------------------------------------------------------

#define RING_ENTRIES_LOG2   6u                               // 64 entries
#define RING_DATA_SIZE      32u                              // 32 bits per entry
#define RING_MASK           ((1u << RING_ENTRIES_LOG2) - 1u)
#define RING_BUF_TYPE       uint32_t                         // 32 bit entries
_Static_assert(sizeof(RING_BUF_TYPE) * 8 == RING_DATA_SIZE, "RING_BUF_TYPE must match RING_DATA_SIZE");

// Put the ring buffer in its own section, so it can be aligned at the start
// of the data region, meaning we maximise the stack space.
ORA_SECTION(".ring_buf")
ORA_RING_BUF_DECLARE_32BIT(ring_buf, RING_ENTRIES_LOG2);

#define RING_BUF_CUR_READ_INDEX()   s_read_idx
#define RING_BUF_ADV_READ_INDEX()   s_read_idx = (s_read_idx + 1u) & RING_MASK
#define RING_BUF_UPDATE_READ_INDEX(X) \
    s_read_idx = (uint32_t)((volatile RING_BUF_TYPE *)(X) - \
                            (volatile RING_BUF_TYPE *)ring_buf) & RING_MASK
#define RING_BUF_RESET_READ_INDEX() RING_BUF_UPDATE_READ_INDEX(*s_write_pos_ptr)
#define RING_BUF_CUR_WRITE_INDEX() \
    ((uint32_t)((volatile RING_BUF_TYPE *)*s_write_pos_ptr - \
                (volatile RING_BUF_TYPE *)ring_buf) & RING_MASK)
#define RING_BUF_GET_ENTRY(X) ((volatile RING_BUF_TYPE *)ring_buf)[(X)]

// Statics used to track ring buffer state.
static volatile uint32_t * volatile *s_write_pos_ptr; // pointer to DMA write pointer (volatile: DMA hardware updates the register)
static uint32_t            s_read_idx;      // our read index, kept masked

// ---------------------------------------------------------------------------
// Knock sequence: "!RBCP!"
// ---------------------------------------------------------------------------

#define KNOCK_LEN  6u
static const uint32_t s_knock_seq[KNOCK_LEN] = {
    '!', 'R', 'B', 'C', 'P', '!'
};

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

#define RBCP_PROTOCOL_VERSION_MAJOR 0u
#define RBCP_PROTOCOL_VERSION_MINOR 1u
#define RBCP_PROTOCOL_VERSION_PATCH 2u
const uint8_t protocol_version[4] = {
    RBCP_PROTOCOL_VERSION_MAJOR,
    RBCP_PROTOCOL_VERSION_MINOR,
    RBCP_PROTOCOL_VERSION_PATCH,
    0u
};

#define RBCP_DEFAULT_COMPLETE   ((uint8_t)0xBBu)
#define RBCP_DEFAULT_STATUS_OK  ((uint8_t)0xCCu)

// Response header byte offsets within the back-channel region
#define HDR_LAST_CMD_GROUP  0u
#define HDR_LAST_CMD_CMD    1u
#define HDR_TOKEN_LSB       2u
#define HDR_TOKEN_MSB       3u
#define HDR_PROGRESS        4u
#define HDR_RESPONSE        5u
#define HDR_RESERVED_0      6u
#define HDR_RESERVED_1      7u
#define HDR_SIZE            8u

// Command groups
#define GRP_CONTROL     0x00u
#define GRP_READ        0x01u
#define GRP_MODIFY      0x02u
#define GRP_NV_STORAGE  0x03u
#define GRP_PIPES       0x04u
#define GRP_AUX         0x05u
#define GRP_LED         0x06u
#define GRP_RESET       0xAAu

// Control commands
#define CMD_NOP                         0x00u
#define CMD_ENTER_CMD_RESP              0x01u
#define CMD_EXIT_CMD_RESP_ACK           0x02u
#define CMD_EXIT_CMD_RESP_SILENT        0x03u
#define CMD_SWITCH_AND_EXIT             0x04u
#define CMD_LOAD_AND_EXIT               0x05u
#define CMD_EXIT_CMD_RESP_RESTORE       0x06u

// Most bytes EXIT_CMD_RESP_RESTORE can put back, and so the largest valid
// count.  Fixed by the protocol at eight, the size of the response header,
// which is the part of the region the device always writes.
#define RESTORE_MAX_BYTES               8u

// Read commands
#define CMD_GET_FLASH_FLASH_SLOT_COUNT  0x00u
#define CMD_GET_FLASH_SLOT_INFO         0x01u
#define CMD_GET_FLASH_SLOT_INFO_ALL     0x02u
#define CMD_GET_RAM_SLOT_INFO_ALL       0x03u
#define CMD_GET_DEVICE_TYPE             0x04u
#define CMD_GET_DEVICE_VERSION          0x05u
#define CMD_GET_PROTOCOL_VERSION        0x06u
#define CMD_SLOT_PEEK                   0x07u
#define CMD_GET_BOOT_SLOT_INFO          0x08u

// Modify commands
#define CMD_SLOT_POKE                   0x00u
#define CMD_SWITCH_SLOT                 0x01u
#define CMD_LOAD_SLOT                   0x02u
#define CMD_SLOT_POKE_ALL_BYTE          0x03u

// NV Storage commands
#define CMD_GET_NV_CAPABILITY             0x00u
#define CMD_NV_PEEK                     0x01u
#define CMD_NV_POKE_BEGIN               0x02u
#define CMD_NV_POKE                     0x03u
#define CMD_NV_POKE_COMMIT              0x04u
#define CMD_NV_POKE_DISCARD             0x05u
#define CMD_NV_POKE_COMMIT_BYTE         0x06u

// Pipe commands
#define CMD_GET_PIPE_CAPABILITY         0x00u
#define CMD_GET_PIPE_INFO               0x01u
#define CMD_PIPE_WRITE                  0x02u

// Pipe type identifiers, as reported by GET_PIPE_INFO.  The type describes the
// shape of the bytes, not what they are for, and an ORA log channel imposes no
// framing of its own - which is what the protocol calls a Raw pipe.
#define PIPE_TYPE_RAW                   0x00u

// Pipe flags, as reported by GET_PIPE_INFO.  This plugin's pipes carry OUT
// only, so bit 1 stays clear.  Bits 2 and 3 report whether the far end is
// attached, and both stay clear: nothing tells this plugin whether anything is
// draining the channel, and a device that cannot tell says so rather than
// guessing.
#define PIPE_FLAG_OUT                   0x01u

// Far end identifiers, as reported by GET_PIPE_INFO.  A pipe is an ORA log
// channel and this plugin cannot see who drains it - the system USB plugin
// usually does, but a debug probe or nothing at all are equally possible - so
// it reports the far end as unspecified rather than naming one it has guessed.
#define PIPE_FAR_END_UNSPECIFIED        0x00u

// Largest payload PIPE_WRITE can carry, and so the largest valid count.  Fixed
// by the protocol at four, which is what leaves room for the pipe and count
// arguments within the nine-argument frame maximum.
#define PIPE_WRITE_MAX_BYTES            4u

// Auxiliary I/O commands
#define CMD_GET_AUX_CAPABILITY          0x00u
#define CMD_GET_AUX_GROUP_INFO          0x01u
#define CMD_GET_AUX_PIN_INFO            0x02u
#define CMD_SET_AUX                     0x03u
#define CMD_SET_AUX_AND_EXIT            0x04u
#define CMD_SET_AUX_SWITCH_EXIT         0x05u

// Auxiliary pin group types, as reported by GET_AUX_GROUP_INFO.  0x01 is the
// protocol's own GPIO type.  The other two are this plugin's, from the range
// the protocol reserves for an implementation.
#define AUX_TYPE_GPIO                   0x01u
#define AUX_TYPE_IMG_SEL                0x80u
#define AUX_TYPE_X                      0x81u

// Auxiliary pin states.  The three the protocol defines, which are the three
// ora_gpio_state_t holds and in the same order, so a state byte is passed
// through to ora_gpio_set unchanged.
#define AUX_STATE_LOW                   0x00u
#define AUX_STATE_HIGH                  0x01u
#define AUX_STATE_INPUT                 0x02u

// Auxiliary pin flags, as reported by GET_AUX_PIN_INFO.
#define AUX_PIN_FLAG_DRIVABLE           0x01u
#define AUX_PIN_FLAG_LEVEL              0x02u

// Bit 0 of the SET_AUX_SWITCH_EXIT flags argument.  Every other bit is
// reserved and rejected.
#define AUX_SWITCH_FLAG_SLOT_FIRST      0x01u

// Largest hold this device accepts, in the protocol's 10ms units.  The byte's
// whole range: RBCP is unresponsive for the duration of a hold, and how long
// the far end of the wire needs is the host's to know.
#define AUX_MAX_HOLD                    0xFFu

// LED commands
#define CMD_GET_LED_CAPABILITY          0x00u
#define CMD_GET_LED_INFO                0x01u
#define CMD_GET_LED_MODE_INFO           0x02u
#define CMD_SET_LED                     0x03u

// LED types, as reported by GET_LED_INFO.
#define LED_TYPE_MONO                   0x00u
#define LED_TYPE_RGB                    0x01u

// RBCP LED modes.  The protocol and the firmware number them differently, so
// led_ora_mode() and led_rbcp_mode() translate.  Flame is One ROM's own, from
// the range the protocol reserves for an implementation.
#define LED_MODE_OFF                    0x00u
#define LED_MODE_ON                     0x01u
#define LED_MODE_BLINK                  0x02u
#define LED_MODE_BREATHE                0x03u
#define LED_MODE_CYCLE                  0x04u
#define LED_MODE_BEACON                 0x05u
#define LED_MODE_FLAME                  0x80u
#define LED_MODE_INVALID                0xFFu

// Modes each kind of LED supports, in the form GET_LED_INFO reports: bit N for
// mode N.  Cycle and breathe are built out of a colour, so the status LED does
// not offer them.  Flame lies outside the byte: accepted, never reported.
#define LED_MODES_MONO  ((1u << LED_MODE_OFF)   | (1u << LED_MODE_ON) | \
                         (1u << LED_MODE_BLINK) | (1u << LED_MODE_BEACON))
#define LED_MODES_RGB   (LED_MODES_MONO | (1u << LED_MODE_BREATHE) | \
                         (1u << LED_MODE_CYCLE))

// GET_LED_MODE_INFO flags.
#define LED_MODE_FLAG_PERIOD            0x01u

// Largest period and hold this device accepts, in the protocol's 100ms units.
// The byte's whole range, 25.5s, which is inside the firmware's own ceiling of
// LED_MAX_HOLD_MS - so no byte a host can send exceeds either limit.
#define LED_MAX_PERIOD                  0xFFu
#define LED_MAX_HOLD                    0xFFu
_Static_assert((uint32_t)LED_MAX_HOLD * 100u <= LED_MAX_HOLD_MS,
               "LED hold range must fit the firmware's ceiling");

// The colour the status LED shows when lit, red on every One ROM board.  The
// firmware holds no record of it, and all-zero is the protocol's way of saying
// a colour is not stated - so the plugin states it here.
#define LED_STATUS_RED                  0xFFu

// Highest ORA LED channel this plugin walks.  A One ROM gaining a third LED
// numbers it from 2 up, which would need raising here.
#define LED_ORA_LAST                    ORA_LED_RGB

// Reset commands
#define CMD_RBCP_RESET                  0xAAu

// NV Storage size supported by this RBCP implementation
#define NV_STORAGE_SIZE 4096u
_Static_assert(NV_STORAGE_SIZE <= 32768u, "Max NV_STORAGE_SIZE is 32KB per the RBCP specification");

// ---------------------------------------------------------------------------
// Linker symbols required by NV storage implementation
// ---------------------------------------------------------------------------
extern const uint8_t __nv_storage_start[];
extern const uint8_t __flash_erase_fn_start[];
extern const uint8_t __flash_erase_fn_end[];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

typedef struct {
    uint16_t command_page;  // command page filter during command-response mode
    uint8_t  complete;      // protocol "complete" byte value
    uint8_t  status_ok;     // protocol "status-OK" byte value
    uint32_t region_offset; // offset of back-channel region within the RAM slot
    uint32_t region_end;    // end address (exclusive) of the back-channel region within the RAM slot;
    uint32_t data_size;     // data section size in bytes
} rbcp_cfg_t;

typedef struct {
    bool       active;       // true while in Command-Response mode
    rbcp_cfg_t cfg;
    uint8_t    token_lsb;
    uint8_t    token_msb;
    uint8_t    active_slot;  // RAM slot in which the back-channel is established;
                             // valid only while active is true.  Cached here so
                             // that back-channel writes always target the correct
                             // slot without repeated get_active_ram_slot() calls,
                             // and to ensure correctness if firmware slot-tracking
                             // state changes unexpectedly after session entry.
} rbcp_state_t;

typedef struct {
    bool     active;
    uint8_t  staging_slot;
    uint32_t staging_base;
    uint32_t staging_size;
} nv_state_t;

static rbcp_state_t s_state;
static nv_state_t s_nv_state;

// Number of low observed-address bits the device omits for the served ROM
// (host signalling stride = 1 << this).  Fixed for the served ROM type, so it
// is read once at setup rather than per command.
static uint8_t s_unobserved_addr_bits;

// What the device loaded at boot, for GET_BOOT_SLOT_INFO. The flash slot it
// came from and the RAM slot it went into.  0xFF in either means the plugin
// does not know, which is what the protocol has a device report where it has
// no answer.
//
// Two bytes, not a byte per RAM slot.  Nothing else needs recording. A host
// that loads a slot itself already knows what it put there, so the boot is the
// only load a host cannot account for.
#define BOOT_SLOT_NONE      0xFFu
static uint8_t s_boot_flash_slot;
static uint8_t s_boot_ram_slot;

// ---------------------------------------------------------------------------
// API function pointers (populated at plugin entry)
// ---------------------------------------------------------------------------

static ora_lookup_fn_t                      s_lookup;
static ora_log_fn_t                         s_log;
static ora_demangle_observed_addr_fn_t      s_demangle;  // observed (bus) address: command signalling lives here, not byte space
static ora_reprogram_ram_rom_slot_fn_t      s_reprogram;
static ora_get_ram_slot_info_fn_t           s_get_ram_slot_info;
static ora_get_ram_slot_count_fn_t          s_get_ram_slot_count;
static ora_get_active_ram_slot_fn_t         s_get_active_ram_slot;
static ora_set_active_ram_slot_fn_t         s_set_active_ram_slot;
static ora_get_flash_slot_count_fn_t        s_get_flash_slot_count;
static ora_get_flash_slot_info_fn_t         s_get_flash_slot_info;
static ora_copy_flash_slot_to_ram_slot_fn_t s_copy_flash_to_ram;
static ora_get_chip_size_from_type_fn_t     s_get_chip_size;
static ora_get_device_version_fn_t          s_get_device_version;
static ora_map_addr_to_phys_fn_t            s_map_addr_to_phys;
static ora_map_data_to_phys_fn_t            s_map_data_to_phys;
static ora_demangle_data_fn_t               s_demangle_data;

// The log channel family, added in firmware 0.7.2 and used to serve the Pipes
// group.  The plugin's min_fw_version stays at 0.7.1, so these may be NULL on
// older firmware: ora_lookup returns NULL for an identifier the running
// firmware does not implement.  A NULL here means the device exposes no pipes,
// which the protocol already provides for, rather than meaning the plugin
// cannot run.  See pipe_count().
static ora_log_open_write_fn_t              s_log_open_write;
static ora_log_write_fn_t                   s_log_write;
static ora_log_query_fn_t                   s_log_query;

// The calls the Auxiliary I/O group is built from.  The GPIO pair arrived in
// firmware 0.7.1 and the other two in 0.7.2, so any of them may be NULL here.
// Each absence takes something away from the host rather than stopping the
// plugin - see aux_available(), aux_kind_pins() and aux_max_hold().
static ora_gpio_set_fn_t                    s_gpio_set;
static ora_gpio_query_fn_t                  s_gpio_query;
static ora_get_metadata_uint_fn_t           s_get_metadata_uint;
static ora_get_metadata_uint_at_fn_t        s_get_metadata_uint_at;
static ora_get_plugin_uptime_ms_fn_t        s_uptime_ms;

// A plugin's debug output, which the firmware emits only where it was built
// with plugin debug logging.  Used for PIPE_WRITE failures, which are ordinary
// traffic rather than faults: a full channel is normal operation, and s_log
// would put the report into the very channel the host is writing.
static ora_debug_log_fn_t                   s_debug_log;

// Which pipes this plugin holds ora_log_open_write on, one bit per pipe.
// Claimed on the first PIPE_WRITE to a pipe rather than at startup, because
// claiming renames the channel for anything reading the log, and a device whose
// host never writes a pipe should not be renamed.  Held for the life of the
// plugin once taken.
//
// A bitmask rather than a flag because the claim is per channel: one bit says
// nothing about the next pipe, and a device gaining a second channel would
// otherwise have its first claim answer for both.  Eight pipes fit, which
// pipe_count() must not exceed - widen this first if it ever could.
static uint8_t s_pipes_claimed;

// ---------------------------------------------------------------------------
// Ring buffer read helpers
// ---------------------------------------------------------------------------

// Block until the next CS-active address capture is available, then return the
// low 8 bits of its observed (bus) address as the command byte.  The address is
// demangled via ORA_ID_DEMANGLE_OBSERVED_ADDR, so on a word- or LSB-omitting
// ROM the value is the observed word/bus address (not the byte address) — which
// is the space command signalling travels in.  Entries where CS is inactive are
// skipped.  In command-response mode, entries whose upper observed address bits
// do not match the configured command page are also skipped.
//
// WARNING: This function blocks indefinitely.  If the host resets or crashes
// mid-command while the plugin is waiting for an argument byte, this function
// will never return and recovery requires a device power cycle.  A future API
// extension providing a timeout or cancellation mechanism would allow a
// cleaner recovery path.
static uint8_t ring_read_byte(void) {
    for (;;) {
        if (RING_BUF_CUR_READ_INDEX() == RING_BUF_CUR_WRITE_INDEX()) {
            // No new byte to read, yet.  Only the capture DMA moves the write
            // pointer, so nothing this loop does can change the condition.
            ORA_TEST_YIELD();
            continue;
        }
        uint32_t phys = (uint32_t)RING_BUF_GET_ENTRY(s_read_idx);
        RING_BUF_ADV_READ_INDEX();

        uint32_t logical;
        if (s_demangle(phys, &logical, 1) == ORA_RESULT_OK) {
            if (s_state.active &&
                ((logical >> 8u) != (uint32_t)s_state.cfg.command_page)) {
                // Ignore accesses that do not match the command page.
                continue;
            }
            return (uint8_t)(logical & 0xFFu);
        }
        // CS inactive or demangle error: discard and try next entry
    }
}

// ---------------------------------------------------------------------------
// Back-channel write helpers
// ---------------------------------------------------------------------------

static inline uint8_t pending_val(void) {
    return (uint8_t)(~s_state.cfg.complete);
}

static inline uint8_t failed_val(void) {
    return (uint8_t)(~s_state.cfg.status_ok);
}

// Most RAM slots this plugin will admit to a host.
//
// Every RBCP command that names a slot rejects 0xAA, so that a reset started
// mid-command stays detectable.  A slot the host can never name is a slot it
// can never use, so slot 170 is where the host-visible range has to stop —
// advertising more would offer a slot that no host could switch to, poke, or
// load into.
#define MAX_HOST_SLOTS 170u

// Number of RAM slots the host is told about, and the range it may name.
//
// The firmware reports as many slots as the RAM holds, which with a small ROM
// is far more than a host can address.  Everything at or above this index is
// this plugin's own — see nv_private_staging.
static uint8_t host_slot_count(void) {
    uint8_t total = s_get_ram_slot_count();
    return total > MAX_HOST_SLOTS ? (uint8_t)MAX_HOST_SLOTS : total;
}

// Whether a slot index is one the host is allowed to name.
//
// Rejects the plugin's own slots as firmly as an out-of-range one: a host
// reaching into them would be writing over a staging buffer mid-transaction.
static bool host_slot_valid(uint8_t slot) {
    return slot < host_slot_count();
}

// Write one byte into the response header at the given header-relative offset.
// These reset_ring arg is intended to be used when update progress->complete,
// to ensure we collect as few new bytes as possible after the host potentially
// sees completion, before we can start processing them. 
static void hdr_write(uint8_t slot, uint32_t hdr_offset, uint8_t val, bool reset_ring) {
    // Do this directly, rather than s_reprogram, for speed
    uint32_t phys_addr = s_map_addr_to_phys(s_state.cfg.region_offset + hdr_offset);
    uint8_t phys_data = s_map_data_to_phys(val);
    uint32_t slot_base, slot_size;
    if (s_get_ram_slot_info(slot, &slot_base, &slot_size, NULL) != ORA_RESULT_OK) {
        s_log("RBCP: hdr_write failed: get_ram_slot_info error");
        return;
    }
    if (phys_addr >= slot_size) {
        s_log("RBCP: hdr_write failed: phys_addr out of bounds (hdr_offset=%u)", (unsigned)hdr_offset);
        return;
    }
    // Reset ring read pointer immediately before writing the byte
    if (reset_ring) {
        RING_BUF_RESET_READ_INDEX();
    }
    ((volatile uint8_t *)ORA_SRAM_PTR(slot_base))[phys_addr] = phys_data;
}

// Read one byte from the back-channel region at the given header-relative offset.
static ora_result_t hdr_read(uint8_t slot, uint32_t hdr_offset, uint8_t *val_out) {
    uint32_t slot_base, slot_size;
    if (s_get_ram_slot_info(slot, &slot_base, &slot_size, NULL) != ORA_RESULT_OK) {
        s_log("RBCP: hdr_read failed: get_ram_slot_info error");
        return ORA_RESULT_INVALID_SLOT;
    }
    uint32_t phys_offset = s_map_addr_to_phys(s_state.cfg.region_offset + hdr_offset);
    uint8_t raw = ((const uint8_t *)ORA_SRAM_PTR(slot_base))[phys_offset];
    if (s_demangle_data(raw, val_out) != ORA_RESULT_OK) {
        s_log("RBCP: hdr_read failed: demangle error at hdr_offset %u", (unsigned)hdr_offset);
        return ORA_RESULT_ERROR;
    }
    return ORA_RESULT_OK;
}

// Write bytes into the data section at the given data-section-relative offset.
// Writes are clamped to the configured data section size.
static void data_write(
    uint8_t slot,
    uint32_t data_offset,
    const uint8_t *buf,
    uint32_t len
) {
    if (data_offset >= s_state.cfg.data_size) return;
    if (data_offset + len > s_state.cfg.data_size) {
        len = s_state.cfg.data_size - data_offset;
    }
    if (s_reprogram(slot,
                    s_state.cfg.region_offset + HDR_SIZE + data_offset,
                    buf, len, 1u) != ORA_RESULT_OK) {
        s_log("RBCP: data_write failed at data offset %u", (unsigned)data_offset);
    }
}

// ---------------------------------------------------------------------------
// Command processing sequence helpers
// ---------------------------------------------------------------------------

// Steps 1-3: set progress=pending, increment token, update last_cmd.
static void cmd_begin(uint8_t slot, uint8_t group, uint8_t cmd) {
    hdr_write(slot, HDR_PROGRESS, pending_val(), false);
    s_state.token_lsb++;
    if (s_state.token_lsb == 0u) s_state.token_msb++;
    hdr_write(slot, HDR_TOKEN_LSB, s_state.token_lsb, false);
    hdr_write(slot, HDR_TOKEN_MSB, s_state.token_msb, false);
    hdr_write(slot, HDR_LAST_CMD_GROUP, group, false);
    hdr_write(slot, HDR_LAST_CMD_CMD, cmd, false);
}

// Steps 5-6: write response field then set progress=complete.
static void cmd_end(uint8_t slot, bool ok) {
    // First update the status byte.
    hdr_write(slot, HDR_RESPONSE, ok ? s_state.cfg.status_ok : failed_val(), false);

    // Now, set progress to complete, which must be the last step.
    hdr_write(slot, HDR_PROGRESS, s_state.cfg.complete, true);
}

// ---------------------------------------------------------------------------
// Back-channel region setup
// ---------------------------------------------------------------------------

// Zero-initialise the response header in the back-channel region.
static void init_back_channel(uint8_t slot) {
    static const uint8_t zeros[HDR_SIZE] = {0u};
    if (s_reprogram(slot, s_state.cfg.region_offset,
                    zeros, HDR_SIZE, 1u) != ORA_RESULT_OK) {
        s_log("RBCP: init_back_channel failed for slot %u", (unsigned)slot);
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

// Zero n bytes starting at p.  Uses a volatile pointer so the compiler does
// not recognise the loop as a memset pattern and emit a library call —
// there is no C runtime available in the plugin environment.
static void zero_bytes(uint8_t *p, uint8_t n) {
    volatile uint8_t *vp = p;
    while (n--) *vp++ = 0u;
}

// Record what the device booted, for GET_BOOT_SLOT_INFO.
//
// The firmware reports the booted slot as an index into the whole slot table,
// while every flash slot number crossing RBCP counts only the slots that are
// not plugins.  The two differ by the number of plugin slots, which the flash
// slot count gives up directly: asking with no filter counts every slot, and
// asking with the plugins excluded counts the ones RBCP names, so the
// difference is the plugins.  Taken from the same filter the numbering itself
// is built from, so the two cannot disagree about what a plugin slot is.
//
// The RAM slot is asked for rather than assumed: this runs at setup, before
// the host can have switched anything, so the slot being served now is the one
// the firmware preloaded into.  A firmware that does not report a boot slot,
// or an active slot, leaves both bytes at 0xFF, which is what the protocol has
// a device say where it does not know.
//
// Called once at setup.  Nothing updates these afterwards. They describe the
// boot, and RBCP_RESET does not change what the device booted any more than a
// host's own LOAD_SLOT does.
static void init_boot_slots(void) {
    s_boot_flash_slot = BOOT_SLOT_NONE;
    s_boot_ram_slot   = BOOT_SLOT_NONE;

    uint32_t rom_slot_index = 0u;
    if (s_get_metadata_uint == NULL) {
        s_log("RBCP: no metadata getter; boot slots unknown");
        return;
    }
    if (s_get_metadata_uint(ORA_METADATA_KEY_ROM_SLOT_INDEX,
                            &rom_slot_index) != ORA_RESULT_OK) {
        s_log("RBCP: firmware reports no boot slot");
        return;
    }
    uint8_t plugin_slots = (uint8_t)(s_get_flash_slot_count(0u) -
                                     s_get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS));
    if (rom_slot_index < plugin_slots) {
        s_log("RBCP: booted slot %u is a plugin slot", (unsigned)rom_slot_index);
        return;
    }

    uint8_t ram_slot = 0u;
    if (s_get_active_ram_slot(&ram_slot) != ORA_RESULT_OK) {
        s_log("RBCP: no active RAM slot at setup; boot slots unknown");
        return;
    }

    s_boot_flash_slot = (uint8_t)(rom_slot_index - plugin_slots);
    s_boot_ram_slot   = ram_slot;
    s_log("RBCP: booted ram_slot=%u from flash_slot=%u",
          (unsigned)s_boot_ram_slot, (unsigned)s_boot_flash_slot);
}

static void init_nv_state(void) {
    s_nv_state.active = false;
    s_nv_state.staging_slot = 0u;
    s_nv_state.staging_base = 0u;
    s_nv_state.staging_size = 0u;
}

static void init_rbcp(bool reset_slot_info) {
    // Initialise device state with protocol defaults
    s_state.active              = false;
    s_state.active_slot         = 0u;
    s_state.cfg.command_page    = 0u;
    s_state.cfg.complete        = RBCP_DEFAULT_COMPLETE;
    s_state.cfg.status_ok       = RBCP_DEFAULT_STATUS_OK;
    s_state.cfg.region_offset   = 0u;
    s_state.cfg.region_end      = 0u;
    s_state.cfg.data_size       = 0u;
    s_state.token_lsb           = 0u;
    s_state.token_msb           = 0u;
    s_read_idx                  = 0u;
    init_nv_state();
    if (reset_slot_info) {
        s_state.active_slot         = 0u;
    }
}

// ---------------------------------------------------------------------------
// Command handlers — Control group (0x00)
// ---------------------------------------------------------------------------

static bool exec_nop(void) {
    return true;
}

// Configures command-response mode parameters and enters command-response mode.
// Reads 9 argument bytes.  Validates all arguments before making any state
// changes.  Silent-discard conditions (as defined by the spec) are logged but
// produce no back-channel response.  Failure conditions are also logged; since
// ENTER_CMD_RESP is only valid in command mode, there is no back-channel for
// the device to write a failure response to — both outcomes are invisible to
// the host, which detects failure by the token not incrementing.
static bool exec_enter_cmd_resp(void) {
    uint8_t cp_lo     = ring_read_byte();  // A0: command page LSB
    uint8_t cp_hi     = ring_read_byte();  // A1: command page MSB
    uint8_t bc_a0     = ring_read_byte();  // A2: back-channel start address byte 0
    uint8_t bc_a1     = ring_read_byte();  // A3: back-channel start address byte 1
    uint8_t bc_a2     = ring_read_byte();  // A4: back-channel start address byte 2
    uint8_t bc_sz_lo  = ring_read_byte();  // A5: back-channel size LSB
    uint8_t bc_sz_hi  = ring_read_byte();  // A6: back-channel size MSB
    uint8_t complete  = ring_read_byte(); // A7
    uint8_t status_ok = ring_read_byte(); // A8

    uint16_t command_page  = (uint16_t)cp_lo | ((uint16_t)cp_hi << 8u);
    uint32_t region_offset = (uint32_t)bc_a0
                           | ((uint32_t)bc_a1 << 8u)
                           | ((uint32_t)bc_a2 << 16u);
    uint16_t region_size   = (uint16_t)bc_sz_lo | ((uint16_t)bc_sz_hi << 8u);

    if (s_state.active) {
        s_log("ENTER_CMD_RESP failed: already in command-response mode");
        return false;
    }
    if (complete == 0xAAu || status_ok == 0xAAu) {
        s_log("ENTER_CMD_RESP discarded: complete or status_ok is 0xAA");
        return false;
    }
    if (region_offset & 0x3u) {
        s_log("ENTER_CMD_RESP discarded: back-channel start address not 4-byte aligned");
        return false;
    }
    if ((uint32_t)region_size < HDR_SIZE) {
        s_log("ENTER_CMD_RESP failed: back-channel size too small to hold header");
        return false;
    }

    uint8_t active_slot;
    if (s_get_active_ram_slot(&active_slot) != ORA_RESULT_OK) {
        s_log("ENTER_CMD_RESP failed: no active slot");
        return false;
    }
    uint32_t slot_size;
    if (s_get_ram_slot_info(active_slot, NULL, &slot_size, NULL) != ORA_RESULT_OK) {
        s_log("ENTER_CMD_RESP failed: get_ram_slot_info error");
        return false;
    }
    // The command page is in observed (bus) address space, which on a word- or
    // otherwise LSB-omitting ROM is narrower than the byte-addressed slot: the
    // observed span is slot_size >> (unobserved low address bits, cached at
    // setup as it is fixed for the served ROM type).
    uint32_t observed_span = slot_size >> s_unobserved_addr_bits;
    if (((uint32_t)command_page << 8u) >= observed_span) {
        s_log("ENTER_CMD_RESP discarded: command page 0x%04X out of range for observed span %u",
              (unsigned)command_page, (unsigned)observed_span);
        return false;
    }
    uint32_t region_end = region_offset + (uint32_t)region_size;

    // Commit the fields the response header is written through, before the
    // size check rather than after it.  An oversized region is the one
    // ENTER_CMD_RESP error the specification requires the device to *report*
    // — "if the requested size exceeds the available space in the RAM slot,
    // the device returns failure" — rather than discard silently, and a
    // failure can only be reported through a header the device can locate.
    s_state.cfg.region_offset = region_offset;
    s_state.cfg.complete      = complete;
    s_state.cfg.status_ok     = status_ok;

    // The token must start from the value already in the back-channel region.
    if (hdr_read(active_slot, HDR_TOKEN_LSB, &s_state.token_lsb) != ORA_RESULT_OK ||
        hdr_read(active_slot, HDR_TOKEN_MSB, &s_state.token_msb) != ORA_RESULT_OK) {
        s_log("ENTER_CMD_RESP failed: could not read existing token");
        return false;
    }

    if (region_end > slot_size) {
        // Report the failure and stay in command mode.  The start address is
        // already known to be 4-byte aligned and, where the 8-byte header fits
        // inside the slot, there is somewhere to write it even though the
        // region as a whole does not fit.  hdr_write bounds-checks each byte,
        // so a start address too close to the end of the slot degrades to the
        // silent discard that is then the only thing available.
        s_log("ENTER_CMD_RESP failed: back-channel region exceeds slot size");
        cmd_begin(active_slot, GRP_CONTROL, CMD_ENTER_CMD_RESP);
        cmd_end(active_slot, false);
        return false;
    }
    s_log("ECR: cp=0x%04X ro=%u rsz=%u cplt=0x%02X stok=0x%02X token=0x%02X%02X",
          (unsigned)command_page, (unsigned)region_offset, (unsigned)region_size,
          complete, status_ok, s_state.token_msb, s_state.token_lsb);

    s_state.cfg.command_page  = command_page;
    s_state.cfg.region_end    = region_end;
    s_state.cfg.data_size     = (uint32_t)region_size - HDR_SIZE;
    s_state.active_slot       = active_slot;
    init_back_channel(active_slot);
    s_state.active = true;

    s_log("ENTER_CMD_RESP succeeded: as=%u, ro=%u, re=%u",
          (unsigned)active_slot, (unsigned)s_state.cfg.region_offset,
          (unsigned)s_state.cfg.region_end);
    return true;
}

// ---------------------------------------------------------------------------
// Command handlers — Read group (0x01)
// ---------------------------------------------------------------------------

static bool exec_get_flash_slot_count(void) {
    uint8_t count = s_get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);
    uint8_t resp[1] = { count };
    data_write(s_state.active_slot, 0u, resp, 1u);
    s_log("GET_FLASH_SLOT_COUNT: count=%u", (unsigned)count);
    return true;
}

static bool get_flash_slot_info(
    uint8_t flash_slot,
    uint8_t record[32]
) {
    const char *name = NULL;
    uint32_t rom_type = 0xFFu;
    if (s_get_flash_slot_info(
            flash_slot, 
            ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
            &name,
            &rom_type,
            NULL
        ) != ORA_RESULT_OK) {
        s_log("GET_FLASH_SLOT_INFO failed: invalid flash_slot %u", (unsigned)flash_slot);
        return false;
    }

    record[0] = (uint8_t)(rom_type & 0xFFu);
    zero_bytes(&record[1], 31u);
    if (name != NULL) {
        uint8_t nlen = 0u;
        while (nlen < 30u && name[nlen] != '\0') nlen++;
        for (uint8_t j = 0u; j < nlen; j++) record[1u + j] = (uint8_t)name[j];
    }
    return true;
}

static bool exec_get_flash_slot_info(uint8_t ram_slot) {
    uint8_t flash_slot = ring_read_byte();

    s_log("GET_FLASH_SLOT_INFO: flash_slot=%u", (unsigned)flash_slot);

    if (flash_slot == 0xAA) {
        s_log("GET_FLASH_SLOT_INFO failed: flash_slot value 0xAA is reserved");
        return false;
    }

    uint32_t space = s_state.cfg.data_size;
    if (space < 32u) {
        s_log("GET_FLASH_SLOT_INFO failed: data section too small");
        return false;
    }

    uint8_t record[32];
    if (!get_flash_slot_info(flash_slot, record)) {
        return false;
    }

    data_write(ram_slot, 0, record, 32u);
    return true;
}

static bool exec_get_flash_slot_info_all(uint8_t slot) {
    uint8_t total = s_get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);

    // If the data section can't hold even the 4-byte preamble, write what fits.
    if (s_state.cfg.data_size < 4u) {
        uint8_t preamble[4] = { total, 0u, 0u, 0u };
        data_write(slot, 0u, preamble, s_state.cfg.data_size);
        return true;
    }

    uint32_t space       = s_state.cfg.data_size - 4u;
    uint8_t  whole_count = (uint8_t)(space / 32u);
    if (whole_count > total) whole_count = total;

    // A partial record follows complete records if there are more slots to
    // report and at least one byte of space remains after the whole records.
    uint32_t partial_bytes = space - ((uint32_t)whole_count * 32u);
    uint8_t  partial_flag  = (whole_count < total && partial_bytes > 0u) ? 0x01u : 0x00u;

    uint8_t preamble[4] = { total, whole_count, partial_flag, 0u };
    data_write(slot, 0u, preamble, 4u);

    uint32_t data_off      = 4u;
    uint8_t  slots_to_emit = whole_count + (partial_flag ? 1u : 0u);

    for (uint8_t i = 0u; i < slots_to_emit; i++) {
        const char *name     = NULL;
        uint32_t    rom_type = 0xFFu;
        s_get_flash_slot_info(i, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
                              &name, &rom_type, NULL);

        // Build a 32-byte record: 1 byte rom_type, 31 bytes name (zero-padded).
        uint8_t record[32];
        if (!get_flash_slot_info(i, record)) {
            return false;
        }

        // Whole records are 32 bytes; the trailing partial record is however
        // many bytes remain in the data section.
        uint32_t bytes = (i < whole_count) ? 32u : partial_bytes;
        if (i == whole_count && partial_bytes >= 2u) {
            // Partial record: force a null terminator at the truncation point,
            // so the truncated name is a C string like every other name in the
            // response and a host needs no separate parsing path for it.
            //
            // Only where a name is present at all.  With a single byte the
            // record is just the rom_type, and terminating would overwrite the
            // one piece of information it carries.
            record[partial_bytes - 1] = 0x00u;
        }
        data_write(slot, data_off, record, bytes);
        data_off += bytes;
    }

    return true;
}

static bool exec_get_ram_slot_info_all(uint8_t slot) {
    s_log("GET_RAM_SLOT_INFO_ALL: slot=%u", (unsigned)slot);
    uint8_t  total    = host_slot_count();
    uint32_t rom_type = 0xFFu;

    // slot is s_state.active_slot — both the back-channel destination and
    // the index of the RAM slot currently being served.  Query its ROM type
    // via the extended get_ram_slot_info API.
    s_get_ram_slot_info(slot, NULL, NULL, &rom_type);

    uint8_t resp[4] = { total, slot, (uint8_t)(rom_type & 0xFFu), 0u };
    data_write(slot, 0u, resp, 4u);
    s_log("GET_RAM_SLOT_INFO: tot=%u sl=%u rt=0x%02X", (unsigned)total, (unsigned)slot, (unsigned)rom_type);
    return true;
}

static const char device_type_str[] = "One ROM";
static bool exec_get_device_type(void) {
    uint8_t device_type[24];
    zero_bytes((uint8_t *)device_type, sizeof(device_type));
    for (size_t i = 0; i < sizeof(device_type_str) && i < sizeof(device_type); i++) {
        device_type[i] = device_type_str[i];
    }
    data_write(s_state.active_slot, 0u, device_type, sizeof(device_type));
    s_log("GET_DEVICE_TYPE: dt=%s", device_type);
    return true;
}

static bool exec_get_device_version(void) {
    uint8_t device_version[24];
    zero_bytes(device_version, sizeof(device_version));
    if (s_get_device_version(device_version, sizeof(device_version)) != ORA_RESULT_OK) {
        s_log("GET_DEVICE_VERSION failed");
        return false;
    }
    data_write(s_state.active_slot, 0u, device_version, sizeof(device_version));
    s_log("GET_DEVICE_VERSION: ver=%s", device_version);
    return true;
}

static bool exec_get_protocol_version(void) {
    data_write(s_state.active_slot, 0u, protocol_version, sizeof(protocol_version));
    s_log("GET_PROTOCOL_VERSION: ver=%u.%u.%u", (unsigned)protocol_version[0], (unsigned)protocol_version[1], (unsigned)protocol_version[2]);
    return true;
}

static bool exec_get_boot_slot_info(void) {
    uint8_t resp[4] = { s_boot_flash_slot, s_boot_ram_slot, 0u, 0u };
    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_log("GET_BOOT_SLOT_INFO: flash_slot=%u ram_slot=%u",
          (unsigned)s_boot_flash_slot, (unsigned)s_boot_ram_slot);
    return true;
}

static bool exec_slot_peek(void) {
    uint8_t count  = ring_read_byte();
    uint8_t a0     = ring_read_byte();
    uint8_t a1     = ring_read_byte();
    uint8_t a2     = ring_read_byte();
    uint8_t target = ring_read_byte();

    s_log("SLOT_PEEK: tgt=%u a0=0x%02X a1=0x%02X a2=0x%02X ct=%u",
          (unsigned)target, (unsigned)a0, (unsigned)a1, (unsigned)a2, (unsigned)count);

    if (target == 0xAAu) {
        s_log("SLOT_PEEK failed: target value 0xAA is reserved");
        return false;
    }
    if (!host_slot_valid(target)) {
        s_log("SLOT_PEEK failed: slot %u is not one the host may name", (unsigned)target);
        return false;
    }

    uint32_t addr       = (uint32_t)a0
                        | ((uint32_t)a1 << 8u)
                        | ((uint32_t)a2 << 16u);
    uint32_t byte_count = (count == 0u) ? 256u : (uint32_t)count;

    if (byte_count > s_state.cfg.data_size) {
        s_log("SLOT_PEEK failed: ct %u vs data size %u",
              (unsigned)byte_count, (unsigned)s_state.cfg.data_size);
        return false;
    }

    uint32_t slot_base, slot_size;
    if (s_get_ram_slot_info(target, &slot_base, &slot_size, NULL) != ORA_RESULT_OK) {
        s_log("SLOT_PEEK failed: tgt slot %u", (unsigned)target);
        return false;
    }

    if (addr + byte_count > slot_size) {
        s_log("SLOT_PEEK failed: read range exceeds slot size");
        return false;
    }

    // Write the requested data in chunks, small enough to not blow the stack
#define SLOT_PEEK_BUF_SIZE 32u
    uint8_t  buf[SLOT_PEEK_BUF_SIZE];
    uint32_t remaining  = byte_count;
    uint32_t data_off   = 0u;

    const uint8_t *slot = ORA_SRAM_PTR(slot_base);

    while (remaining > 0u) {
        uint32_t chunk = (remaining > SLOT_PEEK_BUF_SIZE) ? SLOT_PEEK_BUF_SIZE : remaining;
        for (uint32_t i = 0u; i < chunk; i++) {
            uint32_t phys_offset = s_map_addr_to_phys(addr + data_off + i);
            uint8_t  raw         = slot[phys_offset];
            if (s_demangle_data(raw, &buf[i]) != ORA_RESULT_OK) {
                s_log("SLOT_PEEK failed: demangle error at offset %u",
                      (unsigned)(data_off + i));
                return false;
            }
        }
        data_write(s_state.active_slot, data_off, buf, chunk);
        data_off  += chunk;
        remaining -= chunk;
    }

    s_log("SLOT_PEEK: slot=%u addr=0x%06X count=%u",
          (unsigned)target, (unsigned)addr, (unsigned)byte_count);
    return true;
}

// Defined with the Modify group, whose LOAD_SLOT is the same copy.
static bool load_slot_impl(const char *name);

// Reload a RAM slot and leave command-response mode without writing the
// response header.  Where the slot named is the one being served, that puts
// the whole image back, including the bytes the back-channel displaced.
//
// The exit happens whether or not the load did: the host is told not to poll
// after this command, so a device that stayed in command-response mode on a
// bad argument would be waiting for commands nobody is going to send.
static bool exec_load_and_exit(void) {
    bool ok = load_slot_impl("LOAD_AND_EXIT");
    s_state.active = false;
    return ok;
}

// Write the host's bytes over the start of the back-channel region and leave
// command-response mode, writing nothing else.  The bytes are the region's
// original contents, which only the host knows.
//
// Written through s_reprogram rather than hdr_write: these are the host's
// bytes going back into its image, not header fields, and the region has
// already stopped being a back-channel by the time anything reads them.
static bool exec_exit_cmd_resp_restore(void) {
    uint8_t bytes[RESTORE_MAX_BYTES];
    for (uint8_t i = 0u; i < RESTORE_MAX_BYTES; i++) {
        bytes[i] = ring_read_byte();
    }
    uint8_t count = ring_read_byte();

    // Command mode has no back-channel region, so there is nothing to put
    // back and cfg.region_offset means nothing.  All nine arguments are read
    // above before the command is discarded, as ENTER_CMD_RESP does when it
    // is the one arriving in the wrong mode.
    if (!s_state.active) {
        s_log("EXIT_CMD_RESP_RESTORE failed: not in command-response mode");
        return false;
    }

    // The exit completes either way, as for any terminal command given an
    // argument it cannot use.
    s_state.active = false;

    if ((count == 0u) || (count > RESTORE_MAX_BYTES)) {
        s_log("EXIT_CMD_RESP_RESTORE failed: count %u out of range", (unsigned)count);
        return false;
    }

    s_log("EXIT_CMD_RESP_RESTORE: count=%u", (unsigned)count);
    if (s_reprogram(s_state.active_slot, s_state.cfg.region_offset,
                    bytes, count, 1u) != ORA_RESULT_OK) {
        s_log("EXIT_CMD_RESP_RESTORE failed: reprogram error");
        return false;
    }
    return true;
}

// ---------------------------------------------------------------------------
// Command handlers — Modify group (0x02)
// ---------------------------------------------------------------------------

static bool exec_slot_poke(void) {
    uint8_t byte   = ring_read_byte();
    uint8_t a0     = ring_read_byte();
    uint8_t a1     = ring_read_byte();
    uint8_t a2     = ring_read_byte();
    uint8_t target = ring_read_byte();

    if (target == 0xAAu) {
        s_log("SLOT_POKE failed: target value 0xAA is reserved");
        return false;
    }
    if (!host_slot_valid(target)) {
        s_log("SLOT_POKE failed: slot %u is not one the host may name", (unsigned)target);
        return false;
    }

    uint32_t addr = (uint32_t)a0
                  | ((uint32_t)a1 << 8u)
                  | ((uint32_t)a2 << 16u);
    return (s_reprogram(target, addr, &byte, 1u, 1u) == ORA_RESULT_OK);
}

static bool exec_switch_slot(void) {
    uint8_t target = ring_read_byte();

    if (target == 0xAAu) {
        s_log("SWITCH_SLOT failed: target value 0xAA is reserved");
        return false;
    }
    if (!host_slot_valid(target)) {
        s_log("SWITCH_SLOT failed: slot %u is not one the host may name", (unsigned)target);
        return false;
    }

    s_log("SWITCH_SLOT: target=%u", (unsigned)target);
    return (s_set_active_ram_slot(target) == ORA_RESULT_OK);
}

// Copy a flash slot into a RAM slot, shared by LOAD_SLOT and LOAD_AND_EXIT.
// Reads both argument bytes whatever the outcome, so a rejected command still
// takes its frame off the wire.
//
static bool load_slot_impl(const char *name) {
    uint8_t ram_slot   = ring_read_byte();
    uint8_t flash_slot = ring_read_byte();

    s_log("%s: ram_slot=%u flash_slot=%u", name, (unsigned)ram_slot, (unsigned)flash_slot);

    if ((ram_slot == 0xAAu) || (flash_slot == 0xAAu)) {
        s_log("%s failed: slot value 0xAA is reserved", name);
        return false;
    }
    if (!host_slot_valid(ram_slot)) {
        s_log("%s failed: slot %u is not one the host may name", name, (unsigned)ram_slot);
        return false;
    }

    ora_result_t rc = s_copy_flash_to_ram(
        flash_slot,
        ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
        ram_slot,
        0u
    );
    if (rc != ORA_RESULT_OK) {
        s_log("%s failed: copy_flash_to_ram error %d", name, (int)rc);
        return false;
    }

    return true;
}

static bool exec_load_slot(void) {
    return load_slot_impl("LOAD_SLOT");
}

static bool exec_slot_poke_all_byte(void) {
    uint8_t byte     = ring_read_byte();
    uint8_t target = ring_read_byte();

    if (target == 0xAAu) {
        s_log("SLOT_POKE_ALL_BYTE failed: target value 0xAA is reserved");
        return false;
    }
    if (!host_slot_valid(target)) {
        s_log("SLOT_POKE_ALL_BYTE failed: slot %u is not one the host may name", (unsigned)target);
        return false;
    }

    uint32_t rom_type = 0xFFu;
    if (s_get_ram_slot_info(target, NULL, NULL, &rom_type) != ORA_RESULT_OK) {
        s_log("SLOT_POKE failed: invalid target slot %u", (unsigned)target);
        return false;
    }
    uint32_t chip_size = s_get_chip_size(rom_type);
    if (chip_size == 0u) {
        s_log("SLOT_POKE failed: invalid ROM type 0x%02X", (unsigned)rom_type);
        return false;
    }

    for (uint32_t i = 0u; i < chip_size; i++) {
        if (s_reprogram(target, i, &byte, 1u, 1u) != ORA_RESULT_OK) {
            s_log("SLOT_POKE failed: reprogram error at offset %u", (unsigned)i);
            return false;
        }
    }
    return true;
}

// ---------------------------------------------------------------------------
// NV Storage
// ---------------------------------------------------------------------------

// The bootrom table, the XIP clock divisor, the base of mapped flash, the
// extent of the erase routine and the pointer to its staged copy are all facts
// about the device rather than about this plugin, so each goes through its own
// named ORA macro — see "Device facts" in ora/api.h.  On a device every one of
// them compiles to the expression that used to be written here inline.

static void *nv_lookup_boot_fn(char a, char b) {
    uint32_t code = ((uint32_t)(uint8_t)b << 8) | (uint32_t)(uint8_t)a;
    return ORA_BOOTROM_LOOKUP(code, ORA_BOOTROM_FLAG_ARM_SEC);
}

static void nv_discard_impl(void) {
    init_nv_state();
}

static bool nv_private_staging(uint32_t *base_out, uint8_t *first_out);
static uint32_t nv_staging_required(void);

// Whether a write transaction can be staged anywhere at all.
//
// Two routes, and either will do: this plugin's own slots above the
// host-visible range, or a slot the host lends us — which needs there to be
// more than one, so that the one being served is not the one overwritten, and
// needs that slot to be big enough.
//
// One function rather than the test written at each site, so that what
// GET_NV_CAPABILITY reports and what the write commands do cannot drift apart.
// A device that answered "writable" and then failed every transaction would be
// worse than one that admitted it could not.
static bool nv_writable(void) {
    uint32_t base;
    if (nv_private_staging(&base, NULL)) {
        return true;
    }
    if (host_slot_count() <= 1u) {
        return false;
    }
    uint32_t slot_size;
    if (s_get_ram_slot_info(0u, NULL, &slot_size, NULL) != ORA_RESULT_OK) {
        return false;
    }
    return slot_size >= nv_staging_required();
}

static bool exec_get_nv_capability(void) {
    bool writable = nv_writable();
    uint8_t resp[4] = {
        (uint8_t)(NV_STORAGE_SIZE & 0xFFu),
        (uint8_t)((NV_STORAGE_SIZE >> 8u) & 0xFFu),
        writable ? 0x01u : 0x00u,
        0x00u
    };
    data_write(s_state.active_slot, 0u, resp, 4u);
    return true;
}

static bool exec_nv_peek(void) {
    uint8_t count   = ring_read_byte();
    uint8_t loc_lsb = ring_read_byte();
    uint8_t loc_msb = ring_read_byte();

    if (loc_msb > 0x7Fu) {
        s_log("NV_PEEK: loc_msb 0x%02X exceeds 0x7F", (unsigned)loc_msb);
        return false;
    }
    uint32_t location   = (uint32_t)loc_lsb | ((uint32_t)loc_msb << 8u);
    uint32_t byte_count = (count == 0u) ? 256u : (uint32_t)count;
    if (location + byte_count > NV_STORAGE_SIZE) {
        s_log("NV_PEEK: range exceeds NV storage size");
        return false;
    }
    if (byte_count > s_state.cfg.data_size) {
        s_log("NV_PEEK: count %u exceeds data section", (unsigned)byte_count);
        return false;
    }
    data_write(s_state.active_slot, 0u, &__nv_storage_start[location], byte_count);
    return true;
}

// Bytes a staging area must hold: the whole of NV storage, plus the erase
// routine copied in immediately above it.
static uint32_t nv_staging_required(void) {
    return NV_STORAGE_SIZE
         + ORA_STAGED_FN_SIZE(__flash_erase_fn_start, __flash_erase_fn_end);
}

// Find a staging area among this plugin's own RAM slots, if it has any.
//
// The firmware reports every slot the RAM holds; the host is told about at
// most MAX_HOST_SLOTS of them, so anything above that is ours.  Slots are
// consecutive regions of SRAM, so a run of them is one contiguous buffer — and
// a run is what a small ROM needs, since a slot is only as big as the ROM being
// served and can be 2KB.
//
// The *highest* run, so that adding host-visible slots later — or a plugin
// wanting a private slot of its own — takes from the bottom of the private
// range and does not collide.
//
// Staging here rather than in the host's slot is strictly better for the host:
// nothing it can name is disturbed.  Where there is no private slot at all —
// a large ROM leaves few slots, and a 512KB one leaves a single slot — the
// caller falls back to the slot the host lent us, which is what RBCP describes.
static bool nv_private_staging(uint32_t *base_out, uint8_t *first_out) {
    uint8_t total = s_get_ram_slot_count();
    uint8_t host  = host_slot_count();
    if (total <= host) {
        return false;
    }

    uint32_t slot_size;
    if (s_get_ram_slot_info(host, NULL, &slot_size, NULL) != ORA_RESULT_OK
        || slot_size == 0u) {
        return false;
    }

    uint32_t required     = nv_staging_required();
    uint32_t slots_needed = (required + slot_size - 1u) / slot_size;
    if (slots_needed > (uint32_t)(total - host)) {
        return false;
    }

    uint8_t first = (uint8_t)((uint32_t)total - slots_needed);
    if (first_out != NULL) {
        *first_out = first;
    }
    return s_get_ram_slot_info(first, base_out, NULL, NULL) == ORA_RESULT_OK;
}

static bool nv_poke_begin_impl(uint8_t slot) {
    if (s_nv_state.active) {
        s_log("NPB: transaction already in progress");
        return false;
    }
    // "Fails if ... the RAM slot specified is invalid, active or too small."
    // The first two are checked whichever way the transaction is staged: the
    // host is telling us which slot it is willing to lose, and naming the one
    // being served is a mistake worth reporting even when we do not need it.
    if (!host_slot_valid(slot)) {
        s_log("NPB: slot %u is not one the host may name", (unsigned)slot);
        return false;
    }
    if (slot == s_state.active_slot) {
        s_log("NPB: slot %u is the active slot", (unsigned)slot);
        return false;
    }

    uint32_t erase_fn_size =
        ORA_STAGED_FN_SIZE(__flash_erase_fn_start, __flash_erase_fn_end);
    uint32_t required = nv_staging_required();

    // Prefer our own slots; fall back to the one the host lent us.  Only the
    // fallback cares how big that slot is — which is the whole point, since a
    // slot is only as large as the ROM being served and a small ROM could
    // never lend one big enough.
    uint32_t slot_base, slot_size;
    uint8_t  first_private;
    if (nv_private_staging(&slot_base, &first_private)) {
        slot_size = required;
        s_log("NPB: staging in this plugin's own slots, from %u",
              (unsigned)first_private);
    } else {
        if (s_get_ram_slot_info(slot, &slot_base, &slot_size, NULL) != ORA_RESULT_OK) {
            s_log("NPB: invalid slot %u", (unsigned)slot);
            return false;
        }
        if (slot_size < required) {
            s_log("NPB: slot %u too small (%u < %u)",
                  (unsigned)slot, (unsigned)slot_size, (unsigned)required);
            return false;
        }
    }

    // Copy NV flash contents into staging (linear SRAM write, no mangling)
    volatile uint8_t *staging = ORA_SRAM_PTR(slot_base);
    for (uint32_t i = 0u; i < NV_STORAGE_SIZE; i++) {
        staging[i] = __nv_storage_start[i];
    }

    // Copy erase function binary immediately after staging data.
    // Set Thumb bit on the function pointer at call time, not here.
    volatile uint8_t *erase_dest = staging + NV_STORAGE_SIZE;
    for (uint32_t i = 0u; i < erase_fn_size; i++) {
        erase_dest[i] = __flash_erase_fn_start[i];
    }

    s_nv_state.active       = true;
    s_nv_state.staging_slot = slot;
    s_nv_state.staging_base = slot_base;
    s_nv_state.staging_size = slot_size;

    s_log("NPB: slot=%u base=0x%08X fn_size=%u",
          (unsigned)slot, (unsigned)slot_base, (unsigned)erase_fn_size);
    return true;
}

static bool exec_nv_poke_begin(void) {
    uint8_t slot = ring_read_byte();
    if (slot == 0xAAu) {
        s_log("NPB: slot 0xAA invalid");
        return false;
    }
    return nv_poke_begin_impl(slot);
}

static bool nv_poke_impl(uint8_t byte, uint8_t loc_lsb, uint8_t loc_msb) {
    if (!s_nv_state.active) {
        s_log("NV_POKE: no transaction in progress");
        return false;
    }
    if (loc_msb > 0x7Fu) {
        s_log("NV_POKE: loc_msb 0x%02X exceeds 0x7F", (unsigned)loc_msb);
        return false;
    }
    uint32_t location = (uint32_t)loc_lsb | ((uint32_t)loc_msb << 8u);
    if (location >= NV_STORAGE_SIZE) {
        s_log("NV_POKE: location %u out of range", (unsigned)location);
        return false;
    }
    ((volatile uint8_t *)ORA_SRAM_PTR(s_nv_state.staging_base))[location] = byte;
    return true;
}

static bool exec_nv_poke(void) {
    uint8_t byte    = ring_read_byte();
    uint8_t loc_lsb = ring_read_byte();
    uint8_t loc_msb = ring_read_byte();
    return nv_poke_impl(byte, loc_lsb, loc_msb);
}

static bool exec_nv_poke_discard(void) {
    if (!s_nv_state.active) {
        s_log("NV_POKE_DISCARD: no transaction in progress");
        return false;
    }
    nv_discard_impl();
    return true;
}

static bool exec_nv_poke_commit(void) {
    if (!s_nv_state.active) {
        s_log("NPC: no transaction in progress");
        return false;
    }

    // Look up all bootrom functions while XIP is still active.
    // On any lookup failure, leave the transaction active so the host
    // can retry or discard per the spec.
    connect_internal_flash_fn_t connect_internal_flash =
        (connect_internal_flash_fn_t)nv_lookup_boot_fn('I', 'F');
    if (connect_internal_flash == NULL) {
        s_log("NPC: connect_internal_flash not found");
        return false;
    }
    flash_exit_xip_fn_t flash_exit_xip =
        (flash_exit_xip_fn_t)nv_lookup_boot_fn('E', 'X');
    if (flash_exit_xip == NULL) {
        s_log("NPC: flash_exit_xip not found");
        return false;
    }
    flash_range_erase_fn_t flash_range_erase =
        (flash_range_erase_fn_t)nv_lookup_boot_fn('R', 'E');
    if (flash_range_erase == NULL) {
        s_log("NPC: flash_range_erase not found");
        return false;
    }
    flash_flush_cache_fn_t flash_flush_cache =
        (flash_flush_cache_fn_t)nv_lookup_boot_fn('F', 'C');
    if (flash_flush_cache == NULL) {
        s_log("NPC: flash_flush_cache not found");
        return false;
    }
    flash_select_xip_read_mode_fn_t flash_select_xip_read_mode =
        (flash_select_xip_read_mode_fn_t)nv_lookup_boot_fn('X', 'M');
    if (flash_select_xip_read_mode == NULL) {
        s_log("NPC: flash_select_xip_read_mode not found");
        return false;
    }
    flash_range_program_fn_t flash_range_program =
        (flash_range_program_fn_t)nv_lookup_boot_fn('R', 'P');
    if (flash_range_program == NULL) {
        s_log("NPC: flash_range_program not found");
        return false;
    }
    
    // Get the exclusive mode functions, which we'll use to ensure the flash
    // isn't accessed during the critical section of the commit.  Checked like
    // the bootrom lookups above: the firmware this plugin declares a minimum
    // version for has both, but nothing in the build ties that declaration to
    // this call, and calling through a null pointer faults the core.
    ora_enter_exclusive_mode_fn_t enter_exclusive =
        s_lookup(ORA_ID_ENTER_EXCLUSIVE_MODE);
    ora_exit_exclusive_mode_fn_t exit_exclusive =
        s_lookup(ORA_ID_EXIT_EXCLUSIVE_MODE);
    if (enter_exclusive == NULL || exit_exclusive == NULL) {
        s_log("NPC: exclusive mode not available");
        return false;
    }

    if (enter_exclusive() != ORA_RESULT_OK) {
        s_log("NPC: enter exclusive mode failed");
        return false;
    }

    const uint8_t *staging = ORA_SRAM_PTR(s_nv_state.staging_base);
    nv_flash_erase_critical_fn_t erase_fn = ORA_STAGED_FN_PTR(
        nv_flash_erase_critical_fn_t, s_nv_state.staging_base + NV_STORAGE_SIZE);

    // Exclusive mode parks the other core with its interrupts masked.  This
    // core has to mask its own, and from here rather than from inside the
    // staged routine: connect_internal_flash() has already taken the flash off
    // the settings the running system configured, and an interrupt taken
    // between here and the XIP restore runs a handler that lives in flash.
    // Nothing on this core took interrupts at all until the firmware's LED
    // engine began servicing animations from a timer, which it does on
    // whichever core asked for one - this one, whenever a host drives SET_LED.
    uint32_t primask = flash_irq_disable();

    connect_internal_flash();

    // Read clkdiv and compute flash offset before exiting XIP.
    uint8_t  clkdiv     = ORA_XIP_CLKDIV();
    uint32_t flash_offs = ORA_FLASH_OFFSET(__nv_storage_start);

    // Erase and program the NV sector via the function blob copied into the
    // RAM slot.  Both run between one exit from XIP and one restore of it, so
    // the bootrom's program function gets the flash in the serial command mode
    // it needs.  It returns void, so a failed write is not detectable here.
    erase_fn(
        flash_exit_xip,
        flash_range_erase,
        flash_range_program,
        flash_flush_cache,
        flash_select_xip_read_mode,
        flash_offs,
        staging,
        NV_STORAGE_SIZE,
        clkdiv
    );

    flash_irq_restore(primask);

    exit_exclusive();

    s_log("NPC: complete offs=0x%08X clkdiv=%u", (unsigned)flash_offs,
          (unsigned)clkdiv);
    nv_discard_impl();
    return true;
}

static bool exec_nv_poke_commit_byte(void) {
    uint8_t byte    = ring_read_byte();
    uint8_t loc_lsb = ring_read_byte();
    uint8_t loc_msb = ring_read_byte();
    uint8_t slot    = ring_read_byte();
    if (slot == 0xAAu) {
        s_log("NV_POKE_COMMIT_BYTE: slot 0xAA invalid");
        return false;
    }

    // Checked before the unchanged-byte short cut below, not after.  The
    // command "fails if NV storage is not writable", and a host that gets
    // status-OK from a write command has been told the write happened; on a
    // read-only device it cannot have, whatever the byte was.
    if (!nv_writable()) {
        s_log("NV_POKE_COMMIT_BYTE: NV storage is read-only");
        return false;
    }

    // Avoid erasing/writing flash if the byte hasn't changed.
    uint32_t location = (uint32_t)loc_lsb | ((uint32_t)loc_msb << 8u);
    if (loc_msb <= 0x7Fu && location < NV_STORAGE_SIZE &&
        __nv_storage_start[location] == byte) {
        return true;
    }

    if (!nv_poke_begin_impl(slot)) {
        return false;
    }
    if (!nv_poke_impl(byte, loc_lsb, loc_msb)) {
        nv_discard_impl();
        return false;
    }
    return exec_nv_poke_commit();
}

// ---------------------------------------------------------------------------
// Command handlers — Pipes group (0x04)
// ---------------------------------------------------------------------------
//
// Everything in this group reports through s_debug_log rather than s_log.  The
// pipe is an ORA log channel, and s_log writes that same channel, so anything
// logged here on a normal build would land in the middle of the bytes the host
// is sending.  Debug output is emitted only where the firmware was built for
// it, which is where someone is watching the plugin rather than the stream.

// The name a reader sees for the channel once a host has written to it.  const,
// so it lives in .rodata and costs flash rather than any of the plugin's 512
// bytes of static RAM.  ora_log_open_write keeps this pointer rather than
// copying the string, so it must outlive the claim, which a literal does.
static const char pipe_name[] = "RBCP pipe 0";

// Number of pipes this device exposes.
//
// A pipe is an ORA log channel and pipe N is channel N, so this is the number
// of channels the running firmware has.  Two ways it can be zero, and the
// protocol treats them alike: firmware older than 0.7.2 has no log API, so the
// lookups returned NULL, or the firmware has the API but no channel 0.  Either
// way GET_PIPE_CAPABILITY reports zero and the other two commands fail, which
// is what the specification says an optional feature looks like when absent.
//
// The API header declares one channel and states that firmware may have fewer
// channels than the header declares, never more, so channel 0 is the only one
// to test for.  ora_log_query needs no claim in either direction and has no
// side effects, which is what makes it safe to use as the test.
static uint8_t pipe_count(void) {
    if ((s_log_open_write == NULL) || (s_log_write == NULL) ||
        (s_log_query == NULL)) {
        return 0u;
    }
    if (s_log_query(ORA_LOG_CHANNEL_0, NULL, NULL, NULL) != ORA_RESULT_OK) {
        return 0u;
    }
    return 1u;
}

static bool exec_get_pipe_capability(void) {
    if (s_state.cfg.data_size < 8u) {
        s_debug_log("GET_PIPE_CAPABILITY failed: data section too small");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    resp[0] = pipe_count();

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_debug_log("GET_PIPE_CAPABILITY: pipes=%u", (unsigned)resp[0]);
    return true;
}

static bool exec_get_pipe_info(void) {
    uint8_t pipe = ring_read_byte();

    s_debug_log("GET_PIPE_INFO: pipe=%u", (unsigned)pipe);

    if (pipe == 0xAAu) {
        s_debug_log("GET_PIPE_INFO failed: pipe value 0xAA is reserved");
        return false;
    }
    if (s_state.cfg.data_size < 8u) {
        s_debug_log("GET_PIPE_INFO failed: data section too small");
        return false;
    }
    if (pipe >= pipe_count()) {
        s_debug_log("GET_PIPE_INFO failed: no such pipe");
        return false;
    }

    uint32_t free_bytes = 0u;
    if (s_log_query((ora_log_channel_t)pipe, NULL, &free_bytes, NULL) !=
        ORA_RESULT_OK) {
        s_debug_log("GET_PIPE_INFO failed: query error");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    resp[0] = PIPE_TYPE_RAW;
    resp[1] = PIPE_FLAG_OUT;
    resp[2] = (free_bytes > 0xFFu) ? 0xFFu : (uint8_t)free_bytes;
    // waiting stays zero: the pipe carries no IN direction, so there is never
    // anything for the host to read.
    resp[4] = PIPE_FAR_END_UNSPECIFIED;

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    return true;
}

static bool exec_pipe_write(void) {
    uint8_t data[PIPE_WRITE_MAX_BYTES];
    for (uint8_t i = 0; i < PIPE_WRITE_MAX_BYTES; i++) {
        data[i] = ring_read_byte();
    }
    uint8_t pipe  = ring_read_byte();
    uint8_t count = ring_read_byte();

    // No separate check for the reserved 0xAA value here, unlike every other
    // command taking a slot or an index.  count is the final argument and its
    // valid range is 1 to 4, so the range check below rejects 0xAA already.
    if ((count == 0u) || (count > PIPE_WRITE_MAX_BYTES)) {
        s_debug_log("PIPE_WRITE failed: count %u out of range", (unsigned)count);
        return false;
    }
    if (pipe >= pipe_count()) {
        s_debug_log("PIPE_WRITE failed: no such pipe %u", (unsigned)pipe);
        return false;
    }

    // Claimed on first use.  Only the claiming plugin may write the channel, so
    // this cannot be skipped, but it renames the channel for anything reading
    // the log, so it waits until a host actually sends bytes.
    uint8_t claim_bit = (uint8_t)(1u << pipe);
    if ((s_pipes_claimed & claim_bit) == 0u) {
        ora_result_t rc = s_log_open_write((ora_log_channel_t)pipe, pipe_name);
        if (rc != ORA_RESULT_OK) {
            s_debug_log("PIPE_WRITE failed: cannot claim pipe %u (%d)",
                        (unsigned)pipe, (int)rc);
            return false;
        }
        s_pipes_claimed |= claim_bit;
    }

    // A write is stored whole or dropped whole and never blocks, which is what
    // PIPE_WRITE's all-or-nothing rule requires.  ORA_RESULT_LOG_FULL is
    // ordinary traffic rather than a fault - nothing has drained the channel
    // yet - so it is reported at debug only, and never through s_log, which
    // writes the same channel the host is reading.
    if (s_log_write((ora_log_channel_t)pipe, data, count) != ORA_RESULT_OK) {
        s_debug_log("PIPE_WRITE: pipe %u would not take %u bytes",
                    (unsigned)pipe, (unsigned)count);
        return false;
    }
    return true;
}

// ---------------------------------------------------------------------------
// Command handlers — Auxiliary I/O group (0x05)
// ---------------------------------------------------------------------------

// The kinds of pin group this plugin can expose, in the order it numbers them.
#define AUX_KIND_GPIO       0u
#define AUX_KIND_IMG_SEL    1u
#define AUX_KIND_X          2u
#define AUX_KIND_COUNT      3u

// The GPIOs one auxiliary pin reaches.  An X pad can reach two.
#define AUX_PIN_MAX_GPIOS   2u
typedef struct {
    uint8_t gpio[AUX_PIN_MAX_GPIOS];
    uint8_t count;
} aux_pin_t;

// GPIOs on the running variant, or zero where the firmware cannot say.
//
// Must be the firmware's own MAX_GPIOS, which is what ora_gpio_set and
// ora_gpio_query range-check against.  That count is indexed by the variant
// detected at boot, which is what ORA_METADATA_KEY_RP_VARIANT reports.
static uint8_t aux_gpio_count(void) {
    if (s_get_metadata_uint == NULL) {
        return 0u;
    }
    uint32_t variant = 0u;
    if (s_get_metadata_uint(ORA_METADATA_KEY_RP_VARIANT, &variant) != ORA_RESULT_OK) {
        return 0u;
    }
    switch ((rp235x_variant_t)variant) {
        case RP235XA: return 30u;
        case RP235XB: return 48u;
        default:      return 0u;
    }
}

// Whether the device exposes auxiliary pins at all.  Without the GPIO calls, or
// without a GPIO count to range-check against, nothing in the group can work -
// which RBCP already provides for, as a group count of zero.
static bool aux_available(void) {
    return (s_gpio_set != NULL) && (s_gpio_query != NULL) && (aux_gpio_count() != 0u);
}

// The largest hold this device can time.  Zero where the firmware has no
// millisecond counter, which the protocol defines as offering no timed holds
// and requires the device to enforce.
static uint8_t aux_max_hold(void) {
    return (s_uptime_ms != NULL) ? AUX_MAX_HOLD : 0u;
}

// Entries a GPIO array metadata key holds, stopping at the first unused one.
static uint8_t aux_meta_len(ora_metadata_key_t key) {
    uint8_t n = 0u;
    uint32_t gpio = 0u;
    while ((n < 0xFFu) &&
           (s_get_metadata_uint_at(key, n, &gpio) == ORA_RESULT_OK) &&
           ((uint8_t)gpio != ORA_GPIO_NONE)) {
        n++;
    }
    return n;
}

// The GPIOs X pad `pad` reaches - X1 is pad 0, X2 is pad 1 - none where the
// board has no such pad.
#define AUX_X_PADS  2u
static void aux_x_pad(uint8_t pad, aux_pin_t *out) {
    ora_metadata_key_t key = (pad == 0u) ? ORA_METADATA_KEY_GPIO_X1
                                         : ORA_METADATA_KEY_GPIO_X2;
    out->count = 0u;
    for (uint8_t i = 0u; i < AUX_PIN_MAX_GPIOS; i++) {
        uint32_t gpio = 0u;
        if (s_get_metadata_uint_at(key, i, &gpio) != ORA_RESULT_OK) break;
        if ((uint8_t)gpio == ORA_GPIO_NONE) break;
        out->gpio[out->count++] = (uint8_t)gpio;
    }
}

// X pads this board has.  One the board lacks is skipped rather than numbered,
// so the pins of the group stay dense.
static uint8_t aux_x_count(void) {
    aux_pin_t pad;
    uint8_t   pins = 0u;
    for (uint8_t i = 0u; i < AUX_X_PADS; i++) {
        aux_x_pad(i, &pad);
        if (pad.count != 0u) pins++;
    }
    return pins;
}

// The GPIOs auxiliary pin `pin` of `kind` reaches, or false where there is no
// such pin.
static bool aux_pin_gpios(uint8_t kind, uint8_t pin, aux_pin_t *out) {
    uint32_t gpio = 0u;
    out->count = 0u;

    switch (kind) {
        case AUX_KIND_GPIO:
            if (pin >= aux_gpio_count()) return false;
            out->gpio[0] = pin;
            out->count   = 1u;
            return true;

        case AUX_KIND_IMG_SEL:
            if (pin >= aux_meta_len(ORA_METADATA_KEY_GPIO_SEL)) return false;
            if (s_get_metadata_uint_at(ORA_METADATA_KEY_GPIO_SEL, pin, &gpio)
                != ORA_RESULT_OK) {
                return false;
            }
            out->gpio[0] = (uint8_t)gpio;
            out->count   = 1u;
            return true;

        default: {
            uint8_t seen = 0u;
            for (uint8_t i = 0u; i < AUX_X_PADS; i++) {
                aux_x_pad(i, out);
                if (out->count == 0u) continue;
                if (seen == pin) return true;
                seen++;
            }
            out->count = 0u;
            return false;
        }
    }
}

// Pins in a kind of group, or zero where this board or this firmware has none.
static uint8_t aux_kind_pins(uint8_t kind) {
    if (!aux_available()) {
        return 0u;
    }
    if (kind == AUX_KIND_GPIO) {
        return aux_gpio_count();
    }
    // Neither of the other two can be built without the indexed metadata
    // getter, which arrived later than this plugin's minimum firmware.
    if (s_get_metadata_uint_at == NULL) {
        return 0u;
    }
    return (kind == AUX_KIND_IMG_SEL) ? aux_meta_len(ORA_METADATA_KEY_GPIO_SEL)
                                      : aux_x_count();
}

static uint8_t aux_kind_type(uint8_t kind) {
    switch (kind) {
        case AUX_KIND_GPIO:    return AUX_TYPE_GPIO;
        case AUX_KIND_IMG_SEL: return AUX_TYPE_IMG_SEL;
        default:               return AUX_TYPE_X;
    }
}

static uint8_t aux_group_count(void) {
    uint8_t groups = 0u;
    for (uint8_t kind = 0u; kind < AUX_KIND_COUNT; kind++) {
        if (aux_kind_pins(kind) != 0u) groups++;
    }
    return groups;
}

// Resolve a group number to the kind it names and the pins it holds.
//
// Groups are numbered densely from zero, so a kind with no pins on this board
// is skipped rather than exposed as an empty group, and the kinds after it move
// down.  A host reads the numbering off GET_AUX_GROUP_INFO's type byte.
static bool aux_group_kind(uint8_t group, uint8_t *kind_out, uint8_t *pins_out) {
    uint8_t seen = 0u;
    for (uint8_t kind = 0u; kind < AUX_KIND_COUNT; kind++) {
        uint8_t pins = aux_kind_pins(kind);
        if (pins == 0u) continue;
        if (seen == group) {
            *kind_out = kind;
            *pins_out = pins;
            return true;
        }
        seen++;
    }
    return false;
}

// The flags, level and driven bytes GET_AUX_PIN_INFO reports for a pin, written
// in that order into `out` — which is the response's own first three bytes.
// SET_AUX tests the drivable flag before driving anything.
//
// Every GPIO the pin reaches must be free: an X pad reaching two of them is one
// net, so a use One ROM has for either is a use of the pad.  The level is that
// of the first, which on such a pad is the level of both.
#define AUX_PIN_INFO_BYTES  3u
static void aux_pin_info(const aux_pin_t *pin, uint8_t *out) {
    zero_bytes(out, AUX_PIN_INFO_BYTES);

    bool    drivable = (pin->count > 0u);
    uint8_t level    = 0u;
    uint8_t driven   = 0u;
    for (uint8_t i = 0u; i < pin->count; i++) {
        ora_gpio_info_t info = { (uint8_t)sizeof(ora_gpio_info_t), 0u, 0u, 0u };
        if (s_gpio_query(pin->gpio[i], &info) != ORA_RESULT_OK) {
            return;
        }
        if (i == 0u) {
            level  = info.level;
            driven = info.is_output;
        }
        if (info.use != ORA_GPIO_USE_FREE) {
            drivable = false;
        }
    }
    // Written only once every GPIO has answered, so a failure part way through
    // leaves all three bytes zero rather than a level with its flag clear.
    out[0] = (uint8_t)(AUX_PIN_FLAG_LEVEL |
                       (drivable ? AUX_PIN_FLAG_DRIVABLE : 0u));
    out[1] = level;
    out[2] = driven;
}

static bool aux_state_valid(uint8_t state) {
    return (state == AUX_STATE_LOW) ||
           (state == AUX_STATE_HIGH) ||
           (state == AUX_STATE_INPUT);
}

// Validate the arguments common to the three SET_AUX commands and resolve the
// pin they name.
static bool aux_set_valid(
    uint8_t state,
    uint8_t after,
    uint8_t hold,
    uint8_t pin,
    uint8_t group,
    aux_pin_t *out
) {
    if (group == 0xAAu) {
        s_log("SET_AUX failed: group value 0xAA is reserved");
        return false;
    }
    uint8_t kind, pins;
    if (!aux_group_kind(group, &kind, &pins)) {
        s_log("SET_AUX failed: no such group %u", (unsigned)group);
        return false;
    }
    if (pin >= pins) {
        s_log("SET_AUX failed: no such pin %u in group %u",
              (unsigned)pin, (unsigned)group);
        return false;
    }
    if (!aux_state_valid(state)) {
        s_log("SET_AUX failed: state 0x%02X undefined", (unsigned)state);
        return false;
    }
    if (hold != 0u) {
        if (hold > aux_max_hold()) {
            s_log("SET_AUX failed: hold %u exceeds maximum %u",
                  (unsigned)hold, (unsigned)aux_max_hold());
            return false;
        }
        if (!aux_state_valid(after)) {
            s_log("SET_AUX failed: after 0x%02X undefined", (unsigned)after);
            return false;
        }
    }
    if (!aux_pin_gpios(kind, pin, out)) {
        return false;
    }

    uint8_t info[AUX_PIN_INFO_BYTES];
    aux_pin_info(out, info);
    if ((info[0] & AUX_PIN_FLAG_DRIVABLE) == 0u) {
        s_log("SET_AUX failed: pin %u of group %u is not drivable",
              (unsigned)pin, (unsigned)group);
        return false;
    }
    return true;
}

static bool aux_pin_drive(const aux_pin_t *pin, uint8_t state) {
    for (uint8_t i = 0u; i < pin->count; i++) {
        if (s_gpio_set(pin->gpio[i], state, 0u) != ORA_RESULT_OK) {
            s_log("SET_AUX failed: cannot drive GPIO %u", (unsigned)pin->gpio[i]);
            return false;
        }
    }
    return true;
}

// Spin until the hold has elapsed, then apply `after`.
//
// The plugin has no task loop, so a hold is the command handler waiting, and
// RBCP is unresponsive until it ends.  Bus activity arriving meanwhile is
// discarded: cmd_end resets the ring read index immediately before it signals
// completion.  Unsigned subtraction is what stays correct across the counter's
// 49.7 day wrap.
static void aux_hold(
    const aux_pin_t *pin,
    uint8_t after,
    uint8_t hold,
    uint32_t start_ms
) {
    uint32_t hold_ms = (uint32_t)hold * 10u;
    while ((s_uptime_ms() - start_ms) < hold_ms) {
        ORA_TEST_YIELD();
    }
    (void)aux_pin_drive(pin, after);
}

static bool exec_get_aux_capability(void) {
    if (s_state.cfg.data_size < 8u) {
        s_log("GET_AUX_CAPABILITY failed: data section too small");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    resp[0] = aux_group_count();
    resp[1] = aux_max_hold();

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_log("GET_AUX_CAPABILITY: groups=%u max_hold=%u",
          (unsigned)resp[0], (unsigned)resp[1]);
    return true;
}

static bool exec_get_aux_group_info(void) {
    uint8_t group = ring_read_byte();

    s_log("GET_AUX_GROUP_INFO: group=%u", (unsigned)group);

    if (s_state.cfg.data_size < 8u) {
        s_log("GET_AUX_GROUP_INFO failed: data section too small");
        return false;
    }
    if (group == 0xAAu) {
        s_log("GET_AUX_GROUP_INFO failed: group value 0xAA is reserved");
        return false;
    }
    uint8_t kind, pins;
    if (!aux_group_kind(group, &kind, &pins)) {
        s_log("GET_AUX_GROUP_INFO failed: no such group");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    resp[0] = aux_kind_type(kind);
    resp[1] = pins;

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    return true;
}

static bool exec_get_aux_pin_info(void) {
    uint8_t pin   = ring_read_byte();
    uint8_t group = ring_read_byte();

    s_log("GET_AUX_PIN_INFO: pin=%u group=%u", (unsigned)pin, (unsigned)group);

    if (s_state.cfg.data_size < 8u) {
        s_log("GET_AUX_PIN_INFO failed: data section too small");
        return false;
    }
    if (group == 0xAAu) {
        s_log("GET_AUX_PIN_INFO failed: group value 0xAA is reserved");
        return false;
    }
    uint8_t kind, pins;
    if (!aux_group_kind(group, &kind, &pins)) {
        s_log("GET_AUX_PIN_INFO failed: no such group");
        return false;
    }
    aux_pin_t target;
    if ((pin >= pins) || !aux_pin_gpios(kind, pin, &target)) {
        s_log("GET_AUX_PIN_INFO failed: no such pin");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    aux_pin_info(&target, resp);

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    return true;
}

static bool exec_set_aux(void) {
    uint8_t state = ring_read_byte();
    uint8_t after = ring_read_byte();
    uint8_t hold  = ring_read_byte();
    uint8_t pin   = ring_read_byte();
    uint8_t group = ring_read_byte();

    s_log("SET_AUX: st=%u af=%u hd=%u pin=%u grp=%u", (unsigned)state,
          (unsigned)after, (unsigned)hold, (unsigned)pin, (unsigned)group);

    aux_pin_t target;
    if (!aux_set_valid(state, after, hold, pin, group, &target)) {
        return false;
    }

    uint32_t start_ms = (hold != 0u) ? s_uptime_ms() : 0u;
    if (!aux_pin_drive(&target, state)) {
        return false;
    }
    if (hold != 0u) {
        aux_hold(&target, after, hold, start_ms);
    }
    return true;
}

static bool exec_set_aux_and_exit(void) {
    bool ok = exec_set_aux();
    s_state.active = false;
    return ok;
}

static bool exec_set_aux_switch_exit(void) {
    uint8_t state = ring_read_byte();
    uint8_t after = ring_read_byte();
    uint8_t hold  = ring_read_byte();
    uint8_t flags = ring_read_byte();
    uint8_t pin   = ring_read_byte();
    uint8_t group = ring_read_byte();
    uint8_t slot  = ring_read_byte();

    // Terminal whatever follows, including the reserved 0xAA slot below, where
    // neither operation happens but the exit still does.
    s_state.active = false;

    s_log("SET_AUX_SWITCH_EXIT: st=%u af=%u hd=%u fl=0x%02X pin=%u grp=%u sl=%u",
          (unsigned)state, (unsigned)after, (unsigned)hold, (unsigned)flags,
          (unsigned)pin, (unsigned)group, (unsigned)slot);

    if (slot == 0xAAu) {
        s_log("SET_AUX_SWITCH_EXIT failed: slot value 0xAA is reserved");
        return false;
    }
    if ((flags & (uint8_t)~AUX_SWITCH_FLAG_SLOT_FIRST) != 0u) {
        s_log("SET_AUX_SWITCH_EXIT failed: reserved flag bits set");
        return false;
    }
    if (!host_slot_valid(slot)) {
        s_log("SET_AUX_SWITCH_EXIT failed: slot %u is not one the host may name",
              (unsigned)slot);
        return false;
    }

    aux_pin_t target;
    if (!aux_set_valid(state, after, hold, pin, group, &target)) {
        return false;
    }

    bool     slot_first = (flags & AUX_SWITCH_FLAG_SLOT_FIRST) != 0u;
    uint32_t start_ms   = 0u;

    if (slot_first && (s_set_active_ram_slot(slot) != ORA_RESULT_OK)) {
        return false;
    }
    if (hold != 0u) {
        start_ms = s_uptime_ms();
    }
    if (!aux_pin_drive(&target, state)) {
        return false;
    }
    if (!slot_first && (s_set_active_ram_slot(slot) != ORA_RESULT_OK)) {
        return false;
    }
    if (hold != 0u) {
        // Timed from the pin being driven, so under set-first ordering a switch
        // that takes longer than the hold delays `after` until it is done.
        aux_hold(&target, after, hold, start_ms);
    }
    return true;
}

// ---------------------------------------------------------------------------
// LEDs
// ---------------------------------------------------------------------------

// The LED calls arrived in firmware 0.7.2, later than this plugin's
// min_fw_version, so either may be NULL.  Looked up where they are used rather
// than cached, as this group costs the plugin no state of its own.
//
// Only ORA_ID_LED_SET is tested here.  A missing ORA_ID_LED_GET takes every
// channel away in led_ora_state(), which leaves the count at zero by itself.
static bool led_available(void) {
    return s_lookup(ORA_ID_LED_SET) != NULL;
}

// Read one ORA channel's state.  False where there is no LED engine to ask, or
// the channel is not one this firmware knows.
static bool led_ora_state(uint8_t ora_led, ora_led_state_t *out) {
    ora_led_get_fn_t get = s_lookup(ORA_ID_LED_GET);
    if (get == NULL) {
        return false;
    }
    zero_bytes((uint8_t *)out, (uint8_t)sizeof(*out));
    out->size = (uint8_t)sizeof(*out);
    return get(ora_led, out) == ORA_RESULT_OK;
}

// LEDs this device has.  RBCP numbers only those, contiguously from zero, while
// ORA numbers channels whether the board carries them or not - so the count is
// of the channels reporting present.
static uint8_t led_count(void) {
    if (!led_available()) {
        return 0u;
    }
    ora_led_state_t st;
    uint8_t n = 0u;
    for (uint8_t ch = 0u; ch <= LED_ORA_LAST; ch++) {
        if (led_ora_state(ch, &st) && (st.present != 0u)) {
            n++;
        }
    }
    return n;
}

// Resolve an RBCP LED number to the ORA channel it names, and its state.
static bool led_resolve(uint8_t led, uint8_t *ora_out, ora_led_state_t *state) {
    if (!led_available()) {
        return false;
    }
    uint8_t n = 0u;
    for (uint8_t ch = 0u; ch <= LED_ORA_LAST; ch++) {
        if (!led_ora_state(ch, state) || (state->present == 0u)) {
            continue;
        }
        if (n == led) {
            *ora_out = ch;
            return true;
        }
        n++;
    }
    return false;
}

static uint8_t led_modes(uint8_t ora_led) {
    return (ora_led == ORA_LED_RGB) ? (uint8_t)LED_MODES_RGB
                                    : (uint8_t)LED_MODES_MONO;
}

// Map an RBCP mode onto the firmware's, refusing one this LED does not support.
// The bitmap says what is supported and the switch says how it is numbered:
// two separate facts, so neither is derived from the other.
static bool led_ora_mode(uint8_t ora_led, uint8_t mode, uint8_t *out) {
    if (mode == LED_MODE_FLAME) {
        *out = ORA_LED_MODE_FLAME;
        return true;
    }
    if ((mode > 7u) || ((led_modes(ora_led) & (uint8_t)(1u << mode)) == 0u)) {
        return false;
    }
    switch (mode) {
        case LED_MODE_OFF:     *out = ORA_LED_MODE_OFF;     return true;
        case LED_MODE_ON:      *out = ORA_LED_MODE_ON;      return true;
        case LED_MODE_BLINK:   *out = ORA_LED_MODE_BLINK;   return true;
        case LED_MODE_BREATHE: *out = ORA_LED_MODE_BREATHE; return true;
        case LED_MODE_CYCLE:   *out = ORA_LED_MODE_CYCLE;   return true;
        case LED_MODE_BEACON:  *out = ORA_LED_MODE_BEACON;  return true;
        default:               return false;
    }
}

static uint8_t led_rbcp_mode(uint8_t ora_mode) {
    switch (ora_mode) {
        case ORA_LED_MODE_OFF:     return LED_MODE_OFF;
        case ORA_LED_MODE_ON:      return LED_MODE_ON;
        case ORA_LED_MODE_BLINK:   return LED_MODE_BLINK;
        case ORA_LED_MODE_BREATHE: return LED_MODE_BREATHE;
        case ORA_LED_MODE_CYCLE:   return LED_MODE_CYCLE;
        case ORA_LED_MODE_BEACON:  return LED_MODE_BEACON;
        case ORA_LED_MODE_FLAME:   return LED_MODE_FLAME;
        default:                   return LED_MODE_INVALID;
    }
}

// Milliseconds to the protocol's 100ms units, nearest and saturating.  Nothing
// the firmware runs has a period under 50ms, so only a period of none rounds
// to zero - which is what the protocol reads as no period in force.
static uint8_t led_period_units(uint16_t period_ms) {
    uint32_t units = ((uint32_t)period_ms + 50u) / 100u;
    return (units > 0xFFu) ? 0xFFu : (uint8_t)units;
}

// The shortest period a mode accepts, in milliseconds, or zero for a mode that
// takes no period.  The firmware bounds each repeating mode separately and the
// bound is not reachable through ORA, so the values come from the same metadata
// constants ora_led_set validates against.
static uint16_t led_min_period_ms(uint8_t ora_mode) {
    switch (ora_mode) {
        case ORA_LED_MODE_CYCLE:   return LED_CYCLE_MIN_PERIOD_MS;
        case ORA_LED_MODE_BREATHE: return LED_BREATHE_MIN_PERIOD_MS;
        case ORA_LED_MODE_BLINK:   return LED_BLINK_MIN_PERIOD_MS;
        case ORA_LED_MODE_BEACON:  return LED_BEACON_MIN_PERIOD_MS;
        case ORA_LED_MODE_FLAME:   return LED_FLAME_MIN_PERIOD_MS;
        default:                   return 0u;
    }
}

// The floor as the protocol reports it: whole 100ms units, rounded up so that
// the value named is one the firmware accepts.  A floor of one unit or less is
// reported as zero, one being the smallest period a host can ask for anyway.
static uint8_t led_min_period_units(uint8_t ora_mode) {
    uint32_t units = ((uint32_t)led_min_period_ms(ora_mode) + 99u) / 100u;
    return (units > 1u) ? (uint8_t)units : 0u;
}

static bool exec_get_led_capability(void) {
    if (s_state.cfg.data_size < 8u) {
        s_log("GET_LED_CAPABILITY failed: data section too small");
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    resp[0] = led_count();
    if (resp[0] != 0u) {
        resp[1] = LED_MAX_PERIOD;
        resp[2] = LED_MAX_HOLD;
    }

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_log("GET_LED_CAPABILITY: count=%u", (unsigned)resp[0]);
    return true;
}

static bool exec_get_led_info(void) {
    uint8_t led = ring_read_byte();

    if (s_state.cfg.data_size < 16u) {
        s_log("GET_LED_INFO failed: data section too small");
        return false;
    }
    if (led == 0xAAu) {
        s_log("GET_LED_INFO failed: LED value 0xAA is reserved");
        return false;
    }
    uint8_t ora_led;
    ora_led_state_t st;
    if (!led_resolve(led, &ora_led, &st)) {
        s_log("GET_LED_INFO failed: no such LED %u", (unsigned)led);
        return false;
    }

    uint8_t resp[16];
    zero_bytes(resp, sizeof(resp));
    resp[0] = (ora_led == ORA_LED_RGB) ? LED_TYPE_RGB : LED_TYPE_MONO;
    resp[1] = led_rbcp_mode(st.mode);
    if (ora_led == ORA_LED_RGB) {
        resp[2] = st.red;
        resp[3] = st.green;
        resp[4] = st.blue;
        resp[5] = st.brightness;
    } else {
        resp[2] = LED_STATUS_RED;
    }
    resp[6] = led_period_units(st.period_ms);
    resp[8] = led_modes(ora_led);

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_log("GET_LED_INFO: led=%u type=%u mode=%u",
          (unsigned)led, (unsigned)resp[0], (unsigned)resp[1]);
    return true;
}

static bool exec_get_led_mode_info(void) {
    uint8_t mode = ring_read_byte();
    uint8_t led  = ring_read_byte();

    if (s_state.cfg.data_size < 8u) {
        s_log("GET_LED_MODE_INFO failed: data section too small");
        return false;
    }
    if (led == 0xAAu) {
        s_log("GET_LED_MODE_INFO failed: LED value 0xAA is reserved");
        return false;
    }
    uint8_t ora_led;
    ora_led_state_t st;
    if (!led_resolve(led, &ora_led, &st)) {
        s_log("GET_LED_MODE_INFO failed: no such LED %u", (unsigned)led);
        return false;
    }
    uint8_t ora_mode;
    if (!led_ora_mode(ora_led, mode, &ora_mode)) {
        s_log("GET_LED_MODE_INFO failed: LED %u does not support mode 0x%02X",
              (unsigned)led, (unsigned)mode);
        return false;
    }

    uint8_t resp[8];
    zero_bytes(resp, sizeof(resp));
    if (led_min_period_ms(ora_mode) != 0u) {
        // Every mode the firmware gives a floor to is one that repeats, and
        // only a repeating mode takes a period.
        resp[0] = LED_MODE_FLAG_PERIOD;
        resp[1] = led_min_period_units(ora_mode);
    }

    data_write(s_state.active_slot, 0u, resp, sizeof(resp));
    s_log("GET_LED_MODE_INFO: led=%u mode=0x%02X flags=%u min=%u", (unsigned)led,
          (unsigned)mode, (unsigned)resp[0], (unsigned)resp[1]);
    return true;
}

// The device times the hold, and this command does not wait for it - unlike
// SET_AUX, which spins.  The firmware's engine runs the hold from a timer and
// puts back what the LED was doing when it ends, which is what lets the hold
// outlive the session.
static bool exec_set_led(void) {
    uint8_t mode       = ring_read_byte();
    uint8_t red        = ring_read_byte();
    uint8_t green      = ring_read_byte();
    uint8_t blue       = ring_read_byte();
    uint8_t brightness = ring_read_byte();
    uint8_t period     = ring_read_byte();
    uint8_t hold       = ring_read_byte();
    uint8_t led        = ring_read_byte();

    if (led == 0xAAu) {
        s_log("SET_LED failed: LED value 0xAA is reserved");
        return false;
    }
    uint8_t ora_led;
    ora_led_state_t st;  // led_resolve's working room; SET_LED reads none of it
    if (!led_resolve(led, &ora_led, &st)) {
        s_log("SET_LED failed: no such LED %u", (unsigned)led);
        return false;
    }
    uint8_t ora_mode;
    if (!led_ora_mode(ora_led, mode, &ora_mode)) {
        s_log("SET_LED failed: LED %u does not support mode 0x%02X",
              (unsigned)led, (unsigned)mode);
        return false;
    }
    if (brightness > 100u) {
        s_log("SET_LED failed: brightness %u above 100", (unsigned)brightness);
        return false;
    }
    // Period and hold need no range check: LED_MAX_PERIOD and LED_MAX_HOLD are
    // the byte's whole range, so no value a host can send exceeds them.

    ora_led_set_fn_t set = s_lookup(ORA_ID_LED_SET);
    ora_led_request_t req;
    zero_bytes((uint8_t *)&req, (uint8_t)sizeof(req));
    req.size       = (uint8_t)sizeof(req);
    req.led        = ora_led;
    req.mode       = ora_mode;
    req.brightness = brightness;
    req.red        = red;
    req.green      = green;
    req.blue       = blue;
    req.period_ms  = (uint16_t)((uint16_t)period * 100u);
    req.hold_ms    = (uint32_t)hold * 100u;

    if (set(&req) != ORA_RESULT_OK) {
        s_log("SET_LED failed: engine refused LED %u mode 0x%02X period %u",
              (unsigned)led, (unsigned)mode, (unsigned)period);
        return false;
    }
    s_log("SET_LED: led=%u mode=0x%02X hold=%u", (unsigned)led,
          (unsigned)mode, (unsigned)hold);
    return true;
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

// Number of argument bytes a command declares.
//
// Needed so the device can take a command's arguments off the wire even when
// it will not act on the command.  The Command Mode Constraint makes that
// obligatory: the device "will continue to consume address reads as argument
// bytes of the current partially-received command until that command's
// expected argument count is satisfied".  A command refused because it is not
// valid in the current mode must still satisfy that count, or the host's next
// bytes are read as a command frame and the session desyncs.
//
// Only the groups that can be refused for being in the wrong mode are listed;
// everything else returns 0, which is also right for an unknown command, whose
// argument count the device cannot know.
static uint8_t cmd_arg_count(uint8_t group, uint8_t cmd) {
    if (group == GRP_READ) {
        switch (cmd) {
            case CMD_GET_FLASH_SLOT_INFO: return 1u;
            case CMD_SLOT_PEEK:           return 5u;
            default:                      return 0u;
        }
    }
    if (group == GRP_NV_STORAGE) {
        switch (cmd) {
            case CMD_NV_PEEK:             return 3u;
            case CMD_NV_POKE_BEGIN:       return 1u;
            case CMD_NV_POKE:             return 3u;
            case CMD_NV_POKE_COMMIT_BYTE: return 4u;
            default:                      return 0u;
        }
    }
    if (group == GRP_PIPES) {
        switch (cmd) {
            case CMD_GET_PIPE_INFO:       return 1u;
            case CMD_PIPE_WRITE:          return 6u;
            default:                      return 0u;
        }
    }
    if (group == GRP_AUX) {
        switch (cmd) {
            case CMD_GET_AUX_GROUP_INFO:  return 1u;
            case CMD_GET_AUX_PIN_INFO:    return 2u;
            case CMD_SET_AUX:             return 5u;
            case CMD_SET_AUX_AND_EXIT:    return 5u;
            case CMD_SET_AUX_SWITCH_EXIT: return 7u;
            default:                      return 0u;
        }
    }
    if (group == GRP_LED) {
        switch (cmd) {
            case CMD_GET_LED_INFO:        return 1u;
            case CMD_GET_LED_MODE_INFO:   return 2u;
            case CMD_SET_LED:             return 8u;
            default:                      return 0u;
        }
    }
    return 0u;
}

// Read and throw away a command's argument bytes.
static void discard_args(uint8_t count) {
    while (count--) {
        (void)ring_read_byte();
    }
}

// True for the commands the specification requires to leave the response
// header untouched: RBCP_RESET ("there is never any response from this
// command"), EXIT_CMD_RESP_SILENT, SWITCH_AND_EXIT, LOAD_AND_EXIT,
// EXIT_CMD_RESP_RESTORE, SET_AUX_AND_EXIT and SET_AUX_SWITCH_EXIT (all
// "without updating the response header").
//
// LOAD_AND_EXIT and EXIT_CMD_RESP_RESTORE are the two that need it most: each
// exists to leave the region byte-perfect, and a header write after the
// command had run would be the very damage they undo.
//
// Decided from GROUP and CMD alone, before the command runs.  Every other
// command needs cmd_begin to run *before* it is processed — that ordering is
// what stops a host observing a false complete — so by the time the command
// itself could report being silent, the header has already been written.
static bool cmd_is_silent(uint8_t group, uint8_t cmd) {
    if (group == GRP_RESET) {
        return cmd == CMD_RBCP_RESET;
    }
    if (group == GRP_CONTROL) {
        return (cmd == CMD_EXIT_CMD_RESP_SILENT) ||
               (cmd == CMD_SWITCH_AND_EXIT) ||
               (cmd == CMD_LOAD_AND_EXIT) ||
               (cmd == CMD_EXIT_CMD_RESP_RESTORE);
    }
    if (group == GRP_AUX) {
        return (cmd == CMD_SET_AUX_AND_EXIT) || (cmd == CMD_SET_AUX_SWITCH_EXIT);
    }
    return false;
}

// Dispatch one command.  Reads argument bytes from the ring buffer.
// Uses s_state.active_slot for all back-channel writes.
//
// Returns the ok/fail result for cmd_end.
static bool dispatch(
    uint8_t group,
    uint8_t cmd
) {
    bool ok = false;

    switch (group) {
        case GRP_CONTROL:
            switch (cmd) {
                case CMD_NOP:
                    ok = exec_nop();
                    break;
                case CMD_ENTER_CMD_RESP:
                    ok = exec_enter_cmd_resp();
                    break;
                case CMD_EXIT_CMD_RESP_ACK:
                    // The device completes the full command processing sequence
                    // (including progress=complete) before exiting.  cmd_end is
                    // called by the caller; s_state.active is cleared here so that
                    // run_command knows to terminate the inner loop.
                    s_state.active = false;
                    ok = true;
                    break;
                case CMD_EXIT_CMD_RESP_SILENT:
                    s_state.active = false;
                    ok = true;
                    break;
                case CMD_SWITCH_AND_EXIT:
                    // Activate specified slot then exit silently.
                    // active_slot cache is NOT updated: this command exits with no
                    // back-channel writes to the new slot, so the cached value is
                    // irrelevant for the remainder of the session.
                    ok             = exec_switch_slot();
                    s_state.active = false;
                    break;
                case CMD_LOAD_AND_EXIT:
                    // Reload then exit silently.  active_slot is not updated:
                    // the slot being served does not change, and no
                    // back-channel write follows this command.
                    ok = exec_load_and_exit();
                    break;
                case CMD_EXIT_CMD_RESP_RESTORE:
                    ok = exec_exit_cmd_resp_restore();
                    break;
                default:
                    // Unknown command: no args consumed.  This will desync the
                    // session; the best the host can do is re-knock.
                    ok = false;
                    break;
            }
            break;

        case GRP_READ:
            // "All commands in this group are valid in command-response mode
            // only."  Consume the frame, then discard it.
            if (!s_state.active) {
                discard_args(cmd_arg_count(group, cmd));
                ok = false;
                break;
            }
            switch (cmd) {
                case CMD_GET_FLASH_FLASH_SLOT_COUNT:
                    ok = exec_get_flash_slot_count();
                    break;
                case CMD_GET_FLASH_SLOT_INFO:
                    ok = exec_get_flash_slot_info(s_state.active_slot);
                    break;
                case CMD_GET_FLASH_SLOT_INFO_ALL:
                    ok = exec_get_flash_slot_info_all(s_state.active_slot);
                    break;
                case CMD_GET_RAM_SLOT_INFO_ALL:
                    ok = exec_get_ram_slot_info_all(s_state.active_slot);
                    break;
                case CMD_GET_DEVICE_TYPE:
                    ok = exec_get_device_type();
                    break;
                case CMD_GET_DEVICE_VERSION:
                    ok = exec_get_device_version();
                    break;
                case CMD_GET_PROTOCOL_VERSION:
                    ok = exec_get_protocol_version();
                    break;
                case CMD_SLOT_PEEK:
                    ok = exec_slot_peek();
                    break;
                case CMD_GET_BOOT_SLOT_INFO:
                    ok = exec_get_boot_slot_info();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_MODIFY:
            switch (cmd) {
                case CMD_SLOT_POKE:
                    ok = exec_slot_poke();
                    break;
                case CMD_SWITCH_SLOT:
                    ok = exec_switch_slot();
                    if (ok) {
                        // Update the cached active slot so subsequent back-channel
                        // writes in this session target the new slot's SRAM region.
                        // get_active_ram_slot() should not fail here since
                        // set_active_ram_slot() just succeeded.
                        uint8_t new_slot;
                        if (s_get_active_ram_slot(&new_slot) == ORA_RESULT_OK) {
                            s_state.active_slot = new_slot;
                        } else {
                            s_log("RBCP: active slot desync after SWITCH_SLOT");
                        }
                    }
                    break;
                case CMD_LOAD_SLOT:
                    ok = exec_load_slot();
                    break;
                case CMD_SLOT_POKE_ALL_BYTE:
                    ok = exec_slot_poke_all_byte();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_NV_STORAGE:
            // As GRP_READ: command-response mode only, and the arguments are
            // consumed before the command is discarded.
            if (!s_state.active) {
                discard_args(cmd_arg_count(group, cmd));
                ok = false;
                break;
            }
            switch (cmd) {
                case CMD_GET_NV_CAPABILITY:
                    ok = exec_get_nv_capability();
                    break;
                case CMD_NV_PEEK:
                    ok = exec_nv_peek();
                    break;
                case CMD_NV_POKE_BEGIN:
                    ok = exec_nv_poke_begin();
                    break;
                case CMD_NV_POKE:
                    ok = exec_nv_poke();
                    break;
                case CMD_NV_POKE_COMMIT:
                    ok = exec_nv_poke_commit();
                    break;
                case CMD_NV_POKE_DISCARD:
                    ok = exec_nv_poke_discard();
                    break;
                case CMD_NV_POKE_COMMIT_BYTE:
                    ok = exec_nv_poke_commit_byte();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_PIPES:
            // "All commands in this group are valid in command-response mode
            // only."  Consume the frame, then discard it.
            if (!s_state.active) {
                discard_args(cmd_arg_count(group, cmd));
                ok = false;
                break;
            }
            switch (cmd) {
                case CMD_GET_PIPE_CAPABILITY:
                    ok = exec_get_pipe_capability();
                    break;
                case CMD_GET_PIPE_INFO:
                    ok = exec_get_pipe_info();
                    break;
                case CMD_PIPE_WRITE:
                    ok = exec_pipe_write();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_AUX:
            // "All commands in this group are valid in command-response mode
            // only."  Consume the frame, then discard it.
            if (!s_state.active) {
                discard_args(cmd_arg_count(group, cmd));
                ok = false;
                break;
            }
            switch (cmd) {
                case CMD_GET_AUX_CAPABILITY:
                    ok = exec_get_aux_capability();
                    break;
                case CMD_GET_AUX_GROUP_INFO:
                    ok = exec_get_aux_group_info();
                    break;
                case CMD_GET_AUX_PIN_INFO:
                    ok = exec_get_aux_pin_info();
                    break;
                case CMD_SET_AUX:
                    ok = exec_set_aux();
                    break;
                case CMD_SET_AUX_AND_EXIT:
                    ok = exec_set_aux_and_exit();
                    break;
                case CMD_SET_AUX_SWITCH_EXIT:
                    ok = exec_set_aux_switch_exit();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_LED:
            // "All commands in this group are valid in command-response mode
            // only."  Consume the frame, then discard it.
            if (!s_state.active) {
                discard_args(cmd_arg_count(group, cmd));
                ok = false;
                break;
            }
            switch (cmd) {
                case CMD_GET_LED_CAPABILITY:
                    ok = exec_get_led_capability();
                    break;
                case CMD_GET_LED_INFO:
                    ok = exec_get_led_info();
                    break;
                case CMD_GET_LED_MODE_INFO:
                    ok = exec_get_led_mode_info();
                    break;
                case CMD_SET_LED:
                    ok = exec_set_led();
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        case GRP_RESET:
            switch (cmd) {
                case CMD_RBCP_RESET:
                    init_rbcp(false);
                    s_state.active = false; // Unecessary as init_rbcp sets this.
                    s_log("RBCP_RESET: state reset, active_slot=%u", (unsigned)s_state.active_slot);
                    ok = true;
                    break;
                default:
                    ok = false;
                    break;
            }
            break;

        default:
            ok = false;
            break;
    }

    if (!ok) {
        if (group == GRP_PIPES) {
            // A failure here is ordinary traffic, most often a full channel,
            // and s_log writes the same channel the host is reading - so this
            // report would land in the middle of the host's own output.
            s_debug_log("CMD g=0x%02x c=0x%02x failed", group, cmd);
        } else {
            s_log("CMD g=0x%02x c=0x%02x failed", group, cmd);
        }
    }

    return ok;
}

// ---------------------------------------------------------------------------
// Session execution
// ---------------------------------------------------------------------------

// Execute one command.  Handles the full command processing sequence:
// cmd_begin -> dispatch -> cmd_end, in accordance with the RBCP spec.
//
// All back-channel writes use s_state.active_slot, which is populated at
// ENTER_CMD_RESP and updated by CMD_SWITCH_SLOT.
//
// Special case: ENTER_CMD_RESP arrives while the device is in Command mode
// (was_active=false) but transitions to Command-Response mode.  In this case
// cmd_begin and cmd_end are called after the transition so that the host can
// poll the back-channel to confirm entry.
//
// Returns true if Command-Response mode is active after this command, meaning
// the caller should continue reading commands from the ring buffer rather than
// waiting for the next knock.
static bool run_command(uint8_t group, uint8_t cmd) {
    bool was_active = s_state.active;
    bool silent     = cmd_is_silent(group, cmd);

    if (was_active && !silent) {
        cmd_begin(s_state.active_slot, group, cmd);
    }

    bool ok = dispatch(group, cmd);

    bool now_active = s_state.active;

    if (was_active && !silent) {
        // Normal Command-Response mode: complete the processing sequence.
        // s_state.active_slot may have been updated by CMD_SWITCH_SLOT
        // inside dispatch, so use the current cached value.
        cmd_end(s_state.active_slot, ok);
    } else if (!was_active && now_active) {
        // ENTER_CMD_RESP: the device has just transitioned into
        // Command-Response mode.  s_state.active_slot is now valid.
        // Write the initial response so the host can confirm entry by
        // polling the token and progress fields.
        cmd_begin(s_state.active_slot, group, cmd);
        cmd_end(s_state.active_slot, ok);
    }
    
    if (was_active && !now_active) {
        nv_discard_impl();
    }

    // Command mode (!was_active && !now_active): no back-channel, nothing to write.
    // A silent command (see cmd_is_silent): no header update at all.

    return now_active;
}

// ---------------------------------------------------------------------------
// Plugin setup
// ---------------------------------------------------------------------------

__attribute__((noinline)) static void rbcp_setup(
    ora_lookup_fn_t ora_lookup_fn,
    ora_knock_t *knock
) {
    // Retrieve API function pointers
    s_lookup               = ora_lookup_fn;
    s_log                  = ora_lookup_fn(ORA_ID_LOG);
    s_demangle             = ora_lookup_fn(ORA_ID_DEMANGLE_OBSERVED_ADDR);
    s_reprogram            = ora_lookup_fn(ORA_ID_REPROGRAM_RAM_ROM_SLOT);
    s_get_ram_slot_info    = ora_lookup_fn(ORA_ID_GET_RAM_SLOT_INFO);
    s_get_ram_slot_count   = ora_lookup_fn(ORA_ID_GET_RAM_SLOT_COUNT);
    s_get_active_ram_slot  = ora_lookup_fn(ORA_ID_GET_ACTIVE_RAM_SLOT);
    s_set_active_ram_slot  = ora_lookup_fn(ORA_ID_SET_ACTIVE_RAM_SLOT);
    s_get_flash_slot_count = ora_lookup_fn(ORA_ID_GET_FLASH_SLOT_COUNT);
    s_get_flash_slot_info  = ora_lookup_fn(ORA_ID_GET_FLASH_SLOT_INFO);
    s_copy_flash_to_ram    = ora_lookup_fn(ORA_ID_COPY_FLASH_SLOT_TO_RAM_SLOT);
    s_get_chip_size        = ora_lookup_fn(ORA_ID_GET_CHIP_SIZE_FROM_TYPE);
    s_get_device_version   = ora_lookup_fn(ORA_ID_GET_DEVICE_VERSION);
    s_map_addr_to_phys     = ora_lookup_fn(ORA_ID_MAP_ADDR_TO_PHYS);
    s_map_data_to_phys     = ora_lookup_fn(ORA_ID_MAP_DATA_TO_PHYS);
    s_demangle_data        = ora_lookup_fn(ORA_ID_DEMANGLE_DATA);
    s_debug_log            = ora_lookup_fn(ORA_ID_DEBUG_LOG);

    // The log channel family arrived in firmware 0.7.2, later than this
    // plugin's min_fw_version, so these three may be NULL.  Nothing here checks
    // for that - pipe_count() does, once, and reports no pipes.
    s_log_open_write       = ora_lookup_fn(ORA_ID_LOG_OPEN_WRITE);
    s_log_write            = ora_lookup_fn(ORA_ID_LOG_WRITE);
    s_log_query            = ora_lookup_fn(ORA_ID_LOG_QUERY);

    // As with the log channels, these may be NULL - the Auxiliary I/O group
    // reports less rather than the plugin refusing to run.
    s_gpio_set             = ora_lookup_fn(ORA_ID_GPIO_SET);
    s_gpio_query           = ora_lookup_fn(ORA_ID_GPIO_QUERY);
    s_get_metadata_uint    = ora_lookup_fn(ORA_ID_GET_METADATA_UINT);
    s_get_metadata_uint_at = ora_lookup_fn(ORA_ID_GET_METADATA_UINT_AT);
    s_uptime_ms            = ora_lookup_fn(ORA_ID_GET_PLUGIN_UPTIME_MS);

    ora_start_address_monitor_fn_t start_address_monitor =
        ora_lookup_fn(ORA_ID_START_ADDRESS_MONITOR);
    ora_init_knock_fn_t init_knock = ora_lookup_fn(ORA_ID_INIT_KNOCK);
    ora_setup_address_monitor_fn_t              setup_address_monitor =
        ora_lookup_fn(ORA_ID_SETUP_ADDRESS_MONITOR);
    ora_get_address_monitor_ring_write_pos_fn_t get_write_pos =
        ora_lookup_fn(ORA_ID_GET_ADDRESS_MONITOR_RING_WRITE_POS);

    s_log("RBCP plugin starting");

    init_rbcp(true);
    init_boot_slots();

    // The observed-address geometry is fixed for the served ROM type, so read
    // the unobserved-LSB count once here and cache the value rather than the
    // accessor pointer.
    ora_get_unobserved_addr_bits_fn_t get_unobserved_addr_bits =
        ora_lookup_fn(ORA_ID_GET_UNOBSERVED_ADDR_BITS);
    s_unobserved_addr_bits = 0;
    if (get_unobserved_addr_bits(&s_unobserved_addr_bits) != ORA_RESULT_OK) {
        s_log("RBCP: get_unobserved_addr_bits failed; assuming 0 (fully observed)");
        s_unobserved_addr_bits = 0;
    }

    // Set up address monitor in control mode so the plugin can modify the
    // ROM image being served (required for back-channel writes).
    ora_result_t rc = setup_address_monitor(
        ring_buf,
        RING_ENTRIES_LOG2,
        ORA_MONITOR_MODE_CONTROL,
        RING_DATA_SIZE,
        NULL
    );
    if (rc != ORA_RESULT_OK) {
        s_log("RBCP: address monitor setup failed %d", rc);
        return;
    }

    // Retrieve and cache the DMA write pointer location.  This is called once
    // at init time; the returned pointer is valid for the lifetime of the monitor.
    s_write_pos_ptr = get_write_pos();
    if (s_write_pos_ptr == NULL) {
        s_log("RBCP: failed to get ring buffer write position");
        return;
    }

    // Initialize knock detection with the pre-computed sequence.
    if (init_knock(
            s_knock_seq,
            KNOCK_LEN,
            8u,
            RING_DATA_SIZE,
            knock
        ) != ORA_RESULT_OK) {
        s_log("RBCP: knock init failed");
        return;
    }

    // Begin capturing address bus activity.
    start_address_monitor();
}

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

// Place the plugin's initialised data and clear its zeroed data.
//
// A plugin owns its own RAM sections (see firmware/ora/plugin.h), and the
// static RAM it is handed holds whatever the firmware last left there - so a
// static relying on zero initialisation starts with garbage in it.
//
// A host test build's data belongs to the host process, and these symbols do
// not exist there, so the body is compiled out rather than skipped at run time.
static void init_data_bss(void) {
#if !defined(ORA_HOST_TEST)
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
#endif // !ORA_HOST_TEST
}

void rbcp_main(
    ora_lookup_fn_t         ora_lookup_fn,
    ora_plugin_type_t       plugin_type,
    const ora_entry_args_t *entry_args
) {
    // Before anything reads a static.
    init_data_bss();

    (void)plugin_type;
    (void)entry_args;

    // Pre-compute knock sequence match values before starting the monitor.
    ORA_KNOCK_DECLARE(knock, KNOCK_LEN);

    rbcp_setup(ora_lookup_fn, knock);

    ora_wait_for_knock_fn_t wait_for_knock = ora_lookup_fn(ORA_ID_WAIT_FOR_KNOCK);

    s_log("RBCP: ready, awaiting knock");

    // Main loop — each outer iteration begins with a knock.
    for (;;) {
        // Collect GROUP and CMD as the two payload bytes immediately following
        // the knock.  wait_for_knock blocks until both are captured in the
        // ring buffer.  Payload entries are raw physical GPIO captures.
        //
        // We always call wait_for_knock with start_pos = NULL so that method
        // starts listening from the current write position of the DMA channel.
        // This means that some items may be discarded, but is safer from
        // assuming we can restart from wherever we just read from - because
        // enough time may have passed to mean that the buffer has wrapped,
        // which could cause inconsistent data to be read.
        volatile uint32_t *next_read;
        uint32_t preamble[2];
        if (wait_for_knock(knock, ring_buf, RING_ENTRIES_LOG2,
                           ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS,
                           preamble, 2u, NULL, &next_read) != ORA_RESULT_OK) {
            continue;
        }

        // WARNING.  There must be NO logging between detecting a knock and
        // consuming any argument bytes (in cmd/resp mode) or at all (in cmd 
        // mode) as otherwise the ring buffer may fill with log entries and
        // overwrite some before the code gets a chance to read them.
        //
        // That means no logging here until after the command itself reads
        // any further argument bytes.

        RING_BUF_UPDATE_READ_INDEX(next_read);

        // Demangle GROUP and CMD from physical captures to logical addresses.
        // ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS guarantees payload entries are
        // CS-active, so check_control_pins=0 is correct here.
        uint32_t logical;
        if (s_demangle(preamble[0], &logical, 0) != ORA_RESULT_OK) continue;
        uint8_t group = (uint8_t)(logical & 0xFFu);

        if (s_demangle(preamble[1], &logical, 0) != ORA_RESULT_OK) continue;
        uint8_t cmd = (uint8_t)(logical & 0xFFu);

        // Execute the command.  In command mode run_command returns false, so
        // this executes once.  In command-response mode (until it exits) it
        // returns true, so we loop, reading the next command directly from the
        // ring buffer without waiting for another knock.
        while (run_command(group, cmd)) {
            group = ring_read_byte();
            cmd   = ring_read_byte();
            // run_command (via cmd_end, via hdr_write) discards any reads
            // until the point it signals completion, unless the command was
            // a silent exit, in which case it doesn't - but the
            // wait_for_knock handles that.
        }

        // Command-Response mode exited (or Command-mode session ended).
        // Return to outer loop to await the next knock.
        s_log("RBCP: session ended");
    }
}