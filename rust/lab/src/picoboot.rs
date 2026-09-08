// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! What Lab serves on the PICOBOOT interface.
//!
//! Lab answers the read-only half of the protocol, so a PICOBOOT host describes
//! the running board and reboots it into BOOTSEL without a terminal open and
//! without the button being pressed.  [`LabOps`] is the whole of that decision.
//! The wiring — the two bulk endpoints, the control handler and the driver task
//! — is in [`usb`](crate::usb).

use picobootx::{Ecc, Exclusive, Info, NoCustom, Ops, Reboot, Result, Status, Target};

/// The reboot flag a host sets to ask for the bootloader rather than a restart,
/// per the RP2350 datasheet's reboot flags.
const REBOOT_TO_BOOTSEL: u32 = 0x2;
use picobootx_embassy::{PicobootClass, Rp2350EndpointControl};
use picobootx_rp2350::Rp2350;

use embassy_rp::peripherals::USB;
use embassy_rp::usb::{In, Out};

/// The host-to-device bulk endpoint, as embassy-rp's driver hands it over.
pub type EpOut = embassy_rp::usb::Endpoint<'static, USB, Out>;

/// The device-to-host bulk endpoint.
pub type EpIn = embassy_rp::usb::Endpoint<'static, USB, In>;

/// picobootx as Lab holds it: the RP2350's halt control, no commands of Lab's
/// own, and [`LabOps`] deciding what is answered.
pub type Device = PicobootClass<'static, LabOps, NoCustom, Rp2350EndpointControl>;

/// The control endpoint's half of [`Device`], which `embassy_usb::Builder` is
/// given.
pub type Control =
    picobootx_embassy::ControlHandler<'static, 'static, LabOps, NoCustom, Rp2350EndpointControl>;

/// What Lab does when a PICOBOOT host asks it something.
///
/// Reads, `GET_INFO`, the three advisory commands and `REBOOT2` are answered by
/// the RP2350's own implementations, so a host sees the part it is talking to.
///
/// Everything that changes what the board holds is refused: flash writes, flash
/// erases, and OTP in both directions.  Each is refused from the `_prepare` the
/// library calls before any data moves, so the host is told no before a byte is
/// sent or expected, and the status is [`Status::NotPermitted`] — Lab knows the
/// command and will not do it, which is a different answer from not recognising
/// it.  The operations behind those four refusals are left to `Ops`, whose
/// defaults refuse as well.
pub struct LabOps {
    /// The RP2350's own implementations, which answer the commands Lab serves.
    part: Rp2350,
}

impl LabOps {
    /// The operations Lab serves.
    #[must_use]
    pub const fn new() -> Self {
        Self { part: Rp2350 }
    }
}

impl Default for LabOps {
    fn default() -> Self {
        Self::new()
    }
}

impl Ops for LabOps {
    // The three advisory commands.  An RP2350 has nothing to do for either XIP
    // command and agrees to every kind of exclusivity the protocol defines.
    fn exclusive_access(&mut self, mode: Exclusive) -> Result {
        self.part.exclusive_access(mode)
    }

    fn exit_xip(&mut self) -> Result {
        self.part.exit_xip()
    }

    fn enter_xip(&mut self) -> Result {
        self.part.enter_xip()
    }

    // READ, over the three regions the part answers reads from.  This is what
    // lets a host read the binary info block out of flash and describe what is
    // running.
    fn read_prepare(&mut self, addr: u32, size: u32) -> Result {
        self.part.read_prepare(addr, size)
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result {
        self.part.read(addr, buf)
    }

    // GET_INFO, answered by the bootrom.  Every information type the protocol
    // defines comes through here, and each is a read, so Lab passes the lot to
    // the part: system information and the partition table from the ROM's own
    // routines, the UF2 target as nowhere, and the UF2 download status refused.
    fn get_info_prepare(&mut self, info: Info, param0: u32) -> Result<u32> {
        self.part.get_info_prepare(info, param0)
    }

    fn get_info(&mut self, info: Info, param0: u32, at_word: u32, buf: &mut [u8]) -> Result<usize> {
        self.part.get_info(info, param0, at_word, buf)
    }

    // REBOOT2, which is how a host puts Lab into BOOTSEL over USB.  The library
    // acknowledges the host first and calls the second of these once that
    // acknowledgement has gone, so the host hears the answer before the board
    // goes away.
    fn reboot_prepare(&mut self, args: &Reboot) -> Result {
        self.part.reboot_prepare(args)?;

        // A board that vanishes mid-session with no word looks like a fault.
        // Queuing this here rather than at reboot_execute puts it ahead of the
        // acknowledgement the library sends, so it goes out during the delay
        // the host asked for.
        let line = if args.flags & REBOOT_TO_BOOTSEL != 0 {
            "\r\nReboot triggered - rebooting into the bootloader\r\n"
        } else {
            "\r\nReboot triggered - rebooting into One ROM Lab\r\n"
        };
        crate::usb::cdc_send(alloc::string::String::from(line)).ok();

        Ok(())
    }

    fn reboot_execute(&mut self, args: &Reboot) {
        self.part.reboot_execute(args);
    }

    // The four refusals.  Lab is a bus reader and a tester, and a host on this
    // interface has no business rewriting the board it is measuring with.
    fn write_prepare(&mut self, addr: u32, size: u32) -> core::result::Result<Target, Status> {
        let _ = (addr, size);
        Err(Status::NotPermitted)
    }

    fn flash_erase_prepare(&mut self, addr: u32, size: u32) -> Result {
        let _ = (addr, size);
        Err(Status::NotPermitted)
    }

    fn otp_read_prepare(&mut self, row: u16, count: u16, ecc: Ecc) -> Result {
        let _ = (row, count, ecc);
        Err(Status::NotPermitted)
    }

    fn otp_write_prepare(&mut self, row: u16, count: u16, ecc: Ecc) -> Result {
        let _ = (row, count, ecc);
        Err(Status::NotPermitted)
    }
}
