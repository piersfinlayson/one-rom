// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_CLAIM_HW`, `ORA_ID_SET_IRQ_ENABLED` and the USB
//! reservation made at plugin launch.
//!
//! A claim lasts until One ROM reboots and can't be released, so each test
//! puts the claims back as they were before it ran.
//!
//! The emulator doesn't have an interrupt controller, so the tests check what
//! `ORA_ID_SET_IRQ_ENABLED` returns, not whether the interrupt is enabled.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::PROPERTY1_RESERVE_USB;
use onerom_metadata::onerom_hw_claim_t as HwClaim;
use onerom_metadata::onerom_hw_claim2_t as HwClaim2;
use onerom_metadata::onerom_irq_t as Irq;

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;
const ANY: u32 = ffi::ORA_HW_ANY;

const IRQ_COUNT: u32 = 52;

/// The interrupts a plugin can't claim with ORA_ID_CLAIM_HW.  IRQ_USBCTRL_IRQ
/// is claimed only by the USB reservation.  They are listed here rather than
/// read from the firmware so the test fails if the firmware refuses the wrong
/// ones.
const FIRMWARE_IRQS: &[Irq] = &[
    Irq::IRQ_TIMER0_IRQ_0,
    Irq::IRQ_TIMER0_IRQ_1,
    Irq::IRQ_DMA_IRQ_0,
    Irq::IRQ_USBCTRL_IRQ,
    Irq::IRQ_IO_IRQ_BANK0_NS,
    Irq::IRQ_IO_IRQ_QSPI,
    Irq::IRQ_IO_IRQ_QSPI_NS,
    Irq::IRQ_SIO_IRQ_FIFO,
    Irq::IRQ_SIO_IRQ_FIFO_NS,
    Irq::IRQ_SIO_IRQ_BELL_NS,
    Irq::IRQ_CLOCKS_IRQ,
    Irq::IRQ_OTP_IRQ,
    Irq::IRQ_PROC0_IRQ_CTI,
    Irq::IRQ_PROC1_IRQ_CTI,
    Irq::IRQ_PLL_SYS_IRQ,
    Irq::IRQ_PLL_USB_IRQ,
    Irq::IRQ_POWMAN_IRQ_POW,
    Irq::IRQ_POWMAN_IRQ_TIMER,
];

/// The interrupts that exist on both cores, so both plugins can claim them.
const PER_CORE_IRQS: &[Irq] = &[
    Irq::IRQ_IO_IRQ_BANK0,
    Irq::IRQ_SIO_IRQ_BELL,
    Irq::IRQ_SIO_IRQ_MTIMECMP,
];

fn mask(irqs: &[Irq]) -> u64 {
    irqs.iter().fold(0, |m, irq| m | (1 << irq.0))
}

fn check(what: &str, got: OraResult, want: OraResult) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

fn check_eq<T: PartialEq + std::fmt::LowerHex>(what: &str, got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:#x}, want {want:#x}"))
    }
}

/// The claim state these tests change.
struct Saved {
    dma: u16,
    hw: u8,
    hw2: u8,
    alarms: u8,
    spinlocks: u32,
    pwm: u16,
    irq_system: u64,
    irq_user: u64,
}

fn with_saved(
    emu: &Emulator,
    test: impl FnOnce(&Emulator) -> Result<(), String>,
) -> Result<(), String> {
    let saved = Saved {
        dma: emu.dma_channel_claims(),
        hw: emu.hw_claims(),
        hw2: emu.hw_claims2(),
        alarms: emu.timer_alarm_claims(),
        spinlocks: emu.spinlock_claims(),
        pwm: emu.pwm_slice_claims(),
        irq_system: emu.irq_claims(SYSTEM),
        irq_user: emu.irq_claims(USER),
    };
    let result = test(emu);
    emu.set_dma_channel_claims(saved.dma);
    emu.set_hw_claims(saved.hw);
    emu.set_hw_claims2(saved.hw2);
    emu.set_timer_alarm_claims(saved.alarms);
    emu.set_spinlock_claims(saved.spinlocks);
    emu.set_pwm_slice_claims(saved.pwm);
    emu.set_irq_claims(SYSTEM, saved.irq_system);
    emu.set_irq_claims(USER, saved.irq_user);
    emu.set_calling_plugin(SYSTEM);
    result
}

