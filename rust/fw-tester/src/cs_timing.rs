// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Whether the byte on the data bus was really fetched for the read being made
//! — this address, with these chip selects asserted — or is merely a byte that
//! happens to match.
//!
//! The bulk read pass cannot tell the two apart.  It compares the bus against
//! the oracle, and a fetch made *before* CS asserted satisfies that comparison
//! equally well: a single-chip image is replicated across every SRAM index bit
//! the chip's address lines do not drive, so an entry fetched with the wrong
//! chip-select bits holds the same byte.  What the pass establishes is that *a*
//! matching byte was present, not that the device produced it for this read.
//!
//! What separates them is *when* the byte appears.  A fetch launched by this
//! read costs the address state machine another pass round its loop before the
//! DMA and data machines can run; a stale one is already on the pins.  So this
//! module measures the CS-to-data latency and asserts it exactly — which is the
//! question "was this byte fetched for this read", put in a form the bus can
//! answer.
//!
//! That is not a theoretical gap.  `CYCLES_CS_TO_DATA_MULTI` sat a cycle below
//! the real requirement for as long as it did because nothing could see it.
//!
//! # Why this is its own check
//!
//! [`crate::timing`]'s `CYCLES_*` are the settling margins the bulk pass waits
//! before sampling.  They have to be *correct*, or that pass reports failures
//! which are the tester's fault — so they cannot also serve as the assertion,
//! because a correct margin has slack in it by definition.
//!
//! Asserting the latency exactly is the stricter statement, and it fails in
//! both directions: too slow means serving regressed, too fast means the check
//! has quietly stopped discriminating and is passing on a stale fetch again.
//!
//! # What it does
//!
//! Three steps:
//!
//! 1. **Find the SRAM byte this read fetches** — not calculate it, find it, by
//!    flooding half the slot with a distinguishable value and seeing whether
//!    the bus returns it (`Probe::find_lane_offset`).  The index is the sampled
//!    GPIO word, and which of its bits reach the index — after input inversion,
//!    forced-low overrides, and pins the tester never drives — is easy to model
//!    wrongly and impossible to notice having modelled wrongly, because a wrong
//!    offset still holds the right byte.
//! 2. **Plant a value only this read can return** (`Probe::choose_marker`,
//!    `Probe::mark`).
//! 3. **Sample at a repeatable moment** ([`Probe::serves_at`]), and assert the
//!    marker appears at the expected cycle count and *not* one cycle earlier.
//!
//! # Traps
//!
//! Each of these produced a check that passed while measuring nothing.
//!
//! * The marker cannot just be the complement of the original byte.  A read
//!   that is too early fetches a *different* entry holding an unrelated byte,
//!   and one time in 256 per lane that byte is the complement.
//! * Every data lane must be located before any marker is written: the search
//!   in step 1 restores the slot as it narrows, so marking one lane and then
//!   searching for the next erases the first.
//! * The address state machine free-runs, so the answer depends on where CS
//!   assertion lands in its loop, and a read cycle's length is not a multiple of
//!   that loop.  The sweep has to be aligned rather than started wherever the
//!   previous read left off, or the same cycle count comes out both sufficient
//!   and insufficient within a single run.
//!
//! # What the latency should be
//!
//! It depends on whether the address state machine samples one of the chip's
//! control lines.  If it does, asserting CS changes the SRAM index, so the
//! fetch already in flight is discarded and redone; if it does not, CS only has
//! to switch the output drivers on.  The two differ by roughly a factor of two.
//!
//! Which case applies is a property of the board's routing as much as the chip,
//! so it is derived from configuration by
//! [`onerom_gen::compat::serving_alg_info`] rather than assumed per chip type —
//! see [`expected_cs_to_data`].

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_fw_emulator::{Emulator, ServingAlg, ffi};
use onerom_gen::compat::ServingAlgInfo;
use onerom_gen::{AddrAlgPreference, CsAlgPreference, DataAlgPreference};

use crate::driver;
use crate::pin_cache::PinCache;
use crate::timing;

// ── Algorithm identities ──────────────────────────────────────────────────────
//
// bindgen emits the C enums as bare constants, which no `match` can be
// exhaustive over.  Mirroring them as Rust enums gets that back: a new
// algorithm cannot be given a cycle cost by accident, because
// `expected_cs_to_data` will not compile until it has one.
//
// The `assert!`s below are the other half of it.  A C algorithm added without a
// Rust variant would otherwise only surface as a runtime `UnknownAlg`, in
// whichever CI job happened to use a config that selected it; comparing against
// the firmware's own `NUM_*_ALGS` turns that into a build failure here.

/// Chip select algorithm, mirroring `onerom_alg_cs_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsAlg {
    /// Contiguous CS range.
    Cs0,
    /// One gap in the CS range; masked, otherwise as `Cs0`.
    Cs1,
    /// Enable plus address-qualified select.
    Cs2,
}

/// Address algorithm, mirroring `onerom_alg_addr_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrAlg {
    Addr0,
}

/// Data algorithm, mirroring `onerom_alg_data_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataAlg {
    /// Direct data output: 8-bit, or 16-bit under `force_16_bit`.
    Data0,
    /// 16-bit with `/BYTE` and A-1 handling; more PIO cycles.
    Data1,
}

