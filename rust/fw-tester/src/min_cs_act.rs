// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The CS monitor's arming and release timing.
//!
//! The monitor arms once chip select has been active for long enough, and
//! releases once it has been inactive for long enough.  A config's minimum
//! CS-active time sets the arming.  The release is the same whatever the
//! minimum, except where AlgCs1 reads a lone select pin.
//!
//! `Timing::expected` works out the timing from the slot's CS algorithm, its
//! minimum and the clock it runs at.  The CS algorithm comes from the slot's
//! metadata, built from the config the same way as the firmware's.  [`check`]
//! then drives pulses at every phase of the monitor's sampling loop:
//!
//! - a CS-active pulse of `Timing::ignored` cycles is never captured, and one
//!   a cycle longer is captured at some phase
//! - a CS-active pulse of `Timing::captured` cycles is always captured, and
//!   one a cycle shorter is missed at some phase
//! - a CS-active pulse of `Timing::read` cycles is always captured with its
//!   own address, and one a cycle shorter is captured with the next address at
//!   some phase
//! - a CS-inactive glitch follows the same pattern with
//!   `Timing::release_ignored` and `Timing::released`
//! - where AlgCs1 reads a lone select pin, a glitch of that pin alone follows
//!   it with `Timing::lone_release_ignored`
//! - where AlgCs1 reads a lone select pin a cycle before its other select pins,
//!   the CS-active pulses again with that pin's edges a cycle ahead of the
//!   others', the shape of the shortest pulse AlgCs1 captures
//! - where AlgCs1 has a minimum, the CS-active pulses again on the side of the
//!   masked GPIO its arming loop checks second, with the other side held active
//! - chip select held with some of its select lines inactive and at least one
//!   active is never captured, and completing it is.  The lines held inactive
//!   are every combination on one side of AlgCs1's masked GPIO, or of AlgCs0's
//!   select lines
//!
//! Each pulse follows a capture located to the cycle, so the sampling loop's
//! phase is the same every time and moving the pulse a cycle at a time walks
//! every phase.
//!
//! As on a real host, the address changes on the cycle chip select goes
//! inactive.  The monitor reads the address a few cycles after it arms, so a
//! pulse shorter than `Timing::read` can be captured with the next address.
//! Only a pulse at least that long has its address checked.

use onerom_fw_emulator::{Emulator, OraResult, driver};
use onerom_gen::{ChipSetConfig, Config, FirmwareConfig};
use onerom_metadata::{
    ADDR_MON_MIN_CS_ACT_UNIT_NS, FIRE_FREQ_NONE, FIRE_FREQ_STOCK, OneromAlgCsConfig,
    RP235X_MAX_CONFIGURABLE_MHZ, RP235X_STOCK_CLOCK_SPEED_MHZ, TARGET_FREQ_MHZ,
};

use crate::cs_timing::CsAlg;
use crate::pin_cache::{ControlLine, PinCache};

const DEFAULT_MHZ: u16 = {
    assert!(TARGET_FREQ_MHZ <= u16::MAX as usize);
    TARGET_FREQ_MHZ as u16
};

/// Samples AlgCs0 arms on without a minimum, and releases on.
const CS_0_SAMPLES: u32 = 3;

/// Cycles between AlgCs1's arming samples with a minimum, where a select pin is
/// alone on its side of the masked GPIO and where neither side has one alone.
const CS_1_LONE_SAMPLE_CYCLES: u32 = 4;
const CS_1_SPLIT_SAMPLE_CYCLES: u32 = 7;

/// Cycles between AlgCs2's arming samples with a minimum.
const CS_2_SAMPLE_CYCLES: u32 = 2;

/// Gaps between AlgCs1's and AlgCs2's four unrolled arming samples.
const UNROLLED_GAPS: u32 = 3;

/// Cycles from the CS monitor setting its IRQ to the address read program
/// reading the address.  It waits on the IRQ for a cycle, then reads.
const IRQ_TO_READ_CYCLES: u32 = 2;

/// The fewest cycles an access holds chip select active.
const DEFAULT_ACCESS_CYCLES: u32 = 16;

/// The pulse addresses.  Every bit above the lowest seven is clear, which
/// keeps AlgCs2's qualifiers out of the deselect pattern.
const SYNC_ADDR: usize = 0x21;
const TEST_ADDR: usize = 0x52;

/// Cycles chip select is inactive before a pulse, so the monitor has released.
const SETTLE_CYCLES: u32 = 64;

/// Cycles chip select stays active after the capture that fixes the phase.
const HOLD_CYCLES: u32 = 16;

/// Cycles allowed after a pulse for a capture to reach the ring, beyond the
/// longest arming.
const CAPTURE_SLACK_CYCLES: u32 = 96;

/// The level of AlgCs1's masked GPIO, which the CS monitor reads along with its
/// chip select lines.  A read compared with the GPIO's mask takes longer when
/// the GPIO reads high.
#[derive(Debug, Clone, Copy)]
enum Masked {
    Low,
    High,
}