/// A type of hardware peripheral whose claims are recorded as bits in one
/// runtime info field.
struct HwType {
    name: &'static str,
    hw: ffi::ora_hw_t,
    count: u32,
    /// The instances the firmware claims at boot.
    firmware: u32,
    /// The type's claims, bit N for instance N.
    claims: fn(&Emulator) -> u32,
}

fn hw_bits(emu: &Emulator, first: u8, count: u32) -> u32 {
    (u32::from(emu.hw_claims()) >> first.trailing_zeros()) & ((1 << count) - 1)
}

fn hw2_bits(emu: &Emulator, first: u8) -> u32 {
    (u32::from(emu.hw_claims2()) >> first.trailing_zeros()) & 1
}

const HW_TYPES: &[HwType] = &[
    HwType {
        name: "DMA channel",
        hw: ffi::ora_hw_t_ORA_HW_DMA_CHANNEL,
        count: 16,
        // Channels 0 to 3 for serving, including the address monitor's
        // channel, and channel 15 for copying ROM images.
        firmware: 0x800F,
        claims: |emu| u32::from(emu.dma_channel_claims()),
    },
    HwType {
        name: "UART",
        hw: ffi::ora_hw_t_ORA_HW_UART,
        count: 2,
        firmware: 0,
        claims: |emu| hw_bits(emu, HwClaim::HW_CLAIM_UART_0, 2),
    },
    HwType {
        name: "I2C",
        hw: ffi::ora_hw_t_ORA_HW_I2C,
        count: 2,
        firmware: 0,
        claims: |emu| hw_bits(emu, HwClaim::HW_CLAIM_I2C_0, 2),
    },
    HwType {
        name: "SPI",
        hw: ffi::ora_hw_t_ORA_HW_SPI,
        count: 2,
        firmware: 0,
        claims: |emu| hw_bits(emu, HwClaim::HW_CLAIM_SPI_0, 2),
    },
    HwType {
        name: "ADC",
        hw: ffi::ora_hw_t_ORA_HW_ADC,
        count: 1,
        firmware: 0,
        claims: |emu| hw_bits(emu, HwClaim::HW_CLAIM_ADC, 1),
    },
    HwType {
        name: "USB",
        hw: ffi::ora_hw_t_ORA_HW_USB,
        count: 1,
        firmware: 0,
        claims: |emu| hw_bits(emu, HwClaim::HW_CLAIM_USB, 1),
    },
    HwType {
        name: "SHA256",
        hw: ffi::ora_hw_t_ORA_HW_SHA256,
        count: 1,
        firmware: 0,
        claims: |emu| hw2_bits(emu, HwClaim2::HW_CLAIM2_SHA256),
    },
    HwType {
        name: "TRNG",
        hw: ffi::ora_hw_t_ORA_HW_TRNG,
        count: 1,
        firmware: 0,
        claims: |emu| hw2_bits(emu, HwClaim2::HW_CLAIM2_TRNG),
    },
    HwType {
        name: "timer alarm",
        hw: ffi::ora_hw_t_ORA_HW_TIMER_ALARM,
        count: 8,
        // TIMER0 alarm 0, whose interrupt a plugin registers with
        // ORA_ID_REGISTER_IRQ, and alarm 1 for the LEDs.
        firmware: 0x03,
        claims: |emu| u32::from(emu.timer_alarm_claims()),
    },
    HwType {
        name: "spinlock",
        hw: ffi::ora_hw_t_ORA_HW_SPINLOCK,
        count: 32,
        // The firmware uses spinlocks 28 to 31 and claims every spinlock
        // erratum RP2350-E2 makes unreliable.  Spinlocks 5, 6, 7, 10, 11 and
        // 18 to 27 are free.
        firmware: 0xF003_F31F,
        claims: |emu| emu.spinlock_claims(),
    },
    HwType {
        name: "PWM slice",
        hw: ffi::ora_hw_t_ORA_HW_PWM_SLICE,
        count: 12,
        firmware: 0,
        claims: |emu| u32::from(emu.pwm_slice_claims()),
    },
];

