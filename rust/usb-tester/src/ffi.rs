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

    /// Whether the shim's picoboot handler claims a control request before the
    /// plugin's own sees it.
    pub fn usb_host_test_set_picoboot_claims_control(claims: u8);

    /// Call the plugin's `tud_vendor_control_xfer_cb` with an assembled
    /// request.  Returns whether the plugin claimed it.
    pub fn usb_host_test_vendor_control(
        stage: u8,
        bm_request_type: u8,
        b_request: u8,
        w_index: u16,
    ) -> bool;

    /// How many buffers the plugin has offered through `tud_control_xfer`.
    pub fn usb_host_test_control_xfer_count() -> u32;

    /// What the last offer carried.  Returns the length the plugin declared,
    /// which may exceed `max_len`.
    pub fn usb_host_test_take_control_xfer(buf: *mut u8, max_len: u32) -> u32;
}

// The descriptor callbacks tinyusb fetches from the plugin.
//
// Each returns a pointer to a block that states its own length — byte 0 for the
// device and string descriptors, `wTotalLength` at offset 2 for the
// configuration and BOS.  A scenario reads that length and takes that many
// bytes, which is what a host does, and so is what makes a descriptor lying
// about its own length a failure rather than something the harness quietly
// corrects.
unsafe extern "C" {
    pub fn tud_descriptor_device_cb() -> *const u8;
    pub fn tud_descriptor_configuration_cb(index: u8) -> *const u8;
    pub fn tud_descriptor_bos_cb() -> *const u8;
    pub fn tud_descriptor_string_cb(index: u8, langid: u16) -> *const u16;
}

// How long two of those descriptors really are.
//
// Each declares its own length in a `wTotalLength` the host trusts, so the
// declared length cannot check itself — and the true size is a `sizeof` only the
// plugin's own file can take.  These are compiled into the plugin under
// `ORA_HOST_TEST` for that reason alone.
unsafe extern "C" {
    pub fn onerom_usb_test_configuration_desc_size() -> u32;
    pub fn onerom_usb_test_bos_desc_size() -> u32;
}