const _: () = assert!(
    ffi::onerom_alg_cs_t_NUM_CS_ALGS == 3,
    "a CS algorithm was added or removed: update CsAlg and give the new \
     variant a CS-to-data cost in expected_cs_to_data"
);
const _: () = assert!(
    ffi::onerom_alg_addr_t_NUM_ADDR_ALGS == 1,
    "an address algorithm was added or removed: update AddrAlg and give the \
     new variant a CS-to-data cost in expected_cs_to_data"
);
const _: () = assert!(
    ffi::onerom_alg_data_t_NUM_DATA_ALGS == 2,
    "a data algorithm was added or removed: update DataAlg and give the new \
     variant a CS-to-data cost in expected_cs_to_data"
);

/// The algorithm triple a slot is serving with, resolved to Rust enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Algs {
    pub cs: CsAlg,
    pub addr: AddrAlg,
    pub data: DataAlg,
}

/// An algorithm id the firmware reported that this build has no variant for.
///
/// Only reachable if the `NUM_*_ALGS` assertions above were themselves updated
/// without adding the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownAlg {
    pub family: &'static str,
    pub id: u32,
}

impl Algs {
    /// Resolve `onerom-gen`'s config-side answer to this module's enums.
    pub fn from_config(info: &ServingAlgInfo) -> Self {
        Self {
            cs: match info.cs_alg {
                CsAlgPreference::AlgCs0 => CsAlg::Cs0,
                CsAlgPreference::AlgCs1 => CsAlg::Cs1,
                CsAlgPreference::AlgCs2 => CsAlg::Cs2,
            },
            addr: match info.addr_alg {
                AddrAlgPreference::AlgAddr0 => AddrAlg::Addr0,
            },
            data: match info.data_alg {
                DataAlgPreference::AlgData0 => DataAlg::Data0,
                DataAlgPreference::AlgData1 => DataAlg::Data1,
            },
        }
    }

    /// Resolve the ids the running firmware reports, for cross-checking
    /// against [`Self::from_config`].
    pub fn from_serving(alg: &ServingAlg) -> Result<Self, UnknownAlg> {
        let cs = match alg.cs_alg {
            ffi::onerom_alg_cs_t_ALG_CS_0 => CsAlg::Cs0,
            ffi::onerom_alg_cs_t_ALG_CS_1 => CsAlg::Cs1,
            ffi::onerom_alg_cs_t_ALG_CS_2 => CsAlg::Cs2,
            id => {
                return Err(UnknownAlg { family: "cs", id });
            }
        };
        let addr = match alg.addr_alg {
            ffi::onerom_alg_addr_t_ALG_ADDR_0 => AddrAlg::Addr0,
            id => {
                return Err(UnknownAlg { family: "addr", id });
            }
        };
        let data = match alg.data_alg {
            ffi::onerom_alg_data_t_ALG_DATA_0 => DataAlg::Data0,
            ffi::onerom_alg_data_t_ALG_DATA_1 => DataAlg::Data1,
            id => {
                return Err(UnknownAlg { family: "data", id });
            }
        };
        Ok(Self { cs, addr, data })
    }
}

// ── Expected latencies ────────────────────────────────────────────────────────

/// Cycles from CS assertion to valid data, when a control line lies **inside**
/// the address window so CS assertion forces a refetch.
///
/// The free-running address loop has to come round and re-sample before the
/// DMA and data machines can run, which dominates the figure.
pub const CS_REFETCH: u32 = 13;

/// Cycles from CS assertion to valid data when no control line is inside the
/// address window, so the fetch is already in flight and CS only enables the
/// output drivers.
pub const CS_OUTPUT_ENABLE: u32 = 7;

/// As [`CS_OUTPUT_ENABLE`], for `AlgData0` driving a 16-bit word (a
/// `force_16_bit` slot, which ignores `/BYTE` entirely).
pub const CS_OUTPUT_ENABLE_WORD: u32 = 4;

/// As [`CS_OUTPUT_ENABLE`], for `AlgData1` in 16-bit word mode (`/BYTE` high).
pub const CS_OUTPUT_ENABLE_BYTE_ALG_WORD: u32 = 6;

/// As [`CS_OUTPUT_ENABLE`], for `AlgData1` in 8-bit mode (`/BYTE` low), serving
/// the **low** half of the word.  The `/BYTE` test and the narrower pindirs
/// write cost extra cycles over `AlgData0`.
pub const CS_OUTPUT_ENABLE_BYTE_ALG_BYTE: u32 = 8;

/// Extra cycle `AlgData1` costs in 8-bit mode when A-1 selects the **high**
/// half of the word rather than the low one.
///
/// Measured, not assumed: on both 27C400 and 27C200 every even byte address
/// settles a cycle sooner than every odd one.  A single figure for byte mode
/// would have to be the larger of the two, which would stop the check being
/// able to tell a real one-cycle slowdown from the half-select it already
/// expects.
pub const CS_BYTE_HIGH_HALF_EXTRA: u32 = 1;

