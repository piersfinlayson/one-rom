// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::vec::Vec;
use core::num::Wrapping;
use embassy_rp::gpio::{Flex, Pull};
use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::Board;
use sha1::{Digest, Sha1};

use crate::hw::steal_gpio;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// SHA-1 digest, wrapping 32-bit checksum, and tristate failure count for
/// one bit-mode read pass.
pub struct ModeResult {
    pub mode: u8,
    pub sha1: [u8; 20],
    pub checksum: u32,
    pub failures: u32,
}

pub type ReadResult = Vec<ModeResult>;

/// Active level for a configurable CS line on a mask ROM.
///
/// `true`  = active-high (drive the pin high to assert)
/// `false` = active-low  (drive the pin low  to assert)
///
/// Set via `CS1`, `CS2`, `CS3` environment variables at build time.
/// Required for any chip whose corresponding CS line is
/// [`ControlLineType::Configurable`].
#[derive(Copy, Clone, Default)]
pub struct CsPolarities {
    pub cs1: Option<bool>,
    pub cs2: Option<bool>,
    pub cs3: Option<bool>,
}

impl CsPolarities {
    #[allow(unused)]
    pub const fn none() -> Self {
        Self {
            cs1: None,
            cs2: None,
            cs3: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Look up the MCU GPIO wired to physical socket pin `physical_pin` (1-based)
/// on `board`.
///
/// [`Board::socket_pin_map`] can list more than one GPIO for a pin on boards
/// that route a socket pin to two GPIOs for the serving PIO (Fire32/40).
/// Those GPIOs share a net, so for a reader driving or sampling the pin any
/// one of them is correct; we take the first.
///
/// Returns `None` for non-signal pins (VCC, GND) and any pin absent from the
/// board's map.
fn gpio_for_socket_pin(board: Board, physical_pin: u8) -> Option<u8> {
    board
        .socket_pin_map()
        .iter()
        .find(|&&(pin, _)| pin == physical_pin)
        .and_then(|&(_, gpios)| gpios.first().copied())
}

struct ChecksumState(Wrapping<u32>);

impl ChecksumState {
    fn new() -> Self {
        Self(Wrapping(0))
    }
    #[inline]
    fn update(&mut self, byte: u8) {
        self.0 += Wrapping(byte as u32);
    }
    fn finish(self) -> u32 {
        self.0.0
    }
}

/// A GPIO-backed control line with its assert polarity.
struct ControlLine {
    flex: Flex<'static>,
    assert_high: bool,
}

impl ControlLine {
    fn active_low(flex: Flex<'static>) -> Self {
        Self {
            flex,
            assert_high: false,
        }
    }

    fn configurable(flex: Flex<'static>, assert_high: bool) -> Self {
        Self { flex, assert_high }
    }

    fn init(&mut self) {
        self.flex.set_as_output();
        self.deassert();
    }

    #[inline]
    fn assert(&mut self) {
        if self.assert_high {
            self.flex.set_high();
        } else {
            self.flex.set_low();
        }
    }

    #[inline]
    fn deassert(&mut self) {
        if self.assert_high {
            self.flex.set_low();
        } else {
            self.flex.set_high();
        }
    }
}

// ---------------------------------------------------------------------------
// RomReader
// ---------------------------------------------------------------------------

pub struct RomReader {
    addr: Vec<Flex<'static>>,
    data: Vec<Flex<'static>>,
    ce: Option<ControlLine>,
    oe: Option<ControlLine>,
    cs1: Option<ControlLine>,
    cs2: Option<ControlLine>,
    cs3: Option<ControlLine>,
    /// BYTE# pin (27C400 only).  Active-low: low = 8-bit mode, high = 16-bit.
    byte_n: Option<Flex<'static>>,
    chip: ChipType,
    /// Read-delay cycles at 150 MHz for 8-bit (or sole) mode.
    read_delay_cycles: u32,
    /// Read-delay cycles for 16-bit mode (27C400 only).
    read_delay_16bit_cycles: u32,
    tristate_settle_cycles: Option<u32>,
}

impl RomReader {
    #[inline]
    fn remap_phys_pin(chip: ChipType, board: Board, phys: u8) -> u8 {
        if chip.chip_pins() == 24 && board.chip_pins() == 28 {
            // 24-pin chip in a 28-pin socket adapter position: chip pin 1 maps
            // to board socket pin 3, so shift all logical chip pins by +2.
            phys + 2
        } else {
            phys
        }
    }

    /// Construct a `RomReader` for `chip` on `board`.
    ///
    /// # Panics
    /// - If a required CS polarity env var was not set for a configurable line.
    /// - If `board` does not contain a GPIO for a physical pin required by
    ///   `chip` (indicates a board/chip-type mismatch).
    pub fn new(board: Board, chip: ChipType, cs: CsPolarities, tristate: bool) -> Self {
        // Validate CS polarities are provided for every configurable line.
        for ctrl in chip.control_lines() {
            if ctrl.line_type == ControlLineType::Configurable {
                let provided = match ctrl.name {
                    "cs1" => cs.cs1.is_some(),
                    "cs2" => cs.cs2.is_some(),
                    "cs3" => cs.cs3.is_some(),
                    _ => true,
                };
                assert!(
                    provided,
                    "CS polarity for '{}' must be set via the {} env var for chip {}",
                    ctrl.name,
                    ctrl.name.to_uppercase(),
                    chip.name(),
                );
            }
        }

        // Build address-line GPIO list (A0 … An, or A-1 … An for 27C400).
        let addr: Vec<Flex<'static>> = chip
            .address_pins()
            .iter()
            .map(|&phys| {
                let phys = Self::remap_phys_pin(chip, board, phys);
                let gpio = gpio_for_socket_pin(board, phys).unwrap_or_else(|| {
                    panic!(
                        "address pin {} not mapped for chip {} on this board",
                        phys,
                        chip.name()
                    )
                });
                steal_gpio(gpio)
            })
            .collect();

        // Build data-line GPIO list (D0 … Dn).
        let data: Vec<Flex<'static>> = chip
            .data_pins()
            .iter()
            .map(|&phys| {
                let phys = Self::remap_phys_pin(chip, board, phys);
                let gpio = gpio_for_socket_pin(board, phys).unwrap_or_else(|| {
                    panic!(
                        "data pin {} not mapped for chip {} on this board",
                        phys,
                        chip.name()
                    )
                });
                steal_gpio(gpio)
            })
            .collect();

        // Build control-line GPIOs.
        let mut ce = None;
        let mut oe = None;
        let mut cs1 = None;
        let mut cs2 = None;
        let mut cs3 = None;
        let mut byte_n = None;

        for ctrl in chip.control_lines() {
            let phys = Self::remap_phys_pin(chip, board, ctrl.pin);
            let gpio = gpio_for_socket_pin(board, phys).unwrap_or_else(|| {
                panic!(
                    "control pin {} ('{}') not mapped for chip {} on this board",
                    phys,
                    ctrl.name,
                    chip.name()
                )
            });
            let flex = steal_gpio(gpio);

            match ctrl.name {
                "ce" => ce = Some(ControlLine::active_low(flex)),
                "oe" => oe = Some(ControlLine::active_low(flex)),
                "cs1" => {
                    cs1 = Some(ControlLine::configurable(
                        flex,
                        cs.cs1.expect("cs1 polarity required"),
                    ))
                }
                "cs2" => {
                    cs2 = Some(ControlLine::configurable(
                        flex,
                        cs.cs2.expect("cs2 polarity required"),
                    ))
                }
                "cs3" => {
                    cs3 = Some(ControlLine::configurable(
                        flex,
                        cs.cs3.expect("cs3 polarity required"),
                    ))
                }
                "byte" => byte_n = Some(flex),
                _ => {}
            }
        }

        // Empirically determined timing at 150 MHz.
        //
        // The longer timing belongs to every 16-bit-capable part, not to the
        // 27C400 specifically: it is A-1 taking part in address decoding when
        // such a chip is read in byte mode that costs the extra cycles.  The
        // 27C200 is the same shape and was previously getting the 8-bit
        // timing.
        let (read_delay_cycles, read_delay_16bit_cycles, tristate_settle_cycles) =
            if chip.supports_bit_mode(16) {
                (12, 8, Some(200))
            } else {
                (8, 8, Some(100))
            };

        let tristate_settle_cycles = if tristate {
            tristate_settle_cycles
        } else {
            None
        };
        Self {
            addr,
            data,
            ce,
            oe,
            cs1,
            cs2,
            cs3,
            byte_n,
            chip,
            read_delay_cycles,
            read_delay_16bit_cycles,
            tristate_settle_cycles,
        }
    }

    /// Initialise all GPIO directions and drive control lines to their
    /// deasserted states.
    pub fn init(&mut self) {
        for pin in self.addr.iter_mut() {
            pin.set_as_output();
            pin.set_low();
        }
        for pin in self.data.iter_mut() {
            pin.set_pull(Pull::Down);
            pin.set_as_input();
        }
        if let Some(ref mut l) = self.ce {
            l.init();
        }
        if let Some(ref mut l) = self.oe {
            l.init();
        }
        if let Some(ref mut l) = self.cs1 {
            l.init();
        }
        if let Some(ref mut l) = self.cs2 {
            l.init();
        }
        if let Some(ref mut l) = self.cs3 {
            l.init();
        }
        if let Some(ref mut p) = self.byte_n {
            p.set_as_output();
            p.set_high(); // deasserted: default to 16-bit mode until explicitly set
        }
    }

    /// Read the ROM in every bit mode the chip supports and return results
    /// for each pass.  For most chips this is a single 8-bit pass; the
    /// 27C400 produces both an 8-bit and a 16-bit pass.
    pub fn read(&mut self) -> ReadResult {
        let mut results = Vec::new();
        for &mode in self.chip.bit_modes() {
            let mut sha = Sha1::new();
            let mut csum = ChecksumState::new();
            let failures = self.read_mode(mode, &mut sha, &mut csum);
            let mut sha1 = [0u8; 20];
            sha1.copy_from_slice(&sha.finalize());
            results.push(ModeResult {
                mode,
                sha1,
                checksum: csum.finish(),
                failures,
            });
        }
        results
    }

    // --- Private methods ------------------------------------------------

    /// Read the entire address space in `mode`-bit mode, feeding every byte
    /// into `sha` and `csum` and testing tristate after each address cycle.
    #[inline(never)]
    fn read_mode(&mut self, mode: u8, sha: &mut Sha1, csum: &mut ChecksumState) -> u32 {
        let (addr_count, addr_shift, data_bytes, read_delay) = match mode {
            16 => (
                self.chip.size_bytes() / 2,
                1, // bit 0 (A-1) always 0 in 16-bit mode
                2,
                self.read_delay_16bit_cycles,
            ),
            _ => (self.chip.size_bytes(), 0, 1, self.read_delay_cycles),
        };

        self.begin_read(mode);

        let mut failures = 0u32;
        for addr in 0..addr_count {
            self.set_addr(addr << addr_shift);
            cortex_m::asm::delay(read_delay);

            for b in 0..data_bytes {
                let byte = self.read_data_byte(b);
                sha.update([byte]);
                csum.update(byte);
            }

            failures += self.test_tristate(data_bytes);
        }

        self.end_read();

        failures
    }

    fn assert_control(&mut self) {
        if let Some(ref mut l) = self.ce {
            l.assert();
        }
        if let Some(ref mut l) = self.oe {
            l.assert();
        }
        if let Some(ref mut l) = self.cs1 {
            l.assert();
        }
        if let Some(ref mut l) = self.cs2 {
            l.assert();
        }
        if let Some(ref mut l) = self.cs3 {
            l.assert();
        }
    }

    fn deassert_control(&mut self) {
        if let Some(ref mut l) = self.ce {
            l.deassert();
        }
        if let Some(ref mut l) = self.oe {
            l.deassert();
        }
        if let Some(ref mut l) = self.cs1 {
            l.deassert();
        }
        if let Some(ref mut l) = self.cs2 {
            l.deassert();
        }
        if let Some(ref mut l) = self.cs3 {
            l.deassert();
        }
    }

    #[inline(always)]
    fn set_addr(&mut self, addr: usize) {
        for (i, pin) in self.addr.iter_mut().enumerate() {
            if addr & (1 << i) != 0 {
                pin.set_high();
            } else {
                pin.set_low();
            }
        }
    }

    /// Read 8 data bits starting at `data[byte_index * 8]`.
    #[inline(always)]
    fn read_data_byte(&self, byte_index: usize) -> u8 {
        let mut val = 0u8;
        let offset = byte_index * 8;
        for (i, pin) in self.data[offset..offset + 8].iter().enumerate() {
            if pin.is_high() {
                val |= 1 << i;
            }
        }
        val
    }

    /// Test that data lines go to zero when OE or CE is deasserted (EPROMs),
    /// or CS1 when neither OE nor CE is present (mask ROMs).
    ///
    /// Assumes all control lines are currently asserted on entry; restores
    /// that state on exit.  Returns the number of tristate failures (0–2).
    fn test_tristate(&mut self, data_bytes: usize) -> u32 {
        let settle = match self.tristate_settle_cycles {
            Some(c) => c,
            None => return 0, // tristate testing disabled
        };
        let mut failures = 0u32;

        // Test OE (EPROMs with a dedicated output-enable).
        {
            let (data, oe) = (&self.data, &mut self.oe);
            if let Some(line) = oe {
                line.deassert();
                cortex_m::asm::delay(settle);
                if !Self::data_all_low(data, data_bytes) {
                    failures += 1;
                }
                line.assert();
            }
        }

        // Test CE.
        {
            let (data, ce) = (&self.data, &mut self.ce);
            if let Some(line) = ce {
                line.deassert();
                cortex_m::asm::delay(settle);
                if !Self::data_all_low(data, data_bytes) {
                    failures += 1;
                }
                line.assert();
            }
        }

        // For mask ROMs (no OE / CE), test via CS1.
        if self.oe.is_none() && self.ce.is_none() {
            let (data, cs1) = (&self.data, &mut self.cs1);
            if let Some(line) = cs1 {
                line.deassert();
                cortex_m::asm::delay(settle);
                if !Self::data_all_low(data, data_bytes) {
                    failures += 1;
                }
                line.assert();
            }
        }

        failures
    }

    fn data_all_low(data: &[Flex<'static>], data_bytes: usize) -> bool {
        data[..data_bytes * 8].iter().all(|p| p.is_low())
    }

    /// Configure the BYTE# pin and assert all control lines.
    ///
    /// Must be called before any `read_byte_at` calls.  Pair every call to
    /// `begin_read` with exactly one call to `end_read`.
    pub fn begin_read(&mut self, mode: u8) {
        if let Some(ref mut byte_n) = self.byte_n {
            if mode == 16 {
                byte_n.set_high();
            } else {
                byte_n.set_low();
                if let Some(a1) = self.addr.first_mut() {
                    a1.set_as_output();
                }
            }
        }
        self.assert_control();
    }

    /// Read a single byte from the ROM at the given byte address.
    ///
    /// In 8-bit mode `byte_addr` is the physical ROM address.
    /// In 16-bit mode `byte_addr / 2` is the word address and
    /// `byte_addr % 2` selects the low byte (D0–D7, index 0) or the high
    /// byte (D8–D15, index 1) from the 16-bit data bus.
    ///
    /// `begin_read(mode)` must have been called before the first call to
    /// this method in a read session.
    pub fn read_byte_at(&mut self, byte_addr: usize, mode: u8) -> u8 {
        let (phys_addr, byte_index, delay) = if mode == 16 {
            // Word address with A-1 (bit 0) held low, matching addr_shift=1
            // used in read_mode.
            (
                (byte_addr / 2) << 1,
                byte_addr % 2,
                self.read_delay_16bit_cycles,
            )
        } else {
            (byte_addr, 0, self.read_delay_cycles)
        };

        self.set_addr(phys_addr);
        cortex_m::asm::delay(delay);
        self.read_data_byte(byte_index)
    }

    /// Deassert all control lines and return the BYTE# pin to its idle state.
    ///
    /// Call after all `read_byte_at` calls in a read session are complete.
    pub fn end_read(&mut self) {
        self.deassert_control();
        if let Some(ref mut byte_n) = self.byte_n {
            byte_n.set_high();

            // Reset D15/A-1 shared pin to back to input with pull-down.
            if let Some(a1) = self.addr.first_mut() {
                a1.set_pull(Pull::Down);
                a1.set_as_input();
            }
        }
    }

    pub fn tristate(&self) -> bool {
        self.tristate_settle_cycles.is_some()
    }
}

// ---------------------------------------------------------------------------
// Readable — common byte-read interface for the output formatters
// ---------------------------------------------------------------------------

/// A byte-addressable ROM reader, driven one byte at a time.
///
/// `begin_read`/`end_read` bracket a burst of `read_byte_at` calls.  The
/// generic [`RomReader`] and the dedicated [`Lh53512Reader`] both implement
/// this, so the output formatters work against either without knowing which.
pub trait Readable {
    fn begin_read(&mut self, mode: u8);
    fn read_byte_at(&mut self, byte_addr: usize, mode: u8) -> u8;
    fn end_read(&mut self);
}

impl Readable for RomReader {
    fn begin_read(&mut self, mode: u8) {
        RomReader::begin_read(self, mode);
    }
    fn read_byte_at(&mut self, byte_addr: usize, mode: u8) -> u8 {
        RomReader::read_byte_at(self, byte_addr, mode)
    }
    fn end_read(&mut self) {
        RomReader::end_read(self);
    }
}

// ---------------------------------------------------------------------------
// Lh53512Reader — dedicated reader for the Sharp LH53512
// ---------------------------------------------------------------------------
//
// The LH53512 is a 65,536 x 8 CMOS mask ROM whose 8 data lines are time-
// multiplexed onto the same physical pins as its 16 address lines: a single
// bidirectional A/D bus on pins 3-10.  The address is latched in two phases —
// /LAS captures the low byte, /HAS captures the high byte.  B/D selects the
// bus direction, and the part has four mask-programmable chip selects
// (CS0-CS3) plus /OE.  None of this fits the generic `RomReader`, hence this
// dedicated reader.
//
// Read cycle (8-bit output mode), per the 1986 Sharp MOS Data Book p.642:
//   1. assert CS0-CS3 (at their programmed levels)
//   2. B/D high -> A/D pins are inputs to the chip (host drives address)
//   3. drive A/D with A0-A7, pulse /LAS low, release (latch low byte)
//   4. drive A/D with A8-A15, pulse /HAS low, release (latch high byte)
//   5. release the bus, drop B/D, assert /OE
//   6. after the access time, read D0-D7 from A/D

/// Timing constants, in CPU cycles at the Lab's 150 MHz core clock.
/// Derived from the AC characteristics at Vcc = 5 V (tWL/tWH = 500 ns min,
/// tHLA/tHHA/tHB = 200 ns min, tHAS = 3.0 us max), then relaxed for the part
/// being run below its 4.0 V spec at 3.3 V, where everything runs slower.
const T_AD_SETUP: u32 = 100; // ~0.7 us address setup before a strobe edge
const T_STROBE: u32 = 300; // ~2.0 us /LAS or /HAS pulse width
const T_HOLD: u32 = 120; // ~0.8 us address/B-D hold after an edge
const T_ACCESS: u32 = 900; // ~6.0 us HAS access time

/// Chip pin numbers for the LH53512 (24-pin DIP).
const AD_PINS: [u8; 8] = [10, 9, 8, 7, 6, 5, 4, 3]; // A/D0 .. A/D7
const CS_PINS: [u8; 4] = [2, 23, 22, 21]; // CS0 .. CS3
const OE_PIN: u8 = 15;
const LAS_PIN: u8 = 16;
const HAS_PIN: u8 = 17;
const BD_PIN: u8 = 14;
const VCC_PIN: u8 = 18; // supplied from a GPIO held high (3.3 V) for bench reads
const GND_PIN: u8 = 11; // supplied from a GPIO held low for bench reads

pub struct Lh53512Reader {
    ad: Vec<Flex<'static>>,
    cs: [ControlLine; 4],
    oe: ControlLine,
    las: Flex<'static>,
    has: Flex<'static>,
    bd: Flex<'static>,
    /// Power for the part, supplied from GPIOs so no pin bending is needed:
    /// `vcc` is held high (3.3 V) and `gnd` is held low.  The LH53512 draws
    /// ~1.5 mA max, well within a GPIO's drive capability.
    vcc: Flex<'static>,
    gnd: Flex<'static>,
}

impl Lh53512Reader {
    /// Build a reader for an LH53512, with `cs_high` giving each chip select's
    /// assert level (`true` = active-high).
    ///
    /// # Panics
    /// If a required pin has no GPIO on `board`.
    pub fn new(board: Board, cs_high: [bool; 4]) -> Self {
        let ad = AD_PINS.iter().map(|&pin| Self::gpio(board, pin)).collect();

        let cs = [
            ControlLine::configurable(Self::gpio(board, CS_PINS[0]), cs_high[0]),
            ControlLine::configurable(Self::gpio(board, CS_PINS[1]), cs_high[1]),
            ControlLine::configurable(Self::gpio(board, CS_PINS[2]), cs_high[2]),
            ControlLine::configurable(Self::gpio(board, CS_PINS[3]), cs_high[3]),
        ];

        let oe = ControlLine::active_low(Self::gpio(board, OE_PIN));
        let las = Self::gpio(board, LAS_PIN);
        let has = Self::gpio(board, HAS_PIN);
        let bd = Self::gpio(board, BD_PIN);
        let vcc = Self::gpio(board, VCC_PIN);
        let gnd = Self::gpio(board, GND_PIN);

        Self {
            ad,
            cs,
            oe,
            las,
            has,
            bd,
            vcc,
            gnd,
        }
    }

    /// Resolve the MCU GPIO for an LH53512 chip pin.  Mirrors the 24-pin-chip-
    /// in-28-pin-socket offset used by `RomReader` (`chip pin + 2`).
    fn gpio(board: Board, chip_pin: u8) -> Flex<'static> {
        let socket = if board.chip_pins() == 28 {
            chip_pin + 2
        } else {
            chip_pin
        };
        let gpio = gpio_for_socket_pin(board, socket)
            .unwrap_or_else(|| panic!("socket pin {socket} not mapped on this board"));
        steal_gpio(gpio)
    }

    /// Initialise all GPIO directions and drive the controls to their idle
    /// (deasserted) states.
    pub fn init(&mut self) {
        // Power the part first: VCC high, GND low.
        self.gnd.set_as_output();
        self.gnd.set_low();
        self.vcc.set_as_output();
        self.vcc.set_high();

        self.set_ad_input();
        for line in self.cs.iter_mut() {
            line.init();
        }
        self.oe.init();
        self.las.set_as_output();
        self.las.set_high(); // /LAS deasserted
        self.has.set_as_output();
        self.has.set_high(); // /HAS deasserted
        self.bd.set_as_output();
        self.bd.set_low(); // idle in data mode; each cycle raises it for address
    }

    #[inline]
    fn set_ad_output(&mut self) {
        for pin in self.ad.iter_mut() {
            pin.set_as_output();
        }
    }

    #[inline]
    fn set_ad_input(&mut self) {
        for pin in self.ad.iter_mut() {
            pin.set_pull(Pull::Down);
            pin.set_as_input();
        }
    }

    #[inline]
    fn drive_ad(&mut self, byte: u8) {
        for (i, pin) in self.ad.iter_mut().enumerate() {
            if byte & (1 << i) != 0 {
                pin.set_high();
            } else {
                pin.set_low();
            }
        }
    }

    #[inline]
    fn read_ad(&self) -> u8 {
        let mut val = 0u8;
        for (i, pin) in self.ad.iter().enumerate() {
            if pin.is_high() {
                val |= 1 << i;
            }
        }
        val
    }

    fn assert_cs(&mut self) {
        for line in self.cs.iter_mut() {
            line.assert();
        }
    }

    fn deassert_cs(&mut self) {
        for line in self.cs.iter_mut() {
            line.deassert();
        }
    }

    /// Read one byte using the full multiplexed protocol.
    ///
    /// The part outputs 4-bit nibbles on A/D0-3, selected by B/D (0 -> D0-3,
    /// 1 -> D4-7).  Both nibbles are read and combined.  For an 8-bit-output
    /// mask the B/D=1 nibble reads back 0, so this is harmless; for a
    /// 4-bit-output mask it recovers the missing high nibble.
    pub fn read_byte(&mut self, addr: u16) -> u8 {
        // Select the chip first: CS setup precedes everything else.
        self.assert_cs();

        // Address phase: B/D high -> the host drives the A/D bus.
        self.bd.set_high();
        self.set_ad_output();

        // Low address byte, latched on the /LAS rising edge.  The address is
        // held for tHLA after the rising edge before the bus changes.
        self.drive_ad(addr as u8);
        cortex_m::asm::delay(T_AD_SETUP);
        self.las.set_low();
        cortex_m::asm::delay(T_STROBE);
        self.las.set_high();
        cortex_m::asm::delay(T_HOLD);

        // High address byte, latched on the /HAS rising edge (held tHHA).
        self.drive_ad((addr >> 8) as u8);
        cortex_m::asm::delay(T_AD_SETUP);
        self.has.set_low();
        cortex_m::asm::delay(T_STROBE);
        self.has.set_high();
        cortex_m::asm::delay(T_HOLD);

        // Data phase: the part drives 4-bit nibbles on A/D0-3, selected by
        // B/D (0 -> D0-3, 1 -> D4-7).  B/D goes low before /OE (tSBO).
        self.set_ad_input();
        self.bd.set_low();
        cortex_m::asm::delay(T_HOLD);
        self.oe.assert();
        cortex_m::asm::delay(T_ACCESS);
        let lo = self.read_ad() & 0x0F;

        self.bd.set_high();
        cortex_m::asm::delay(T_ACCESS);
        let hi = self.read_ad() & 0x0F;

        self.oe.deassert();
        self.deassert_cs();
        (hi << 4) | lo
    }
}

impl Readable for Lh53512Reader {
    fn begin_read(&mut self, _mode: u8) {
        // Each read_byte_at is a complete, self-contained cycle.
    }

    fn read_byte_at(&mut self, byte_addr: usize, _mode: u8) -> u8 {
        self.read_byte(byte_addr as u16)
    }

    fn end_read(&mut self) {
        self.oe.deassert();
        self.deassert_cs();
        self.set_ad_input();
    }
}
