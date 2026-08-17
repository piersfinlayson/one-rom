// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What a scenario drives the plugin through.
//!
//! A scenario does not touch the shim or the emulator directly.  It says what a
//! host or a terminal did — a port opened, an endpoint that will take no more,
//! time passing — and then lets the plugin run.  Keeping that behind one type
//! is what stops a scenario depending on which of those the shim happens to
//! model and which the firmware does.

use onerom_fw_emulator::Emulator;
use onerom_plugin_tester::harness::Plugin;

use crate::ffi;

/// The device under test, as a scenario sees it.
pub struct Device<'a> {
    emu: &'a Emulator,
    plugin: &'a Plugin,
    /// How many passes of the plugin's main loop have been run, for messages.
    passes: u32,
}

impl<'a> Device<'a> {
    pub fn new(emu: &'a Emulator, plugin: &'a Plugin) -> Self {
        Device {
            emu,
            plugin,
            passes: 0,
        }
    }

    pub fn emulator(&self) -> &Emulator {
        self.emu
    }

    /// Let the plugin run one pass of its main loop.
    pub fn step(&mut self) -> Result<(), String> {
        self.passes += 1;
        let passes = self.passes;
        self.plugin.resume(&format!("main loop pass {passes}"))
    }

    /// Run `n` passes.
    pub fn step_n(&mut self, n: u32) -> Result<(), String> {
        for _ in 0..n {
            self.step()?;
        }
        Ok(())
    }

    /// Open or close the CDC port, which is a terminal raising or dropping DTR.
    pub fn set_dtr(&mut self, dtr: bool) {
        // SAFETY: calls into the plugin's own callback while it is parked at a
        // yield, so nothing else is touching its state.
        unsafe { ffi::usb_host_test_set_dtr(u8::from(dtr)) };
    }

    /// Whether the bus can carry data.  False models a suspend, where the
    /// terminal is still attached but nothing can be sent.
    pub fn set_connected(&mut self, connected: bool) {
        // SAFETY: as above.
        unsafe { ffi::usb_host_test_set_connected(u8::from(connected)) };
    }

    /// How much the CDC endpoint holds before it is full.  Set below a packet to
    /// make the plugin resume a line it could not finish.
    pub fn set_tx_capacity(&mut self, capacity: u32) {
        // SAFETY: as above.
        unsafe { ffi::usb_host_test_set_tx_capacity(capacity) };
    }

    /// Run `passes` of the plugin's main loop, reading the endpoint after each
    /// one, and return everything it wrote.
    ///
    /// Reading as it goes is what a terminal does, and it is what returns the
    /// endpoint's room — so a plugin resuming a line across passes makes
    /// progress here for the same reason it does on a device.
    pub fn collect_cdc(&mut self, passes: u32) -> Result<String, String> {
        let mut out = String::new();
        for _ in 0..passes {
            self.step()?;
            out.push_str(&self.take_cdc_text());
        }
        Ok(out)
    }

    /// Everything flushed to the CDC endpoint since this was last called.
    pub fn take_cdc(&mut self) -> Vec<u8> {
        // SAFETY: the length is asked for first and the buffer is that size.
        let pending = unsafe { ffi::usb_host_test_tx_pending() } as usize;
        let mut buf = vec![0u8; pending];
        // SAFETY: `buf` is `pending` bytes and outlives the call.
        let n = unsafe { ffi::usb_host_test_take_tx(buf.as_mut_ptr(), pending as u32) } as usize;
        buf.truncate(n);
        buf
    }

    /// The same, as text.  The log is a stream of lines, so a scenario that
    /// asserts on content wants it this way round.
    pub fn take_cdc_text(&mut self) -> String {
        String::from_utf8_lossy(&self.take_cdc()).into_owned()
    }

    /// Call the plugin's One ROM command dispatch, as picoboot's core would.
    ///
    /// `cmd_id` carries the direction bit exactly as it arrives on the wire, so
    /// a scenario says 0x82 where a host would.  Returns the `pb_status_t` the
    /// plugin answered with.
    pub fn dispatch(&mut self, cmd_id: u8, transfer_len: u32, args: &[u8; 16]) -> i32 {
        // SAFETY: the args are 16 bytes, which is what the packet carries.
        unsafe { ffi::usb_host_test_dispatch(cmd_id, 16, transfer_len, args.as_ptr()) }
    }

    /// The same, with a command size the wire would not carry.
    pub fn dispatch_sized(&mut self, cmd_id: u8, cmd_size: u8, args: &[u8; 16]) -> i32 {
        // SAFETY: as above.
        unsafe { ffi::usb_host_test_dispatch(cmd_id, cmd_size, 0, args.as_ptr()) }
    }

