// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Declarations for the host shim (`csrc/host_shim.c`).
//!
//! The shim supplies the plugin with the parts of its device environment that
//! do not exist in a host process: the symbols its linker script would define,
//! the ORA host-test seams, and an entry point.

/// What the plugin asked the flash hardware to do, as the shim's model
/// recorded it.
///
/// Mirrors `ora_host_test_flash_log_t` in `csrc/host_shim.h`, field for field;
/// the two must be kept in step.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlashLog {
    pub connect_calls: u32,
    pub exit_xip_calls: u32,
    pub erase_calls: u32,
    pub flush_calls: u32,
    pub select_xip_calls: u32,
    pub program_calls: u32,

    pub erase_offs: u32,
    pub erase_count: u32,
    pub erase_block_size: u32,
    pub erase_block_cmd: u32,

    pub program_offs: u32,
    pub program_count: u32,

    pub select_xip_mode: u32,
    pub select_xip_clkdiv: u32,

    /// Address the plugin formed its staged-routine pointer from.
    pub staged_fn_addr: u32,

    /// Order in which the calls arrived: each is the value of a counter that
    /// increments on every flash call, or 0 if the call never came.
    pub connect_seq: u32,
    pub exit_xip_seq: u32,
    pub erase_seq: u32,
    pub select_xip_seq: u32,
    pub program_seq: u32,
    pub flush_seq: u32,

    /// Non-zero if XIP is currently active.
    pub xip_active: u32,
    /// Non-zero if an erase arrived with XIP active, or named a range outside
    /// the modelled region.
    pub bad_erase: u32,
    /// Non-zero if a program arrived with XIP active, or named a range
    /// outside the modelled region.
    pub bad_program: u32,
    /// Non-zero if any call in the sequence arrived with interrupts unmasked.
    pub bad_unmasked: u32,
}

/// Mirrors `ORA_HOST_TEST_SRAM_LOG_MAX` in `csrc/host_shim.h`.
pub const SRAM_LOG_MAX: usize = 2048;

/// Mirrors `ORA_HOST_TEST_SRAM_WATCH_MAX` in `csrc/host_shim.h`.
pub const SRAM_WATCH_MAX: usize = 64;

/// One byte a command wrote into a RAM slot.  Physical address, physical data.
///
/// Mirrors `ora_host_test_sram_write_t` in `csrc/host_shim.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SramWrite {
    pub addr: u32,
    pub val: u8,
}

/// What the device wrote since the log was reset, in order.
///
/// Mirrors `ora_host_test_sram_log_t` in `csrc/host_shim.h`, field for field.
/// The two must be kept in step.
#[repr(C)]
pub struct SramLog {
    pub count: u32,
    pub overflowed: u32,
    pub writes: [SramWrite; SRAM_LOG_MAX],
}

unsafe extern "C" {
    /// The shim's record of the last commit's flash calls.
    pub fn ora_host_test_flash_log() -> *const FlashLog;

    /// Clear that record.  The harness does this before every scenario.
    pub fn ora_host_test_reset_flash_log();

    /// The shim's record of what the device wrote into its RAM slots.  The
    /// only place write ordering is observable — the plugin runs a whole
    /// command between two yields.
    pub fn ora_host_test_sram_log() -> *const SramLog;

    /// Clear that record and start recording every write.
    pub fn ora_host_test_reset_sram_log();

    /// Clear that record and record only writes to `count` addresses at
    /// `addrs`.  For a command writing more of a slot than the log holds.
    pub fn ora_host_test_reset_sram_log_watching(addrs: *const u32, count: u32);

    /// The XIP clock divisor the shim answers `ORA_XIP_CLKDIV` with.
    pub fn ora_host_test_xip_clkdiv() -> u8;

    /// Install the hook `ORA_TEST_YIELD` calls.  Must be installed on the
    /// thread that runs the plugin — see [`crate::harness`].
    pub fn ora_host_test_set_yield_hook(hook: Option<unsafe extern "C" fn()>);

    /// Point the plugin's ring buffer at emulated SRAM.  Must be called before
    /// the plugin starts, and the target must be aligned to the ring's size.
    pub fn ora_host_test_set_ring_buf(p: *mut u32);

    /// The shim's stand-in for the plugin's reserved NV flash sector.
    pub fn ora_host_test_nv_storage() -> *mut u8;

    /// Size of the region [`ora_host_test_nv_storage`] points at.
    pub fn ora_host_test_nv_storage_size() -> u32;

    /// The plugin's own SRAM seam: what ORA_SRAM_PTR resolves to.
    pub fn ora_host_test_sram_ptr(addr: u32) -> *mut core::ffi::c_void;

    /// Make the plugin's lookup answer NULL for each of these API identifiers,
    /// so a scenario can exercise what the plugin does on firmware that
    /// predates a call.  Must be set before the plugin starts, which is when
    /// it resolves its pointers.  An empty slice restores the full API.
    pub fn ora_host_test_withhold_api(ids: *const u32, count: u32);

    /// Enter the plugin.  Never returns.
    pub fn ora_host_test_run_plugin();

    /// The plugin header's version, packed `major:minor:patch:build`.
    pub fn ora_host_test_plugin_version() -> u32;
}
