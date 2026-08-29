// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! USB support for RP2350
//!
//! Lab and One ROM share VID/PID 1209:F542, so a host that has met one and then
//! meets the other has to find the same device.  What is built here is
//! therefore descriptor-for-descriptor what One ROM's USB plugin presents, down
//! to the endpoint addresses and the string indices:
//!
//!   Interface 0 - dummy (0xFF/0x00/0x00, no endpoints), string 4
//!                 — reserves slot for picoboot
//!   Interface 1 - vendor / WinUSB, serving PICOBOOT, string 5
//!                 — bulk out 0x03, bulk in 0x83
//!   Interface 2 - CDC ACM control, string 6
//!                 — interrupt in 0x81, 1ms
//!   Interface 3 - CDC ACM data, unnamed
//!                 — bulk out 0x02, bulk in 0x82
//!
//! Interfaces 2 and 3 carry the one interface association descriptor, written
//! by hand below.  Interfaces 0 and 1 have none, and the device class is
//! 0x00/0x00/0x00.
//!
//! The MS OS 2.0 descriptor scopes WinUSB to interface 1 only.
//!
//! # PICOBOOT
//!
//! Interface 1 serves PICOBOOT through `picobootx-embassy`, which needs two
//! halves running: the control endpoint's, handed to the USB builder as a
//! handler, and the bulk endpoints', which is a task of its own.  What Lab
//! answers and what it refuses is [`crate::picoboot::LabOps`].
//!
//! # Public API
//!
//! - [`cdc_wait_connection`] — await enumeration by a host
//! - [`cdc_wait_dtr`]        — await a terminal opening the port
//! - [`cdc_send`]            — queue a string for transmission (non-blocking)
//! - [`cdc_recv`]            — await the next byte from the host
//! - [`cdc_drain_rx`]        — discard anything received but not yet read
//!
//! Enumeration and a terminal opening the port are separate events.  A host
//! enumerates the device as soon as it is plugged in, whether or not anybody
//! is watching, and raises DTR when a terminal opens the port.

#![allow(static_mut_refs)]

use core::sync::atomic::{AtomicBool, Ordering};

use alloc::string::String;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, Either3, select, select3};
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use embassy_usb::driver::{Endpoint as _, EndpointAddress};
use embassy_usb::msos::{self, windows_version};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config as UsbConfig, Handler, UsbDevice, UsbVersion};
use log::debug;
use picobootx::{Endpoints, NoCustom};
use picobootx_embassy::{PicobootClass, Rp2350EndpointControl};
use static_cell::StaticCell;

use super::serial_id;
use crate::cdc::{CdcAcmClass, ControlChanged, Receiver, Sender, State};
use crate::picoboot::{self, EpIn, EpOut, LabOps};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

static mut CONFIG_DESCRIPTOR: [u8; 256] = [0; 256];
static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
static mut MSOS_DESCRIPTOR: [u8; 256] = [0; 256];
// A control reply is built here, and the longest one is a string descriptor -
// two bytes plus two per character, so interface 0's 64-character name needs
// 130.  embassy asserts rather than truncating when a reply will not fit, and
// Lab's panic handler leaves interrupts off, so a buffer one byte short takes
// the whole device down the first time a host asks for that name.
static mut CONTROL_BUF: [u8; 256] = [0; 256];

// CDC ACM internal state.
static CDC_STATE: StaticCell<State> = StaticCell::new();

// The interface names.  The USB stack asks for these while it is answering a
// control request, so the handler outlives `Usb::new` like the others do.
static STRINGS: StaticCell<InterfaceStrings> = StaticCell::new();

// picobootx and its control handler.  Both outlive `Usb::new`: the handler is
// borrowed by the USB builder for the device's lifetime, and the protocol it
// reads is shared with the task holding the bulk endpoints.
static PICOBOOT: StaticCell<picoboot::Device> = StaticCell::new();
static PICOBOOT_CONTROL: StaticCell<picoboot::Control> = StaticCell::new();

/// True while the host is connected.  Set by `cdc_writer`.
static CONNECTED: AtomicBool = AtomicBool::new(false);