/// How the select pins switch at a CS-active pulse's edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edges {
    Together,
    /// AlgCs1's lone select pin switches a cycle before the others.
    LoneLeads,
    /// The select lines AlgCs1's arming loop checks first stay active, and
    /// those on the other side of the masked GPIO switch.
    FirstSideHeld,
}

/// A slot's CS algorithm, from its metadata.
#[derive(Debug, Clone, Copy)]
pub struct Cs {
    pub alg: CsAlg,
    /// AlgCs1's masked GPIO.
    masked_gpio: Option<u8>,
    /// AlgCs1's select GPIOs below and above the masked GPIO.
    sides: Option<[u64; 2]>,
    /// AlgCs1's select GPIO alone on its side of the masked GPIO, the lower
    /// where both sides have one.
    lone_gpio: Option<u8>,
    /// AlgCs0's select GPIOs.  In a multi-ROM set they are the first chip's
    /// alone.
    select_gpios: Option<u64>,
    qualifiers: Option<Qualifiers>,
}

/// The lines AlgCs2 reads after its last sample of the enable, to tell a
/// deselected address.
#[derive(Debug, Clone, Copy)]
struct Qualifiers {
    first_gpio: u8,
    count: u8,
    /// What the CS monitor reads from them at a deselected address.
    deselected: u8,
}

impl Qualifiers {
    /// Whether the CS monitor reads the lines as deselected with `levels`
    /// driven.
    fn deselected_by(self, emu: &Emulator, levels: (u64, u64)) -> bool {
        let read = (0..self.count).fold(0u32, |read, i| {
            let gpio = self.first_gpio + i;
            let high = (levels.1 >> gpio) & 1 == 1;
            read | (u32::from(high != emu.gpio_input_inverted(gpio)) << i)
        });
        read == u32::from(self.deselected)
    }
}

/// The mask of GPIOs `from` up to but not including `to`.
fn gpio_span(from: u8, to: u8) -> u64 {
    (from..to).fold(0, |mask, g| mask | (1 << g))
}

impl Cs {
    pub fn from_metadata(alg_cs: &OneromAlgCsConfig) -> Result<Self, String> {
        let (alg, clkdiv_int, clkdiv_frac, masked_gpio, sides, select_gpios) = match *alg_cs {
            OneromAlgCsConfig::AlgCs0 {
                clkdiv_int,
                clkdiv_frac,
                gpio_base,
                base_cs_pin,
                num_cs_pins,
                first_rom_cs_base,
                first_rom_num_cs_pins,
                ..
            } => {
                // The pins pio_setup_address_monitor_pios() reads.
                let (base, num) = if (1..u8::MAX).contains(&first_rom_num_cs_pins) {
                    (first_rom_cs_base, first_rom_num_cs_pins)
                } else {
                    (base_cs_pin, num_cs_pins)
                };
                let first = gpio_base + base;
                (
                    CsAlg::Cs0,
                    clkdiv_int,
                    clkdiv_frac,
                    None,
                    None,
                    Some(gpio_span(first, first + num)),
                )
            }
            OneromAlgCsConfig::AlgCs1 {
                clkdiv_int,
                clkdiv_frac,
                gpio_base,
                base_cs_pin,
                num_cs_pins,
                cs_ignore_index,
                ..
            } => {
                let first = gpio_base + base_cs_pin;
                let masked = first + cs_ignore_index;
                let sides = [
                    gpio_span(first, masked),
                    gpio_span(masked + 1, first + num_cs_pins),
                ];
                (
                    CsAlg::Cs1,
                    clkdiv_int,
                    clkdiv_frac,
                    Some(masked),
                    Some(sides),
                    None,
                )
            }
            OneromAlgCsConfig::AlgCs2 {
                clkdiv_int,
                clkdiv_frac,
                ..
            } => (CsAlg::Cs2, clkdiv_int, clkdiv_frac, None, None, None),
            OneromAlgCsConfig::Unknown { alg, .. } => {
                return Err(format!(
                    "CS algorithm {alg} isn't one this build has timing for"
                ));
            }
        };
        if (clkdiv_int, clkdiv_frac) != (1, 0) {
            return Err(format!(
                "CS clock divider {clkdiv_int}/{clkdiv_frac} isn't 1, so the minimum's cycles \
                 aren't system clock cycles"
            ));
        }
        let lone_gpio = sides
            .and_then(|sides| sides.into_iter().find(|side| side.count_ones() == 1))
            .map(|side| side.trailing_zeros() as u8);
        let qualifiers = if let OneromAlgCsConfig::AlgCs2 {
            gpio_base,
            base_qualifier_pin,
            num_qualifier_pins,
            qualifier_inactive_pattern,
            ..
        } = *alg_cs
        {
            Some(Qualifiers {
                first_gpio: gpio_base + base_qualifier_pin,
                count: num_qualifier_pins,
                deselected: qualifier_inactive_pattern,
            })
        } else {
            None
        };
        Ok(Self {
            alg,
            masked_gpio,
            sides,
            lone_gpio,
            select_gpios,
            qualifiers,
        })
    }
}

