// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! An RBCP host, driving the device over emulated ROM bus cycles.
//!
//! Modelled on the 6502 reference host (`reference/host/6502/rbcp/rbcp.s` in
//! the rom-bus-control-protocol repository), because that is what a real host
//! does and therefore what the device must actually work against.
//!
//! # Everything is a ROM read
//!
//! A host has exactly one primitive: read a byte from the ROM.  Commands are
//! encoded as *where* it reads; responses are read back as ordinary ROM data.
//! So is every operation here, and all of them go through [`Bus::read`].
//!
//! Command data travels on the address lines the device observes.  Where the
//! device does not observe the ROM's lowest address line(s), the host advances
//! its read address by two (or more) per command byte — see "Address Line
//! Presentation" in the specification.  That is the only place the distinction
//! appears: [`Bus::command_addr`] shifts an observed address into the byte
//! address space the host actually drives, and everything else works in byte
//! addresses exactly as the host does.
//!
//! # Reads are not free
//!
//! Every read the host performs — including polling the back-channel — is a
//! CS-active bus cycle, so it lands in the device's capture ring alongside
//! command bytes.  In command-response mode the device is required to discard
//! the ones whose upper address bits do not match the configured command page.
//! Reading the back-channel over the bus rather than out of emulated memory is
//! what puts that filter under test; it is also simply what a host does.
//!
//! The device only drains that ring when it runs, and here it runs only when
//! the driver lets it, so each read hands the plugin a turn.  A scenario that
//! wants to overrun the ring can withhold those turns with
//! [`Bus::read_without_resuming`].

use onerom_config::chip::ChipType;
use onerom_fw_emulator::{Emulator, driver as gpio};
use onerom_fw_tester::pin_cache::PinCache;
use onerom_fw_tester::{runner, timing};
use onerom_plugin_tester::ffi;
use onerom_plugin_tester::harness::Plugin;

use crate::Ctx;

/// A field of the response header, from the specification's "Response Header".
///
/// Named rather than a bare offset so that a failed assertion can say which
/// field disagreed without every scenario spelling it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Scenarios are added section by section, so not every field is read yet.
#[allow(dead_code)]
pub enum Hdr {
    LastCmdGroup,
    LastCmdCmd,
    TokenLsb,
    TokenMsb,
    Progress,
    Response,
    Reserved0,
    Reserved1,
}

impl Hdr {
    /// Byte offset within the back-channel region.
    pub fn offset(self) -> u32 {
        match self {
            Hdr::LastCmdGroup => 0,
            Hdr::LastCmdCmd => 1,
            Hdr::TokenLsb => 2,
            Hdr::TokenMsb => 3,
            Hdr::Progress => 4,
            Hdr::Response => 5,
            Hdr::Reserved0 => 6,
            Hdr::Reserved1 => 7,
        }
    }
}

impl std::fmt::Display for Hdr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Hdr::LastCmdGroup => "last command GROUP",
            Hdr::LastCmdCmd => "last command CMD",
            Hdr::TokenLsb => "token LSB",
            Hdr::TokenMsb => "token MSB",
            Hdr::Progress => "progress",
            Hdr::Response => "response",
            Hdr::Reserved0 => "reserved byte 0",
            Hdr::Reserved1 => "reserved byte 1",
        })
    }
}

/// The response header is 8 bytes; the data section follows it.
pub const HDR_SIZE: u32 = 8;

/// The knock this device implementation uses.  Not defined by the protocol —
/// the specification requires only that device and host agree in advance.
pub const KNOCK: [u8; 6] = *b"!RBCP!";

/// Protocol-recommended defaults for the two boolean values the host chooses
/// when entering command-response mode ("Protocol Defaults").
pub const DEFAULT_COMPLETE: u8 = 0xBB;
pub const DEFAULT_STATUS_OK: u8 = 0xCC;

/// How many polls the host makes before giving up on a device response.
///
/// The reference host uses a platform-tuned loop count; here it bounds a test,
/// so it only has to be larger than any legitimate response.
const POLL_LIMIT: u32 = 512;

/// Cycles from asserting CS to sampling the data pins.
///
/// The serving pipeline is: the address state machine latches the GPIO word,
/// the DMA reads SRAM at that offset, the data state machine drives the pins.
/// The latched word is the *whole* GPIO word, chip selects included, so the
/// fetch already in flight while CS is still deasserted is for a different
/// offset from the one this cycle is asking for.  Sample too early and the
/// pins still carry that earlier byte.
///
/// Measured minimum on fire-24-a / 2316: 10 cycles.  Twelve is used, matching
/// `CYCLES_CS_TO_DATA_MULTI`, which the core tester already applies to the
/// case where "the address is only sampled after CS goes active".
///
/// The core tester's single-set value of 6 is not wrong for what it tests: the
/// firmware stores the image replicated across the chip-select encodings, so
/// the early fetch returns the same byte and nothing is amiss.  They stop
/// being the same byte the moment a test writes one offset and reads it back
/// over the bus — which is exactly what RBCP's back-channel does.
const CS_TO_DATA_CYCLES: u32 = 12;