    /// Call the plugin's fill for the command the last dispatch described, with
    /// `max_len` bytes of room.  Returns the status, what it produced, and
    /// whether it says the transfer is complete.
    pub fn fill(&mut self, max_len: u32) -> (i32, Vec<u8>, bool) {
        let mut buf = vec![0u8; max_len as usize];
        let mut written = 0u32;
        let mut done = 0u8;
        // SAFETY: `buf` is `max_len` bytes and the two out-params are ours.
        let st =
            unsafe { ffi::usb_host_test_fill(buf.as_mut_ptr(), max_len, &mut written, &mut done) };
        buf.truncate(written as usize);
        (st, buf, done != 0)
    }

    /// Everything a fill produces, asked for `chunk` bytes at a time.
    pub fn fill_all(&mut self, chunk: u32, total: u32) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        // Bounded so a fill that never reports completion fails rather than
        // spinning: a byte at a time is the slowest it can legitimately go.
        for _ in 0..(total + 1) {
            let (st, part, done) = self.fill(chunk);
            if st != 0 {
                return Err(format!(
                    "fill answered status {st} after {} bytes",
                    out.len()
                ));
            }
            out.extend_from_slice(&part);
            if done {
                return Ok(out);
            }
        }
        Err(format!(
            "fill produced {} of {total} bytes without reporting completion",
            out.len()
        ))
    }

    /// Read through the plugin's picoboot ops table, prepare first.
    pub fn pb_read(&mut self, addr: u32, len: u32) -> (i32, Vec<u8>) {
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` is `len` bytes.
        let st = unsafe { ffi::usb_host_test_read(addr, buf.as_mut_ptr(), len) };
        (st, buf)
    }

    /// Write through the plugin's picoboot ops table, prepare first.
    pub fn pb_write(&mut self, addr: u32, data: &[u8]) -> i32 {
        // SAFETY: `data` outlives the call.
        unsafe { ffi::usb_host_test_write(addr, data.as_ptr(), data.len() as u32) }
    }

    /// What the device's status LED is doing.
    ///
    /// Read through the firmware's own `status_led_enabled`, which is the live
    /// state and the channel a second plugin reads, rather than the USB
    /// plugin's private copy of it.
    pub fn status_led(&self) -> (onerom_fw_emulator::OraResult, Option<u32>) {
        self.emu.get_metadata_uint(
            onerom_fw_emulator::ffi::ora_metadata_key_t_ORA_METADATA_KEY_STATUS_LED_STATE,
        )
    }

    /// Drive the status LED as a second plugin would.
    ///
    /// The LED is a shared channel rather than this plugin's own, so a scenario
    /// can move it from underneath and see whether the USB plugin puts it back.
    pub fn set_status_led_elsewhere(&mut self, on: bool) {
        self.emu.set_status_led(on);
    }

    /// Move the device's clock on.
    ///
    /// What the plugin reads is the firmware's uptime, which a test build takes
    /// from a counter the harness owns rather than from wall time — so a wait
    /// the plugin makes is stepped over rather than waited out.
    pub fn advance_ms(&mut self, ms: u64) {
        self.emu.advance_timer_us(ms * 1000);
    }

    /// What the device's clock reads, in the milliseconds the plugin sees.
    ///
    /// A scenario that cares only about an interval does not need this.  One
    /// that has to be at a particular point on the clock does, because where
    /// the clock starts is the harness's business rather than the scenario's.
    pub fn uptime_ms(&self) -> u32 {
        self.emu.get_plugin_uptime_ms()
    }
}

/// Put the shim's endpoint model back to how a device comes up: no terminal
/// attached, a bus that can carry data, and a full packet of room.
///
/// The model is process-global, as the plugin's own statics are, so without
/// this one scenario's endpoint state would be the next one's starting point.
pub fn reset_endpoint() {
    // SAFETY: called between scenarios, with no plugin running.
    unsafe {
        ffi::usb_host_test_reset_plugin();
        ffi::usb_host_test_set_dtr(0);
        ffi::usb_host_test_set_connected(1);
        ffi::usb_host_test_set_tx_capacity(u32::MAX);
        let pending = ffi::usb_host_test_tx_pending();
        if pending > 0 {
            let mut buf = vec![0u8; pending as usize];
            ffi::usb_host_test_take_tx(buf.as_mut_ptr(), pending);
        }
    }
}