/// Messages queued for transmission to the host.
static CDC_TX: Channel<CriticalSectionRawMutex, String, 8> = Channel::new();

/// Bytes received from the host.
///
/// Deep enough to take a whole 64-byte packet without the reader having to
/// stop for the consumer mid-packet.  `cdc_reader` blocks rather than dropping
/// when it does fill, so a host that sends faster than the session consumes is
/// held up by USB flow control instead of losing characters.
static CDC_RX: Channel<CriticalSectionRawMutex, u8, 64> = Channel::new();

/// Raised when the session's other end goes away - the host disconnecting, or a
/// terminal closing the port.
///
/// Separate from `CDC_RX` so that it cannot be lost behind bytes the consumer
/// has not read yet.  As a sentinel in the byte channel it was dropped whenever
/// the channel was full, which is exactly when a session most needs to end.
static CDC_GONE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Raised when the host asks for a break.
///
/// Separate from `CDC_RX` for the same reason as `CDC_GONE`, and it matters
/// more here: a break is what a terminal sends when it wants attention and is
/// not getting it, so losing one behind unread bytes defeats the whole point
/// of the request.  Delivered to the consumer as Ctrl-C, which every reader
/// already treats as cancel - a break must not depend on a reader that stops
/// for any byte at all.
static CDC_BREAK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by `cdc_writer` each time the host connects.
static CDC_CONNECTED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// True while DTR is raised, which is what says a terminal has the port open.
/// Written only by `cdc_control`.
static DTR: AtomicBool = AtomicBool::new(false);

/// Fired by `cdc_control` each time DTR is raised.
static CDC_DTR: Signal<CriticalSectionRawMutex, ()> = Signal::new();

const VID: u16 = 0x1209;
const PID: u16 = 0xF542;
const VENDOR_REQUEST_MICROSOFT: u8 = 1;
const WINUSB_GUID: &str = "{53F67517-1850-422C-91F8-C56F657195AF}";
const MAX_PACKET: usize = 64;

/// What a break is delivered as.  Ctrl-C is what every reader already treats as
/// cancel, so a break does not rest on a reader that stops for any byte.
const CTRL_C: u8 = 0x03;

/// Device release, in BCD.  0x0100 is what One ROM reports.
const DEVICE_RELEASE: u16 = 0x0100;

// The interface names, which One ROM puts at string indices 4, 5 and 6.
const IFACE0_NAME: &str = "One ROM, the most flexible replacement ROM for your retro system";
const IFACE1_NAME: &str = "One ROM Control";
const IFACE2_NAME: &str = "One ROM Serial";

// PICOBOOT's bulk endpoints.  Pinned rather than taken in turn so that they
// land where One ROM's are.  An address already in use fails the allocation,
// so a clash shows up instead of the endpoint quietly moving elsewhere.
const PICOBOOT_EP_OUT_ADDR: u8 = 0x03;
const PICOBOOT_EP_IN_ADDR: u8 = 0x83;

/// The interface association descriptor covering the two CDC interfaces:
/// bFirstInterface 2, bInterfaceCount 2, CDC / ACM / no protocol, unnamed.
const CDC_IAD_BODY: [u8; 6] = [0x02, 0x02, 0x02, 0x02, 0x00, 0x00];
const DESCRIPTOR_TYPE_IAD: u8 = 0x0B;

/// Answers a host asking for an interface name.
///
/// embassy allocates custom string indices from 4 upwards in the order
/// [`Builder::string`] is called, and resolves them by asking each handler in
/// turn, so the three indices are held here alongside the text they name.
struct InterfaceStrings {
    iface0: StringIndex,
    iface1: StringIndex,
    iface2: StringIndex,
}

impl Handler for InterfaceStrings {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        let index = u8::from(index);
        if index == u8::from(self.iface0) {
            Some(IFACE0_NAME)
        } else if index == u8::from(self.iface1) {
            Some(IFACE1_NAME)
        } else if index == u8::from(self.iface2) {
            Some(IFACE2_NAME)
        } else {
            None
        }
    }
}

/// Errors returned by [`cdc_send`] and [`cdc_recv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The host is not connected.
    Disconnected,
    /// The host is connected but the TX channel is full.
    Full,
}