/// Total cycles CS is held active for one host read.
///
/// Covers [`CS_TO_DATA_CYCLES`] for the data, plus a hold before release.  The
/// hold matters independently: the device's address monitor captures the
/// access while CS is active, and its capture path is slower than its serving
/// path.  A window sized only for data validity is never observed at all — at
/// six cycles the knock is not detected, at sixteen it is.
///
/// At 150 MHz this is ~107 ns, an order of magnitude shorter than a real
/// host's bus cycle, so it does not flatter the device.
const CS_ACTIVE_CYCLES: u32 = 16;

/// Which step of the host polling sequence failed.
///
/// The reference host distinguishes these because they mean different things:
/// a command that was never received is a framing or filtering problem, one
/// that never completed is a device that has wedged, and an explicit failure
/// is the device rejecting the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdFailure {
    /// The token never incremented: the device did not receive the command,
    /// or silently discarded it.
    NotReceived,
    /// The token incremented but progress never reached complete.
    NeverCompleted,
    /// The device completed the command and reported failure.
    Failed,
}

impl std::fmt::Display for CmdFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotReceived => write!(f, "command not received (token never incremented)"),
            Self::NeverCompleted => write!(f, "command received but never completed"),
            Self::Failed => write!(f, "device reported failure"),
        }
    }
}

/// The host's view of command-response mode, once entered.
#[derive(Debug, Clone, Copy)]
pub struct Session {
    /// Upper address bits (above A7) the device treats as command signalling.
    pub command_page: u16,
    /// Back-channel start, as a byte offset within the active RAM slot.
    pub bch_start: u32,
    /// Back-channel size in bytes, header included.
    pub bch_size: u16,
    pub complete: u8,
    pub status_ok: u8,
}

impl Session {
    pub fn pending(&self) -> u8 {
        !self.complete
    }
    pub fn failed(&self) -> u8 {
        !self.status_ok
    }
    /// Size of the response data section: the region less its header.
    pub fn data_size(&self) -> u32 {
        u32::from(self.bch_size) - HDR_SIZE
    }

    /// The nine argument bytes of ENTER_CMD_RESP: "A0/A1=command page (16-bit
    /// LE), A2/A3/A4=back-channel start address (24-bit LE), A5/A6=back-channel
    /// size in bytes (16-bit LE), A7=complete, A8=status-OK".
    ///
    /// A session *is* those nine bytes, so it builds them: the driver sends the
    /// frame for the entries it makes itself, and a scenario needs the same
    /// frame with one field deliberately spoiled, or sent from inside a session
    /// where no knock belongs in front of it.
    pub fn enter_args(&self) -> [u8; 9] {
        [
            self.command_page as u8,
            (self.command_page >> 8) as u8,
            self.bch_start as u8,
            (self.bch_start >> 8) as u8,
            (self.bch_start >> 16) as u8,
            self.bch_size as u8,
            (self.bch_size >> 8) as u8,
            self.complete,
            self.status_ok,
        ]
    }
}

/// SLOT_POKE arguments: "A0=byte, A1/A2/A3=24-bit address (little-endian),
/// A4=slot".
pub fn slot_poke_args(addr: u32, value: u8, slot: u8) -> [u8; 5] {
    [
        value,
        addr as u8,
        (addr >> 8) as u8,
        (addr >> 16) as u8,
        slot,
    ]
}

/// SLOT_PEEK arguments: "A0=count, A1/A2/A3=24-bit address (little-endian),
/// A4=slot".  A count of zero means 256 bytes.
pub fn slot_peek_args(addr: u32, count: u8, slot: u8) -> [u8; 5] {
    [
        count,
        addr as u8,
        (addr >> 8) as u8,
        (addr >> 16) as u8,
        slot,
    ]
}

/// A byte of the served image holding a value the test put there, used to tell
/// whether the device acted on a command mode command.
///
/// Command mode has no back-channel and no confirmation, so a scenario about
/// framing — a knock that should not have opened a session, a desync that
/// should have swallowed a command — has nothing to read.  SLOT_POKE is the
/// way in: the specification makes it valid in command mode, and what it
/// writes into the active slot is served back over the bus like any other ROM
/// byte.
///
/// # Why this is not "check nothing changed"
///
/// Asserting that a byte is unchanged is close to worthless.  The device is
/// asynchronous, so "nothing has happened yet" and "nothing will ever happen"
/// read identically, and no amount of polling separates them; and a byte that
/// held its value because the device was wedged, or because the command was
/// malformed for some unrelated reason, or because the address was wrong,
/// looks exactly the same as one the device correctly declined to write.
///
/// So the probe is *armed* instead — [`Bus::arm_probe`] performs a properly
/// knocked poke and verifies it over the bus, which proves the whole path
/// works at that moment and leaves behind a value the test chose.  The
/// stimulus then tries to write a second, different value.  The verdict,
/// [`Bus::expect_stimulus_ignored`], first drives a fence — another knocked,
/// verified poke to a different address — so the device has demonstrably
/// processed a command *after* the stimulus and cannot merely be running
/// late.  Only then is the probe read, and the question is which of two values
/// the device is plainly capable of writing it holds.
pub struct Probe {
    /// Byte address within the active slot, from [`Ctx::probe_addr`].
    addr: u32,
    /// The value the arming poke wrote, and which must still be there.
    armed: u8,
    /// The different value the stimulus will try to write.
    stimulus: u8,
}

