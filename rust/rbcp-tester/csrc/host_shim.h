// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Types the host shim shares with the Rust harness.
//
// `ora_host_test_flash_log_t` is mirrored by `ffi::FlashLog` in
// `src/ffi.rs`; the two must be kept in step, field for field.

#if !defined(ORA_HOST_SHIM_H)
#define ORA_HOST_SHIM_H

#include <stdint.h>

/// Pack a bootrom function's two-character code as the lookup expects it:
/// first character in the low byte.
#define ORA_SHIM_ROM_CODE(a, b) \
    ((uint32_t)((uint32_t)(uint8_t)(b) << 8) | (uint32_t)(uint8_t)(a))

/// What the plugin asked the flash hardware to do.
///
/// The commit path is a fixed sequence — connect, exit XIP, erase, program,
/// restore XIP — whose ordering matters: the bootrom needs the flash in serial
/// command mode for both the erase and the program, so either one issued while
/// XIP was still active would be a real defect on a device and is invisible in
/// the resulting bytes alone.  So the calls are counted and their arguments
/// recorded, and the two `bad_*` flags mark a call the model refused to honour
/// because it arrived in the wrong state or named the wrong range.
typedef struct {
    uint32_t connect_calls;
    uint32_t exit_xip_calls;
    uint32_t erase_calls;
    uint32_t flush_calls;
    uint32_t select_xip_calls;
    uint32_t program_calls;

    uint32_t erase_offs;
    uint32_t erase_count;
    uint32_t erase_block_size;
    uint32_t erase_block_cmd;

    uint32_t program_offs;
    uint32_t program_count;

    uint32_t select_xip_mode;
    uint32_t select_xip_clkdiv;

    /// Address the plugin formed its staged-routine pointer from.
    uint32_t staged_fn_addr;

    /// Order in which the calls arrived: each is the value of a counter that
    /// increments on every flash call, or 0 if the call never came.  The
    /// commit sequence is fixed — connect, exit XIP, erase, program, restore
    /// XIP — and a device that programmed after XIP came back would produce
    /// the right bytes here while failing on hardware, so the order is
    /// asserted rather than inferred from the outcome.
    uint32_t connect_seq;
    uint32_t exit_xip_seq;
    uint32_t erase_seq;
    uint32_t select_xip_seq;
    uint32_t program_seq;
    uint32_t flush_seq;

    /// Non-zero if XIP is currently active.  A device runs from XIP, so this
    /// starts set and only the erase sequence clears it.
    uint32_t xip_active;
    /// Non-zero if an erase arrived with XIP active, or named a range outside
    /// the modelled region.
    uint32_t bad_erase;
    /// Non-zero if a program arrived with XIP active, or named a range
    /// outside the modelled region.
    uint32_t bad_program;
    /// Non-zero if any call in the sequence arrived with interrupts unmasked.
    /// On a device the handler an interrupt would run lives in flash, which is
    /// unreadable from the exit out of XIP until the restore back into it.
    uint32_t bad_unmasked;
} ora_host_test_flash_log_t;

/// The log of what the last commit asked for.  Valid until the next reset.
const ora_host_test_flash_log_t *ora_host_test_flash_log(void);

/// Clear the log.  The harness does this before every scenario.
void ora_host_test_reset_flash_log(void);

/// One byte a command wrote into a RAM slot.  Physical address, physical data.
typedef struct {
    uint32_t addr;
    uint8_t  val;
} ora_host_test_sram_write_t;

/// Writes the log holds.  Several commands at the largest region the tester
/// configures.
#define ORA_HOST_TEST_SRAM_LOG_MAX 2048

/// What the device wrote since the last reset, in order.  Where `overflowed`
/// is set the entries are the first ones only, and a write missing from them
/// does not mean the device did not make it.
typedef struct {
    uint32_t count;
    uint32_t overflowed;
    ora_host_test_sram_write_t writes[ORA_HOST_TEST_SRAM_LOG_MAX];
} ora_host_test_sram_log_t;

const ora_host_test_sram_log_t *ora_host_test_sram_log(void);

/// Clear the log and start recording every write.
void ora_host_test_reset_sram_log(void);

/// Addresses the log will watch at once.
#define ORA_HOST_TEST_SRAM_WATCH_MAX 64

/// Clear the log and record only writes to these device SRAM addresses.
///
/// For a command that writes more of a slot than the log holds - a copy over a
/// whole slot does - where the scenario is about particular bytes rather than
/// everything the device did.  A `count` above ORA_HOST_TEST_SRAM_WATCH_MAX is
/// clamped, so the caller checks it first.
void ora_host_test_reset_sram_log_watching(const uint32_t *addrs, uint32_t count);

/// Most API identifiers the harness will withhold from the plugin at once.
#define ORA_HOST_TEST_WITHHOLD_MAX 8

/// Make the plugin's lookup answer NULL for each of `count` identifiers.
///
/// A plugin declares a minimum firmware version and then degrades where a
/// later call is missing, and those branches are otherwise unreachable here:
/// the emulator implements the whole API, so every lookup succeeds.  Applies
/// from the next `ora_host_test_run_plugin`, since a plugin resolves its
/// pointers once at entry.  A `count` of zero restores the full API.
void ora_host_test_withhold_api(const uint32_t *ids, uint32_t count);

#endif // ORA_HOST_SHIM_H
