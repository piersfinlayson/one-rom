// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Cached ROM-socket-pin → MCU-GPIO mappings for one chip on one board.
//!
//! `PinCache::build` does the O(pins) mapping work once so the hot test loop
//! can operate purely on pre-resolved GPIO numbers.

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::Board;
use onerom_gen::{ChipConfig, CsLogic};

// ── Public types ──────────────────────────────────────────────────────────────

/// A single decoded control line with its assertion polarity baked in.
pub struct ControlLine {
    /// Name for diagnostics ("ce", "oe", "cs1", "cs2", "cs3").
    // Not read in the hot path; retained for future diagnostic/tristate use.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Every MCU GPIO driven by this physical pin.
    /// Usually one; some boards (e.g. Fire32B fly-leads) wire one socket pin
    /// to two GPIOs and both must be driven.
    pub gpios: Vec<u8>,
    /// `true` → assert by driving HIGH; `false` → assert by driving LOW.
    pub assert_high: bool,
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

        let addr_gpios = chip_type
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

            if spec.name == "write" {
                // Not a select line; excluded from CS detection and bus tristate checks.
                continue;
            }

            let assert_high = match spec.line_type {
                ControlLineType::FixedActiveLow => false,
                ControlLineType::Configurable => {
                    let logic = match spec.name {
                        "cs1" => chip_config.cs1,
                        "cs2" => chip_config.cs2,
                        "cs3" => chip_config.cs3,
                        other => panic!(
                            "Unrecognised configurable control line '{}' on chip {}",
                            other,
                            chip_type.name()
                        ),
                    };
                    match logic {
                        Some(CsLogic::ActiveHigh) => true,
                        Some(CsLogic::ActiveLow) => false,
                        // Ignore: permanently tied active on the board.
                        // The tester does not drive it.
                        Some(CsLogic::Ignore) => continue,
                        None => panic!(
                            "Chip {} has configurable CS line '{}' but no polarity \
                             is specified in the config — add cs1/cs2/cs3 field",
                            chip_type.name(),
                            spec.name,
                        ),
                    }
                }
            };

            control_lines.push(ControlLine { name: spec.name, gpios, assert_high });
        }

        Self { addr_gpios, data_gpios, control_lines, byte_n_gpio }
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
fn gpios_for(
    socket_pin: u8,
    map: &[(u8, &[u8])],
    chip_type: ChipType,
    role: &str,
) -> Vec<u8> {
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