/// Drives the ROM bus, and lets the device run between reads.
pub struct Bus<'a> {
    emu: &'a Emulator,
    plugin: &'a Plugin,
    cache: &'a PinCache,
    /// Number of the ROM's low address lines the device does not observe.
    unobserved: u8,
    /// Held across every phase of a read: `/BYTE` where the chip has one.
    background: (u64, u64),
    /// True when the ROM is being served as words rather than bytes.
    word_mode: bool,
    /// Index of the lowest address pin the host drives as an address.
    ///
    /// Zero except in word mode on a chip that can *also* be read as bytes.
    /// Such a chip has a byte-select line below its word lines, which in word
    /// mode is not an address input at all — on these parts it doubles as a
    /// data line, so driving it would fight the data bus.  A chip with no byte
    /// mode has no byte-select, and every one of its address pins is a word
    /// line.
    addr_lo: usize,
    addr_before_cs: u32,
    cs_to_data: u32,
    /// Every read performed, for diagnostics when a scenario fails.
    pub reads: u64,
}

impl<'a> Bus<'a> {
    pub fn new(
        emu: &'a Emulator,
        plugin: &'a Plugin,
        cache: &'a PinCache,
        chip_type: ChipType,
        unobserved: u8,
        mode: u8,
    ) -> Self {
        let background = match cache.byte_n_gpio {
            Some(g) if mode != 0 => gpio::byte_n_mask(g, mode),
            _ => (0, 0),
        };
        let word_mode = mode == 16;
        Bus {
            emu,
            plugin,
            cache,
            unobserved,
            background,
            word_mode,
            addr_lo: usize::from(word_mode && chip_type.bit_modes().contains(&8)),
            addr_before_cs: runner::addr_before_cs_cycles(chip_type),
            cs_to_data: CS_TO_DATA_CYCLES.max(runner::cs_to_data_cycles(chip_type, mode)),
            reads: 0,
        }
    }

    /// The byte address a host reads to signal `byte` on command page `page`.
    ///
    /// Command signalling lives in the *observed* address space; where the
    /// device does not observe the ROM's lowest line(s), consecutive command
    /// values are further apart in the host's own address space.
    pub fn command_addr(&self, page: u16, byte: u8) -> u32 {
        ((u32::from(page) << 8) | u32::from(byte)) << self.unobserved
    }

    /// Perform one ROM read cycle at `byte_addr` and return the byte served.
    ///
    /// Settle the address with CS deasserted, assert CS, sample the data pins,
    /// then release — a normal ROM access.  The device is then given a turn,
    /// so it can consume whatever this read put in its capture ring.
    pub fn read(&mut self, byte_addr: u32) -> Result<u8, String> {
        let data = self.read_without_resuming(byte_addr);
        self.plugin
            .resume(&format!("after read of 0x{byte_addr:06X}"))?;

        // Apply whatever PIO configuration the plugin just asked for.
        //
        // A plugin can reconfigure the serving hardware at any point, and the
        // firmware accumulates those changes as apio pre-instructions that
        // reach the live epio instance only when this is called.  The obvious
        // case is SWITCH_SLOT: `pio_switch_rom_region` rewrites the address
        // state machine's region base, and without this the emulation goes on
        // serving the old slot — the device does the right thing somewhere the
        // host cannot see it, which reads as the command having been ignored.
        //
        // `Emulator::set_active_ram_slot` does this for a switch the *test*
        // performs; this is the same thing for a switch the *plugin* performs.
        // Done per read rather than at known points because nothing tells the
        // driver which command a given byte belongs to.
        self.emu.update_from_apio();

        Ok(data)
    }

    /// As [`Bus::read`], but without letting the device run afterwards.
    ///
    /// Use this to deliberately outpace the device — a host that reads faster
    /// than the device drains its ring will lose entries, exactly as on
    /// hardware.  Every other caller wants [`Bus::read`].
    pub fn read_without_resuming(&mut self, byte_addr: u32) -> u8 {
        // Word mode addresses the ROM by word, so the host drives the word
        // index on the word address lines; byte mode drives the byte address
        // on all of them.  See `addr_lo`.
        let drive_addr = if self.word_mode {
            byte_addr >> 1
        } else {
            byte_addr
        };
        let a = gpio::addr_mask(drive_addr as usize, &self.cache.addr_gpios[self.addr_lo..]);
        let cs_on = gpio::ctrl_mask(&self.cache.control_lines, true);
        let cs_off = gpio::ctrl_mask(&self.cache.control_lines, false);

        let settle = gpio::merge(gpio::merge(a, cs_off), self.background);
        self.emu.drive_gpios(settle.0, settle.1);
        self.emu.step_cycles(self.addr_before_cs);

        let active = gpio::merge(gpio::merge(a, cs_on), self.background);
        self.emu.drive_gpios(active.0, active.1);
        self.emu.step_cycles(self.cs_to_data);
        // A word carries two consecutive image bytes — the even offset on
        // D0-D7 and the odd one on D8-D15 (RBCP, "Back-Channel on a
        // Word-Organised ROM").  In byte mode only the low lane is driven,
        // even on a chip whose pin map has sixteen data lines.
        let lane = if self.word_mode && byte_addr & 1 == 1 {
            &self.cache.data_gpios[8..16]
        } else {
            &self.cache.data_gpios[..8.min(self.cache.data_gpios.len())]
        };
        let data = gpio::extract_byte(self.emu.read_pin_states(), lane);

        // Hold CS for the rest of the host's bus cycle — see CS_ACTIVE_CYCLES.
        self.emu
            .step_cycles(CS_ACTIVE_CYCLES.saturating_sub(self.cs_to_data));

        self.emu.drive_gpios(settle.0, settle.1);
        self.emu.step_cycles(timing::CYCLES_AFTER_READ);

        self.reads += 1;
        data
    }

