// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lab's host bus under its `c64` profile.  See [`crate::lab_host`].

use onerom_fw_tester::min_cs_act;
use onerom_fw_tester::oracle;
use std::num::NonZeroU8;

use onerom_lab_bus::{C64, HostProfile, Segment};
use onerom_rbcp_host::{Config, Error, KNOCK, Session, control, group};

use crate::driver::Bus;
use crate::lab_host::{Device, LAB_MHZ, LabHost, START_CYCLES};
use crate::{Ctx, Outcome};

/// Glitches 100ns long, which the CS monitor captures at 150MHz without a
/// minimum CS-active time, on 1 in 196 non-ROM cycles.  Lab's `H` command
/// sets it with `H:1015:3:100:450:507:196`.
///
/// In a session with [`LAB_POLL_LIMIT`], the one glitch from the knock to the
/// end of ENTER_CMD_RESP's arguments is in front of A7.
const A7_GLITCH: HostProfile = HostProfile {
    glitch_ns: 100,
    glitch_every: NonZeroU8::new(196).unwrap(),
    ..C64
};

/// One ROM Lab's poll limit, `POLL_LIMIT` in `rust/lab/src/rbcp.rs`.
const LAB_POLL_LIMIT: u32 = 1_000;

/// Lab's machines for the served chip in the mode under test, or why there
/// aren't any.
fn host(bus: &Bus, ctx: &Ctx, profile: HostProfile) -> Result<LabHost, Outcome> {
    let mode = if bus.word_mode() { 16 } else { 8 };
    let profile = profile.to_bus().expect("the profile is valid");
    LabHost::new(ctx.board, ctx.chip_type, bus.cache(), mode, profile).map_err(Outcome::Skip)
}

/// Reads the byte at `byte_addr` through Lab's host bus, then lets the device
/// run, as [`Bus::read`] does.  Returns the byte and whether the burst had a
/// glitch.
fn read(
    bus: &mut Bus,
    host: &mut LabHost,
    device: &Device,
    byte_addr: u32,
) -> Result<(u8, bool), String> {
    let word_mode = bus.word_mode();
    let addr = if word_mode { byte_addr >> 1 } else { byte_addr };
    let read = host.read(addr, |lab| device.cycle(lab))?;
    let glitched = read.segments.iter().filter(|s| s.selected).count() > 1;
    bus.resume_after(byte_addr)?;
    // A word contains the even byte on D0-D7 and the odd one on D8-D15.
    let byte = if word_mode && byte_addr & 1 == 1 {
        (read.data >> 8) as u8
    } else {
        read.data as u8
    };
    Ok((byte, glitched))
}

/// Lab drives each line, A's and S's together, and R samples the data lines at
/// the cycles the burst sets.
///
/// The data lines hold a count of cycles, so R's sample identifies its cycle.
pub fn bus_timing(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let mut host = match host(bus, ctx, C64) {
        Ok(host) => host,
        Err(skip) => return Ok(skip),
    };
    let data_gpios = host.data_gpios().to_vec();
    let count = |cycle: usize| {
        data_gpios
            .iter()
            .enumerate()
            .fold(0u64, |pins, (line, &gpio)| {
                pins | ((cycle as u64 >> line) & 1) << gpio
            })
    };
    let lines_mask = (1u32 << data_gpios.len()) - 1;

    for addr in [0x5555, 0x2AAA] {
        let mut seen = Vec::new();
        let read = host.read(addr, |lab| {
            let cycle = seen.len();
            seen.push(lab);
            count(cycle)
        })?;

        // A line driven in a cycle is read back at the start of the next.
        let driven = host.driven();
        let wanted = read
            .segments
            .iter()
            .flat_map(|s| std::iter::repeat_n(host.levels(s.addr, s.selected), s.cycles as usize));
        for (cycle, want) in wanted.enumerate() {
            let Some(&got) = seen.get(cycle + START_CYCLES as usize + 1) else {
                break;
            };
            if got & driven != want {
                return Err(format!(
                    "burst at 0x{addr:X}, cycle {cycle}: Lab's lines are 0x{:X}, expected \
                     0x{want:X}",
                    got & driven
                ));
            }
        }

        let want = (START_CYCLES + read.sample_at) & lines_mask;
        if u32::from(read.data) != want {
            return Err(format!(
                "burst at 0x{addr:X}: R sampled cycle {}, expected cycle {want}",
                read.data
            ));
        }
    }
    Ok(Outcome::Pass)
}