/// The CS monitor's timing for one slot, in PIO cycles.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// The longest CS-active pulse that is never captured.
    ignored: u32,
    /// The shortest CS-active pulse that is always captured.
    captured: u32,
    /// The shortest CS-active pulse whose address is always read before chip
    /// select goes inactive.
    read: u32,
    /// Cycles between samples while chip select is inactive.
    poll: u32,
    /// The longest CS-inactive glitch that never releases.
    release_ignored: u32,
    /// The shortest CS-inactive glitch that always releases.
    released: u32,
    /// Cycles between samples while chip select is active.
    release_poll: u32,
    /// Where AlgCs1 reads a lone select pin, the longest glitch of that pin
    /// alone that never releases.
    lone_release_ignored: Option<u32>,
}

impl Timing {
    /// The timing where capture and release each depend on one run of
    /// samples, the first of which can fall anywhere in its polling loop.
    /// `to_irq` is the cycles from the sample that ends a pulse to the IRQ.
    fn polled(
        ignored: u32,
        poll: u32,
        release_ignored: u32,
        release_poll: u32,
        to_irq: u32,
    ) -> Self {
        Self {
            ignored,
            captured: ignored + poll,
            read: ignored + poll + to_irq + IRQ_TO_READ_CYCLES,
            poll,
            release_ignored,
            released: release_ignored + release_poll,
            release_poll,
            lone_release_ignored: None,
        }
    }

    /// AlgCs2 reads its qualifier lines `after` cycles after its last sample
    /// of the enable.  Where the lines read as deselected once a pulse ends, a
    /// pulse must be that much longer to be captured.
    fn qualified(self, after: u32, end_deselects: bool) -> Self {
        if !end_deselects {
            return self;
        }
        Self {
            ignored: self.ignored + after,
            captured: self.captured + after,
            ..self
        }
    }

    /// The expected timing for `cs` with a minimum of `min_cycles`, where
    /// `end_deselects` says whether AlgCs2's qualifier lines read as
    /// deselected once a pulse ends.
    ///
    /// The cycle counts follow each algorithm's program, built in
    /// `pio_setup_address_monitor_pios` (`firmware/src/piodma/pioplugin.c`).
    fn expected(
        cs: Cs,
        min_cycles: Option<u32>,
        masked: Masked,
        edges: Edges,
        end_deselects: bool,
    ) -> Self {
        let masked_release_poll = match masked {
            Masked::Low => 2,
            Masked::High => 4,
        };
        match (cs.alg, min_cycles) {
            // MOV, JMP X--, JMP Y--, IRQ.
            (CsAlg::Cs0, _) => Self::polled(
                cs_0_arming_y(min_cycles) * 3,
                3,
                (CS_0_SAMPLES - 1) * 3,
                3,
                3,
            ),
            (CsAlg::Cs1, None) => {
                let gap = match masked {
                    Masked::Low => 2,
                    Masked::High => 3,
                };
                // The last check jumps to the IRQ as the others jump to the
                // next sample.
                Self::polled(UNROLLED_GAPS * gap, 3, 3, masked_release_poll, gap)
            }
            (CsAlg::Cs1, Some(cycles)) if cs.lone_gpio.is_some() => {
                let span = CS_1_LONE_SAMPLE_CYCLES * cs_1_arming_y(cs, cycles);
                // With the lone select pin held active, its check passes and
                // each poll runs the whole loop.
                // MOV, JMP X--, JMP Y--, IRQ.  Where the lone select pin
                // leads, a pulse ends on its JMP PIN, a cycle before the MOV.
                let (ignored, poll, to_irq) = match edges {
                    Edges::Together => (span + 1, 2, 3),
                    Edges::LoneLeads => (span - 1, 2, 4),
                    Edges::FirstSideHeld => (span, CS_1_LONE_SAMPLE_CYCLES, 3),
                };
                // A glitch is first read within a cycle of its start.  The
                // next read is three cycles later where the first was of the
                // lone select pin, and four where it was of the others, so
                // every glitch of five cycles releases.
                Self {
                    released: 5,
                    lone_release_ignored: Some(3),
                    ..Self::polled(ignored, poll, 3, 3, to_irq)
                }
            }
            (CsAlg::Cs1, Some(cycles)) => {
                // With the side below the masked GPIO held active, its check
                // passes and each poll runs the whole loop.
                let poll = match edges {
                    Edges::Together | Edges::LoneLeads => 4,
                    Edges::FirstSideHeld => CS_1_SPLIT_SAMPLE_CYCLES,
                };
                // MOV, OUT, JMP X--, OUT, OUT, JMP X--, JMP Y--, IRQ.
                Self::polled(
                    CS_1_SPLIT_SAMPLE_CYCLES * cs_1_arming_y(cs, cycles),
                    poll,
                    3,
                    masked_release_poll,
                    7,
                )
            }
            // JMP PIN, MOV reads the qualifiers, JMP, IRQ.
            (CsAlg::Cs2, None) => {
                Self::polled(UNROLLED_GAPS, 1, 1, 3, 3).qualified(1, end_deselects)
            }
            // JMP PIN, JMP X--, MOV reads the qualifiers, JMP, IRQ.
            (CsAlg::Cs2, Some(cycles)) => Self::polled(
                CS_2_SAMPLE_CYCLES * cs_2_arming_x(cycles),
                CS_2_SAMPLE_CYCLES,
                1,
                3,
                4,
            )
            .qualified(2, end_deselects),
        }
    }
}