pub struct Usb {
    cdc: CdcAcmClass<'static, Driver<'static, USB>>,
    device: UsbDevice<'static, Driver<'static, USB>>,
    picoboot: &'static picoboot::Device,
    picoboot_out: EpOut,
    picoboot_in: EpIn,
}

impl Usb {
    /// Build the USB device.  Must be called exactly once, before [`run`].
    pub fn new(usb: Peri<'static, USB>) -> Self {
        let serial = serial_id();

        let driver = Driver::new(usb, Irqs);

        let mut config = UsbConfig::new(VID, PID);
        config.manufacturer = Some("piers.rocks");
        config.product = Some("One ROM");
        config.serial_number = Some(serial);
        config.max_power = 250;
        config.max_packet_size_0 = 64;
        config.bcd_usb = UsbVersion::TwoOne;
        config.device_release = DEVICE_RELEASE;
        // One ROM reports no device class, leaving each interface to say what
        // it is.  embassy will only allow that with composite_with_iads off,
        // which also stops it writing an IAD in front of every function - it
        // has no way to write one for some functions and not others, and One
        // ROM associates only the two CDC interfaces.  The one association is
        // written by hand below.
        config.device_class = 0x00;
        config.device_sub_class = 0x00;
        config.device_protocol = 0x00;
        config.composite_with_iads = false;

        let mut builder = Builder::new(
            driver,
            config,
            unsafe { &mut CONFIG_DESCRIPTOR },
            unsafe { &mut BOS_DESCRIPTOR },
            unsafe { &mut MSOS_DESCRIPTOR },
            unsafe { &mut CONTROL_BUF },
        );

        builder.msos_descriptor(windows_version::WIN8_1, VENDOR_REQUEST_MICROSOFT);

        // Indices 4, 5 and 6, in that order, matching One ROM.
        let strings = InterfaceStrings {
            iface0: builder.string(),
            iface1: builder.string(),
            iface2: builder.string(),
        };
        let strings = STRINGS.init(strings);
        let (iface0_name, iface1_name, iface2_name) =
            (strings.iface0, strings.iface1, strings.iface2);
        builder.handler(strings);

        // Interface 0: dummy (0xFF/0x00/0x00, no endpoints).
        // Occupies slot 0 so picoboot (added later) lands at interface 1.
        {
            let mut func = builder.function(0xFF, 0, 0);
            let mut iface = func.interface();
            let _alt = iface.alt_setting(0xFF, 0, 0, Some(iface0_name));
        }

        // Interface 1: vendor / WinUSB, bulk endpoints, carrying PICOBOOT.
        // MS OS 2.0 features scoped to this function only.
        let (picoboot_out, picoboot_in) = {
            let mut func = builder.function(0xFF, 0, 0);
            func.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
            func.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
                "DeviceInterfaceGUIDs",
                msos::PropertyData::RegMultiSz(&[WINUSB_GUID]),
            ));
            let mut iface = func.interface();
            let mut alt = iface.alt_setting(0xFF, 0, 0, Some(iface1_name));
            let ep_out =
                alt.endpoint_bulk_out(Some(EndpointAddress::from(PICOBOOT_EP_OUT_ADDR)), 64);
            let ep_in = alt.endpoint_bulk_in(Some(EndpointAddress::from(PICOBOOT_EP_IN_ADDR)), 64);

            // The CDC interface association, written by hand because embassy
            // will only write one per function and only for every function at
            // once.  Descriptors land in the order the builder is called, so
            // putting it here - on interface 1, after its endpoints, and before
            // the CDC class is added - is what puts it immediately in front of
            // interface 2, where One ROM has it.
            alt.descriptor(DESCRIPTOR_TYPE_IAD, &CDC_IAD_BODY);

