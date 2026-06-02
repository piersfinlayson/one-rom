// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::vec::Vec;
use core::num::Wrapping;
use embassy_rp::gpio::{Flex, Pull};
use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::pin_map::BoardPinMap;
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
    fn remap_phys_pin(chip: ChipType, pin_map: &BoardPinMap, phys: u8) -> u8 {
        if chip.chip_pins() == 24 && pin_map.chip_pins() == 28 {
            // 24-pin chip in a 28-pin socket adapter position: chip pin 1 maps
            // to board socket pin 3, so shift all logical chip pins by +2.
            phys + 2
        } else {
            phys
        }
    }

    /// Construct a `RomReader` for `chip` on the board described by `pin_map`.
    ///
    /// # Panics
    /// - If a required CS polarity env var was not set for a configurable line.
    /// - If `pin_map` does not contain a GPIO for a physical pin required by
    ///   `chip` (indicates a board/chip-type mismatch).
    pub fn new(pin_map: &BoardPinMap, chip: ChipType, cs: CsPolarities, tristate: bool) -> Self {
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
                let phys = Self::remap_phys_pin(chip, pin_map, phys);
                let gpio = pin_map.gpio_for_chip_pin(phys).unwrap_or_else(|| {
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
                let phys = Self::remap_phys_pin(chip, pin_map, phys);
                let gpio = pin_map.gpio_for_chip_pin(phys).unwrap_or_else(|| {
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
            let phys = Self::remap_phys_pin(chip, pin_map, ctrl.pin);
            let gpio = pin_map.gpio_for_chip_pin(phys).unwrap_or_else(|| {
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
        let (read_delay_cycles, read_delay_16bit_cycles, tristate_settle_cycles) = match chip {
            // 8-bit delay is longer: A-1 participates in address decoding.
            ChipType::Chip27C400 => (12, 8, Some(200)),
            _ => (8, 8, Some(100)),
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
                self.chip.size_bytes()/2,
                1,                   // bit 0 (A-1) always 0 in 16-bit mode
                2,
                self.read_delay_16bit_cycles,
            ),
            _ => (
                self.chip.size_bytes(),
                0,
                1,
                self.read_delay_cycles,
            ),
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