/// A ROM read through Lab's host bus under the C64 profile returns the ROM's
/// data.
pub fn c64_read(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let mut host = match host(bus, ctx, C64) {
        Ok(host) => host,
        Err(skip) => return Ok(skip),
    };
    let device = host.device(bus.emu(), bus.background());
    if let Err(reason) = readable(ctx, &host, &device, bus) {
        return Ok(Outcome::Skip(reason));
    }

    let chip = &ctx.config.chip_sets[ctx.set_idx].chips[0];
    let image = oracle::load(chip, ctx.chip_type, &ctx.base_dir);
    let size = ctx.addressable().min(image.len() as u32);
    let addrs = (0..16)
        .chain((0..32).map(|bit| 1 << bit))
        .chain([size - 1])
        .filter(|&addr| addr < size);
    for addr in addrs {
        let (got, _) = read(bus, &mut host, &device, addr)?;
        let want = image[addr as usize];
        if got != want {
            return Err(format!(
                "0x{addr:06X} reads 0x{got:02X}, the ROM holds 0x{want:02X}"
            ));
        }
    }
    Ok(Outcome::Pass)
}

/// Whether the CS monitor captures the glitches in Lab's bursts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glitches {
    Missed,
    Captured,
}

/// Whether Lab's host bus reads the served chip as on hardware.  Returns why
/// not where it doesn't.
fn readable(ctx: &Ctx, host: &LabHost, device: &Device, bus: &Bus) -> Result<(), String> {
    if ctx.monitor.mhz != LAB_MHZ {
        return Err(format!(
            "the slot runs at {}MHz and Lab at {LAB_MHZ}MHz, and both instances step at one rate",
            ctx.monitor.mhz
        ));
    }
    if !device.selected(bus.cache(), host.levels(0, true)) {
        return Err("Lab's select lines don't select the chip at address 0".to_string());
    }
    Ok(())
}

/// [`Glitches`] from the served slot's config, or why it isn't known.
fn glitches(ctx: &Ctx, host: &LabHost, device: &Device, bus: &Bus) -> Result<Glitches, String> {
    readable(ctx, host, device, bus)?;
    let monitor = &ctx.monitor;
    let cs = monitor.cs.clone()?;
    let min_cycles = monitor.min_cycles;

    // The first burst's first non-ROM cycle has a glitch.  Under the C64
    // profile every burst has the same shape.
    let segments = host.next_burst(0);
    let starts = segments.iter().scan(0, |at, s| {
        let start = *at;
        *at += s.cycles;
        Some((start, s))
    });
    let mut pulses: Vec<(u32, &Segment)> = starts.filter(|(_, s)| s.selected).collect();
    let (read_start, read) = pulses.pop().ok_or("the burst doesn't have a ROM read")?;

    let access = min_cs_act::access_cycles(cs, min_cycles);
    if read.cycles < access {
        return Err(format!(
            "the ROM read holds chip select for {} cycles and the CS monitor requires {access}",
            read.cycles
        ));
    }
    let idle = pulses
        .last()
        .map_or(read_start, |(start, s)| read_start - start - s.cycles);
    let rearm = min_cs_act::break_cycles(cs, min_cycles);
    if idle < rearm {
        return Err(format!(
            "chip select is inactive for {idle} cycles before the ROM read and the CS monitor \
             requires {rearm}"
        ));
    }

    let glitch = pulses.first().map_or(0, |(_, s)| s.cycles);
    let starts: Vec<u32> = pulses
        .iter()
        .filter(|(_, s)| device.selected(bus.cache(), host.levels(s.addr, true)))
        .map(|&(start, _)| start)
        .collect();
    match min_cs_act::pulses_captured(cs, min_cycles, glitch, &starts) {
        Some(true) => Ok(Glitches::Captured),
        Some(false) => Ok(Glitches::Missed),
        None => Err(format!(
            "the CS monitor captures a {glitch}-cycle glitch at some phases of its sampling only"
        )),
    }
}

/// As `HostBus`, through Lab's host bus.
struct LabBus<'a, 'b> {
    bus: &'a mut Bus<'b>,
    host: LabHost,
    device: Device<'b>,
    error: Option<String>,
    /// Each address read and whether its burst had a glitch.
    reads: Vec<(u32, bool)>,
}

impl onerom_rbcp_host::Bus for LabBus<'_, '_> {
    fn read(&mut self, addr: u32) -> u8 {
        match read(self.bus, &mut self.host, &self.device, addr) {
            Ok((byte, glitched)) => {
                self.reads.push((addr, glitched));
                byte
            }
            Err(e) => {
                self.error.get_or_insert(e);
                0xFF
            }
        }
    }
}