            (ep_out, ep_in)
        };

        // picobootx halts an endpoint by address and serves in packets of the
        // size the endpoint was allocated with, so both come from the endpoints
        // themselves rather than being restated here.  Both are allocated at 64,
        // which is the most a full-speed bulk endpoint carries.
        let picoboot = PICOBOOT.init(PicobootClass::new(
            LabOps::new(),
            NoCustom,
            // Lab refuses a flash write, so there is nothing for a page buffer
            // to accumulate.
            None,
            Endpoints {
                out: picoboot_out.info().addr.into(),
                r#in: picoboot_in.info().addr.into(),
            },
            picoboot_out.info().max_packet_size,
            Rp2350EndpointControl,
        ));
        builder.handler(PICOBOOT_CONTROL.init(picoboot.handler()));

        // Interfaces 2+3: CDC ACM.
        let cdc = CdcAcmClass::new(
            &mut builder,
            CDC_STATE.init(State::new()),
            64,
            Some(iface2_name),
        );

        let device = builder.build();

        Self {
            cdc,
            device,
            picoboot,
            picoboot_out,
            picoboot_in,
        }
    }
}

pub fn run(spawner: Spawner, usb: Usb) {
    let (sender, receiver, control) = usb.cdc.split_with_control();
    spawner.spawn(usb_task(usb.device).unwrap());
    spawner.spawn(picoboot_task(usb.picoboot, usb.picoboot_out, usb.picoboot_in).unwrap());
    spawner.spawn(cdc_writer(sender).unwrap());
    spawner.spawn(cdc_reader(receiver).unwrap());
    spawner.spawn(cdc_control(control).unwrap());
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    device.run().await
}

/// The bulk half of PICOBOOT.  The control half is the handler `usb_task`
/// calls, and the two share the protocol, so both have to run for a command to
/// complete.
#[embassy_executor::task]
async fn picoboot_task(dev: &'static picoboot::Device, ep_out: EpOut, ep_in: EpIn) -> ! {
    dev.run(ep_out, ep_in).await
}

#[embassy_executor::task]
async fn cdc_writer(mut sender: Sender<'static, Driver<'static, USB>>) -> ! {
    loop {
        // Drain any messages left from the previous session before the host
        // sees a new connection, so it always starts with a clean slate.
        while CDC_TX.try_receive().is_ok() {}

        sender.wait_connection().await;
        CONNECTED.store(true, Ordering::Relaxed);
        CDC_CONNECTED.signal(());

        'connected: loop {
            let msg = CDC_TX.receive().await;
            let bytes = msg.as_bytes();

            // Send in MAX_PACKET-sized chunks.
            let mut offset = 0;
            while offset < bytes.len() {
                let end = (offset + MAX_PACKET).min(bytes.len());
                if sender.write_packet(&bytes[offset..end]).await.is_err() {
                    break 'connected;
                }
                offset = end;
            }

            // Send a ZLP if the final chunk exactly filled a packet, so the
            // host knows the transfer is complete.
            if bytes.len() % MAX_PACKET == 0 && sender.write_packet(&[]).await.is_err() {
                break 'connected;
            }
        }

        // Mark disconnected and say so before looping, so that any task blocked
        // in cdc_recv() is unblocked with Err(Error::Disconnected).
        CONNECTED.store(false, Ordering::Relaxed);
        CDC_GONE.signal(());
    }
}

#[embassy_executor::task]
async fn cdc_reader(mut receiver: Receiver<'static, Driver<'static, USB>>) -> ! {
    loop {
        receiver.wait_connection().await;
        let mut buf = [0u8; 64];
        while let Ok(n) = receiver.read_packet(&mut buf).await {
            for &b in &buf[..n] {
                // Wait rather than drop.  A full channel means the session has
                // not caught up, and stopping here NAKs the endpoint until it
                // does, which is what keeps a pasted line intact.
                CDC_RX.send(b).await;
            }
        }
    }
}