/// The CS-to-data latency expected for this algorithm triple and geometry.
///
/// `cs_in_window` is whether any asserted control line is sampled by the
/// address state machine — see the module docs.  `mode` is 8 or 16.
/// `drive_addr` is the address being read; in `AlgData1` byte mode its low bit
/// is A-1, which selects the word half and costs a cycle when high.
///
/// Every arm is spelled out rather than defaulted, so that adding an algorithm
/// forces a decision here instead of silently inheriting a number measured for
/// a different one.
pub fn expected_cs_to_data(algs: Algs, mode: u8, cs_in_window: bool, drive_addr: usize) -> u32 {
    let AddrAlg::Addr0 = algs.addr;

    // A-1 is bit 0 of the byte address in 8-bit mode.
    let high_half = if drive_addr & 1 == 1 {
        CS_BYTE_HIGH_HALF_EXTRA
    } else {
        0
    };

    // A refetch is dominated by the address loop, which is the same work
    // whichever CS or data algorithm sits downstream of it.
    if cs_in_window {
        return match (algs.cs, algs.data) {
            (CsAlg::Cs0, DataAlg::Data0) => CS_REFETCH,
            (CsAlg::Cs1, DataAlg::Data0) => CS_REFETCH,
            (CsAlg::Cs2, DataAlg::Data0) => CS_REFETCH,
            (CsAlg::Cs0, DataAlg::Data1) => CS_REFETCH,
            (CsAlg::Cs1, DataAlg::Data1) => CS_REFETCH,
            (CsAlg::Cs2, DataAlg::Data1) => CS_REFETCH,
        };
    }

    match (algs.cs, algs.data, mode) {
        // Direct output: the byte is already fetched, so this is the cost of
        // turning the drivers on.  Word mode writes both lanes in one go and
        // is a cycle or two quicker than the 8-bit path.
        (CsAlg::Cs0, DataAlg::Data0, 16) => CS_OUTPUT_ENABLE_WORD,
        (CsAlg::Cs1, DataAlg::Data0, 16) => CS_OUTPUT_ENABLE_WORD,
        (CsAlg::Cs2, DataAlg::Data0, 16) => CS_OUTPUT_ENABLE_WORD,
        (CsAlg::Cs0, DataAlg::Data0, _) => CS_OUTPUT_ENABLE,
        (CsAlg::Cs1, DataAlg::Data0, _) => CS_OUTPUT_ENABLE,
        (CsAlg::Cs2, DataAlg::Data0, _) => CS_OUTPUT_ENABLE,

        // /BYTE-aware output: the CS machine tests /BYTE before choosing which
        // pindirs write to make, so both modes cost more than AlgData0's.
        (CsAlg::Cs0, DataAlg::Data1, 16) => CS_OUTPUT_ENABLE_BYTE_ALG_WORD,
        (CsAlg::Cs1, DataAlg::Data1, 16) => CS_OUTPUT_ENABLE_BYTE_ALG_WORD,
        (CsAlg::Cs2, DataAlg::Data1, 16) => CS_OUTPUT_ENABLE_BYTE_ALG_WORD,
        (CsAlg::Cs0, DataAlg::Data1, _) => CS_OUTPUT_ENABLE_BYTE_ALG_BYTE + high_half,
        (CsAlg::Cs1, DataAlg::Data1, _) => CS_OUTPUT_ENABLE_BYTE_ALG_BYTE + high_half,
        (CsAlg::Cs2, DataAlg::Data1, _) => CS_OUTPUT_ENABLE_BYTE_ALG_BYTE + high_half,
    }
}

// ── Probe ─────────────────────────────────────────────────────────────────────

/// Loop phases walked per cycle count.
///
/// The address loop is a handful of cycles long and free-running, so where CS
/// assertion lands within it shifts the latency by up to a full period.  A
/// cycle count only counts as sufficient if it serves correctly at every phase;
/// checking one phase understates the requirement (measurably — 11 against 13
/// on a 2364).
///
/// A read cycle's own length is not a multiple of the loop period, so where the
/// sweep starts drifts between calls.  Sweeping comfortably more phases than
/// any loop is long makes the answer independent of that drift; at 12 the same
/// run could report a count both sufficient and insufficient.
const JITTER_PHASES: u32 = 24;

/// Cycles the flush read holds each of its phases.  Long enough that the flush
/// itself is never the thing being measured.
const FLUSH_CYCLES: u32 = 32;

/// A CS-to-data delay no serving path needs more than, for reads whose purpose
/// is to observe *what* is served rather than how quickly.
const SETTLED_CYCLES: u32 = 24;

/// Drives read cycles at a chosen CS-to-data delay, for one chip and bit mode.
pub struct Probe<'a> {
    emulator: &'a Emulator,
    cache: &'a PinCache,
    addr_gpios: &'a [Vec<u8>],
    const_mask: (u64, u64),
    addr_before_cs: u32,
    mode: u8,
    slot: u8,
    /// Host base and length of the slot's SRAM, for the raw placements.
    pub(crate) slot_base: u32,
    pub(crate) slot_size: u32,
    /// Cycles this probe has stepped, so a measured read can be aligned to a
    /// known point in the free-running address loop.  Without that the sweep
    /// starts wherever the preceding reads happened to leave it, and the same
    /// cycle count can come out both sufficient and insufficient within one
    /// run — which would make the assertion flaky in CI.
    cycles: std::cell::Cell<u64>,

    /// The slot's raw contents as the pass found them.  The bisection scribbles
    /// over the slot, so every write is made against this and the slot is put
    /// back from it; `run_pass` then digests the slot to prove it was.
    snapshot: Vec<u8>,
}

