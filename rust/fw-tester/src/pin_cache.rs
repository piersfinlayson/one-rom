// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Cached ROM-socket-pin → MCU-GPIO mappings for one chip on one board.
//!
//! `PinCache::build` does the O(pins) mapping work once so the hot test loop
//! can operate purely on pre-resolved GPIO numbers.
//!
//! For secondary chips in a multi-ROM set (chips[1], chips[2], …) that are not
//! in One ROM's socket, use `PinCache::build_secondary`.  These chips share
//! the address and data bus with the primary chip (chips[0]) and are selected
//! by an X pin acting as their unique CS line.

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_gen::{ChipConfig, CsLogic, MAX_IMAGE_SIZE, num_excess_addr_lines};

// ── Public types ──────────────────────────────────────────────────────────────

/// A single decoded control line with its assertion polarity baked in.
pub struct ControlLine {
    /// Name for diagnostics ("ce", "oe", "cs1", "cs2", "cs3", "x_cs").
    // Not read in the hot path; retained for future diagnostic/tristate use.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Every MCU GPIO driven by this physical pin.
    /// Usually one; some boards (e.g. Fire32B fly-leads) wire one socket pin
    /// to two GPIOs and both must be driven.
    pub gpios: Vec<u8>,
    /// `true` → assert by driving HIGH; `false` → assert by driving LOW.
    pub assert_high: bool,
    /// `true` for a *commoned* line on a Multi-set primary: a CS line asserted
    /// on every read that does not select a chip. Set by the runner after
    /// `build`; `false` everywhere else. The tristate combo sweep holds these
    /// deasserted and never enumerates them (an asserted commoned line fires
    /// the CS gate on its own).
    pub commoned: bool,
}

/// All GPIO information needed to run the test loop for one chip.
pub struct PinCache {
    /// `addr_gpios[i]` — list of GPIOs to drive for address bit A_i (A0 first).
    pub addr_gpios: Vec<Vec<u8>>,

    /// `data_gpios[i]` — GPIO that carries data bit D_i.
    ///
    /// Because the SRAM image is pre-mangled at build time, the GPIO wired to
    /// physical data pin D_i carries the original (unmangled) logical bit i.
    /// Extracting each GPIO at position i therefore reconstructs the raw ROM
    /// byte directly, with no further transformation needed.
    ///
    /// Where a socket pin is wired to multiple GPIOs (fly-lead boards), the
    /// emulator drives them all to the same level; reading the first is enough.
    pub data_gpios: Vec<u8>,

    /// CE, OE and any active CS lines — everything asserted during a read.
    ///
    /// `CsLogic::Ignore` lines are excluded (they are permanently tied active
    /// on the board and the tester does not drive them).
    ///
    /// For oversized ROMs (e.g. 27C080 at 1MB), the top address pin(s) that
    /// exceed `MAX_IMAGE_SIZE` are also included here as half-select lines,
    /// driven by `cs1` polarity. The firmware treats these GPIOs as part of
    /// the CS range (not address range), so the tester must do the same.
    ///
    /// For secondary chips in a multi-ROM set, this contains exactly one entry:
    /// the X pin ControlLine whose assertion selects that secondary chip.
    pub control_lines: Vec<ControlLine>,

    /// GPIO for the BYTE# pin (27C400/27C200 only; `None` for all others).
    ///
    /// High = 16-bit mode (deasserted); low = 8-bit mode (asserted).
    pub byte_n_gpio: Option<u8>,
}