/// Track DTR, so a terminal opening and closing the port is an event rather
/// than something only the bytes flowing reveal.
///
/// A rising edge is published for whoever is waiting to start a session.  A
/// falling edge raises the same signal `cdc_writer` raises on disconnect, so a
/// session in progress ends through the path it already has and nothing reading
/// input needs a second one.
///
/// `control_changed` also fires for RTS and the line coding, so both edges are
/// taken from DTR itself rather than from the notification.  A bus reset clears
/// DTR and reports the change, which is how an unplug arrives here.
#[embassy_executor::task]
async fn cdc_control(control: ControlChanged<'static>) -> ! {
    loop {
        control.control_changed().await;

        // A break is the terminal's older way of saying stop, and it arrives on
        // the control endpoint rather than in the data stream.  It goes to its
        // own signal rather than into the byte channel, so a full channel
        // cannot lose it, and `cdc_recv` hands it to the consumer as Ctrl-C.
        if control.take_break() {
            debug!("Break requested");
            CDC_BREAK.signal(());
        }

        let raised = control.dtr();
        if DTR.swap(raised, Ordering::Relaxed) == raised {
            continue;
        }

        if raised {
            CDC_DTR.signal(());
        } else {
            CDC_GONE.signal(());
        }
    }
}

/// Wait until a host enumerates the device.
///
/// Returns immediately if one already has, so a caller that arrives after the
/// event, or comes back round its own loop while the host is still there, is
/// not left waiting for a connection that has already happened.  Typical
/// usage:
/// ```ignore
/// loop {
///     usb::cdc_wait_connection().await;
///     usb::cdc_send("Hello!\r\n".to_string()).ok();
///     loop {
///         match usb::cdc_recv().await {
///             Ok(b)  => handle(b),
///             Err(_) => break,
///         }
///     }
/// }
/// ```
pub async fn cdc_wait_connection() {
    loop {
        if CONNECTED.load(Ordering::Relaxed) {
            CDC_CONNECTED.reset();
            return;
        }
        CDC_CONNECTED.wait().await;
    }
}

/// Wait until a terminal opens the port.
///
/// Returns immediately if one already has it open, so a caller arriving after
/// the edge is not left waiting for the next one.  The signal is only a nudge
/// to look: what is returned on is DTR itself, so a signal left over from a
/// terminal that has since closed the port does not read as one opening it.
pub async fn cdc_wait_dtr() {
    loop {
        if DTR.load(Ordering::Relaxed) {
            CDC_DTR.reset();
            return;
        }
        CDC_DTR.wait().await;
    }
}

/// Discard anything received but not yet read, and forget that a previous
/// session's other end went away.
///
/// What a terminal sent while it was being opened is not input, and neither is
/// anything typed at a session that has since ended.  Called as a session
/// starts, so a stale departure cannot end the new one before it begins.
pub fn cdc_drain_rx() {
    while CDC_RX.try_receive().is_ok() {}
    CDC_GONE.reset();
    CDC_BREAK.reset();
}

/// Queue a string for transmission to the host.  Never blocks.
///
/// Returns:
/// - `Ok(())`               — message queued successfully
/// - `Err(Error::Disconnected)` — host is not connected; message dropped
/// - `Err(Error::Full)`         — TX channel full; message dropped
pub fn cdc_send(s: String) -> Result<(), Error> {
    if !CONNECTED.load(Ordering::Relaxed) {
        return Err(Error::Disconnected);
    }
    CDC_TX.try_send(s).map_err(|_| Error::Full)
}

/// Wait for the next byte from the host.
///
/// Returns:
/// - `Ok(b)`                    — a byte was received
/// - `Err(Error::Disconnected)` — the host has disconnected
pub async fn cdc_recv() -> Result<u8, Error> {
    // Going away is checked ahead of the next byte, so a session ends promptly
    // rather than after working through whatever was typed before it did.
    match select3(CDC_GONE.wait(), CDC_BREAK.wait(), CDC_RX.receive()).await {
        Either3::First(()) => Err(Error::Disconnected),
        // Ahead of the bytes, because a break is asking for attention now
        // rather than once whatever was typed before it has been worked
        // through.
        Either3::Second(()) => Ok(CTRL_C),
        Either3::Third(b) => Ok(b),
    }
}

/// Non-blocking receive: returns the next byte if one is already buffered,
/// or None if nothing arrives within the timeout.
pub async fn cdc_try_recv() -> Result<Option<u8>, Error> {
    match select(Timer::after_millis(5), cdc_recv()).await {
        Either::First(_) => Ok(None),
        Either::Second(Ok(b)) => Ok(Some(b)),
        Either::Second(Err(e)) => Err(e),
    }
}
