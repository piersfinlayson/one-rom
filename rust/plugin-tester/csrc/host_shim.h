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
/// The commit path is a fixed sequence — connect, exit XIP, erase, restore
/// XIP, program — whose ordering matters: a program issued before XIP came
/// back, or an erase issued while it was still active, would be a real defect
/// on a device and is invisible in the resulting bytes alone.  So the calls
/// are counted and their arguments recorded, and the two `bad_*` flags mark a
/// call the model refused to honour because it arrived in the wrong state or
/// named the wrong range.
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
    /// commit sequence is fixed — connect, exit XIP, erase, restore XIP,
    /// program — and a device that programmed before XIP came back would
    /// produce the right bytes here while failing on hardware, so the order is
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
    /// Non-zero if a program arrived with XIP inactive, or named a range
    /// outside the modelled region.
    uint32_t bad_program;
} ora_host_test_flash_log_t;

/// The log of what the last commit asked for.  Valid until the next reset.
const ora_host_test_flash_log_t *ora_host_test_flash_log(void);

/// Clear the log.  The harness does this before every scenario.
void ora_host_test_reset_flash_log(void);

#endif // ORA_HOST_SHIM_H