    /// Read bytes from a RAM slot through the plugin API rather than over the
    /// bus — the device's own view of what it stores.
    ///
    /// Diagnostic only.  An assertion should be made against what the bus
    /// serves, because that is all a host can see; this is for telling apart
    /// "the device wrote the wrong thing" from "the device wrote the right
    /// thing somewhere the host cannot read it".
    pub fn api_slot_bytes(&self, slot: u8, offset: u32, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        let _ = self.emu.read_ram_rom_slot(slot, offset, &mut buf);
        buf
    }

    /// Put bytes into the device's NV storage directly, as a previous boot's
    /// host would have left them.
    ///
    /// Setup, never an assertion.  A host's only way to write NV storage is
    /// NV_POKE_COMMIT, so a scenario about *reading* it — that NV_PEEK answers
    /// from NV storage, that it goes on doing so mid-transaction — would
    /// otherwise have to write it with the very command it is trying to be
    /// independent of.  This writes the storage behind the device's back, and
    /// the assertion is then made over the bus, through the device, which is
    /// two different paths agreeing.
    ///
    /// Every scenario starts with NV storage erased to 0xFF, the state the
    /// specification requires of a device no host has written to.
    pub fn seed_nv(&self, offset: u32, bytes: &[u8]) {
        // SAFETY: the plugin is parked at a yield, so nothing else is touching
        // the region; the caller's range is checked against its size.
        unsafe {
            let size = ffi::ora_host_test_nv_storage_size();
            assert!(
                offset + bytes.len() as u32 <= size,
                "seeding {} bytes at 0x{offset:04X} runs past the device's {size}-byte NV storage",
                bytes.len()
            );
            let base = ffi::ora_host_test_nv_storage().add(offset as usize);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), base, bytes.len());
        }
    }

    /// What the plugin asked the flash hardware to do.
    ///
    /// The commit path is a fixed sequence whose *ordering* matters — a
    /// program issued before XIP came back, or an erase issued while it was
    /// still active, would be a real defect on a device and is invisible in
    /// the resulting bytes alone.  This is how a scenario asserts the sequence
    /// rather than only its outcome.
    pub fn flash_log(&self) -> ffi::FlashLog {
        // SAFETY: the plugin is parked at a yield, so the shim's record is not
        // being written; the pointer is to a static and is never null.
        unsafe { *ffi::ora_host_test_flash_log() }
    }

    /// The XIP clock divisor the device would read before disabling XIP.
    ///
    /// The harness answers for it, so a scenario can require the divisor the
    /// device hands back to the routine that restores XIP to be the one it
    /// read — which is the whole reason it is read first.
    pub fn xip_clkdiv(&self) -> u8 {
        // SAFETY: the seam is a pure function over a constant.
        unsafe { ffi::ora_host_test_xip_clkdiv() }
    }

    /// What the device's NV storage actually holds.
    ///
    /// Diagnostic only, and the counterpart of [`Bus::api_slot_bytes`]: an
    /// assertion belongs against what NV_PEEK answers over the bus, because
    /// that is all a host can see.  This tells apart "the device wrote the
    /// wrong thing" from "the device wrote the right thing somewhere the host
    /// cannot read it".
    pub fn nv_bytes(&self, offset: u32, len: usize) -> Vec<u8> {
        // SAFETY: as `seed_nv`.
        unsafe {
            let size = ffi::ora_host_test_nv_storage_size();
            assert!(offset + len as u32 <= size);
            let base = ffi::ora_host_test_nv_storage().add(offset as usize);
            std::slice::from_raw_parts(base, len).to_vec()
        }
    }

    /// Signal one command byte on the given page.  The data read back is
    /// meaningless — the host discards it.
    pub fn send_byte(&mut self, page: u16, byte: u8) -> Result<(), String> {
        let addr = self.command_addr(page, byte);
        self.read(addr)?;
        Ok(())
    }

    /// Send the knock that opens a session.
    pub fn knock(&mut self, page: u16) -> Result<(), String> {
        for b in KNOCK {
            self.send_byte(page, b)?;
        }
        Ok(())
    }

    /// Send the first `n` bytes of the knock.
    ///
    /// The specification requires the knock's reads to be contiguous, so a
    /// truncated or interrupted one must not open a session.
    pub fn partial_knock(&mut self, page: u16, n: usize) -> Result<(), String> {
        for b in &KNOCK[..n] {
            self.send_byte(page, *b)?;
        }
        Ok(())
    }

    /// The argument bytes of a SLOT_POKE writing `value` to `addr`.
    ///
    /// Exposed so a scenario can send a deliberately incomplete frame: the
    /// Command Mode Constraint is about what the device does when it has been
    /// given fewer argument bytes than the command declares.
    pub fn poke_args(&self, ctx: &Ctx, addr: u32, value: u8) -> [u8; 5] {
        slot_poke_args(addr, value, ctx.active_ram_slot)
    }

    /// Send a SLOT_POKE frame: GROUP, CMD and its five arguments.
    ///
    /// The frame only — no knock.  The caller decides how, or whether, to
    /// frame it, which is the whole point in a scenario about framing.
    pub fn send_poke(&mut self, ctx: &Ctx, addr: u32, value: u8) -> Result<(), String> {
        self.send_poke_slot(ctx.command_page(), ctx.active_ram_slot, addr, value)
    }

    /// As [`Bus::send_poke`], naming the slot rather than taking the active
    /// one.
    pub fn send_poke_slot(
        &mut self,
        page: u16,
        slot: u8,
        addr: u32,
        value: u8,
    ) -> Result<(), String> {
        self.send_cmd(
            page,
            group::MODIFY,
            modify::SLOT_POKE,
            &slot_poke_args(addr, value, slot),
        )
    }

    /// Poll an address until the device serves `want` there.
    ///
    /// A positive wait — it ends as soon as the expected thing happens, so a
    /// generous bound costs time only when something is already wrong.
    pub fn await_byte(&mut self, addr: u32, want: u8) -> Result<(), String> {
        let mut last = 0u8;
        for _ in 0..POLL_LIMIT {
            last = self.read(addr)?;
            if last == want {
                return Ok(());
            }
        }
        Err(format!(
            "0x{addr:06X} still serves 0x{last:02X}, expected 0x{want:02X} — the device \
             did not act on the SLOT_POKE"
        ))
    }

    /// Knock, poke `value` into `addr`, and verify the device served it back.
    ///
    /// The building block for every command-mode assertion.  A poke that lands
    /// proves, at that instant, that the knock was detected, the frame was well
    /// formed, the write path works and the harness can read the result — so
    /// it serves equally as setup, as a fence, and as a liveness check.
    pub fn poke_verified(&mut self, ctx: &Ctx, addr: u32, value: u8) -> Result<(), String> {
        self.poke_verified_slot(ctx.command_page(), ctx.active_ram_slot, addr, value)
    }

    /// As [`Bus::poke_verified`], naming the slot rather than taking the one
    /// that was active when the scenario began.
    ///
    /// The verification is a bus read, so `slot` must be the slot being served
    /// *at that point* — which is the whole reason this exists: after a switch,
    /// the active slot is no longer [`Ctx::active_ram_slot`], and a fence aimed
    /// at the stale one would fail rather than fence.
    pub fn poke_verified_slot(
        &mut self,
        page: u16,
        slot: u8,
        addr: u32,
        value: u8,
    ) -> Result<(), String> {
        self.knock(page)?;
        self.send_poke_slot(page, slot, addr, value)?;
        self.await_byte(addr, value)
    }

    /// Read `addr` and require the device to serve `want` there.
    pub fn expect_byte(&mut self, addr: u32, want: u8) -> Result<(), String> {
        let got = self.read(addr)?;
        if got != want {
            return Err(format!(
                "0x{addr:06X} serves 0x{got:02X}, expected 0x{want:02X}"
            ));
        }
        Ok(())
    }

    /// Knock, poke a chosen value, and verify the device served it back.
    ///
    /// Everything after this depends on it.  See [`Probe`].
    pub fn arm_probe(&mut self, ctx: &Ctx) -> Result<Probe, String> {
        let addr = ctx.probe_addr();
        let original = self.read(addr)?;

        // Both values are derived from what the image already holds, so
        // neither can be satisfied by the existing contents, and they differ
        // from each other whatever that byte happens to be.
        let p = Probe {
            addr,
            armed: original ^ 0xFF,
            stimulus: original ^ 0x5A,
        };

        self.poke_verified(ctx, addr, p.armed)
            .map_err(|e| format!("arming the probe: {e}"))?;
        Ok(p)
    }

    /// The value a probe was armed with, for a scenario asserting it directly.
    pub fn probe_armed(p: &Probe) -> u8 {
        p.armed
    }

    /// Send the stimulus poke at the probe, unframed.
    ///
    /// The scenario supplies whatever framing it is testing — a short knock,
    /// an interrupted one, or none at all — immediately before this.
    pub fn send_probe_poke(&mut self, ctx: &Ctx, p: &Probe) -> Result<(), String> {
        self.send_poke(ctx, p.addr, p.stimulus)
    }

    /// Drive a fence: a properly knocked poke to a different address, verified.
    ///
    /// Its bytes reach the capture ring after the stimulus's, so the device
    /// cannot have acted on the fence without having already had the stimulus
    /// in front of it.  That is what rules out "the device simply has not got
    /// round to it yet", which no amount of waiting can.
    pub fn fence(&mut self, ctx: &Ctx) -> Result<(), String> {
        self.fence_slot(ctx, ctx.active_ram_slot)
    }

    /// As [`Bus::fence`], naming the slot the poke is aimed at.
    ///
    /// Wanted wherever *which slot is active* is the question under test: a
    /// fence aimed at the slot that used to be active would fail on precisely
    /// the device the scenario exists to catch, and a failing fence names the
    /// wrong fault.
    pub fn fence_slot(&mut self, ctx: &Ctx, slot: u8) -> Result<(), String> {
        let addr = ctx.fence_addr();
        let value = self.read(addr)? ^ 0xFF;
        self.poke_verified_slot(ctx.command_page(), slot, addr, value)
            .map_err(|e| format!("fence: {e}"))
    }

    /// Require that the device did not act on the stimulus.
    ///
    /// Fences first, then discriminates between the two values — so this is
    /// never an assertion that nothing happened, but that the byte holds the
    /// one of two writes the device should have made.  The fence is not
    /// optional and so is not separable: without it the read below would prove
    /// nothing.
    pub fn expect_stimulus_ignored(&mut self, ctx: &Ctx, p: &Probe) -> Result<(), String> {
        self.fence(ctx)?;

        let got = self.read(p.addr)?;
        if got == p.armed {
            return Ok(());
        }
        if got == p.stimulus {
            return Err(format!(
                "the device acted on the stimulus: 0x{:06X} serves 0x{got:02X}, the value \
                 the stimulus tried to write, rather than the armed 0x{:02X}",
                p.addr, p.armed
            ));
        }
        Err(format!(
            "0x{:06X} serves 0x{got:02X}, which is neither the armed value 0x{:02X} nor \
             the stimulus value 0x{:02X} — something other than these two writes reached it",
            p.addr, p.armed, p.stimulus
        ))
    }

    /// Send a command frame: GROUP, CMD, then its arguments.
    pub fn send_cmd(&mut self, page: u16, group: u8, cmd: u8, args: &[u8]) -> Result<(), String> {
        self.send_byte(page, group)?;
        self.send_byte(page, cmd)?;
        for &a in args {
            self.send_byte(page, a)?;
        }
        Ok(())
    }

    /// Read one byte of the back-channel response header.
    pub fn read_hdr(&mut self, s: &Session, field: Hdr) -> Result<u8, String> {
        self.read(s.bch_start + field.offset())
    }

    /// Read a response header field and require it to hold `want`.
    ///
    /// The read and the assertion are one act: a scenario asserts what the
    /// specification requires of a field and has no separate use for the
    /// value, so splitting them only puts a temporary and a branch between the
    /// reader and the requirement.
    pub fn expect_hdr(&mut self, s: &Session, field: Hdr, want: u8) -> Result<(), String> {
        let got = self.read_hdr(s, field)?;
        if got != want {
            return Err(format!("{field} is 0x{got:02X}, expected 0x{want:02X}"));
        }
        Ok(())
    }

    /// Read from the response data section and require it to equal `want`.
    ///
    /// `what` names the response format under test, e.g. "GET_PROTOCOL_VERSION
    /// response".
    pub fn expect_data(
        &mut self,
        s: &Session,
        offset: u32,
        want: &[u8],
        what: &str,
    ) -> Result<(), String> {
        let got = self.read_data(s, offset, want.len() as u32)?;
        let Some(i) = (0..want.len()).find(|&i| got[i] != want[i]) else {
            return Ok(());
        };
        Err(format!(
            "{what}: byte {} is 0x{:02X}, expected 0x{:02X} — read {}, expected {}",
            offset as usize + i,
            got[i],
            want[i],
            hex_window(&got, i),
            hex_window(want, i),
        ))
    }

    /// Issue a command the specification requires the device to *reject*, and
    /// require that it did.
    ///
    /// Rejection is not discard: the device receives the command, runs the
    /// processing sequence, and writes the failed value into the response
    /// field — so the token does increment.  This is what the specification's
    /// "is invalid and rejected" means, and it is what nine of the commands
    /// must do with a 0xAA in their final argument.
    pub fn expect_rejected(
        &mut self,
        s: &Session,
        group: u8,
        cmd: u8,
        args: &[u8],
    ) -> Result<(), String> {
        match self.issue_cmd(s, group, cmd, args) {
            Err(CmdFailure::Failed) => Ok(()),
            Ok(()) => Err(format!(
                "the device accepted 0x{group:02X}/0x{cmd:02X}, which the specification \
                 requires it to reject"
            )),
            Err(e) => Err(format!("0x{group:02X}/0x{cmd:02X}: {e}")),
        }
    }

    /// Issue a command the specification requires the device to *discard
    /// silently*, and require that it did.
    ///
    /// A discarded command leaves no trace, so the assertion is negative: the
    /// token location must not change.  The bound is the same poll limit a
    /// successful command is given, so a device that merely answers slowly is
    /// not mistaken for one that discarded — if it were going to answer, it
    /// has had exactly as long to do it.
    ///
    /// Sound before entry as well as after: outside command-response mode the
    /// token location is ordinary ROM data, and the device writing to it is
    /// itself proof that it acted on the command.
    pub fn expect_discarded(
        &mut self,
        s: &Session,
        group: u8,
        cmd: u8,
        args: &[u8],
    ) -> Result<(), String> {
        let before = self.read_hdr(s, Hdr::TokenLsb)?;
        self.send_cmd(s.command_page, group, cmd, args)?;
        for _ in 0..POLL_LIMIT {
            let now = self.read_hdr(s, Hdr::TokenLsb)?;
            if now != before {
                return Err(format!(
                    "the device acted on 0x{group:02X}/0x{cmd:02X}: the token went \
                     0x{before:02X} → 0x{now:02X}, but the specification requires the \
                     command to be discarded silently"
                ));
            }
        }
        Ok(())
    }

    /// Read `len` bytes from the response data section, which begins at
    /// offset 8 within the back-channel region.
    pub fn read_data(&mut self, s: &Session, offset: u32, len: u32) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(self.read(s.bch_start + HDR_SIZE + offset + i)?);
        }
        Ok(out)
    }

    /// Issue a command in command-response mode and wait for it to complete,
    /// following the specification's "Host Polling Sequence": snapshot the
    /// token, send, poll the token, poll progress, then read the response.
    pub fn issue_cmd(
        &mut self,
        s: &Session,
        group: u8,
        cmd: u8,
        args: &[u8],
    ) -> Result<(), CmdFailure> {
        let before = self
            .read_hdr(s, Hdr::TokenLsb)
            .map_err(|_| CmdFailure::NotReceived)?;

        self.send_cmd(s.command_page, group, cmd, args)
            .map_err(|_| CmdFailure::NotReceived)?;

        self.poll(|b| {
            b.read_hdr(s, Hdr::TokenLsb)
                .map(|t| t != before)
                .unwrap_or(false)
        })
        .then_some(())
        .ok_or(CmdFailure::NotReceived)?;

        self.poll(|b| {
            b.read_hdr(s, Hdr::Progress)
                .map(|p| p == s.complete)
                .unwrap_or(false)
        })
        .then_some(())
        .ok_or(CmdFailure::NeverCompleted)?;

        match self.read_hdr(s, Hdr::Response) {
            Ok(r) if r == s.status_ok => Ok(()),
            _ => Err(CmdFailure::Failed),
        }
    }

    /// SLOT_POKE one byte into a named RAM slot, from inside a session.
    ///
    /// The command-response counterpart to [`Bus::poke_verified`], which is
    /// command mode and always targets the active slot.  Naming the slot is the
    /// point: SLOT_POKE's "target slot need not be active", and most of what
    /// the Modify group is for happens in a slot that is not being served.
    pub fn poke_slot(&mut self, s: &Session, slot: u8, addr: u32, value: u8) -> Result<(), String> {
        self.issue_cmd(
            s,
            group::MODIFY,
            modify::SLOT_POKE,
            &slot_poke_args(addr, value, slot),
        )
        .map_err(|e| format!("SLOT_POKE of 0x{value:02X} to 0x{addr:06X} in slot {slot}: {e}"))
    }

    /// As [`Bus::poke_slot`], and require the device to serve the byte back.
    ///
    /// Only sound for the slot being served.  Proves the whole path at that
    /// moment and leaves a value the scenario chose in the served image — the
    /// value a peek at another slot must be told apart from.
    pub fn poke_slot_verified(
        &mut self,
        s: &Session,
        slot: u8,
        addr: u32,
        value: u8,
    ) -> Result<(), String> {
        self.poke_slot(s, slot, addr, value)?;
        self.expect_byte(addr, value)
    }

    /// Read bytes out of any RAM slot with SLOT_PEEK.
    ///
    /// The host's only view of a slot it is not being served: a device-side
    /// read of the slot named, answered over the bus in the response data
    /// section.
    pub fn peek_slot(
        &mut self,
        s: &Session,
        slot: u8,
        addr: u32,
        len: u8,
    ) -> Result<Vec<u8>, String> {
        self.issue_cmd(
            s,
            group::READ,
            read::SLOT_PEEK,
            &slot_peek_args(addr, len, slot),
        )
        .map_err(|e| format!("SLOT_PEEK of {len} byte(s) from 0x{addr:06X} in slot {slot}: {e}"))?;
        self.read_data(s, 0, u32::from(len))
    }

    /// Send an ENTER_CMD_RESP frame — the frame only, no knock.
    ///
    /// [`Bus::enter_cmd_resp`] is the whole exchange, knock included, and polls
    /// it to completion.  This is for the scenarios that must not do that: one
    /// spoiling an argument, where there is nothing to poll for because the
    /// device is required to discard the command, and one issuing it from
    /// inside a session, where a knock's bytes would be read as command bytes.
    pub fn send_enter_cmd_resp(&mut self, page: u16, s: &Session) -> Result<(), String> {
        self.send_cmd(
            page,
            group::CONTROL,
            control::ENTER_CMD_RESP,
            &s.enter_args(),
        )
    }

    /// Poll a back-channel condition until it holds or the limit is reached.
    fn poll(&mut self, mut cond: impl FnMut(&mut Bus<'a>) -> bool) -> bool {
        for _ in 0..POLL_LIMIT {
            if cond(self) {
                return true;
            }
        }
        false
    }

    /// Enter command-response mode: knock, then ENTER_CMD_RESP.
    ///
    /// The token is not initialised by the device — it continues from whatever
    /// is already at that location — so the host snapshots it beforehand and
    /// waits for that value to change, as it would for any other command.
    pub fn enter_cmd_resp(&mut self, s: &Session) -> Result<(), CmdFailure> {
        self.enter_cmd_resp_inner(s).map(|_| ())
    }

    /// As [`Bus::enter_cmd_resp`], returning every token LSB value the host
    /// observed while waiting for the entry to complete.
    ///
    /// A host detects a command by watching that byte change, so the values it
    /// passes through are part of the contract and not merely internal: any
    /// value other than the one before and the one after is a change a host
    /// can act on.
    pub fn enter_cmd_resp_sampling_token(&mut self, s: &Session) -> Result<Vec<u8>, String> {
        self.enter_cmd_resp_inner(s)
            .map_err(|e| format!("ENTER_CMD_RESP: {e}"))
    }

    fn enter_cmd_resp_inner(&mut self, s: &Session) -> Result<Vec<u8>, CmdFailure> {
        let mut seen = Vec::new();
        let before = self
            .read_hdr(s, Hdr::TokenLsb)
            .map_err(|_| CmdFailure::NotReceived)?;

        self.knock(s.command_page)
            .map_err(|_| CmdFailure::NotReceived)?;

        self.send_enter_cmd_resp(s.command_page, s)
            .map_err(|_| CmdFailure::NotReceived)?;

        let mut samples = Vec::new();
        let changed = self.poll(|b| match b.read_hdr(s, Hdr::TokenLsb) {
            Ok(t) => {
                samples.push(t);
                t != before
            }
            Err(_) => false,
        });
        seen.append(&mut samples);
        changed.then_some(()).ok_or(CmdFailure::NotReceived)?;

        self.poll(|b| {
            b.read_hdr(s, Hdr::Progress)
                .map(|p| p == s.complete)
                .unwrap_or(false)
        })
        .then_some(())
        .ok_or(CmdFailure::NeverCompleted)?;

        match self.read_hdr(s, Hdr::Response) {
            Ok(r) if r == s.status_ok => Ok(seen),
            _ => Err(CmdFailure::Failed),
        }
    }

    /// The specification's recommended reset sequence: five RBCP_RESETs to
    /// flush any argument collection in progress, one to reset a now-idle
    /// device, then a knock and one more in case the device was in command
    /// mode.  Sent on the command page, since a device already in
    /// command-response mode filters everything else out.
    pub fn reset(&mut self, page: u16) -> Result<(), String> {
        for _ in 0..5 {
            self.send_reset(page)?;
        }
        self.send_reset(page)?;
        self.knock(page)?;
        self.send_reset(page)?;
        Ok(())
    }

    pub fn send_reset(&mut self, page: u16) -> Result<(), String> {
        self.send_byte(page, group::RESET)?;
        self.send_byte(page, reset::RBCP_RESET)
    }
}