impl<'a> Probe<'a> {
    /// `addr_gpios` and `const_mask` must be the ones the bulk pass uses for
    /// this mode, and `slot` the RAM slot the bus is serving.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        emulator: &'a Emulator,
        cache: &'a PinCache,
        addr_gpios: &'a [Vec<u8>],
        const_mask: (u64, u64),
        addr_before_cs: u32,
        mode: u8,
        slot: u8,
    ) -> Self {
        let (_, info) = emulator.get_ram_slot_info(slot);
        let (slot_base, slot_size) = info.map(|i| (i.addr, i.size)).unwrap_or((0, 0));
        let mut probe = Self {
            emulator,
            cache,
            addr_gpios,
            const_mask,
            addr_before_cs,
            mode,
            slot,
            slot_base,
            slot_size,
            snapshot: Vec::new(),
            cycles: std::cell::Cell::new(0),
        };
        probe.snapshot = (0..probe.slot_size)
            .map(|off| probe.raw_byte(off).unwrap_or(0))
            .collect();
        probe
    }

    /// Step the emulator, keeping the phase counter in step with it.
    fn step(&self, cycles: u32) {
        self.emulator.step_cycles(cycles);
        self.cycles.set(self.cycles.get() + cycles as u64);
    }

    /// Step to the next multiple of [`JITTER_PHASES`], so the read that follows
    /// starts at a known phase regardless of what ran before it.
    fn align(&self) {
        let p = JITTER_PHASES as u64;
        let pad = (p - (self.cycles.get() % p)) % p;
        if pad > 0 {
            self.step(pad as u32);
        }
    }

    /// The byte `offset` held when the pass started.
    fn snapshot_byte(&self, offset: u32) -> Option<u8> {
        self.snapshot.get(offset as usize).copied()
    }

    /// Put the whole slot back to how the pass found it.
    fn restore_all(&self) {
        for (off, &b) in self.snapshot.iter().enumerate() {
            self.set_raw_byte(off as u32, b);
        }
    }

    /// Locate, exactly, the SRAM byte that a read of `drive_addr` serves on
    /// data lane `lane`.
    ///
    /// Found by bisection rather than computed.  The index is the sampled GPIO
    /// word, but which of its bits actually reach the index — after input
    /// inversion, forced-low overrides, and the pins the tester does not drive
    /// at all — is exactly the sort of thing that is easy to model wrongly and
    /// impossible to notice having modelled wrongly, because a wrong offset
    /// still *looks* right: the image is replicated, so the byte there matches.
    ///
    /// So: flood half the slot with a value the read cannot otherwise return,
    /// see whether the bus starts returning it, and narrow.  ~log2(size) bus
    /// reads, and correct by construction for any geometry.  The caller must
    /// have taken a snapshot; this leaves the slot scribbled on.
    fn find_lane_offset(&self, drive_addr: usize, decoy_addr: usize, lane: usize) -> Option<u32> {
        let cs = SETTLED_CYCLES;
        let served = *self.read_cycle(drive_addr, decoy_addr, cs, 0).get(lane)?;
        let marker = self.emulator.map_data_to_phys(!served);

        let (mut lo, mut hi) = (0u32, self.slot_size);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            // Restore the whole range, then mark only the low half, so exactly
            // one half is distinguishable on each iteration.
            for off in lo..hi {
                self.set_raw_byte(off, self.snapshot_byte(off)?);
            }
            for off in lo..mid {
                self.set_raw_byte(off, marker);
            }
            let got = *self.read_cycle(drive_addr, decoy_addr, cs, 0).get(lane)?;
            if got == !served {
                hi = mid;
            } else if got == served {
                lo = mid;
            } else {
                // Neither value: the read is not coming from this slot at all.
                return None;
            }
        }

        // Confirm the single candidate really is the byte served, rather than
        // trusting the bisection's last step.
        for off in 0..self.slot_size {
            self.set_raw_byte(off, self.snapshot_byte(off)?);
        }
        self.set_raw_byte(lo, marker);
        let got = *self.read_cycle(drive_addr, decoy_addr, cs, 0).get(lane)?;
        self.set_raw_byte(lo, self.snapshot_byte(lo)?);
        (got == !served).then_some(lo)
    }

    /// Raw SRAM byte at `offset` within the slot.
    pub(crate) fn raw_byte(&self, offset: u32) -> Option<u8> {
        (offset < self.slot_size).then(|| unsafe {
            *self
                .emulator
                .sram_host_ptr(self.slot_base)
                .cast::<u8>()
                .add(offset as usize)
        })
    }

    /// Write `value` (a raw, already-mangled byte) to `offset` within the slot.
    fn set_raw_byte(&self, offset: u32, value: u8) -> bool {
        if offset >= self.slot_size {
            return false;
        }
        unsafe {
            *self
                .emulator
                .sram_host_ptr(self.slot_base)
                .cast::<u8>()
                .add(offset as usize) = value;
        }
        true
    }

    /// Logical byte offsets the read of `drive_addr` serves.
    fn logical(&self, drive_addr: usize) -> Vec<usize> {
        if self.mode == 16 {
            vec![drive_addr * 2, drive_addr * 2 + 1]
        } else {
            vec![drive_addr]
        }
    }

    /// The bytes the slot holds at the offsets `drive_addr` serves.
    fn slot_bytes(&self, drive_addr: usize) -> Option<Vec<u8>> {
        self.logical(drive_addr)
            .iter()
            .map(|&off| {
                let mut b = [0u8; 1];
                self.emulator
                    .read_ram_rom_slot(self.slot, off as u32, &mut b)
                    .is_ok()
                    .then_some(b[0])
            })
            .collect()
    }

    /// One read cycle, returning the bytes on the data lines.
    ///
    /// A generously-timed read at `decoy_addr` runs first so the data lines do
    /// not still carry `drive_addr`'s byte from an earlier call — without it
    /// this measures pin retention rather than the serving pipeline.  The decoy
    /// address is held through the idle gap so `drive_addr` is present for
    /// exactly `addr_before_cs` cycles before CS asserts, as in the bulk pass.
    fn read_cycle(
        &self,
        drive_addr: usize,
        decoy_addr: usize,
        cs_to_data: u32,
        jitter: u32,
    ) -> Vec<u8> {
        let states = self.read_cycle_states(drive_addr, decoy_addr, cs_to_data, jitter);
        self.extract(states)
    }

    /// As [`Self::read_cycle`], returning the raw pin states rather than the
    /// extracted bytes.
    fn read_cycle_states(
        &self,
        drive_addr: usize,
        decoy_addr: usize,
        cs_to_data: u32,
        jitter: u32,
    ) -> u64 {
        let deasserted = driver::ctrl_mask(&self.cache.control_lines, false);
        let asserted = driver::ctrl_mask(&self.cache.control_lines, true);
        let am = driver::addr_mask(drive_addr, self.addr_gpios);
        let decoy = driver::addr_mask(decoy_addr, self.addr_gpios);

        let decoy_idle = driver::merge(driver::merge(decoy, deasserted), self.const_mask);
        let decoy_read = driver::merge(driver::merge(decoy, asserted), self.const_mask);
        self.emulator.drive_gpios(decoy_idle.0, decoy_idle.1);
        self.step(FLUSH_CYCLES);
        self.emulator.drive_gpios(decoy_read.0, decoy_read.1);
        self.step(FLUSH_CYCLES);

        self.emulator.drive_gpios(decoy_idle.0, decoy_idle.1);
        self.step(timing::CYCLES_AFTER_READ + jitter);

        let phase1 = driver::merge(driver::merge(am, deasserted), self.const_mask);
        self.emulator.drive_gpios(phase1.0, phase1.1);
        self.step(self.addr_before_cs);

        let phase2 = driver::merge(driver::merge(am, asserted), self.const_mask);
        self.emulator.drive_gpios(phase2.0, phase2.1);
        self.step(cs_to_data);

        let states = self.emulator.read_pin_states();

        self.emulator.drive_gpios(decoy_idle.0, decoy_idle.1);
        self.step(timing::CYCLES_AFTER_READ);

        states
    }

    /// Data-line bytes for the current mode: both lanes for a 16-bit word, the
    /// low lane alone otherwise (`/BYTE` tristates the high lane).
    fn extract(&self, states: u64) -> Vec<u8> {
        let d = &self.cache.data_gpios;
        if self.mode == 16 {
            vec![
                driver::extract_byte(states, &d[..8]),
                driver::extract_byte(states, &d[8..16]),
            ]
        } else {
            vec![driver::extract_byte(states, &d[..8.min(d.len())])]
        }
    }

    /// The lowest cycle count from which this count and every larger one serve
    /// `expect` at every loop phase, searched up to `max`.
    ///
    /// Reported when an assertion fails, so the message says what the latency
    /// actually is rather than only that it is not what was expected — which
    /// is the number the developer needs in order to decide whether a change
    /// is a regression or a new correct value.
    pub fn measure_threshold(
        &self,
        drive_addr: usize,
        decoy_addr: usize,
        expect: &[u8],
        max: u32,
    ) -> Option<u32> {
        let mut lowest = None;
        for cs in (1..=max).rev() {
            if self.serves_at(drive_addr, decoy_addr, expect, cs) {
                lowest = Some(cs);
            } else {
                break;
            }
        }
        lowest
    }

    /// Measure this address's CS-to-data latency: mark the exact bytes the
    /// read serves, find the threshold, and put the slot back.
    ///
    /// `None` if the served bytes could not be located or no count up to `max`
    /// serves them.
    pub fn measure_at(&self, drive_addr: usize, span: usize, max: u32) -> Option<u32> {
        let orig = self.slot_bytes(drive_addr)?;
        let marked: Vec<u8> = orig.iter().map(|b| !b).collect();
        let decoy = self.pick_decoy(drive_addr, span, &marked)?;
        let measured = self
            .mark(drive_addr, decoy, &marked)
            .and_then(|_| self.measure_threshold(drive_addr, decoy, &marked, max));
        self.restore_all();
        measured
    }

    /// Whether `expect` is served at `cs_to_data` at **every** loop phase.
    pub fn serves_at(
        &self,
        drive_addr: usize,
        decoy_addr: usize,
        expect: &[u8],
        cs_to_data: u32,
    ) -> bool {
        (0..JITTER_PHASES).all(|j| {
            self.align();
            self.read_cycle(drive_addr, decoy_addr, cs_to_data, j) == expect
        })
    }

    /// Mark the exact SRAM bytes this read serves, so only a post-CS fetch can
    /// return them.  Returns what was overwritten, or `None` if the served
    /// bytes could not be located.
    fn mark(&self, drive_addr: usize, decoy_addr: usize, marked: &[u8]) -> Option<Vec<u32>> {
        // Locate every lane before writing any marker: the search restores the
        // slot from the snapshot as it narrows, so a marker written before a
        // later lane's search would be wiped out by it.
        let offsets: Vec<u32> = (0..marked.len())
            .map(|lane| self.find_lane_offset(drive_addr, decoy_addr, lane))
            .collect::<Option<_>>()?;
        self.restore_all();
        for (&offset, &value) in offsets.iter().zip(marked) {
            // SRAM holds mangled bytes; the probe compares logical ones.
            if !self.set_raw_byte(offset, self.emulator.map_data_to_phys(value)) {
                return None;
            }
        }
        trace!("CS timing: addr={drive_addr:#x} served from SRAM {offsets:x?}");
        Some(offsets)
    }

    /// A marker value per lane that no read below `expected` cycles can serve.
    ///
    /// Complementing the original byte is not enough.  The entry a too-early
    /// read fetches is a *different* entry holding an unrelated byte, and one
    /// time in 256 per lane that byte equals the complement — whereupon the
    /// marker appears to be served early and the check reports the serving path
    /// as quicker than it is.  Across a sweep that is not a rare event: it hit
    /// one of 45 checks on a banked 2716.
    ///
    /// So the bytes actually served below the threshold are observed first, and
    /// the marker is chosen to be none of them.
    fn choose_marker(
        &self,
        drive_addr: usize,
        decoy_addr: usize,
        orig: &[u8],
        expected: u32,
    ) -> Option<Vec<u8>> {
        let mut forbidden: Vec<Vec<u8>> = orig.iter().map(|&b| vec![b]).collect();
        for cs in 1..expected.max(2) {
            for jitter in 0..JITTER_PHASES {
                self.align();
                let served = self.read_cycle(drive_addr, decoy_addr, cs, jitter);
                for (lane, &b) in served.iter().enumerate() {
                    if let Some(f) = forbidden.get_mut(lane)
                        && !f.contains(&b)
                    {
                        f.push(b);
                    }
                }
            }
        }
        forbidden
            .iter()
            .map(|f| (0u8..=255).find(|b| !f.contains(b)))
            .collect()
    }

    /// A flush address whose *served* bytes differ from `expect` in every lane,
    /// so the flush read can never leave a byte the probe would accept.
    ///
    /// Established by reading the bus, not by consulting the slot through
    /// `ora_read_ram_rom_slot`: that maps through the slot's own chip, so for a
    /// Multi secondary or a non-primary bank it names a different entry.  A
    /// decoy picked that way can serve the marker value itself, and the flush
    /// then leaves the marker on the data lines — which reads as the device
    /// serving it far sooner than it does.
    fn pick_decoy(&self, drive_addr: usize, span: usize, expect: &[u8]) -> Option<usize> {
        (1..256usize).find_map(|step| {
            let cand = (drive_addr + step * 0x53) % span;
            if cand == drive_addr {
                return None;
            }
            let served = self.read_cycle(cand, drive_addr, SETTLED_CYCLES, 0);
            (served.len() == expect.len() && served.iter().zip(expect).all(|(g, e)| g != e))
                .then_some(cand)
        })
    }
}