impl PinCache {
    /// Build a `PinCache` for `chip_type` fitted to `board`, reading CS
    /// polarities from `chip_config`.
    ///
    /// # Panics
    /// Panics if any required chip pin is absent from the board's socket pin
    /// map, or if a configurable CS line has no polarity in `chip_config`.
    pub fn build(chip_type: ChipType, chip_config: &ChipConfig, board: Board) -> Self {
        let offset = socket_offset(chip_type, board);
        let pin_map = board.socket_pin_map();

        // Build addr_gpios from all address pins initially; excess pins for
        // oversized ROMs are trimmed into control_lines below.
        let mut addr_gpios: Vec<Vec<u8>> = chip_type
            .address_pins()
            .iter()
            .map(|&pin| gpios_for(pin + offset, pin_map, chip_type, "address"))
            .collect();

        let data_gpios = chip_type
            .data_pins()
            .iter()
            .map(|&pin| gpios_for(pin + offset, pin_map, chip_type, "data")[0])
            .collect();

        let mut control_lines = Vec::new();
        let mut byte_n_gpio = None;

        for spec in chip_type.control_lines() {
            let gpios = gpios_for(spec.pin + offset, pin_map, chip_type, spec.name);

            if spec.name == "byte" {
                // BYTE# is a bit-mode select, not a read enable; handled
                // separately in the test loop.
                byte_n_gpio = Some(gpios[0]);
                continue;
            }

            if matches!(spec.name, "write" | "busy") {
                // Not a select line; excluded from CS detection and bus tristate checks.
                continue;
            }

            // The user's setting for this line, if any. For a fixed-polarity
            // line this can only be Ignore - check_cs_v2 rejects a stated
            // polarity there. For a configurable line it carries the polarity.
            let configured = match spec.name {
                "cs1" => chip_config.cs1,
                "cs2" => chip_config.cs2,
                "cs3" => chip_config.cs3,
                "cs4" => chip_config.cs4,
                "ce" => chip_config.ce,
                "oe" => chip_config.oe,
                other => panic!(
                    "Unrecognised control line '{}' on chip {}",
                    other,
                    chip_type.name()
                ),
            };

            // Ignore: permanently tied active on the board.
            // The tester does not drive it.
            if configured == Some(CsLogic::Ignore) {
                continue;
            }

            let assert_high = match spec.line_type.fixed_active_level() {
                // Polarity fixed by the silicon: the JEDEC CE/OE enables, and
                // chips whose chip selects are not mask-programmable (e.g. the
                // HM7641, CS1/CS2 active low and CS3/CS4 active high).
                Some(active_high) => active_high,
                // Mask-programmed at manufacture: the config must state it.
                None => match configured {
                    Some(CsLogic::ActiveHigh) => true,
                    Some(CsLogic::ActiveLow) => false,
                    Some(CsLogic::Ignore) => unreachable!("filtered above"),
                    None => panic!(
                        "Chip {} has configurable CS line '{}' but no polarity \
                         is specified in the config — add cs1/cs2/cs3/cs4 field",
                        chip_type.name(),
                        spec.name,
                    ),
                },
            };

            control_lines.push(ControlLine {
                name: spec.name,
                gpios,
                assert_high,
                commoned: false,
            });
        }

        // Oversized ROMs: chips whose full address space exceeds MAX_IMAGE_SIZE
        // (e.g. 27C080 at 1MB = 2 × MAX_IMAGE_SIZE) have their top address
        // line(s) repurposed as half-select CS pins by the gen/firmware. The
        // gen carves them off into excess_addr_pin_gpios and folds them into
        // the CS range; the tester must do the same so it drives them at the
        // right level rather than leaving them at 0 as part of the address.
        //
        // `num_excess_addr_lines` is gen's own answer, shared rather than
        // recomputed here: for 27C080 it is 1 (just A19).
        //
        // The polarity comes from cs1_logic: active_high means the half-select
        // pin must be HIGH for the chip to respond (cs1=active_high serves the
        // upper half); active_low means it must be LOW (lower half).
        let num_excess = num_excess_addr_lines(&chip_type);
        if num_excess > 0 {
            let assert_high = match chip_config.cs1 {
                Some(CsLogic::ActiveHigh) => true,
                Some(CsLogic::ActiveLow) => false,
                other => panic!(
                    "Oversized ROM chip {} ({}B > {}B MAX_IMAGE_SIZE) requires \
                     cs1 active_low or active_high for half-select, got {:?}",
                    chip_type.name(),
                    chip_type.size_bytes(),
                    MAX_IMAGE_SIZE,
                    other,
                ),
            };
            // Drain the top num_excess entries from addr_gpios — these are the
            // highest address lines (e.g. A19 for 27C080), which the firmware
            // treats as CS pins, not address pins.
            let split = addr_gpios.len().saturating_sub(num_excess);
            for gpios in addr_gpios.drain(split..) {
                control_lines.push(ControlLine {
                    name: "cs1",
                    gpios,
                    assert_high,
                    commoned: false,
                });
            }
        }

        Self {
            addr_gpios,
            data_gpios,
            control_lines,
            byte_n_gpio,
        }
    }