/// Hex dump of up to four bytes either side of `centre`, that byte bracketed.
///
/// A response format that is wrong in one byte is usually wrong in a way its
/// neighbours explain; for a long section — a 256-byte SLOT_PEEK — the
/// neighbours are the only part worth printing.
fn hex_window(bytes: &[u8], centre: usize) -> String {
    let lo = centre.saturating_sub(4);
    let hi = (centre + 5).min(bytes.len());
    let mut out = String::new();
    if lo > 0 {
        out.push_str("… ");
    }
    for (i, b) in bytes[lo..hi].iter().enumerate() {
        if lo + i == centre {
            out.push_str(&format!("[{b:02X}] "));
        } else {
            out.push_str(&format!("{b:02X} "));
        }
    }
    let mut out = out.trim_end().to_string();
    if hi < bytes.len() {
        out.push_str(" …");
    }
    out
}

// The command constants below are the protocol's complete command surface,
// transcribed from the specification's Command Reference so that the whole set
// is visible in one place and can be checked against it.  Scenarios are added
// section by section, so some are not referenced yet.

/// Command groups.
#[allow(dead_code)]
pub mod group {
    pub const CONTROL: u8 = 0x00;
    pub const READ: u8 = 0x01;
    pub const MODIFY: u8 = 0x02;
    pub const NV_STORAGE: u8 = 0x03;
    pub const RESET: u8 = 0xAA;
}