fn hw_type_refusals(emu: &Emulator, hw_type: &HwType) -> Result<(), String> {
    let name = hw_type.name;
    check(
        &format!("{name}: NULL instance"),
        emu.claim_hw_null(hw_type.hw),
        OraResult::InvalidArg,
    )?;
    let past_last = (hw_type.count < 32).then_some(hw_type.count);
    for (what, instance, flags) in [
        ("instance one past the last", past_last, 0),
        ("instance just below ANY", Some(ANY - 1), 0),
        ("flags bit 0", Some(ANY), 1),
        ("flags bit 31", Some(ANY), 1 << 31),
    ] {
        let Some(instance) = instance else { continue };
        let what = format!("{name}: {what}");
        let (r, out) = emu.claim_hw(hw_type.hw, instance, flags);
        check(&what, r, OraResult::InvalidArg)?;
        check_eq(&what, out, instance)?;
    }
    check_eq(
        &format!("{name}: after refusals"),
        (hw_type.claims)(emu),
        hw_type.firmware,
    )
}

fn hw_type_claims(emu: &Emulator, hw_type: &HwType) -> Result<(), String> {
    let name = hw_type.name;
    let all = if hw_type.count == 32 {
        u32::MAX
    } else {
        (1 << hw_type.count) - 1
    };
    check_eq(
        &format!("{name}: at boot"),
        (hw_type.claims)(emu),
        hw_type.firmware,
    )?;
    hw_type_refusals(emu, hw_type)?;

    for instance in (0..hw_type.count).filter(|i| hw_type.firmware & (1 << i) != 0) {
        let what = format!("{name}: claim the firmware's instance {instance}");
        let (r, out) = emu.claim_hw(hw_type.hw, instance, 0);
        check(&what, r, OraResult::InUse)?;
        check_eq(&what, out, instance)?;
    }

    let mut claims = hw_type.firmware;
    emu.set_calling_plugin(SYSTEM);
    if hw_type.count == 1 {
        let what = format!("{name}: system claims any instance");
        let (r, out) = emu.claim_hw(hw_type.hw, ANY, 0);
        check(&what, r, OraResult::Ok)?;
        check_eq(&what, out, 0)?;
        claims |= 1;
    } else {
        // Claim the highest free instance so the test fails if the firmware
        // claims the lowest free one instead.
        let specific = (0..hw_type.count)
            .rev()
            .find(|i| hw_type.firmware & (1 << i) == 0)
            .ok_or_else(|| format!("{name}: no instance is free at boot"))?;
        let what = format!("{name}: system claims instance {specific}");
        let (r, out) = emu.claim_hw(hw_type.hw, specific, 0);
        check(&what, r, OraResult::Ok)?;
        check_eq(&what, out, specific)?;
        claims |= 1 << specific;
        emu.set_calling_plugin(USER);
        let what = format!("{name}: user claims the system's instance");
        let (r, out) = emu.claim_hw(hw_type.hw, specific, 0);
        check(&what, r, OraResult::InUse)?;
        check_eq(&what, out, specific)?;
    }
    check_eq(
        &format!("{name}: after the first claim"),
        (hw_type.claims)(emu),
        claims,
    )?;

    // Alternate the plugins until every instance is claimed.
    let mut plugin = USER;
    while claims != all {
        emu.set_calling_plugin(plugin);
        let what = format!("{name}: claim any instance");
        let (r, out) = emu.claim_hw(hw_type.hw, ANY, 0);
        check(&what, r, OraResult::Ok)?;
        if (out >= hw_type.count) || (claims & (1 << out) != 0) {
            return Err(format!(
                "{what}: got instance {out:#x}, want a free instance below {} (claims {claims:#x})",
                hw_type.count
            ));
        }
        claims |= 1 << out;
        check_eq(&what, (hw_type.claims)(emu), claims)?;
        plugin = if plugin == SYSTEM { USER } else { SYSTEM };
    }
    let what = format!("{name}: claim any instance with none free");
    let (r, out) = emu.claim_hw(hw_type.hw, ANY, 0);
    check(&what, r, OraResult::InUse)?;
    check_eq(&what, out, ANY)?;
    check_eq(&what, (hw_type.claims)(emu), all)
}