// ── The check ─────────────────────────────────────────────────────────────────

/// Outcome of one CS-to-data timing check.
#[derive(Debug, Clone)]
pub struct TimingResult {
    /// Addresses checked.
    pub checks: u64,
    /// Addresses where the measured latency was not `expected`.
    pub failures: u64,
}

/// Assert the CS-to-data latency is exactly what `expected` gives, at `addrs`.
///
/// `expected` is a function of the address rather than a constant because in
/// `AlgData1` byte mode A-1 selects the word half, and the high half costs a
/// cycle more.
///
/// Two-sided, and both sides matter:
///
/// * at `expected` the served bytes must be right at every loop phase — a
///   slowdown in the serving path breaks this;
/// * at `expected - 1` they must be wrong at some phase — if that passes, the
///   path got quicker, or the check stopped discriminating, and either way the
///   expected value no longer describes the firmware.
///
/// The check is only meaningful against a byte the stale pre-CS fetch cannot
/// supply, so each address is marked with a distinguishing byte first and
/// restored afterwards; on any failure to write or restore, the address is
/// counted as a failure rather than skipped.
pub fn check_cs_timing(
    probe: &Probe,
    span: usize,
    addrs: &[usize],
    expected: impl Fn(usize) -> u32,
    label: &str,
) -> TimingResult {
    let mut result = TimingResult {
        checks: 0,
        failures: 0,
    };

    for &drive_addr in addrs {
        result.checks += 1;
        let expected = expected(drive_addr);

        let Some(orig) = probe.slot_bytes(drive_addr) else {
            error!("CS TIMING {label} addr={drive_addr:#x}: cannot read slot bytes");
            result.failures += 1;
            continue;
        };

        // A first-guess marker, only to pick a flush address against; the real
        // one is chosen below once the bytes to avoid have been observed.
        let provisional: Vec<u8> = orig.iter().map(|b| !b).collect();

        let Some(decoy) = probe.pick_decoy(drive_addr, span, &provisional) else {
            result.failures += 1;
            error!(
                "CS TIMING {label} addr={drive_addr:#x}: no flush address found whose \
                 bytes differ from the marker"
            );
            continue;
        };

        let Some(marked) = probe.choose_marker(drive_addr, decoy, &orig, expected) else {
            result.failures += 1;
            error!(
                "CS TIMING {label} addr={drive_addr:#x}: every byte value is \
                 served by some read below {expected} cycles, so no marker can \
                 discriminate"
            );
            continue;
        };

        let marks = probe.mark(drive_addr, decoy, &marked);
        let outcome = marks.as_ref().map(|_| {
            let at = probe.serves_at(drive_addr, decoy, &marked, expected);
            // `expected - 1` is only meaningful above zero; a zero-cycle step
            // is not expressible, and no real path is that quick.
            let below = expected > 0 && probe.serves_at(drive_addr, decoy, &marked, expected - 1);
            let measured = if at && !below {
                None
            } else {
                probe.measure_threshold(drive_addr, decoy, &marked, MEASURE_CEILING)
            };
            (at, below, measured)
        });

        // Always put the slot back, whatever happened — the bisection scribbles
        // over it, and every later chip, bank and mode reads the same image.
        probe.restore_all();

        match outcome {
            Some((true, false, _)) => {}
            Some((false, _, measured)) => {
                result.failures += 1;
                error!(
                    "CS TIMING {label} addr={drive_addr:#x}: not served correctly at \
                     cs_to_data={expected}, measured {} — the serving path got \
                     slower, or the expected value is wrong for this \
                     configuration",
                    fmt_measured(measured),
                );
            }
            Some((true, true, measured)) => {
                result.failures += 1;
                error!(
                    "CS TIMING {label} addr={drive_addr:#x}: already served correctly \
                     at cs_to_data={} — the serving path got quicker, or the \
                     check has stopped discriminating; measured {}",
                    expected - 1,
                    fmt_measured(measured),
                );
            }
            None => {
                result.failures += 1;
                error!(
                    "CS TIMING {label} addr={drive_addr:#x}: could not locate the SRAM \
                     bytes the bus serves, so the check could not be made \
                     discriminating"
                );
            }
        }
    }

    result
}