/// AlgCs0's starting value for Y while arming.  It arms on one sample more,
/// three cycles apart.
fn cs_0_arming_y(min_cycles: Option<u32>) -> u32 {
    min_cycles.map_or(CS_0_SAMPLES - 1, |cycles| cycles.div_ceil(3))
}

/// AlgCs1's starting value for Y while arming with a minimum of `cycles`.  It
/// arms on one sample more.  A lone select pin is read a cycle before the
/// others, so where its edges lead theirs a pulse of Y's starting value times
/// the loop's period is captured.
fn cs_1_arming_y(cs: Cs, cycles: u32) -> u32 {
    match cs.lone_gpio {
        Some(_) => (cycles + 1).div_ceil(CS_1_LONE_SAMPLE_CYCLES),
        None => cycles.div_ceil(CS_1_SPLIT_SAMPLE_CYCLES),
    }
}

/// AlgCs2's starting value for X while arming with a minimum of `cycles`.  It
/// arms on one sample more.
fn cs_2_arming_x(cycles: u32) -> u32 {
    cycles.div_ceil(CS_2_SAMPLE_CYCLES)
}

/// `chip_set`'s own minimum CS-active time in nanoseconds, or else `config`'s.
pub fn slot_min_cs_act_ns(config: &Config, chip_set: &ChipSetConfig) -> Option<u16> {
    chip_set
        .firmware_overrides
        .as_ref()
        .and_then(|overrides| overrides.fire.as_ref())
        .and_then(|fire| fire.addr_mon_min_cs_act_ns)
        .or(config.addr_mon_min_cs_act_ns)
}

/// The clock in MHz a ROM slot runs at, from its `firmware_overrides`.
///
/// This is the clock `get_clock_config()` in `firmware/src/rp235x.c` sets. Its
/// PLL search succeeds for every clock this returns so the fallback for a
/// failed search doesn't apply.
pub fn slot_sysclk_mhz(overrides: Option<&FirmwareConfig>) -> u16 {
    let fire = overrides.and_then(|overrides| overrides.fire.as_ref());
    let requested = fire
        .and_then(|fire| fire.cpu_freq)
        .map_or(FIRE_FREQ_NONE, |freq| freq.get());
    let overclock = fire.and_then(|fire| fire.overclock).unwrap_or(false);

    let mhz = match requested {
        FIRE_FREQ_NONE => DEFAULT_MHZ,
        FIRE_FREQ_STOCK => RP235X_STOCK_CLOCK_SPEED_MHZ,
        mhz if mhz <= RP235X_MAX_CONFIGURABLE_MHZ => mhz,
        _ => RP235X_STOCK_CLOCK_SPEED_MHZ,
    };
    if mhz > RP235X_STOCK_CLOCK_SPEED_MHZ && !overclock {
        RP235X_STOCK_CLOCK_SPEED_MHZ
    } else {
        mhz
    }
}

/// A config's minimum CS-active time in PIO cycles at `mhz`, once rounded up
/// to the metadata's units.
pub fn min_cycles(ns: u16, mhz: u32) -> u32 {
    let unit = u32::from(ADDR_MON_MIN_CS_ACT_UNIT_NS);
    let stored_ns = u32::from(ns).div_ceil(unit) * unit;
    (stored_ns * mhz).div_ceil(1000)
}

/// Cycles an access holds chip select active, so its address is read with
/// chip select still active at every phase.
pub fn access_cycles(cs: Cs, min_cycles: Option<u32>) -> u32 {
    [Masked::Low, Masked::High]
        .into_iter()
        .map(|masked| Timing::expected(cs, min_cycles, masked, Edges::Together, false).read)
        .fold(DEFAULT_ACCESS_CYCLES, u32::max)
}

/// What a pulse left in the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    Missed,
    /// Captured with the address that followed the pulse.
    Late,
    /// Captured with the pulse's own address.
    OnTime,
}

/// How often a pulse is captured or a glitch releases the monitor across a
/// sweep's phases.
#[derive(Debug, Clone, Copy)]
enum Expect {
    Never,
    Sometimes,
    NotAlways,
    Always,
}

/// Drives pulses for one of the slot's timings.
struct Bus<'a> {
    emu: &'a Emulator,
    observed_gpios: &'a [Vec<u8>],
    background: (u64, u64),
    cs_on: (u64, u64),
    cs_off: (u64, u64),
    /// AlgCs1's lone select pin's GPIOs, where it reads one.
    lone_gpios: Option<u64>,
    edges: Edges,
    /// The GPIOs of the select lines held active between pulses.
    held: u64,
    ring_entries: usize,
    sync_addr: usize,
    test_addr: usize,
    /// The address driven once chip select goes inactive.
    end_addr: usize,
    timing: Timing,
}

