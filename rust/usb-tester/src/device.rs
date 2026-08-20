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

    /// The device descriptor, as long as its own `bLength` says it is.
    pub fn device_descriptor(&self) -> Result<Vec<u8>, String> {
        // SAFETY: the plugin returns a pointer to its own static descriptor.
        let ptr = unsafe { ffi::tud_descriptor_device_cb() };
        take_descriptor("device", ptr, |d| Ok(usize::from(d[0])), 1, DEVICE_DESC_LEN)
    }

    /// The configuration descriptor, as long as its own `wTotalLength` says.
    pub fn configuration_descriptor(&self, index: u8) -> Result<Vec<u8>, String> {
        // SAFETY: as above.
        let ptr = unsafe { ffi::tud_descriptor_configuration_cb(index) };
        take_descriptor(
            "configuration",
            ptr,
            total_length,
            4,
            self.configuration_desc_size() as usize,
        )
    }

    /// The BOS descriptor, as long as its own `wTotalLength` says.
    pub fn bos_descriptor(&self) -> Result<Vec<u8>, String> {
        // SAFETY: as above.
        let ptr = unsafe { ffi::tud_descriptor_bos_cb() };
        take_descriptor("BOS", ptr, total_length, 4, self.bos_desc_size() as usize)
    }

    /// How many bytes the configuration descriptor actually occupies, which is
    /// what its `wTotalLength` is supposed to say.
    pub fn configuration_desc_size(&self) -> u32 {
        // SAFETY: reads a sizeof compiled into the plugin.
        unsafe { ffi::onerom_usb_test_configuration_desc_size() }
    }

    /// The same, for the BOS descriptor.
    pub fn bos_desc_size(&self) -> u32 {
        // SAFETY: as above.
        unsafe { ffi::onerom_usb_test_bos_desc_size() }
    }

    /// A string descriptor, or `None` where the plugin refuses the index.
    ///
    /// Returned as the UTF-16 code units it is made of, header included, so a
    /// scenario can check the header the device built as well as the text.
    pub fn string_descriptor(&self, index: u8) -> Result<Option<Vec<u16>>, String> {
        // SAFETY: as above.  `langid` is English, which the plugin ignores.
        let ptr = unsafe { ffi::tud_descriptor_string_cb(index, 0x0409) };
        if ptr.is_null() {
            return Ok(None);
        }

        // SAFETY: the header unit is always present when the pointer is not
        // null, and its low byte is the total length in bytes.
        let bytes = usize::from(unsafe { *ptr } & 0xff);
        if bytes < 2 || bytes % 2 != 0 || bytes > STRING_DESC_MAX_LEN {
            return Err(format!(
                "string descriptor {index} declares {bytes} bytes, which is not a \
                 whole number of code units within the {STRING_DESC_MAX_LEN} the \
                 plugin's buffer holds"
            ));
        }

        // SAFETY: the declared length has been bounded above.
        let units = unsafe { std::slice::from_raw_parts(ptr, bytes / 2) };
        Ok(Some(units.to_vec()))
    }

    /// Send a vendor control request, as a host asking for the Windows
    /// descriptor does.  Returns whether the plugin claimed it.
    pub fn vendor_control(
        &mut self,
        stage: u8,
        bm_request_type: u8,
        b_request: u8,
        w_index: u16,
    ) -> bool {
        // SAFETY: calls the plugin's own callback while it is parked.
        unsafe { ffi::usb_host_test_vendor_control(stage, bm_request_type, b_request, w_index) }
    }

    /// How many buffers the plugin has offered to send in reply to a control
    /// request.  Zero tells a refusal apart from an empty answer.
    pub fn control_xfer_count(&self) -> u32 {
        // SAFETY: reads a shim counter.
        unsafe { ffi::usb_host_test_control_xfer_count() }
    }

    /// What the plugin last offered to send.
    pub fn take_control_xfer(&mut self) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; MAX_DESCRIPTOR_LEN];
        // SAFETY: `buf` is `MAX_DESCRIPTOR_LEN` bytes.
        let len =
            unsafe { ffi::usb_host_test_take_control_xfer(buf.as_mut_ptr(), buf.len() as u32) }
                as usize;
        if len > buf.len() {
            return Err(format!(
                "the plugin offered {len} bytes, more than the {MAX_DESCRIPTOR_LEN} a \
                 descriptor may be"
            ));
        }
        buf.truncate(len);
        Ok(buf)
    }

    /// Whether picoboot claims a control request before the plugin's own
    /// handler sees it.
    pub fn set_picoboot_claims_control(&mut self, claims: bool) {
        // SAFETY: sets a shim flag.
        unsafe { ffi::usb_host_test_set_picoboot_claims_control(u8::from(claims)) };
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

    /// What one of the device's LEDs is doing, read through the firmware's own
    /// `ORA_ID_LED_GET`.
    ///
    /// The firmware's engine holds an LED's mode and drives it from there, so
    /// this is the live state rather than the plugin's account of what it asked
    /// for — which is what lets a scenario see whether the colour, brightness
    /// and mode the wire carried arrived where they were aimed.
    pub fn led(&self, led: u8) -> Result<LedState, String> {
        use onerom_fw_emulator::ffi as fw;

        // Pre-filled with a sentinel, so a field the firmware leaves alone is
        // not read as one it wrote as zero.
        let mut state = fw::ora_led_state_t {
            size: size_of::<fw::ora_led_state_t>() as u8,
            led: 0xFF,
            present: 0xFF,
            mode: 0xFF,
            brightness: 0xFF,
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
            gpio: 0xFF,
            shared_gpio: 0xFF,
            period_ms: 0xFFFF,
        };

        // SAFETY: the firmware's own lookup, called with a structure whose size
        // field bounds what it may write.
        let result = unsafe {
            let ptr = fw::ora_fn_lookup(fw::api_id_t_ORA_ID_LED_GET);
            if ptr.is_null() {
                return Err("this firmware has no LED engine to read".to_string());
            }
            let get: fw::ora_led_get_fn_t = std::mem::transmute(ptr);
            get.unwrap()(led, &mut state)
        };

        let result = onerom_fw_emulator::OraResult::from(result);
        if !result.is_ok() {
            return Err(format!("could not read LED {led}: {result:?}"));
        }

        Ok(LedState {
            present: state.present != 0,
            mode: state.mode,
            brightness: state.brightness,
            r: state.r,
            g: state.g,
            b: state.b,
            period_ms: state.period_ms,
            gpio: state.gpio,
            shared_gpio: state.shared_gpio != 0,
        })
    }

    /// Drive the status LED as a second plugin would.
    ///
    /// The LED is a shared channel rather than this plugin's own, so a scenario
    /// can move it from underneath and see whether the USB plugin puts it back.
    pub fn set_status_led_elsewhere(&mut self, on: bool) {
        self.emu.set_status_led(on);
    }

    /// Run the LED engine's frame, as TIMER0 alarm 1 does on a device.
    ///
    /// The engine drives itself from an interrupt this process does not have,
    /// so a scenario stands where that interrupt does.  Pair it with
    /// [`Device::led_deadline_ms`]: move the clock there, then call this.
    pub fn led_frame(&mut self) {
        self.emu.led_frame();
    }

    /// When the LED engine next wants a frame, in the milliseconds the plugin
    /// sees, or `None` when nothing is animating and no hold is running.
    pub fn led_deadline_ms(&self) -> Option<u32> {
        self.emu.led_next_deadline_ms()
    }

    /// The colour the RGB LED is showing and how many the engine has sent.
    ///
    /// What the chip reads, with brightness and any fade applied, so this is
    /// the LED's output rather than what a request asked for.
    pub fn led_pixel(&self) -> (u32, u32) {
        self.emu.led_last_pixel()
    }

    /// What a pin is actually at, from the pad model the firmware drives.
    ///
    /// A device's own `ora_gpio_query`, so this is the pin rather than what any
    /// piece of firmware believes about it.
    pub fn gpio_level(&self, gpio: u8) -> u8 {
        self.emu.gpio_query(gpio).1.level
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

/// What one of the device's LEDs is doing, as the firmware's LED engine
/// reports it.
///
/// The fields the engine was told, and whether the board has the LED at all.
/// Which GPIO it is on and whether that pin is shared describe the board rather
/// than the request, so they are left where the firmware keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedState {
    /// Whether this board has this LED.
    pub present: bool,
    /// `ora_led_mode_t` — off, on, beacon or flame, which SET_LED's
    /// sub-commands number the same way.
    pub mode: u8,
    /// Brightness as a percentage.
    pub brightness: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// How long one repetition of the mode takes, in milliseconds.
    pub period_ms: u16,
    /// The GPIO this LED is on, or 0xFF where the board has none.
    pub gpio: u8,
    /// Whether this LED shares its GPIO with the other one.
    pub shared_gpio: bool,
}

/// The largest control transfer this harness will take.
///
/// Nothing the plugin sends is close to it.
const MAX_DESCRIPTOR_LEN: usize = 512;

/// A device descriptor is 18 bytes.  That is the USB specification, not this
/// device's choice, so it is the bound as well as the expected value.
const DEVICE_DESC_LEN: usize = 18;

/// The plugin builds string descriptors in a 65-unit buffer, so 130 bytes is as
/// long as one can be.  See `_desc_str` in `usb_descriptors.c`.
const STRING_DESC_MAX_LEN: usize = 130;

/// `wTotalLength`, at offset 2 of the configuration and BOS descriptors.
fn total_length(d: &[u8]) -> Result<usize, String> {
    Ok(usize::from(u16::from_le_bytes([d[2], d[3]])))
}

/// Take a descriptor as long as it says it is, within what it can be.
///
/// `header` is how many bytes must be readable before the length can be, and
/// `len_of` reads the length out of them.  `limit` is how much there really is
/// — a `sizeof` for the configuration and BOS descriptors, a fixed size for the
/// others.
///
/// A declared length beyond `limit` is a failure rather than a longer read,
/// because reading it is exactly the out-of-bounds access a real host would
/// make: the point is to report the lie, not to reproduce its consequence.
fn take_descriptor(
    what: &str,
    ptr: *const u8,
    len_of: fn(&[u8]) -> Result<usize, String>,
    header: usize,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if ptr.is_null() {
        return Err(format!("the plugin has no {what} descriptor"));
    }

    // SAFETY: a descriptor is at least its own header, which is what the
    // declared length is read from.
    let head = unsafe { std::slice::from_raw_parts(ptr, header) };
    let len = len_of(head)?;
    if len < header || len > limit {
        return Err(format!(
            "the {what} descriptor declares {len} bytes, but there are {limit}"
        ));
    }

    // SAFETY: the declared length has been bounded by the real one above.
    let all = unsafe { std::slice::from_raw_parts(ptr, len) };
    Ok(all.to_vec())
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