#[allow(dead_code)]
pub mod control {
    pub const NOP: u8 = 0x00;
    pub const ENTER_CMD_RESP: u8 = 0x01;
    pub const EXIT_CMD_RESP_ACK: u8 = 0x02;
    pub const EXIT_CMD_RESP_SILENT: u8 = 0x03;
    pub const SWITCH_AND_EXIT: u8 = 0x04;
}

#[allow(dead_code)]
pub mod read {
    pub const GET_FLASH_SLOT_COUNT: u8 = 0x00;
    pub const GET_FLASH_SLOT_INFO: u8 = 0x01;
    pub const GET_FLASH_SLOT_INFO_ALL: u8 = 0x02;
    pub const GET_RAM_SLOT_INFO_ALL: u8 = 0x03;
    pub const GET_DEVICE_TYPE: u8 = 0x04;
    pub const GET_DEVICE_VERSION: u8 = 0x05;
    pub const GET_PROTOCOL_VERSION: u8 = 0x06;
    pub const SLOT_PEEK: u8 = 0x07;
}

#[allow(dead_code)]
pub mod modify {
    pub const SLOT_POKE: u8 = 0x00;
    pub const SWITCH_SLOT: u8 = 0x01;
    pub const LOAD_SLOT: u8 = 0x02;
    pub const SLOT_POKE_ALL_BYTE: u8 = 0x03;
}

#[allow(dead_code)]
pub mod nv {
    pub const GET_NV_CAPABILITY: u8 = 0x00;
    pub const NV_PEEK: u8 = 0x01;
    pub const NV_POKE_BEGIN: u8 = 0x02;
    pub const NV_POKE: u8 = 0x03;
    pub const NV_POKE_COMMIT: u8 = 0x04;
    pub const NV_POKE_DISCARD: u8 = 0x05;
    pub const NV_POKE_COMMIT_BYTE: u8 = 0x06;
}

#[allow(dead_code)]
pub mod reset {
    pub const RBCP_RESET: u8 = 0xAA;
}
