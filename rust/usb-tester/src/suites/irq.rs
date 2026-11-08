// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests of the USB controller interrupt, which is reserved for the plugin at
//! launch and handled through its header's `irq_handler`.
//!
//! tinyusb's dcd_rp2040 driver isn't built into this harness.  A stand-in for
//! its `dcd_int_handler()` counts the plugin's calls, and a scenario enables
//! the interrupt through the plugin's interrupt functions, as the driver does.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::PROPERTY1_RESERVE_USB;
use onerom_metadata::onerom_irq_t as Irq;
use onerom_plugin_tester::run::Outcome;

use crate::device::{self, Device};
use crate::ffi as shim;
use crate::{Ctx, Scenario};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USBCTRL: u8 = Irq::IRQ_USBCTRL_IRQ.0;
const UART0: u8 = Irq::IRQ_UART0_IRQ.0;

/// Reserves USB for the system plugin, as the firmware does at launch for a
/// plugin whose header sets ORA_PROPERTY1_RESERVE_USB.
fn arrange_reserved(emu: &Emulator) {
    let flag = emu.usb_reserve(SYSTEM, PROPERTY1_RESERVE_USB);
    device::set_entry_flags(flag);
}

fn the_header_reserves_usb(_dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let props = unsafe { shim::usb_host_test_header_properties1() };
    let want = PROPERTY1_RESERVE_USB | ffi::ORA_PROPERTY1_SUPPORTS_YIELD as u8;
    if props & want != want {
        return Err(format!(
            "properties1 is {props:#04x}, want bits {want:#04x} set"
        ));
    }
    Ok(Outcome::Pass)
}

fn dcd_int_handler_calls() -> u32 {
    unsafe { shim::usb_host_test_dcd_int_handler_calls() }
}

fn header_irq(irq: u8) {
    unsafe { shim::usb_host_test_header_irq(irq) };
}

fn the_usb_interrupt_reaches_tinyusb(_dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let start = dcd_int_handler_calls();
    header_irq(USBCTRL);
    if dcd_int_handler_calls() != start + 1 {
        return Err("the USB interrupt didn't call dcd_int_handler()".into());
    }
    header_irq(UART0);
    if dcd_int_handler_calls() != start + 1 {
        return Err("the UART0 interrupt called dcd_int_handler()".into());
    }
    Ok(Outcome::Pass)
}

/// The last `ORA_ID_SET_IRQ_ENABLED` call, as (interrupt, enable, result).
fn last_irq_enabled() -> (u8, u8, OraResult) {
    let packed = unsafe { shim::usb_host_test_last_irq_enabled() };
    (
        packed as u8,
        (packed >> 8) as u8,
        OraResult::from((packed >> 16) as ffi::ora_result_t),
    )
}

fn enable_usb_irq(enabled: u8, want: OraResult) -> Result<(), String> {
    let calls = unsafe { shim::usb_host_test_irq_enabled_calls() };
    unsafe { shim::usb_host_test_enable_usb_irq(enabled) };
    if unsafe { shim::usb_host_test_irq_enabled_calls() } != calls + 1 {
        return Err("the plugin didn't call ORA_ID_SET_IRQ_ENABLED".into());
    }
    let got = last_irq_enabled();
    if got != (USBCTRL, enabled, want) {
        return Err(format!(
            "ORA_ID_SET_IRQ_ENABLED(irq {}, enable {}) returned {:?}, want (irq {USBCTRL}, enable {enabled}) returning {want:?}",
            got.0, got.1, got.2
        ));
    }
    Ok(())
}

fn the_reserved_interrupt_enables_and_disables(
    _dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    enable_usb_irq(1, OraResult::Ok)?;
    enable_usb_irq(0, OraResult::Ok)?;
    Ok(Outcome::Pass)
}

fn enabling_fails_without_the_reservation(
    _dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    enable_usb_irq(1, OraResult::InvalidArg)?;
    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "irq.the_header_reserves_usb",
        about: "the plugin header sets ORA_PROPERTY1_RESERVE_USB and ORA_PROPERTY1_SUPPORTS_YIELD",
        run: the_header_reserves_usb,
        before_start: None,
    },
    Scenario {
        name: "irq.the_usb_interrupt_reaches_tinyusb",
        about: "the header's irq_handler calls dcd_int_handler() for the USB interrupt and for no other",
        run: the_usb_interrupt_reaches_tinyusb,
        before_start: Some(arrange_reserved),
    },
    Scenario {
        name: "irq.the_reserved_interrupt_enables_and_disables",
        about: "with USB reserved, enabling and disabling the USB interrupt through ORA_ID_SET_IRQ_ENABLED succeeds",
        run: the_reserved_interrupt_enables_and_disables,
        before_start: Some(arrange_reserved),
    },
    Scenario {
        name: "irq.enabling_fails_without_the_reservation",
        about: "without the reservation, enabling the USB interrupt fails with InvalidArg",
        run: enabling_fails_without_the_reservation,
        before_start: None,
    },
];