impl Bus<'_> {
    fn drive(&self, addr: usize, cs_active: bool) {
        let cs = if cs_active { self.cs_on } else { self.cs_off };
        self.drive_selects(addr, cs);
    }

    fn drive_selects(&self, addr: usize, cs: (u64, u64)) {
        let levels = self.levels(addr, cs);
        self.emu.drive_gpios(levels.0, levels.1);
    }

    fn levels(&self, addr: usize, cs: (u64, u64)) -> (u64, u64) {
        driver::merge(
            driver::merge(driver::addr_mask(addr, self.observed_gpios), cs),
            self.background,
        )
    }

    /// The select levels with the lines whose GPIOs are in `active` asserted
    /// and the others deasserted.
    fn selects(&self, active: u64) -> (u64, u64) {
        (
            self.cs_on.0,
            (self.cs_on.1 & active) | (self.cs_off.1 & !active),
        )
    }

    /// Where the lone select pin leads, a cycle with it alone at `cs_active`.
    fn lead(&self, addr: usize, cs_active: bool) {
        if self.edges == Edges::LoneLeads
            && let Some(cs) = self.lone_alone(cs_active)
        {
            self.drive_selects(addr, cs);
            self.emu.step_cycles(1);
        }
    }

    /// The select levels with the lone select pin at `lone_active` and the
    /// others the other way.
    fn lone_alone(&self, lone_active: bool) -> Option<(u64, u64)> {
        let lone = self.lone_gpios?;
        let active = if lone_active {
            lone
        } else {
            self.cs_on.0 & !lone
        };
        Some(self.selects(active))
    }

    fn write_pos(&self) -> *mut u32 {
        // SAFETY: setup_monitor checked the slot is non-null, and the firmware
        // keeps it valid for the monitor's lifetime.
        unsafe { *self.emu.get_address_monitor_ring_write_pos() }
    }

    /// Settles with chip select inactive, then asserts it and steps a cycle at
    /// a time to the capture.  Returns the write position after it.
    fn sync(&self) -> Result<*mut u32, String> {
        self.drive(self.sync_addr, false);
        self.emu.step_cycles(SETTLE_CYCLES);
        let before = self.write_pos();
        self.drive(self.sync_addr, true);
        for _ in 0..self.timing.captured + CAPTURE_SLACK_CYCLES {
            self.emu.step_cycles(1);
            if self.write_pos() != before {
                return Ok(self.write_pos());
            }
        }
        Err(format!(
            "chip select held active for {} cycles was not captured",
            self.timing.captured + CAPTURE_SLACK_CYCLES
        ))
    }

    /// What was captured after `pos`.
    fn captured_since(&self, pos: *mut u32) -> Result<Capture, String> {
        let after = self.write_pos();
        // SAFETY: both positions are within the ring, which is one allocation.
        let entries = unsafe { after.offset_from(pos) }.rem_euclid(self.ring_entries as isize);
        match entries {
            0 => Ok(Capture::Missed),
            1 => {
                // SAFETY: pos is within the ring and the entry there is written.
                let (r, observed) = self.emu.demangle_observed_addr(unsafe { *pos }, false);
                if r != OraResult::Ok {
                    return Err(format!("capture failed to demangle: {r:?}"));
                }
                match observed as usize {
                    addr if addr == self.test_addr => Ok(Capture::OnTime),
                    addr if addr == self.end_addr => Ok(Capture::Late),
                    _ => Err(format!(
                        "captured 0x{observed:X}, driven 0x{:X} then 0x{:X}",
                        self.test_addr, self.end_addr
                    )),
                }
            }
            n => Err(format!("{n} captures of one pulse")),
        }
    }

    /// As [`Self::captured_since`], after chip select was held long enough
    /// for the address to be read.
    fn held_since(&self, pos: *mut u32) -> Result<Capture, String> {
        match self.captured_since(pos)? {
            Capture::Late => Err("captured the address that followed".to_string()),
            capture @ (Capture::Missed | Capture::OnTime) => Ok(capture),
        }
    }

    /// A CS-active pulse of `length` cycles, starting `offset` cycles into the
    /// phase.
    fn pulse(&self, offset: u32, length: u32) -> Result<Capture, String> {
        let between = self.selects(self.held);
        let pos = self.sync()?;
        self.emu.step_cycles(HOLD_CYCLES);
        self.drive_selects(self.test_addr, between);
        self.emu.step_cycles(SETTLE_CYCLES + offset);
        if length > 0 {
            self.lead(self.test_addr, true);
            self.drive(self.test_addr, true);
            self.emu.step_cycles(length);
            self.lead(self.end_addr, false);
            self.drive_selects(self.end_addr, between);
        }
        self.emu
            .step_cycles(self.timing.captured + CAPTURE_SLACK_CYCLES);
        self.captured_since(pos)
    }

    /// Chip select held after a release with the lines whose GPIOs are in
    /// `active` asserted and the others deasserted, then completed.  Fails if
    /// the partial select is captured or the completed one isn't.
    fn partial_select(&self, active: u64) -> Result<(), String> {
        let pos = self.sync()?;
        self.emu.step_cycles(HOLD_CYCLES);
        self.drive(self.test_addr, false);
        self.emu.step_cycles(SETTLE_CYCLES);
        self.drive_selects(self.test_addr, self.selects(active));
        self.emu
            .step_cycles(self.timing.captured + CAPTURE_SLACK_CYCLES);
        if self.held_since(pos)? != Capture::Missed {
            return Err("captured".to_string());
        }
        self.drive(self.test_addr, true);
        self.emu
            .step_cycles(self.timing.captured + CAPTURE_SLACK_CYCLES);
        self.drive(self.end_addr, false);
        self.emu.step_cycles(SETTLE_CYCLES);
        if self.held_since(pos)? == Capture::Missed {
            return Err("completing the select wasn't captured".to_string());
        }
        Ok(())
    }

    /// A CS-inactive glitch of `length` cycles after a capture, starting
    /// `offset` cycles into the phase, then chip select held long enough to
    /// arm again.  A capture means the glitch released the monitor.
    fn glitch(&self, offset: u32, length: u32) -> Result<Capture, String> {
        self.glitch_selects(offset, length, self.cs_off)
    }

    /// As [`Self::glitch`], with the lone select pin alone inactive.
    fn lone_glitch(&self, offset: u32, length: u32) -> Result<Capture, String> {
        let cs = self
            .lone_alone(false)
            .ok_or("the slot doesn't have a lone select pin")?;
        self.glitch_selects(offset, length, cs)
    }

    fn glitch_selects(&self, offset: u32, length: u32, cs: (u64, u64)) -> Result<Capture, String> {
        let pos = self.sync()?;
        self.emu.step_cycles(HOLD_CYCLES + offset);
        self.drive_selects(self.test_addr, cs);
        self.emu.step_cycles(length);
        self.drive(self.test_addr, true);
        self.emu
            .step_cycles(self.timing.captured + CAPTURE_SLACK_CYCLES);
        self.drive(self.end_addr, false);
        self.emu.step_cycles(SETTLE_CYCLES);
        self.held_since(pos)
    }

    /// Runs `trial` at every phase of a loop `period` cycles long, twice over,
    /// counting the phases where it captures, or captures on time where
    /// `on_time` is set.
    fn sweep(
        &self,
        what: &str,
        length: u32,
        period: u32,
        expect: Expect,
        on_time: bool,
        trial: impl Fn(&Self, u32, u32) -> Result<Capture, String>,
    ) -> Result<(), String> {
        let phases = 2 * period;
        let mut hits = 0;
        for offset in 0..phases {
            let capture = trial(self, offset, length)
                .map_err(|e| format!("{what} of {length} cycles at phase {offset}: {e}"))?;
            let hit = match capture {
                Capture::Missed => false,
                Capture::Late => !on_time,
                Capture::OnTime => true,
            };
            if hit {
                hits += 1;
            }
        }
        let counted = if on_time { "read on time" } else { "captured" };
        let met = match expect {
            Expect::Never => hits == 0,
            Expect::Sometimes => hits > 0,
            Expect::NotAlways => hits < phases,
            Expect::Always => hits == phases,
        };
        if met {
            Ok(())
        } else {
            Err(format!(
                "{what} of {length} cycles: {counted} at {hits} of {phases} phases, expected \
                 {expect:?}"
            ))
        }
    }

    fn check_arming(&self) -> Result<(), String> {
        let t = self.timing;
        let arming = [
            (t.ignored, Expect::Never),
            (t.ignored + 1, Expect::Sometimes),
            (t.captured - 1, Expect::NotAlways),
            (t.captured, Expect::Always),
        ];
        for (length, expect) in arming {
            self.sweep(
                "CS-active pulse",
                length,
                t.poll,
                expect,
                false,
                Self::pulse,
            )?;
        }
        for (length, expect) in [(t.read - 1, Expect::NotAlways), (t.read, Expect::Always)] {
            self.sweep("CS-active pulse", length, t.poll, expect, true, Self::pulse)?;
        }
        Ok(())
    }

    fn check_release(&self) -> Result<(), String> {
        let t = self.timing;
        let release = [
            (t.release_ignored, Expect::Never),
            (t.release_ignored + 1, Expect::Sometimes),
            (t.released - 1, Expect::NotAlways),
            (t.released, Expect::Always),
        ];
        for (length, expect) in release {
            self.sweep(
                "CS-inactive glitch",
                length,
                t.release_poll,
                expect,
                false,
                Self::glitch,
            )?;
        }
        Ok(())
    }

    /// Glitches of AlgCs1's lone select pin alone, where it reads one.  The
    /// release reads that pin twice, three cycles apart, so where a glitch
    /// of three cycles never releases neither does a shorter one.
    fn check_lone_release(&self) -> Result<(), String> {
        let t = self.timing;
        let Some(ignored) = t.lone_release_ignored else {
            return Ok(());
        };
        let released = ignored + t.release_poll;
        let release = [
            (ignored, Expect::Never),
            (ignored + 1, Expect::Sometimes),
            (released - 1, Expect::NotAlways),
            (released, Expect::Always),
        ];
        for (length, expect) in release {
            self.sweep(
                "lone select pin glitch",
                length,
                t.release_poll,
                expect,
                false,
                Self::lone_glitch,
            )?;
        }
        Ok(())
    }

    /// Partial selects.  Every combination of a group's select lines is held
    /// inactive with the other select lines active, as long as one of them
    /// stays active.
    fn check_partial_selects(&self, groups: &[Vec<&ControlLine>]) -> Result<(), String> {
        let selects = groups
            .iter()
            .fold(0, |mask, group| mask | lines_mask(group));
        for group in groups {
            for combination in 1..1usize << group.len() {
                let inactive: Vec<&ControlLine> = group
                    .iter()
                    .enumerate()
                    .filter(|&(index, _)| (combination >> index) & 1 == 1)
                    .map(|(_, &line)| line)
                    .collect();
                let inactive_gpios = lines_mask(&inactive);
                if inactive_gpios == selects {
                    continue;
                }
                self.partial_select(self.cs_on.0 & !inactive_gpios)
                    .map_err(|e| {
                        format!(
                            "{} inactive with the other select lines active: {e}",
                            line_names(&inactive)
                        )
                    })?;
            }
        }
        Ok(())
    }
}