/// Tests each type in `HW_TYPES`: the firmware's claims at boot, invalid
/// arguments, a claim of one instance, claims of any instance until none is
/// free, and unknown types.
pub fn test_hw_claims(emu: &Emulator) -> Result<(), String> {
    for hw_type in HW_TYPES {
        with_saved(emu, |emu| hw_type_claims(emu, hw_type))?;
    }
    with_saved(emu, |emu| {
        for hw in [ffi::ora_hw_t_ORA_HW_IRQ + 1, 0xFF] {
            let what = format!("unknown hardware type {hw}");
            let (r, out) = emu.claim_hw(hw, 0, 0);
            check(&what, r, OraResult::NotSupported)?;
            check_eq(&what, out, 0)?;
        }
        Ok(())
    })
}

fn claim_irq(emu: &Emulator, plugin: ffi::ora_plugin_type_t, irq: u32) -> OraResult {
    emu.set_calling_plugin(plugin);
    emu.claim_hw(ffi::ora_hw_t_ORA_HW_IRQ, irq, 0).0
}

fn irq_claims(emu: &Emulator) -> Result<(), String> {
    let firmware = mask(FIRMWARE_IRQS);
    let per_core = mask(PER_CORE_IRQS);
    let all = (1u64 << IRQ_COUNT) - 1;
    let claimable = all & !firmware;
    let irq = ffi::ora_hw_t_ORA_HW_IRQ;

    check_eq(
        "system's interrupt claims at boot",
        emu.irq_claims(SYSTEM),
        0,
    )?;
    check_eq("user's interrupt claims at boot", emu.irq_claims(USER), 0)?;

    for (what, instance) in [
        ("interrupt 52", IRQ_COUNT),
        ("interrupt just below ANY", ANY - 1),
    ] {
        let (r, out) = emu.claim_hw(irq, instance, 0);
        check(what, r, OraResult::InvalidArg)?;
        check_eq(what, out, instance)?;
    }
    for fw in FIRMWARE_IRQS {
        for (plugin, who) in [(SYSTEM, "system"), (USER, "user")] {
            let what = format!("{who} claims interrupt {}, which plugins can't claim", fw.0);
            check(
                &what,
                claim_irq(emu, plugin, u32::from(fw.0)),
                OraResult::InUse,
            )?;
        }
    }
    check_eq(
        "system's interrupt claims after the failed claims",
        emu.irq_claims(SYSTEM),
        0,
    )?;
    check_eq(
        "user's interrupt claims after the failed claims",
        emu.irq_claims(USER),
        0,
    )?;

    for irq in PER_CORE_IRQS.iter().map(|i| u32::from(i.0)) {
        let what = format!("per-core interrupt {irq}");
        check(
            &format!("system claims {what}"),
            claim_irq(emu, SYSTEM, irq),
            OraResult::Ok,
        )?;
        check(
            &format!("user claims {what}"),
            claim_irq(emu, USER, irq),
            OraResult::Ok,
        )?;
        check(
            &format!("system claims {what} again"),
            claim_irq(emu, SYSTEM, irq),
            OraResult::InUse,
        )?;
    }
    check_eq("system per-core claims", emu.irq_claims(SYSTEM), per_core)?;
    check_eq("user per-core claims", emu.irq_claims(USER), per_core)?;

    let uart0 = u32::from(Irq::IRQ_UART0_IRQ.0);
    check(
        "system claims UART0's interrupt",
        claim_irq(emu, SYSTEM, uart0),
        OraResult::Ok,
    )?;
    check(
        "user claims the system's UART0 interrupt",
        claim_irq(emu, USER, uart0),
        OraResult::InUse,
    )?;
    check(
        "system claims UART0's interrupt again",
        claim_irq(emu, SYSTEM, uart0),
        OraResult::InUse,
    )?;
    check_eq(
        "user's interrupt claims after the UART0 claims",
        emu.irq_claims(USER),
        per_core,
    )?;

    // A claim of any interrupt returns the lowest one this plugin can claim.
    emu.set_irq_claims(SYSTEM, 0);
    emu.set_irq_claims(USER, 0);
    let lowest = claimable.trailing_zeros();
    emu.set_calling_plugin(SYSTEM);
    let (r, out) = emu.claim_hw(irq, ANY, 0);
    check("system claims any interrupt", r, OraResult::Ok)?;
    check_eq("system claims any interrupt", out, lowest)?;

    // Alternate the plugins until neither can claim more.
    let mut plugin = USER;
    let mut full = [false, false];
    while !(full[0] && full[1]) {
        emu.set_calling_plugin(plugin);
        let (r, out) = emu.claim_hw(irq, ANY, 0);
        let index = if plugin == SYSTEM { 0 } else { 1 };
        if r == OraResult::InUse {
            check_eq("claim any interrupt with none free", out, ANY)?;
            full[index] = true;
        } else if r != OraResult::Ok || claimable & (1 << out) == 0 {
            return Err(format!(
                "claim any interrupt: got {r:?} and interrupt {out}, want Ok with a claimable interrupt or InUse"
            ));
        }
        plugin = if plugin == SYSTEM { USER } else { SYSTEM };
    }
    let (system, user) = (emu.irq_claims(SYSTEM), emu.irq_claims(USER));
    check_eq(
        "every claimable interrupt claimed",
        system | user,
        claimable,
    )?;
    check_eq(
        "interrupts claimed by both plugins",
        system & user,
        per_core,
    )
}

