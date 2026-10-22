// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Host scenarios: does `onerom-rbcp-host` work against the device?
//!
//! One ROM Lab uses that crate to query a One ROM in its socket. These
//! scenarios run it here over emulated bus cycles, so it is tested without
//! hardware.

use std::ops::Range;

use onerom_rbcp_host::{Config, Session};

use crate::Scenario;
use crate::driver::{Bus, POLL_LIMIT};
use crate::{Ctx, Outcome};

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "host.identity",
        spec_ref: "onerom-rbcp-host: enter, GET_DEVICE_TYPE, GET_DEVICE_VERSION, GET_BOOT_SLOT_INFO, restore_and_exit",
        run: identity,
    },
    Scenario {
        name: "host.restore_on_drop",
        spec_ref: "onerom-rbcp-host: a session dropped without restore_and_exit",
        run: restore_on_drop,
    },
];

/// The tester's bus as `onerom-rbcp-host` reads it.
///
/// The crate's reads can't fail, so the first failure is kept here and
/// reported once the crate returns.
struct HostBus<'a, 'b> {
    bus: &'a mut Bus<'b>,
    error: Option<String>,
}

impl onerom_rbcp_host::Bus for HostBus<'_, '_> {
    fn read(&mut self, addr: u32) -> u8 {
        self.bus.read(addr).unwrap_or_else(|e| {
            self.error.get_or_insert(e);
            0xFF
        })
    }
}

impl HostBus<'_, '_> {
    fn check(&mut self) -> Result<(), String> {
        self.error.take().map_or(Ok(()), Err)
    }
}

fn config(ctx: &Ctx) -> Config {
    Config {
        command_page: ctx.command_page(),
        bch_start: ctx.bch_start(),
        bch_size: ctx.bch_size(),
        unobserved: ctx.unobserved,
        poll_limit: POLL_LIMIT,
    }
}

/// The identity query One ROM Lab makes, then a check that the session left
/// the image as it found it.
fn identity(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let region = ctx.bch_start()..ctx.bch_start() + u32::from(ctx.bch_size());
    let before = read_region(bus, region.clone())?;
    let firmware_version = bus.device_version()?;

    let mut host = HostBus { bus, error: None };
    let mut session = Session::enter(&mut host, config(ctx)).map_err(|e| format!("enter: {e}"))?;

    let device_type = session
        .get_device_type()
        .map_err(|e| format!("GET_DEVICE_TYPE: {e}"))?;
    if device_type.as_bytes() != b"One ROM" {
        return Err(format!("device type {:?}", device_type.as_bytes()));
    }

    let version = session
        .get_device_version()
        .map_err(|e| format!("GET_DEVICE_VERSION: {e}"))?;
    if version.as_bytes() != firmware_version.as_bytes() {
        return Err(format!(
            "device version {:?}, firmware reports {firmware_version:?}",
            String::from_utf8_lossy(version.as_bytes())
        ));
    }

    let boot = session
        .get_boot_slot_info()
        .map_err(|e| format!("GET_BOOT_SLOT_INFO: {e}"))?;
    let want = (Some(ctx.boot_flash_slot()), Some(ctx.active_ram_slot));
    if (boot.flash_slot, boot.ram_slot) != want {
        return Err(format!("boot slot {boot:?}, expected {want:?}"));
    }

    session
        .restore_and_exit()
        .map_err(|e| format!("restore_and_exit: {e}"))?;
    host.check()?;

    expect_region(bus, region, &before, "restore_and_exit")
}

/// The back-channel region read over the bus.
fn read_region(bus: &mut Bus, region: Range<u32>) -> Result<Vec<u8>, String> {
    region.map(|a| bus.read(a)).collect()
}

fn expect_region(
    bus: &mut Bus,
    region: Range<u32>,
    before: &[u8],
    after: &str,
) -> Result<Outcome, String> {
    for (addr, &want) in region.zip(before) {
        let got = bus.read(addr)?;
        if got != want {
            return Err(format!(
                "after {after} 0x{addr:06X} serves 0x{got:02X}, was 0x{want:02X}"
            ));
        }
    }
    Ok(Outcome::Pass)
}

/// A session dropped part way through, as a failure drops it, restores the
/// back-channel.
fn restore_on_drop(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let region = ctx.bch_start()..ctx.bch_start() + u32::from(ctx.bch_size());
    let before = read_region(bus, region.clone())?;

    let mut host = HostBus { bus, error: None };
    let mut session = Session::enter(&mut host, config(ctx)).map_err(|e| format!("enter: {e}"))?;
    session
        .get_device_type()
        .map_err(|e| format!("GET_DEVICE_TYPE: {e}"))?;
    drop(session);
    host.check()?;

    expect_region(bus, region, &before, "dropping the session")
}