/// One ROM Lab's identity query under the C64 profile works where the CS
/// monitor doesn't capture the glitches, and `enter` fails where it captures
/// one before every ROM read.
pub fn c64_identity(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let host = match host(bus, ctx, C64) {
        Ok(host) => host,
        Err(skip) => return Ok(skip),
    };
    let device = host.device(bus.emu(), bus.background());
    let glitches = match glitches(ctx, &host, &device, bus) {
        Ok(glitches) => glitches,
        Err(reason) => return Ok(Outcome::Skip(reason)),
    };

    log::debug!("RBCP under the c64 profile: glitches {glitches:?}");
    let mut lab = LabBus {
        bus,
        host,
        device,
        error: None,
        reads: Vec::new(),
    };
    // A glitch captured before each ROM read means the knock never arrives
    // whole.
    let result = match (glitches, Session::enter(&mut lab, super::config(ctx))) {
        (Glitches::Missed, Ok(session)) => identity(session),
        (Glitches::Missed, Err(e)) => Err(format!("enter: {e}")),
        (Glitches::Captured, Ok(_)) => {
            Err("enter succeeded, but the CS monitor captures the C64 glitches".to_string())
        }
        (Glitches::Captured, Err(_)) => Ok(()),
    };
    // A read that failed explains whatever followed it.
    match lab.error.take() {
        Some(e) => Err(e),
        None => result.map(|()| Outcome::Pass),
    }
}

/// The identity query's device type, then `restore_and_exit`.
fn identity<B: onerom_rbcp_host::Bus>(mut session: Session<'_, B>) -> Result<(), String> {
    let device_type = session
        .get_device_type()
        .map_err(|e| format!("GET_DEVICE_TYPE: {e}"))?;
    if device_type.as_bytes() != b"One ROM" {
        return Err(format!("device type {:?}", device_type.as_bytes()));
    }
    session
        .restore_and_exit()
        .map_err(|e| format!("restore_and_exit: {e}"))
}

/// A session whose ENTER_CMD_RESP is received but never completes restores
/// the back-channel.
///
/// [`A7_GLITCH`] puts a captured glitch in front of ENTER_CMD_RESP's A7, so
/// the device takes the glitch's address as complete and A7 as status-OK.
/// The host then never sees the complete value it sent.
pub fn enter_never_completed(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let host = match host(bus, ctx, A7_GLITCH) {
        Ok(host) => host,
        Err(skip) => return Ok(skip),
    };
    let device = host.device(bus.emu(), bus.background());
    match glitches(ctx, &host, &device, bus) {
        Ok(Glitches::Captured) => {}
        Ok(Glitches::Missed) => {
            return Ok(Outcome::Skip(
                "the CS monitor doesn't capture the glitches".to_string(),
            ));
        }
        Err(reason) => return Ok(Outcome::Skip(reason)),
    }

    let region = ctx.bch_start()..ctx.bch_start() + u32::from(ctx.bch_size());
    let before = super::read_region(bus, region.clone())?;

    let mut lab = LabBus {
        bus,
        host,
        device,
        error: None,
        reads: Vec::new(),
    };
    let config = Config {
        poll_limit: LAB_POLL_LIMIT,
        ..super::config(ctx)
    };
    let result = Session::enter(&mut lab, config).map(drop);
    if let Some(e) = lab.error.take() {
        return Err(e);
    }
    glitch_at_a7(&lab.reads, &config)?;
    match result {
        Err(Error::NeverCompleted) => {}
        Err(e) => return Err(format!("enter: {e}, expected {}", Error::NeverCompleted)),
        Ok(()) => return Err("enter succeeded".to_string()),
    }

    super::expect_region(bus, region, &before, "the failed session")
}

/// Checks the only glitch from ENTER_CMD_RESP's knock to its last argument is
/// in front of A7.
fn glitch_at_a7(reads: &[(u32, bool)], config: &Config) -> Result<(), String> {
    let page = u32::from(config.command_page) << 8;
    let sent: Vec<u32> = KNOCK
        .iter()
        .chain(&[group::CONTROL, control::ENTER_CMD_RESP])
        .map(|&b| (page | u32::from(b)) << config.unobserved)
        .collect();
    let start = reads
        .windows(sent.len())
        .position(|w| w.iter().map(|&(addr, _)| addr).eq(sent.iter().copied()))
        .ok_or("the session didn't send ENTER_CMD_RESP")?;
    // The knock, group, CMD and 9 arguments.
    let enter = reads
        .get(start..start + sent.len() + 9)
        .ok_or("the session didn't send ENTER_CMD_RESP's arguments")?;
    let glitched: Vec<usize> = (0..enter.len()).filter(|&i| enter[i].1).collect();
    let a7 = sent.len() + 7;
    if glitched != [a7] {
        return Err(format!(
            "ENTER_CMD_RESP's reads {glitched:?} had glitches, expected only {a7}, A7"
        ));
    }
    Ok(())
}