pub fn test_irq_claims(emu: &Emulator) -> Result<(), String> {
    with_saved(emu, irq_claims)
}

fn set_irq_enabled(emu: &Emulator) -> Result<(), String> {
    let uart0 = Irq::IRQ_UART0_IRQ.0;
    let gpio = Irq::IRQ_IO_IRQ_BANK0.0;
    check(
        "system claims UART0's interrupt",
        claim_irq(emu, SYSTEM, u32::from(uart0)),
        OraResult::Ok,
    )?;
    check(
        "user claims its GPIO interrupt",
        claim_irq(emu, USER, u32::from(gpio)),
        OraResult::Ok,
    )?;

    emu.set_calling_plugin(SYSTEM);
    check(
        "system enables its interrupt",
        emu.set_irq_enabled(uart0, 1, 0),
        OraResult::Ok,
    )?;
    check(
        "system disables its interrupt",
        emu.set_irq_enabled(uart0, 0, 0),
        OraResult::Ok,
    )?;
    for (what, irq, enable, flags) in [
        ("enable argument 2", uart0, 2, 0),
        ("flags bit 0", uart0, 1, 1),
        ("flags bit 31", uart0, 1, 1 << 31),
        ("interrupt 52", 52, 1, 0),
        ("interrupt 0xFF", 0xFF, 1, 0),
        ("an unclaimed interrupt", Irq::IRQ_UART1_IRQ.0, 1, 0),
        ("the firmware's interrupt", Irq::IRQ_DMA_IRQ_0.0, 1, 0),
        ("the user's GPIO interrupt", gpio, 1, 0),
    ] {
        check(
            &format!("system: {what}"),
            emu.set_irq_enabled(irq, enable, flags),
            OraResult::InvalidArg,
        )?;
    }

    emu.set_calling_plugin(USER);
    check(
        "user enables its GPIO interrupt",
        emu.set_irq_enabled(gpio, 1, 0),
        OraResult::Ok,
    )?;
    check(
        "user enables the system's interrupt",
        emu.set_irq_enabled(uart0, 1, 0),
        OraResult::InvalidArg,
    )
}

pub fn test_set_irq_enabled(emu: &Emulator) -> Result<(), String> {
    with_saved(emu, set_irq_enabled)
}

fn usb_held(emu: &Emulator) -> bool {
    emu.hw_claims() & HwClaim::HW_CLAIM_USB != 0
}

fn holds_usb_irq(emu: &Emulator, plugin: ffi::ora_plugin_type_t) -> bool {
    emu.irq_claims(plugin) & (1 << Irq::IRQ_USBCTRL_IRQ.0) != 0
}

fn check_holder(
    emu: &Emulator,
    what: &str,
    holder: Option<ffi::ora_plugin_type_t>,
) -> Result<(), String> {
    let got = (
        usb_held(emu),
        holds_usb_irq(emu, SYSTEM),
        holds_usb_irq(emu, USER),
    );
    let want = (
        holder.is_some(),
        holder == Some(SYSTEM),
        holder == Some(USER),
    );
    if got == want {
        Ok(())
    } else {
        Err(format!(
            "{what}: (USB claimed, system has USB interrupt, user has USB interrupt) is {got:?}, want {want:?}"
        ))
    }
}

