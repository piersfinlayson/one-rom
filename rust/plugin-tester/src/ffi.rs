// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Declarations for the host shim (`csrc/host_shim.c`).
//!
//! The shim supplies the plugin with the parts of its device environment that
//! do not exist in a host process: the symbols its linker script would define,
//! the ORA host-test seams, and an entry point.

unsafe extern "C" {
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
