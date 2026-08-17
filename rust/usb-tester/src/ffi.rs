// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Declarations for the host shim (`csrc/usb_shim.c`).
//!
//! The shim supplies the plugin with the parts of its device environment that
//! do not exist in a host process: tinyusb, picoboot, and an entry point.
//! These are the controls it offers a scenario on top of that.
//!
//! The three entry points the harness itself needs —
//! `ora_host_test_run_plugin`, `ora_host_test_set_yield_hook` and
//! `ora_host_test_plugin_version` — are declared by `onerom-plugin-tester` and
//! resolved against this crate's shim at the final link.

unsafe extern "C" {
    /// How much the modelled CDC endpoint holds before it is full.  Room comes
    /// back as the harness takes bytes.
    pub fn usb_host_test_set_tx_capacity(capacity: u32);

    /// Whether the bus can carry data, as `tud_cdc_n_connected` reports it.
    pub fn usb_host_test_set_connected(connected: u8);

    /// Raise or drop DTR, through the plugin's own line-state callback.
    pub fn usb_host_test_set_dtr(dtr: u8);

    /// Copy out what has been flushed to the endpoint, and drop what was
    /// copied.  Returns the number of bytes written.
    pub fn usb_host_test_take_tx(buf: *mut u8, max_len: u32) -> u32;

    /// Bytes flushed and not yet taken.
    pub fn usb_host_test_tx_pending() -> u32;

    /// Call the plugin's `dispatch` with a command packet built as it would
    /// arrive on the wire.  Returns a `pb_status_t`, or -1 if the plugin
    /// registered no handler.
    pub fn usb_host_test_dispatch(
        cmd_id: u8,
        cmd_size: u8,
        transfer_len: u32,
        args: *const u8,
    ) -> i32;

    /// Call the plugin's `fill` for the command the last dispatch described.
    pub fn usb_host_test_fill(buf: *mut u8, max_len: u32, written: *mut u32, done: *mut u8) -> i32;

    /// The magic the plugin registered its commands under.
    pub fn usb_host_test_custom_magic() -> u32;

    /// Bit 0 set if a dispatch handler was registered, bit 1 if a fill one was.
    pub fn usb_host_test_custom_handlers() -> u8;

    /// Route a read through the plugin's ops table, prepare first.
    pub fn usb_host_test_read(addr: u32, buf: *mut u8, len: u32) -> i32;

    /// Route a write through the plugin's ops table, prepare first.
    pub fn usb_host_test_write(addr: u32, buf: *const u8, len: u32) -> i32;

    /// Clear the plugin's own state and the log channels it held, as a device
    /// does by entering a plugin with its .bss zeroed.
    pub fn usb_host_test_reset_plugin();
}
