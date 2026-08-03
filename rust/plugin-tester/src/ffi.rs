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
    /// Non-zero if a program arrived with XIP inactive, or named a range
    /// outside the modelled region.
    pub bad_program: u32,
}

unsafe extern "C" {
    /// The shim's record of the last commit's flash calls.
    pub fn ora_host_test_flash_log() -> *const FlashLog;

    /// Clear that record.  The harness does this before every scenario.
    pub fn ora_host_test_reset_flash_log();

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

    /// Enter the plugin.  Never returns.
    pub fn ora_host_test_run_plugin();

    /// The plugin header's version, packed `major:minor:patch:build`.
    pub fn ora_host_test_plugin_version() -> u32;
}