    /// Build a `PinCache` for a secondary chip in a multi-ROM set.
    ///
    /// Secondary chips (chips[1], chips[2], …) are not in One ROM's socket.
    /// They share the address and data bus with the primary chip (chips[0])
    /// and are selected by an X pin acting as their unique CS line.
    ///
    /// Address GPIOs are derived from the secondary chip's own pin definitions
    /// via the board's socket_pin_map.  Because the shared bus means that
    /// socket pin N on One ROM's board carries the same signal as socket pin N
    /// on the secondary chip's socket, the same map applies correctly — and
    /// naturally handles chips with different address-line counts (e.g. a 2332
    /// with 12 address lines alongside a 2364 with 13).
    ///
    /// Data GPIOs are always taken from the primary cache; the data bus is
    /// fully shared and its GPIO mapping does not vary by chip type.
    ///
    /// Fixed CE/OE lines on the secondary chip are handled by the target PCB
    /// (typically tied permanently active) and are not driven by the tester.
    /// Only configurable CS lines can serve as the unique X pin selector; chips
    /// with only fixed CS lines are not supported as secondary chips.
    ///
    /// 27C400/27C200-family chips are not supported as secondary chips in
    /// multi-ROM sets; `byte_n_gpio` is always `None`.
    ///
    /// # Panics
    /// Panics if any required address pin is absent from the board's socket pin
    /// map.
    pub fn build_secondary(
        chip_type: ChipType,
        primary: &PinCache,
        board: Board,
        x_pin_gpios: Vec<u8>,
        x_assert_high: bool,
    ) -> Self {
        let offset = socket_offset(chip_type, board);
        let pin_map = board.socket_pin_map();

        let addr_gpios: Vec<Vec<u8>> = chip_type
            .address_pins()
            .iter()
            .map(|&pin| gpios_for(pin + offset, pin_map, chip_type, "address"))
            .collect();

        Self {
            addr_gpios,
            data_gpios: primary.data_gpios.clone(),
            control_lines: vec![ControlLine {
                name: "x_cs",
                gpios: x_pin_gpios,
                assert_high: x_assert_high,
                commoned: false,
            }],
            byte_n_gpio: None,
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Socket-pin offset to add to chip physical pin numbers.
///
/// A 24-pin chip in a 28-pin board socket is mechanically shifted by 2
/// positions (chip pin 1 aligns with socket pin 3, because pin 12 of a
/// 24-pin chip occupies the same physical slot as pin 14 of the 28-pin
/// socket, both being ground).  All other board/chip combinations are 1:1.
fn socket_offset(chip_type: ChipType, board: Board) -> u8 {
    if chip_type.chip_pins() == 24 && board.chip_pins() == 28 {
        2
    } else {
        0
    }
}

/// Return all GPIOs mapped to `socket_pin` in the board's socket pin map.
///
/// # Panics
/// Panics if the pin is absent — indicates a chip/board mismatch or an
/// incomplete board definition.
fn gpios_for(socket_pin: u8, map: &[(u8, &[u8])], chip_type: ChipType, role: &str) -> Vec<u8> {
    map.iter()
        .find(|(p, _)| *p == socket_pin)
        .map(|(_, gpios)| gpios.to_vec())
        .unwrap_or_else(|| {
            panic!(
                "socket pin {} ({} pin, chip {}) not found in socket_pin_map \
                 — check board/chip combination",
                socket_pin,
                role,
                chip_type.name(),
            )
        })
}