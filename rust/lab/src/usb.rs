// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! USB support for RP2350
//!
//! Presents a composite device matching VID/PID 1209:F542 with:
//!   Interface 0 - dummy (0xFF/0x00/0x00, no endpoints) — reserves slot for picoboot
//!   Interface 1 - vendor / WinUSB (bulk in + out)
//!   Interface 2 - CDC ACM control
//!   Interface 3 - CDC ACM data
//!
//! The MS OS 2.0 descriptor scopes WinUSB to interface 1 only.
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
use embassy_futures::select::{Either, select};
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, ControlChanged, Receiver, Sender, State};
use embassy_usb::msos::{self, windows_version};
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use static_cell::StaticCell;

use super::serial_id;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

static mut CONFIG_DESCRIPTOR: [u8; 256] = [0; 256];
static mut BOS_DESCRIPTOR: [u8; 256] = [0; 256];
static mut MSOS_DESCRIPTOR: [u8; 256] = [0; 256];
static mut CONTROL_BUF: [u8; 64] = [0; 64];

// CDC ACM internal state.
static CDC_STATE: StaticCell<State> = StaticCell::new();

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
        // composite_with_iads sets bDeviceClass = 0xEF/0x02/0x01, which
        // differs from the tinyusb reference (0x00) but works on all hosts.
        config.composite_with_iads = true;

        let mut builder = Builder::new(
            driver,
            config,
            unsafe { &mut CONFIG_DESCRIPTOR },
            unsafe { &mut BOS_DESCRIPTOR },
            unsafe { &mut MSOS_DESCRIPTOR },
            unsafe { &mut CONTROL_BUF },
        );

        builder.msos_descriptor(windows_version::WIN8_1, VENDOR_REQUEST_MICROSOFT);

        // Interface 0: dummy (0xFF/0x00/0x00, no endpoints).
        // Occupies slot 0 so picoboot (added later) lands at interface 1.
        {
            let mut func = builder.function(0xFF, 0, 0);
            let mut iface = func.interface();
            let _alt = iface.alt_setting(0xFF, 0, 0, None);
        }

        // Interface 1: vendor / WinUSB, bulk endpoints.
        // MS OS 2.0 features scoped to this function only.
        // Used for exposing picobootx.
        {
            let mut func = builder.function(0xFF, 0, 0);
            func.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
            func.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
                "DeviceInterfaceGUIDs",
                msos::PropertyData::RegMultiSz(&[WINUSB_GUID]),
            ));
            let mut iface = func.interface();
            let mut alt = iface.alt_setting(0xFF, 0, 0, None);
            let _ep_out = alt.endpoint_bulk_out(None, 64);
            let _ep_in = alt.endpoint_bulk_in(None, 64);
        }

        // Interfaces 2+3: CDC ACM.
        let cdc = CdcAcmClass::new(&mut builder, CDC_STATE.init(State::new()), 64);

        let device = builder.build();

        Self { cdc, device }
    }
}

pub fn run(spawner: Spawner, usb: Usb) {
    let (sender, receiver, control) = usb.cdc.split_with_control();
    spawner.spawn(usb_task(usb.device).unwrap());
    spawner.spawn(cdc_writer(sender).unwrap());
    spawner.spawn(cdc_reader(receiver).unwrap());
    spawner.spawn(cdc_control(control).unwrap());
}

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    device.run().await
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
    match select(CDC_GONE.wait(), CDC_RX.receive()).await {
        Either::First(()) => Err(Error::Disconnected),
        Either::Second(b) => Ok(b),
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