/// How the checks drive the bus for one level of AlgCs1's masked GPIO and one
/// shape of pulse edge.
#[derive(Debug, Clone, Copy)]
struct Path {
    masked: Masked,
    edges: Edges,
    /// The GPIOs of the select lines held active between pulses.
    held: u64,
    /// Bits set in every address.
    addr: usize,
    background: (u64, u64),
}

/// The paths that put AlgCs1's masked GPIO at each level, as the PIO reads it.
///
/// The GPIO is an address line on some chips and not on others.  An address
/// line is driven through the address, so the address and the GPIO's level
/// always agree.
fn masked_paths(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    gpio: u8,
) -> Result<[Path; 2], String> {
    if cache.data_gpios.contains(&gpio) || cache.byte_n_gpio == Some(gpio) {
        return Err(format!(
            "AlgCs1's masked GPIO {gpio} is a data or /BYTE line"
        ));
    }
    let inverted = emu.gpio_input_inverted(gpio);
    let path =
        |masked: Masked, high: bool| match observed_gpios.iter().position(|g| g.contains(&gpio)) {
            Some(bit) => Path {
                masked,
                edges: Edges::Together,
                held: 0,
                addr: if high { 1 << bit } else { 0 },
                background,
            },
            None => {
                let mask = 1u64 << gpio;
                Path {
                    masked,
                    edges: Edges::Together,
                    held: 0,
                    addr: 0,
                    background: driver::merge(background, (mask, if high { mask } else { 0 })),
                }
            }
        };
    Ok([path(Masked::Low, inverted), path(Masked::High, !inverted)])
}

