// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lab's host bus under its `c64` profile.  See [`crate::lab_host`].

use onerom_fw_tester::min_cs_act;
use onerom_fw_tester::oracle;
use onerom_lab_bus::{C64, Segment};
use onerom_rbcp_host::Session;

use crate::driver::Bus;
use crate::lab_host::{Device, LAB_MHZ, LabHost, START_CYCLES};
use crate::{Ctx, Outcome};

/// Lab's machines for the served chip in the mode under test, or why there
/// aren't any.
fn host(bus: &Bus, ctx: &Ctx) -> Result<LabHost, Outcome> {
    let mode = if bus.word_mode() { 16 } else { 8 };
    let profile = C64.to_bus().expect("the C64 profile is valid");
    LabHost::new(ctx.board, ctx.chip_type, bus.cache(), mode, profile).map_err(Outcome::Skip)
}

/// Reads the byte at `byte_addr` through Lab's host bus, then lets the device
/// run, as [`Bus::read`] does.
fn read(bus: &mut Bus, host: &mut LabHost, device: &Device, byte_addr: u32) -> Result<u8, String> {
    let word_mode = bus.word_mode();
    let addr = if word_mode { byte_addr >> 1 } else { byte_addr };
    let data = host.read(addr, |lab| device.cycle(lab))?.data;
    bus.resume_after(byte_addr)?;
    // A word contains the even byte on D0-D7 and the odd one on D8-D15.
    Ok(if word_mode && byte_addr & 1 == 1 {
        (data >> 8) as u8
    } else {
        data as u8
    })
}

/// Lab drives each line, A's and S's together, and R samples the data lines at
/// the cycles the burst sets.
///
/// The data lines hold a count of cycles, so R's sample identifies its cycle.
pub fn bus_timing(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let mut host = match host(bus, ctx) {
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
    let mut host = match host(bus, ctx) {
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
        let got = read(bus, &mut host, &device, addr)?;
        let want = image[addr as usize];
        if got != want {
            return Err(format!(
                "0x{addr:06X} reads 0x{got:02X}, the ROM holds 0x{want:02X}"
            ));
        }
    }
    Ok(Outcome::Pass)
}

/// What an RBCP session through Lab's host bus under the C64 profile does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Works,
    /// The CS monitor captures a C64 glitch before each ROM read, so the knock
    /// never arrives whole.
    Fails,
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

/// [`Expect`] from the served slot's config, or why it isn't known.
fn expect(ctx: &Ctx, host: &LabHost, device: &Device, bus: &Bus) -> Result<Expect, String> {
    readable(ctx, host, device, bus)?;
    let monitor = &ctx.monitor;
    let cs = monitor.cs.clone()?;
    let min_cycles = monitor.min_cycles;

    // Every burst has the same shape, as the C64 profile glitches every bus
    // cycle that isn't a ROM read.
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
        Some(true) => Ok(Expect::Fails),
        Some(false) => Ok(Expect::Works),
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
}

impl onerom_rbcp_host::Bus for LabBus<'_, '_> {
    fn read(&mut self, addr: u32) -> u8 {
        read(self.bus, &mut self.host, &self.device, addr).unwrap_or_else(|e| {
            self.error.get_or_insert(e);
            0xFF
        })
    }
}

/// One ROM Lab's identity query under the C64 profile works where the CS
/// monitor doesn't capture the glitches, and `enter` fails where it captures
/// one before every ROM read.
pub fn c64_identity(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let host = match host(bus, ctx) {
        Ok(host) => host,
        Err(skip) => return Ok(skip),
    };
    let device = host.device(bus.emu(), bus.background());
    let expect = match expect(ctx, &host, &device, bus) {
        Ok(expect) => expect,
        Err(reason) => return Ok(Outcome::Skip(reason)),
    };

    log::debug!("RBCP under the c64 profile: {expect:?}");
    let mut lab = LabBus {
        bus,
        host,
        device,
        error: None,
    };
    let result = match (expect, Session::enter(&mut lab, super::config(ctx))) {
        (Expect::Works, Ok(session)) => identity(session),
        (Expect::Works, Err(e)) => Err(format!("enter: {e}")),
        (Expect::Fails, Ok(_)) => {
            Err("enter succeeded, but the CS monitor captures the C64 glitches".to_string())
        }
        (Expect::Fails, Err(_)) => Ok(()),
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