// ── Driving the pass ──────────────────────────────────────────────────────────

/// Highest cycle count the diagnostic re-measurement searches to.
const MEASURE_CEILING: u32 = 32;

/// Addresses checked per chip and mode.  Three is plenty — the latency does not
/// vary with address, and each one costs two cycle counts × [`JITTER_PHASES`]
/// read cycles, against the thousands the bulk pass already runs.
const CHECK_ADDRS: usize = 3;

/// Outcome of the timing pass for one chip and mode.
pub struct PassResult {
    pub checks: u64,
    pub failures: u64,
    /// Why no check ran, when `checks` is 0.
    pub note: Option<String>,
}

impl PassResult {
    pub fn skipped(note: impl Into<String>) -> Self {
        Self {
            checks: 0,
            failures: 0,
            note: Some(note.into()),
        }
    }
}

/// Run the CS-to-data timing check for one chip and bit mode.
///
/// `background` is the same constant GPIO background the bulk pass uses for
/// this set (X pins for Multi/Banked, empty for Single).  Everything else is
/// derived from the running firmware, so a new chip type or board needs no
/// change here: the expected latency follows from the algorithms in use and
/// from whether a control line sits inside the sampled address window.
#[allow(clippy::too_many_arguments)]
pub fn run_pass(
    emulator: &Emulator,
    cache: &PinCache,
    mode: u8,
    addr_before_cs: u32,
    background: (u64, u64),
    info: &ServingAlgInfo,
    num_addrs: usize,
    gap_gpios: &[u8],
    label: &str,
) -> PassResult {
    // The expectation comes from the configuration, via onerom-gen.  Taking it
    // from the running firmware instead would make a firmware that programmed
    // the wrong address window self-consistent: the expected latency would move
    // along with the bug.  The firmware's own report is used to check that it
    // programmed what the configuration called for.
    let algs = Algs::from_config(info);

    let Some(serving) = emulator.serving_alg() else {
        return PassResult::skipped("firmware reported no slot being served");
    };
    match Algs::from_serving(&serving) {
        Ok(running) if running != algs => {
            error!(
                "CS TIMING {label}: firmware is serving with {running:?} but the \
                 configuration derives {algs:?}"
            );
            return PassResult {
                checks: 1,
                failures: 1,
                note: None,
            };
        }
        Err(u) => {
            return PassResult::skipped(format!(
                "firmware reported {} algorithm id {}, which this tester has \
                 no variant for",
                u.family, u.id
            ));
        }
        Ok(_) => {}
    }
    if serving.addr_window_base != info.addr_window_base
        || serving.addr_window_pins != info.addr_window_pins
    {
        error!(
            "CS TIMING {label}: firmware samples GPIO window [{},{}) but the \
             configuration derives [{},{})",
            serving.addr_window_base,
            serving.addr_window_base + serving.addr_window_pins,
            info.addr_window_base,
            info.addr_window_base + info.addr_window_pins,
        );
        return PassResult {
            checks: 1,
            failures: 1,
            note: None,
        };
    }

    if cache.control_lines.is_empty() {
        return PassResult::skipped("chip has no driveable control lines");
    }

    let byte_mask = match cache.byte_n_gpio {
        Some(g) => driver::byte_n_mask(g, mode),
        None => (0, 0),
    };
    let const_mask = driver::merge(byte_mask, background);

    // Mirrors the bulk pass: in 16-bit mode addr_gpios[0] is A-1, which is
    // also D15, so it is driven by the chip rather than the tester.
    let addr_gpios: &[Vec<u8>] = if mode == 16 {
        &cache.addr_gpios[1..]
    } else {
        &cache.addr_gpios
    };
    if addr_gpios.is_empty() {
        return PassResult::skipped("chip has no tester-driven address lines");
    }
    // Addresses to choose from.  Not 2^lines: a chip whose size is not a power
    // of two (23QL384 is 48KB across 16 lines) has addresses in that range it
    // never serves, and a marker cannot be placed in an image that has no entry
    // for them.
    let span = (1usize << addr_gpios.len()).min(num_addrs.max(1));

    // A control line only changes the SRAM index if the state machine both
    // samples it *and* the firmware lets its level through.  A `GpioOverLow`
    // override pins one low whatever the bus does, so it contributes a constant
    // to the index and asserting it forces no refetch — a 2708's OE lands
    // inside the window a banked set widens it to, but is overridden, and
    // serves in the output-enable time rather than the refetch time.
    let cs_in_window = cache
        .control_lines
        .iter()
        .flat_map(|cl| cl.gpios.iter())
        .any(|&g| info.samples_gpio(g) && !gap_gpios.contains(&g));

    let slot = match resolve_slot(
        emulator,
        cache,
        addr_gpios,
        const_mask,
        mode,
        addr_before_cs,
    ) {
        Some(s) => s,
        None => {
            return PassResult::skipped(
                "no RAM slot matches what the bus serves, so a marker could \
                 not be placed where the read would find it",
            );
        }
    };

    let probe = Probe::new(
        emulator,
        cache,
        addr_gpios,
        const_mask,
        addr_before_cs,
        mode,
        slot,
    );

    debug!(
        "CS timing {label}: algs={algs:?} window=[{},{}) cs_in_window={cs_in_window} \
         mode={mode} slot={slot} slot_base={:#x} slot_size={}",
        serving.addr_window_base,
        serving.addr_window_base + serving.addr_window_pins,
        probe.slot_base,
        probe.slot_size,
    );

    // Spread across the space rather than clustering at the bottom, so a
    // latency that somehow depended on address bits would not be missed.
    let addrs: Vec<usize> = (0..CHECK_ADDRS)
        .map(|i| (span / (CHECK_ADDRS + 1)) * (i + 1) % span)
        .collect();

    // The pass writes markers into the served image and puts them back.  If it
    // ever failed to, every later chip, bank and mode would be read against a
    // corrupted image and the bulk pass would report failures that are really
    // this pass's fault — so the restoration is verified, not trusted.
    let before = slot_digest(emulator, &probe);

    let r = check_cs_timing(
        &probe,
        span,
        &addrs,
        |a| expected_cs_to_data(algs, mode, cs_in_window, a),
        label,
    );

    let after = slot_digest(emulator, &probe);
    if before != after {
        error!(
            "CS TIMING {label}: the timing pass left the served image modified \
             (slot {slot}, mode {mode}) — markers were not fully restored"
        );
        return PassResult {
            checks: r.checks,
            failures: r.failures + 1,
            note: None,
        };
    }

    PassResult {
        checks: r.checks,
        failures: r.failures,
        note: None,
    }
}