/// The GPIOs driven for `lines`.
fn lines_mask(lines: &[&ControlLine]) -> u64 {
    lines
        .iter()
        .flat_map(|line| &line.gpios)
        .fold(0, |mask, &g| mask | (1u64 << g))
}

fn line_names(lines: &[&ControlLine]) -> String {
    lines
        .iter()
        .map(|line| line.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The control lines wired to AlgCs1's select GPIOs `gpios`.
fn lines_on(cache: &PinCache, gpios: u64) -> Result<Vec<&ControlLine>, String> {
    let lines: Vec<&ControlLine> = cache
        .control_lines
        .iter()
        .filter(|line| lines_mask(&[line]) & gpios != 0)
        .collect();
    match gpios & !lines_mask(&lines) {
        0 => Ok(lines),
        unwired => Err(format!(
            "AlgCs1's select GPIO {} isn't a control line",
            unwired.trailing_zeros()
        )),
    }
}

/// Checks the monitor's arming and release timing for the running slot, whose
/// ring holds `ring_entries` entries.
pub fn check(
    emu: &Emulator,
    cache: &PinCache,
    observed_gpios: &[Vec<u8>],
    background: (u64, u64),
    ring_entries: usize,
    cs: Cs,
    min_cycles: Option<u32>,
) -> Result<(), String> {
    let alg = cs.alg;
    let together = match cs.masked_gpio {
        Some(gpio) => masked_paths(emu, cache, observed_gpios, background, gpio)?.to_vec(),
        None => vec![Path {
            masked: Masked::Low,
            edges: Edges::Together,
            held: 0,
            addr: 0,
            background,
        }],
    };
    let sides = match cs.sides {
        Some([low, high]) => Some([lines_on(cache, low)?, lines_on(cache, high)?]),
        None => None,
    };
    let lone = match (min_cycles, cs.lone_gpio) {
        (Some(_), Some(gpio)) => Some(lines_on(cache, 1 << gpio)?),
        _ => None,
    };
    // With a minimum, AlgCs1's arming loop checks the lone select pin first
    // where it reads one, and otherwise the side below the masked GPIO.
    let first_side = match (min_cycles, &sides) {
        (Some(_), Some([low, _])) => Some(lone.clone().unwrap_or_else(|| low.clone())),
        _ => None,
    };
    let lone_gpios = lone.as_deref().map(lines_mask);
    let held_names = first_side.as_deref().map(line_names).unwrap_or_default();
    let groups = match (&sides, cs.select_gpios) {
        (Some(sides), _) => sides.to_vec(),
        (None, Some(gpios)) => vec![lines_on(cache, gpios)?],
        (None, None) => Vec::new(),
    };
    let addr_bits = (1usize << observed_gpios.len()) - 1;

    let mut paths = together.clone();
    if lone_gpios.is_some() {
        paths.extend(together.iter().map(|path| Path {
            edges: Edges::LoneLeads,
            ..*path
        }));
    }
    if let Some(first) = &first_side {
        paths.extend(together.iter().map(|path| Path {
            edges: Edges::FirstSideHeld,
            held: lines_mask(first),
            ..*path
        }));
    }
    for path in paths {
        let mut bus = Bus {
            emu,
            observed_gpios,
            background: path.background,
            cs_on: driver::ctrl_mask(&cache.control_lines, true),
            cs_off: driver::ctrl_mask(&cache.control_lines, false),
            lone_gpios,
            edges: path.edges,
            held: path.held,
            ring_entries,
            sync_addr: SYNC_ADDR | path.addr,
            test_addr: TEST_ADDR | path.addr,
            end_addr: !(TEST_ADDR | path.addr) & addr_bits,
            timing: Timing::expected(cs, min_cycles, path.masked, path.edges, false),
        };
        if let Some(qualifiers) = cs.qualifiers {
            let end = bus.levels(bus.end_addr, bus.selects(bus.held));
            let end_deselects = qualifiers.deselected_by(emu, end);
            bus.timing = Timing::expected(cs, min_cycles, path.masked, path.edges, end_deselects);
        }
        if let Some(cycles) = min_cycles
            && bus.timing.ignored < cycles
        {
            return Err(format!(
                "{alg:?} ignores pulses up to {} cycles, below the minimum of {cycles}",
                bus.timing.ignored
            ));
        }
        let result = match path.edges {
            Edges::Together => bus
                .check_arming()
                .and_then(|()| bus.check_release())
                .and_then(|()| bus.check_lone_release())
                .and_then(|()| bus.check_partial_selects(&groups)),
            // The lead and the held side shape a CS-active pulse, not a
            // CS-inactive glitch.
            Edges::LoneLeads | Edges::FirstSideHeld => bus.check_arming(),
        };
        result.map_err(|e| match (cs.masked_gpio, path.edges) {
            (Some(gpio), Edges::Together) => format!("masked GPIO {gpio} {:?}: {e}", path.masked),
            (Some(gpio), Edges::LoneLeads) => format!(
                "masked GPIO {gpio} {:?}, lone select pin leading: {e}",
                path.masked
            ),
            (Some(gpio), Edges::FirstSideHeld) => format!(
                "masked GPIO {gpio} {:?}, {held_names} held active: {e}",
                path.masked
            ),
            (None, _) => e,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_gen::{FireConfig, FireCpuFreq};
    use onerom_metadata::{ADDR_MON_UNSET_MIN_CS_ACT_NS, ADDR_MON_UNSET_MIN_CS_ACT_NS_ALG_CS_2};

    fn fire(cpu_freq: Option<FireCpuFreq>, overclock: Option<bool>) -> FirmwareConfig {
        let mut fire = FireConfig::default();
        fire.cpu_freq = cpu_freq;
        fire.overclock = overclock;
        let mut overrides = FirmwareConfig::default();
        overrides.fire = Some(fire);
        overrides
    }

    fn mhz(freq: u16) -> Option<FireCpuFreq> {
        Some(FireCpuFreq::mhz(freq).unwrap())
    }

    #[test]
    fn the_slot_clock_follows_the_firmware() {
        for (overrides, clock) in [
            (None, 150),
            (Some(fire(None, Some(true))), 150),
            (Some(fire(Some(FireCpuFreq::none()), Some(true))), 150),
            (Some(fire(Some(FireCpuFreq::stock()), Some(true))), 150),
            (Some(fire(mhz(100), None)), 100),
            (Some(fire(mhz(200), None)), 150),
            (Some(fire(mhz(200), Some(false))), 150),
            (Some(fire(mhz(200), Some(true))), 200),
            (Some(fire(mhz(800), None)), 150),
            (Some(fire(mhz(800), Some(true))), 800),
        ] {
            assert_eq!(slot_sysclk_mhz(overrides.as_ref()), clock, "{overrides:?}");
        }
    }

    // The help and docs state these figures.  AlgCs1 is taken with its masked
    // GPIO low.
    #[test]
    fn unset_never_captured_matches_the_constants() {
        for (alg, ns) in [
            (CsAlg::Cs0, ADDR_MON_UNSET_MIN_CS_ACT_NS),
            (CsAlg::Cs1, ADDR_MON_UNSET_MIN_CS_ACT_NS),
            (CsAlg::Cs2, ADDR_MON_UNSET_MIN_CS_ACT_NS_ALG_CS_2),
        ] {
            let cs = Cs {
                alg,
                masked_gpio: None,
                sides: None,
                lone_gpio: None,
                select_gpios: None,
                qualifiers: None,
            };
            let ignored = Timing::expected(cs, None, Masked::Low, Edges::Together, false).ignored;
            assert_eq!(
                ignored * 1000,
                u32::from(ns) * u32::from(DEFAULT_MHZ),
                "{alg:?}"
            );
        }
    }
}