fn check_usb_claim(
    emu: &Emulator,
    what: &str,
    plugin: ffi::ora_plugin_type_t,
    want: OraResult,
) -> Result<(), String> {
    emu.set_calling_plugin(plugin);
    let (r, out) = emu.claim_hw(ffi::ora_hw_t_ORA_HW_USB, ANY, 0);
    check(what, r, want)?;
    check_eq(what, out, if want == OraResult::Ok { 0 } else { ANY })
}

fn usb_reservation(emu: &Emulator, first: ffi::ora_plugin_type_t) -> Result<(), String> {
    let second = if first == SYSTEM { USER } else { SYSTEM };
    let flag = ffi::ORA_ENTRY_FLAG_USB_RESERVED as u8;
    emu.set_hw_claims(0);
    emu.set_irq_claims(SYSTEM, 0);
    emu.set_irq_claims(USER, 0);

    check_eq(
        "reserve without PROPERTY1_RESERVE_USB",
        emu.usb_reserve(first, 0),
        0,
    )?;
    check_holder(emu, "reserve without PROPERTY1_RESERVE_USB", None)?;

    check_eq(
        "reserve with PROPERTY1_RESERVE_USB",
        emu.usb_reserve(first, PROPERTY1_RESERVE_USB),
        flag,
    )?;
    check_holder(emu, "reserve with PROPERTY1_RESERVE_USB", Some(first))?;
    check_eq(
        "the other plugin reserves too",
        emu.usb_reserve(second, PROPERTY1_RESERVE_USB),
        0,
    )?;
    check_holder(emu, "the other plugin reserves too", Some(first))?;

    check_usb_claim(emu, "the holder claims USB", first, OraResult::Ok)?;
    check_usb_claim(emu, "the holder claims USB again", first, OraResult::Ok)?;
    check_usb_claim(emu, "the other plugin claims USB", second, OraResult::InUse)?;

    let usbctrl = Irq::IRQ_USBCTRL_IRQ.0;
    emu.set_calling_plugin(first);
    check(
        "the holder enables USB's interrupt",
        emu.set_irq_enabled(usbctrl, 1, 0),
        OraResult::Ok,
    )?;
    check(
        "the holder claims USB's interrupt directly",
        claim_irq(emu, first, u32::from(usbctrl)),
        OraResult::InUse,
    )?;
    emu.set_calling_plugin(second);
    check(
        "the other plugin enables USB's interrupt",
        emu.set_irq_enabled(usbctrl, 1, 0),
        OraResult::InvalidArg,
    )?;
    check_holder(emu, "after the claims", Some(first))
}

fn usb_claimed_first(emu: &Emulator) -> Result<(), String> {
    emu.set_hw_claims(0);
    emu.set_irq_claims(SYSTEM, 0);
    emu.set_irq_claims(USER, 0);
    check_usb_claim(emu, "user claims USB", USER, OraResult::Ok)?;
    check_eq(
        "system reserves USB after the user claimed it",
        emu.usb_reserve(SYSTEM, PROPERTY1_RESERVE_USB),
        0,
    )?;
    let got = (
        usb_held(emu),
        holds_usb_irq(emu, SYSTEM),
        holds_usb_irq(emu, USER),
    );
    if got != (true, false, false) {
        return Err(format!(
            "system reserves USB after the user claimed it: (USB claimed, system has USB interrupt, user has USB interrupt) is {got:?}, want (true, false, false)"
        ));
    }
    check_usb_claim(emu, "system claims USB", SYSTEM, OraResult::InUse)
}

/// Tests the launch-time USB reservation made first by each plugin in turn,
/// followed by USB claims and interrupt enables.  Also tests a reservation made
/// after USB is already claimed.
pub fn test_usb_reservation(emu: &Emulator) -> Result<(), String> {
    with_saved(emu, |emu| usb_reservation(emu, SYSTEM))?;
    with_saved(emu, |emu| usb_reservation(emu, USER))?;
    with_saved(emu, usb_claimed_first)
}