fn fmt_measured(m: Option<u32>) -> String {
    match m {
        Some(c) => c.to_string(),
        None => format!("nothing up to {MEASURE_CEILING}"),
    }
}

/// A cheap digest of the slot's raw SRAM, for confirming the pass restored it.
fn slot_digest(emulator: &Emulator, probe: &Probe) -> u64 {
    let _ = emulator;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for off in 0..probe.slot_size {
        if let Some(b) = probe.raw_byte(off) {
            h ^= b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

/// Identify which RAM slot the bus is serving under the current background.
///
/// Public so a diagnostic can build a [`Probe`] the same way the pass does.
///
/// Established by trying to locate a served byte in each slot in turn, rather
/// than by comparing bus reads against `ora_read_ram_rom_slot`.  That mapping
/// goes through the *slot's* chip, so for a Multi set's secondary chips — and
/// for a Banked set's non-selected banks — it names a different entry and no
/// slot appears to match, which silently skipped the check on exactly the sets
/// that have no replication to fall back on.
pub fn resolve_slot(
    emulator: &Emulator,
    cache: &PinCache,
    addr_gpios: &[Vec<u8>],
    const_mask: (u64, u64),
    mode: u8,
    addr_before_cs: u32,
) -> Option<u8> {
    // Addresses to choose from.  Not 2^lines: a chip whose size is not a power
    // of two (23QL384 is 48KB across 16 lines) has addresses in that range it
    // never serves, and a marker cannot be placed in an image that has no entry
    // for them.
    let span = 1usize << addr_gpios.len();
    let addr = span / 2;
    let decoy = (addr + span / 4 + 1) % span;

    (0..emulator.get_ram_slot_count()).find(|&slot| {
        let probe = Probe::new(
            emulator,
            cache,
            addr_gpios,
            const_mask,
            addr_before_cs,
            mode,
            slot,
        );
        let found = probe.find_lane_offset(addr, decoy, 0).is_some();
        probe.restore_all();
        found
    })
}
