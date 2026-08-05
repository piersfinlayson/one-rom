// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Chip-board compatibility checking for v2 (Fire/RP2350) boards.
//!
//! [`check_chip_set_on_board`] runs the full v2 address and CS/data layout
//! derivation for a (board, chip_type, set shape) triple and returns a
//! [`CompatResult`] describing the ROM table parameters, or `None` if the
//! combination is not supportable.
//!
//! Used by the `compat` binary to generate the compatibility matrix and
//! per-board chip tables, and by the CLI's `chips` command - which share
//! [`supported_chips`], [`format_size`] and [`CompatResult::fit_description`]
//! so the tool and the document cannot disagree.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use onerom_config::chip::{CHIP_TYPE_NAMES, ChipType};
use onerom_config::hw::Board;
#[cfg(test)]
use onerom_config::hw::Model;
use onerom_metadata::BitModes;

use crate::image::{ChipSetType, CsConfig, CsLogic};
use crate::v2::addr_layout::{LayoutError, derive_addr_layout};
use crate::v2::alg_config::bit_mode_for;
use crate::v2::alg_preference::{
    AddrAlgPreference, CsAlgPreference, DataAlgPreference, cs_alg_preference,
};
use crate::v2::cs_data_layout::derive_cs_data_layout;
use crate::v2::multi_cs_config::derive_multi_cs_config;
use crate::v2::slot_context::{SlotContext, socket_pin_offset};

/// ROM table parameters for a supported chip-board combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompatResult {
    /// Address bits in the ROM table index. The table has `2^num_addr_pins`
    /// entries and `slot_size_bytes` bytes total.
    pub num_addr_pins: u8,

    /// ROM table size in bytes: `2^num_addr_pins * bytes_per_word`
    /// (bytes_per_word is 1 for BitMode8, 2 for BitMode16).
    pub slot_size_bytes: u32,

    /// Socket-pin translation offset: 0 for native (chip pins == board socket
    /// pins), positive for smaller chip in larger socket (One ROM overhangs
    /// the target socket), negative for larger chip in smaller socket
    /// (fly-leads required from the chip socket's address pins to One ROM's
    /// X1/X2 header pins).
    pub pin_offset: i16,

    /// Number of fly-lead connections required. 0 for native and overhang
    /// combinations; 1 for a single fly-lead to X1, 2 for fly-leads to both
    /// X1 and X2.
    pub num_fly_lead_pins: u8,

    /// The table-index bits that carry nothing: one bit set per GPIO inside
    /// the address window `[gpio_base, gpio_base + num_addr_pins)` that is
    /// neither one of the chip's address lines nor, for a banked set, X1/X2.
    ///
    /// Each such GPIO doubles the ROM table without addressing any more of
    /// the chip, so [`CompatResult::excess_addr_bits`] is exactly the power
    /// of two by which `slot_size_bytes` exceeds the smallest table that
    /// could serve this slot. Note that floor is `2^(address lines)`, not the
    /// chip's capacity: a 23QL384 holds 48KB but has 16 address lines, so
    /// 64KB is the least any board can serve it from. A GPIO lands here
    /// either because the board maps some unrelated signal into the middle of
    /// the chip's address range, or because the range was widened to
    /// `MIN_ADDR_PINS`.
    ///
    /// Absolute GPIO numbers, so bit `n` is GPIO `n`; the RP2350B's 48 GPIOs
    /// all fit. Resolve a bit to the pin responsible with
    /// [`Board::socket_pin_for_gpio`] / [`Board::x_pin_for_gpio`].
    pub hole_gpios: u64,
}

impl CompatResult {
    /// How many table-index bits address nothing — the population count of
    /// [`CompatResult::hole_gpios`].
    ///
    /// `slot_size_bytes` is `2^excess_addr_bits` times the flash this chip
    /// set would occupy on a board that placed its address lines
    /// contiguously, so `0` means no board layout could serve this slot from
    /// less flash and there is nothing left to win.
    pub fn excess_addr_bits(&self) -> u32 {
        self.hole_gpios.count_ones()
    }

    /// The GPIOs of [`CompatResult::hole_gpios`], ascending.
    pub fn hole_gpio_list(&self) -> Vec<u8> {
        (0..u64::BITS)
            .filter(|bit| self.hole_gpios & (1u64 << bit) != 0)
            .map(|bit| bit as u8)
            .collect()
    }

    /// True if the chip and board socket are the same size — no adapter or
    /// fly-leads needed.
    pub fn is_native(&self) -> bool {
        self.pin_offset == 0
    }

    /// True if the chip has fewer pins than the board socket and One ROM
    /// overhangs the target socket when installed.
    pub fn is_overhang(&self) -> bool {
        self.pin_offset > 0
    }

    /// True if the chip has more pins than the board socket and fly-leads are
    /// required from the chip socket's address pin(s) to One ROM's X1 (and
    /// optionally X2) header pins.
    pub fn requires_fly_leads(&self) -> bool {
        self.pin_offset < 0
    }

    /// How the chip sits in the board's socket, as a short human-readable
    /// phrase: `native`, `overhang`, `larger socket (no fly-leads)`, or
    /// `fly-lead to X1[ and X2]`.
    ///
    /// The `larger socket (no fly-leads)` case is a chip with more pins than
    /// the board whose extra pins carry no address lines: One ROM sits
    /// bottom-justified in the larger socket and nothing needs wiring. It is
    /// spelled out rather than left as a bare "no fly-leads" because these rows
    /// sit under a "(with fly-leads)" heading, which on its own reads as a
    /// contradiction. It still does not simply drop in - the socket's VCC is
    /// among the pins One ROM cannot reach, so power must be rerouted, as for
    /// any cross-size fit.
    ///
    /// Used by the `compat` binary for `docs/COMPATIBILITY.md`'s per-board
    /// tables and by the CLI's `chips` command, so the two agree.
    pub fn fit_description(&self) -> String {
        if self.is_native() {
            "native".to_string()
        } else if self.requires_fly_leads() {
            match self.num_fly_lead_pins {
                0 => "larger socket (no fly-leads)".to_string(),
                1 => "fly-lead to X1".to_string(),
                2 => "fly-lead to X1 and X2".to_string(),
                n => alloc::format!("fly-lead ({n} pins)"),
            }
        } else {
            "overhang".to_string()
        }
    }
}

/// Render a ROM or image size the way `docs/COMPATIBILITY.md` and the CLI do:
/// whole `MB`/`KB` units where the value divides exactly, `B` below 1KB.
///
/// Every size this is applied to is a power of two, so the truncating division
/// is exact; it is not a general-purpose byte formatter.
pub fn format_size(bytes: u32) -> String {
    if bytes >= 1024 * 1024 {
        alloc::format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        alloc::format!("{}KB", bytes / 1024)
    } else {
        alloc::format!("{bytes}B")
    }
}

/// Returns true if `chip_type` is supported in v2 firmware at all, regardless
/// of board. False for plugins and chips not in `SUPPORTED_CHIP_TYPES_V2`.
pub fn is_v2_chip(chip_type: ChipType) -> bool {
    !chip_type.is_plugin() && crate::SUPPORTED_CHIP_TYPES_V2.contains(&chip_type)
}

/// The CS configuration a user gets for `chip_type` without opting out of
/// anything: every control line the chip has is monitored.
///
/// Which lines One ROM monitors changes the layout, because
/// `derive_cs_data_layout` only pulls a line into the select range when it is
/// configured active. Setting one to `Ignore` drops it out, which derives on
/// boards the full configuration cannot reach — but `check_cs_v2` requires
/// `allow_cs_ignore` to do that ("Misuse can cause bus contention"), and the
/// CLI's `--slot` will not accept `ignore` at all. So this is the
/// configuration compatibility should be reported against; anything looser
/// describes hardware a user cannot ask for.
///
/// Polarity is left to `CsConfig::from_chip_type`, which takes fixed lines
/// from the silicon. `ActiveLow` for the configurable ones is arbitrary and
/// harmless: polarity places a line on the same GPIO either way and is
/// resolved later by `cs_overrides`.
pub fn default_cs_config(chip_type: ChipType) -> CsConfig {
    let logic = |name: &str| {
        chip_type
            .control_lines()
            .iter()
            .any(|l| l.name == name)
            .then_some(CsLogic::ActiveLow)
    };

    CsConfig::from_chip_type(
        &chip_type,
        logic("cs1"),
        logic("cs2"),
        logic("cs3"),
        logic("cs4"),
        logic("ce"),
        logic("oe"),
    )
}

/// How a chip is served on a board: the algorithms chosen, and the GPIO window
/// the address state machine samples.
///
/// The window is the interesting part.  The address machine samples its pins
/// free-running, ungated by chip select, so a control line inside the window is
/// part of the SRAM index the DMA reads: asserting it changes which entry is
/// fetched, and the fetch already in flight has to be discarded and redone.  A
/// control line outside it only gates the data output drivers.  The two cost
/// very different numbers of cycles, and which case applies depends on the
/// board's routing as much as on the chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServingAlgInfo {
    /// First GPIO the address state machine samples.
    pub addr_window_base: u8,
    /// How many consecutive GPIOs it samples from `addr_window_base`.
    pub addr_window_pins: u8,
    pub cs_alg: CsAlgPreference,
    pub addr_alg: AddrAlgPreference,
    pub data_alg: DataAlgPreference,
}

impl ServingAlgInfo {
    /// Whether `gpio` is inside the sampled address window, and so forms part
    /// of the SRAM index.
    pub fn samples_gpio(&self, gpio: u8) -> bool {
        gpio >= self.addr_window_base && (gpio - self.addr_window_base) < self.addr_window_pins
    }
}

/// Derive [`ServingAlgInfo`] for a chip set on a board, from configuration
/// alone.
///
/// Deliberately independent of any built firmware: a test that wants to know
/// what the device *should* be doing needs an answer that does not come from
/// the device, or a firmware that programmed the wrong window would simply
/// move the expectation along with it.
///
/// `cs_config` names which control lines One ROM monitors on chip 0 — see
/// [`check_chip_set_on_board`], whose inputs these mirror.
///
/// `secondary_cs_config` is the first secondary chip's own configuration, for
/// Multi sets: which of its lines is the per-chip select decides how chip 0's
/// remaining lines split into commoned and ignored, and so which GPIOs end up
/// in the select range.  Pass `None` for Single and Banked sets, or to fall
/// back to `multi_secondary_config` when the real configuration is not to
/// hand — but note that a set whose secondaries differ from that shape may then
/// fail to derive even though it builds.
pub fn serving_alg_info(
    board: Board,
    chip_type: ChipType,
    set_type: ChipSetType,
    num_chips: usize,
    cs_config: CsConfig,
    secondary_cs_config: Option<CsConfig>,
    force_16_bit: bool,
) -> Result<ServingAlgInfo, crate::Error> {
    let pin_offset = socket_pin_offset(chip_type.chip_pins(), board.chip_pins())
        .ok_or(crate::Error::UnsupportedBoardChipType { board, chip_type })?;
    let bit_mode = bit_mode_for(chip_type, board);

    let multi_cs_config = match set_type {
        ChipSetType::Multi => Some(derive_multi_cs_config(
            chip_type,
            &cs_config,
            &secondary_cs_config.unwrap_or_else(|| multi_secondary_config(chip_type)),
        )),
        ChipSetType::Single | ChipSetType::Banked => None,
    };

    let ctx = SlotContext {
        board,
        set_type,
        chip_types: alloc::vec![chip_type; num_chips],
        cs_config,
        bit_mode,
        pin_offset,
        force_16_bit,
        multi_cs_config,
    };

    let addr_layout = derive_addr_layout(&ctx)?;
    let cs_data_layout = derive_cs_data_layout(&ctx, Some(&addr_layout))?;

    // `base_addr_pin` is an offset within the PIO's GPIOBASE window, so the
    // absolute first GPIO sampled is the layout's own gpio_base.
    Ok(ServingAlgInfo {
        addr_window_base: addr_layout.gpio_base,
        addr_window_pins: addr_layout.num_addr_pins,
        cs_alg: cs_alg_preference(
            cs_data_layout.cs_ignore_index,
            cs_data_layout.alg_cs2.as_ref(),
        ),
        addr_alg: AddrAlgPreference::AlgAddr0,
        data_alg: match (bit_mode, force_16_bit) {
            (BitModes::BitMode16, false) => DataAlgPreference::AlgData1,
            _ => DataAlgPreference::AlgData0,
        },
    })
}

/// The CS configuration a secondary chip of a multi set necessarily has.
///
/// A secondary reaches One ROM through a single fly-leaded select on an X
/// pin, so exactly one of its control lines is monitored and the rest are
/// `Ignore` — which `check_cs_v2` permits unconditionally for secondaries,
/// and which `derive_multi_cs_config` reads to identify the per-chip select
/// before using chip0's own configuration to split the remainder into
/// commoned and truly-ignored. This is not a guess at the user's config: it
/// is the only shape a secondary can take.
///
/// For a CE/OE chip that means [`CsConfig::CeOeExplicit`] with CE as the
/// select and OE `Ignore`d — the variant exists for exactly this. Leaving
/// both active (plain `CsConfig::CeOe`, where `control_line_logic` reports
/// both as `ActiveLow`) would present two active lines and make
/// `derive_multi_cs_config` pick whichever came last as the select.
fn multi_secondary_config(chip_type: ChipType) -> CsConfig {
    let has = |name: &str| chip_type.control_lines().iter().any(|l| l.name == name);

    if has("ce") && has("oe") {
        CsConfig::CeOeExplicit {
            ce: CsLogic::ActiveLow,
            oe: CsLogic::Ignore,
        }
    } else {
        default_cs_config(chip_type)
    }
}

/// The largest multi set the firmware can serve: the chip in the socket plus
/// one secondary per X pin, and there are only ever X1 and X2.
///
/// Mirrors `builder::check_config`'s `TooManyChips` limit, so a shape the
/// builder would reject is not reported here as servable.
const MAX_MULTI_CHIPS: usize = 3;

/// Check if a slot of `num_chips` × `chip_type` can be served on `board`.
///
/// Returns `Some(CompatResult)` if both the address-layout and CS/data-layout
/// derivation succeed. Returns `None` if:
/// - The chip is a plugin or not in `SUPPORTED_CHIP_TYPES_V2`.
/// - `socket_pin_offset` returns `None` (pin counts not a supported pair).
/// - `derive_addr_layout` fails — e.g. the GPIO span for the chip's address
///   lines does not fit any PIO window, or an overhanging address pin cannot
///   be assigned to an X pin.
/// - `derive_cs_data_layout` fails — e.g. the chip's select lines do not land
///   on contiguous GPIOs in this configuration.
/// - The resulting ROM table would exceed `MAX_IMAGE_SIZE`, which
///   `build_rom_slot` rejects. Reachable for banked sets of large chips.
/// - `num_chips` does not match `set_type`: `Single` takes exactly 1, `Banked`
///   2 or more, `Multi` between 2 and `MAX_MULTI_CHIPS`; or the board does
///   not support that set type.
///
/// Native, overhang (smaller chip in larger socket), and fly-lead (larger
/// chip in smaller socket) combinations are all evaluated where
/// `socket_pin_offset` permits. For fly-lead results, `num_fly_lead_pins`
/// indicates how many connections from the chip socket's address pins to One
/// ROM's X1/X2 header pins are required.
///
/// `cs_config` names which control lines One ROM monitors, and is a genuine
/// input rather than a detail: a line configured `Ignore` is left out of the
/// select range entirely, so the same chip can derive on a board in one
/// configuration and be refused in another. Use [`default_cs_config`] for the
/// configuration a user gets by default. Polarity within that (`ActiveLow`
/// vs `ActiveHigh`) does not affect GPIO placement.
pub fn check_chip_set_on_board(
    board: Board,
    chip_type: ChipType,
    set_type: ChipSetType,
    num_chips: usize,
    cs_config: CsConfig,
) -> Result<CompatResult, crate::Error> {
    if !is_v2_chip(chip_type) {
        return Err(crate::Error::UnsupportedBoardChipType { board, chip_type });
    }

    match set_type {
        ChipSetType::Single if num_chips == 1 => {}
        ChipSetType::Banked if num_chips >= 2 && board.supports_banked_roms() => {}
        ChipSetType::Multi
            if (2..=MAX_MULTI_CHIPS).contains(&num_chips) && board.supports_multi_chip_sets() => {}
        // Anything else is not a slot shape.
        ChipSetType::Single | ChipSetType::Banked | ChipSetType::Multi => {
            return Err(crate::Error::UnsupportedBoardConfig {
                board,
                reason: alloc::format!("board cannot serve a {num_chips}-chip {set_type:?} set"),
            });
        }
    }

    let pin_offset = socket_pin_offset(chip_type.chip_pins(), board.chip_pins())
        .ok_or(crate::Error::UnsupportedBoardChipType { board, chip_type })?;
    let bit_mode = bit_mode_for(chip_type, board);

    let multi_cs_config = match set_type {
        ChipSetType::Multi => Some(derive_multi_cs_config(
            chip_type,
            &cs_config,
            &multi_secondary_config(chip_type),
        )),
        ChipSetType::Single | ChipSetType::Banked => None,
    };

    let ctx = SlotContext {
        board,
        set_type,
        chip_types: alloc::vec![chip_type; num_chips],
        cs_config,
        bit_mode,
        pin_offset,
        force_16_bit: false,
        multi_cs_config,
    };

    let addr_layout = derive_addr_layout(&ctx)?;
    derive_cs_data_layout(&ctx, Some(&addr_layout))?;

    let bytes_per_word: u32 = if matches!(bit_mode, BitModes::BitMode16) {
        2
    } else {
        1
    };

    // Count overhanging address pins that required fly-leads. Mirrors the
    // logic in derive_addr_layout so the count matches what was actually
    // wired to X pins during layout derivation.
    let num_fly_lead_pins = if pin_offset < 0 {
        let addr_line_start = if matches!(bit_mode, BitModes::BitMode16) {
            1
        } else {
            0
        };
        chip_type.address_pins()[addr_line_start..]
            .iter()
            .filter(|&&ap| {
                let sp = ap as i16 + pin_offset;
                sp < 1 || sp > board.chip_pins() as i16
            })
            .count() as u8
    } else {
        0
    };

    let slot_size_bytes = (1u32 << addr_layout.num_addr_pins) * bytes_per_word;

    // build_rom_slot rejects a table over MAX_IMAGE_SIZE, so a combination
    // that produces one is not servable however the layout derives. Reported
    // through the same LayoutError build_rom_slot would raise, so the message
    // a caller shows is the one the builder would have shown.
    if slot_size_bytes as usize > crate::MAX_IMAGE_SIZE {
        return Err(LayoutError::RomTableTooLarge {
            board,
            chip_type,
            set_type,
            num_chips,
            num_addr_pins: addr_layout.num_addr_pins,
            table_size: slot_size_bytes as usize,
        }
        .into());
    }

    // The window's table-index bits that carry no address line and no bank
    // select. Excess address pins are excluded by construction: they sit
    // outside the window, acting as CS half-selects rather than table bits.
    let live: u64 = addr_layout
        .addr_pin_gpios
        .iter()
        .chain(addr_layout.x1_gpio.iter())
        .chain(addr_layout.x2_gpio.iter())
        .fold(0u64, |acc, &gpio| acc | (1u64 << gpio));

    let window_end = addr_layout.gpio_base + addr_layout.num_addr_pins;
    let hole_gpios = (addr_layout.gpio_base..window_end)
        .filter(|gpio| live & (1u64 << gpio) == 0)
        .fold(0u64, |acc, gpio| acc | (1u64 << gpio));

    Ok(CompatResult {
        num_addr_pins: addr_layout.num_addr_pins,
        slot_size_bytes,
        pin_offset,
        num_fly_lead_pins,
        hole_gpios,
    })
}

/// One chip type a board can emulate, as listed by [`supported_chips`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChipCompat {
    /// The chip type.
    pub chip_type: ChipType,

    /// The name this entry was listed under. Chip types with several accepted
    /// spellings appear once per alias (e.g. `2316` and `9316A`), since a user
    /// looking up the part number stamped on their chip needs to find it.
    pub alias: &'static str,

    /// The chip's own storage capacity, which may be smaller than the flash
    /// the image occupies ([`CompatResult::slot_size_bytes`]).
    pub rom_size_bytes: u32,

    /// How the chip fits this board, and how much flash its image uses, in
    /// the configuration [`default_cs_config`] describes.
    pub result: CompatResult,
}

/// Sort key ordering fit classes: native, then overhang, then fly-lead.
pub fn pin_offset_order(pin_offset: i16) -> i32 {
    match pin_offset {
        0 => 0,
        n if n > 0 => 1,
        _ => 2,
    }
}

/// Every chip type `board` can emulate in a slot of `num_chips` × that chip,
/// with the flash each one's image uses.
///
/// Ordered as `docs/COMPATIBILITY.md` presents it - native fits first, then
/// overhang, then fly-lead; within a class by how far the chip's pin count is
/// from the board's, then ascending ROM size, then name - so a caller can group
/// consecutive runs of equal `result.pin_offset` into that document's sections.
///
/// Pass `(ChipSetType::Single, 1)` for chips served alone. A banked set draws
/// X1/X2 into the slot's address window, which can make its table more than
/// `num_chips` times the single-chip figure, and can put the set beyond
/// `MAX_IMAGE_SIZE` entirely - such entries are absent rather than listed.
///
/// Chips are checked in the configuration [`default_cs_config`] describes -
/// every control line monitored. A chip that derives only with a line set to
/// `Ignore` is absent, because reaching that needs `allow_cs_ignore`, and the
/// CLI cannot express it at all.
///
/// [`ChipSetType::Multi`] lists a homogeneous set - `num_chips` of the same
/// chip type. A heterogeneous set (a C64's 2364 + 2332 + 2364) is the
/// builder's business: its layout turns on each member's own configuration.
pub fn supported_chips(board: Board, set_type: ChipSetType, num_chips: usize) -> Vec<ChipCompat> {
    let mut entries: Vec<ChipCompat> = CHIP_TYPE_NAMES
        .iter()
        .filter_map(|alias| {
            let chip_type = ChipType::try_from_str(alias)?;
            let cs_config = default_cs_config(chip_type);
            let result =
                check_chip_set_on_board(board, chip_type, set_type, num_chips, cs_config).ok()?;
            Some(ChipCompat {
                chip_type,
                alias,
                rom_size_bytes: chip_type.size_bytes() as u32,
                result,
            })
        })
        .collect();

    entries.sort_by_key(|e| {
        (
            pin_offset_order(e.result.pin_offset),
            e.result.pin_offset.abs(),
            e.rom_size_bytes,
            e.alias,
        )
    });

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(board: Board, alias: &str) -> ChipCompat {
        *supported_chips(board, ChipSetType::Single, 1)
            .iter()
            .find(|e| e.alias == alias)
            .unwrap_or_else(|| panic!("{alias} should be listed for {}", board.name()))
    }

    #[test]
    fn format_size_picks_whole_units() {
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1KB");
        assert_eq!(format_size(48 * 1024), "48KB");
        assert_eq!(format_size(1024 * 1024), "1MB");
    }

    /// The image sizes and fits `supported_chips` reports are what
    /// `docs/COMPATIBILITY.md` publishes, since the document is generated from
    /// it. Spot-check one entry of each fit class, including the two cases the
    /// figure exists to expose: a chip whose image is far larger than the chip
    /// (2364 overhanging a 28-pin board, 8KB served from a 256KB table) and one
    /// where the two match.
    #[test]
    fn reports_the_documented_image_sizes() {
        let native = find(Board::Fire24F, "2364");
        assert_eq!(native.rom_size_bytes, 8 * 1024);
        assert_eq!(native.result.slot_size_bytes, 8 * 1024);
        assert_eq!(native.result.fit_description(), "native");

        let overhang = find(Board::Fire28C, "2364");
        assert_eq!(overhang.rom_size_bytes, 8 * 1024);
        assert_eq!(overhang.result.slot_size_bytes, 256 * 1024);
        assert_eq!(overhang.result.fit_description(), "overhang");

        let fly_lead = find(Board::Fire24F, "2764");
        assert_eq!(fly_lead.rom_size_bytes, 8 * 1024);
        assert_eq!(fly_lead.result.slot_size_bytes, 32 * 1024);
        assert_eq!(fly_lead.result.fit_description(), "fly-lead to X1");
    }

    /// A chip with more pins than the board, but no address line among the
    /// extra ones, needs no fly-leads - One ROM just sits bottom-justified in
    /// the larger socket. The 32-pin 28C512 on a 28-pin board is the case.
    /// It is still a cross-size fit, not a native one.
    #[test]
    fn larger_socket_without_fly_leads_says_so() {
        let entry = find(Board::Fire28C, "28C512");
        assert!(entry.result.requires_fly_leads());
        assert_eq!(entry.result.num_fly_lead_pins, 0);
        assert!(!entry.result.is_native());
        assert_eq!(
            entry.result.fit_description(),
            "larger socket (no fly-leads)"
        );
    }

    /// Callers group consecutive runs of equal `pin_offset` into the document's
    /// sections, which only works if the entries are ordered by fit class - so
    /// each class must appear exactly once in the listing.
    #[test]
    fn orders_by_fit_class_without_interleaving() {
        for board in [Board::Fire24F, Board::Fire28C, Board::Fire32B] {
            let entries = supported_chips(board, ChipSetType::Single, 1);
            assert!(!entries.is_empty(), "{} lists no chips", board.name());

            let classes: Vec<i32> = entries
                .iter()
                .map(|e| pin_offset_order(e.result.pin_offset))
                .collect();
            assert!(
                classes.windows(2).all(|w| w[0] <= w[1]),
                "{} entries are not ordered by fit class: {classes:?}",
                board.name()
            );

            let mut offsets: Vec<i16> = entries.iter().map(|e| e.result.pin_offset).collect();
            offsets.dedup();
            let unique = offsets.len();
            offsets.sort_unstable();
            offsets.dedup();
            assert_eq!(
                unique,
                offsets.len(),
                "{} has a pin offset split across sections",
                board.name()
            );
        }
    }

    /// Every alias of a chip type is listed, so a user can look up the part
    /// number stamped on the chip rather than One ROM's preferred name for it.
    #[test]
    fn lists_each_alias_separately() {
        let entries = supported_chips(Board::Fire24F, ChipSetType::Single, 1);
        for alias in ["2316", "9316", "9316A"] {
            assert!(
                entries.iter().any(|e| e.alias == alias),
                "{alias} missing from the fire-24-f listing"
            );
        }
    }

    /// A chip the board cannot serve has no size to report.
    #[test]
    fn omits_unsupported_chips() {
        let entries = supported_chips(Board::Fire24F, ChipSetType::Single, 1);
        assert!(entries.iter().all(|e| e.alias != "27C400"));
        assert!(
            check_chip_set_on_board(
                Board::Fire24F,
                ChipType::Chip27C400,
                ChipSetType::Single,
                1,
                default_cs_config(ChipType::Chip27C400)
            )
            .is_err()
        );
    }

    fn single(board: Board, chip_type: ChipType) -> CompatResult {
        check_chip_set_on_board(
            board,
            chip_type,
            ChipSetType::Single,
            1,
            default_cs_config(chip_type),
        )
        .expect("chip should be servable on this board")
    }

    /// The wasted table-index bits name the GPIO, and so the socket pin,
    /// costing the flash.
    ///
    /// Fire28D's 23128 is the worked case: 14 address lines spanning GPIO
    /// 13..=27, with socket pin 1 (A15, unused by a 23128) mapped to GPIO 18
    /// in the middle of them. That one GPIO widens the window to 15 bits and
    /// doubles the image to 32KB for a 16KB chip.
    #[test]
    fn wasted_bits_name_the_pin_responsible() {
        let result = single(Board::Fire28D, ChipType::Chip23128);

        assert_eq!(result.slot_size_bytes, 32 * 1024);
        assert_eq!(result.excess_addr_bits(), 1);
        assert_eq!(result.hole_gpio_list(), alloc::vec![18]);
        assert_eq!(Board::Fire28D.socket_pin_for_gpio(18), Some(1));
    }

    /// A chip whose address lines the board places contiguously wastes
    /// nothing, and says so with an empty hole set rather than an absent one.
    /// Fire28D's 27512 uses all 16 GPIOs of its window.
    #[test]
    fn a_chip_at_its_floor_has_no_holes() {
        let result = single(Board::Fire28D, ChipType::Chip27512);

        assert_eq!(result.slot_size_bytes, 64 * 1024);
        assert_eq!(result.excess_addr_bits(), 0);
        assert_eq!(result.hole_gpios, 0);
        assert!(result.hole_gpio_list().is_empty());
    }

    /// Excess bits and image size agree by construction: the image is
    /// `2^excess` times the smallest table the chip's address lines could be
    /// served from, on every board and chip the generator supports.
    #[test]
    fn excess_bits_account_for_the_whole_image() {
        for board in Model::Fire.boards().iter().filter(|b| b.mcu_pio()) {
            for entry in supported_chips(*board, ChipSetType::Single, 1) {
                let floor = entry.result.slot_size_bytes >> entry.result.excess_addr_bits();
                assert_eq!(
                    floor << entry.result.excess_addr_bits(),
                    entry.result.slot_size_bytes,
                    "{} {}: {} excess bits does not account for a {}B image",
                    board.name(),
                    entry.alias,
                    entry.result.excess_addr_bits(),
                    entry.result.slot_size_bytes,
                );
                // The floor is never below the chip's own size, except for
                // an oversized ROM (27C080 at 1MB): its top address lines are
                // carved out as CS half-selects, so the table holds half the
                // chip and MAX_IMAGE_SIZE is the real bound.
                let bound = entry.rom_size_bytes.min(crate::MAX_IMAGE_SIZE as u32);
                assert!(
                    floor >= bound,
                    "{} {}: floor {floor}B is below the {bound}B this chip needs",
                    board.name(),
                    entry.alias,
                );
            }
        }
    }

    /// A banked set is a different layout problem from the same chip alone:
    /// it draws X1/X2 into the address window. On Fire24F a single 2364 is
    /// served from an 8KB table with nothing wasted, but a banked pair has to
    /// reach X1 at GPIO 9 - below the whole address block - so the window
    /// widens past every address line in between.
    #[test]
    fn a_banked_set_is_measured_separately_from_the_single() {
        let alone = single(Board::Fire24F, ChipType::Chip2364);
        assert_eq!(alone.slot_size_bytes, 8 * 1024);
        assert_eq!(alone.excess_addr_bits(), 0);

        let banked = check_chip_set_on_board(
            Board::Fire24F,
            ChipType::Chip2364,
            ChipSetType::Banked,
            2,
            default_cs_config(ChipType::Chip2364),
        )
        .expect("Fire24F should serve a banked pair of 2364s");

        assert_eq!(banked.slot_size_bytes, 32 * 1024);
        assert_eq!(banked.excess_addr_bits(), 1);
    }

    /// A set whose table would exceed `MAX_IMAGE_SIZE` is not servable, and
    /// is reported as such rather than as an oversized image `build_rom_slot`
    /// would go on to reject. Fire28D serves a banked pair of 23QL384s from a
    /// 512KB table - exactly the limit - so four of them cannot fit.
    #[test]
    fn a_set_beyond_the_image_limit_is_unsupported() {
        for (num_chips, servable) in [(2, true), (4, false)] {
            let result = check_chip_set_on_board(
                Board::Fire28D,
                ChipType::Chip23QL384,
                ChipSetType::Banked,
                num_chips,
                default_cs_config(ChipType::Chip23QL384),
            );
            assert_eq!(
                result.is_ok(),
                servable,
                "banked x{num_chips} of 23QL384 on Fire28D"
            );
        }
    }

    /// A (set type, chip count) pairing that is not a slot shape is declined
    /// rather than derived from nonsense - a single set holds one chip, and a
    /// banked or multi set holds at least two.
    #[test]
    fn shapes_that_are_not_slot_shapes_are_declined() {
        for (set_type, num_chips) in [
            (ChipSetType::Single, 2),
            (ChipSetType::Banked, 1),
            (ChipSetType::Multi, 1),
        ] {
            assert!(
                check_chip_set_on_board(
                    Board::Fire28D,
                    ChipType::Chip27512,
                    set_type,
                    num_chips,
                    default_cs_config(ChipType::Chip27512),
                )
                .is_err(),
                "{set_type:?} x{num_chips} is not a slot shape"
            );
        }
    }

    /// A homogeneous multi set is checkable at board level, which is the
    /// point of taking the chip set's configuration rather than its images.
    ///
    /// Fire24F serving two 2364s is the Commodore case - a Kernal and a Basic
    /// in one One ROM, the second selected through X1. The pair costs 32KB
    /// against 16KB of ROM, X1 sitting one GPIO below the address block.
    #[test]
    fn a_homogeneous_multi_set_is_checkable() {
        let result = check_chip_set_on_board(
            Board::Fire24F,
            ChipType::Chip2364,
            ChipSetType::Multi,
            2,
            default_cs_config(ChipType::Chip2364),
        )
        .expect("Fire24F should serve two 2364s as a multi set");

        assert_eq!(result.slot_size_bytes, 32 * 1024);
        assert_eq!(result.excess_addr_bits(), 1);
    }

    /// A multi set's secondaries have exactly one monitored control line, so
    /// a CE/OE chip's secondary must ignore OE and select on CE. Modelling it
    /// as plain `CeOe` would present both lines as active and leave
    /// `derive_multi_cs_config` picking the last one it saw as the select.
    #[test]
    fn a_ce_oe_secondary_selects_on_ce_alone() {
        assert_eq!(
            multi_secondary_config(ChipType::Chip27512),
            CsConfig::CeOeExplicit {
                ce: CsLogic::ActiveLow,
                oe: CsLogic::Ignore,
            }
        );

        assert!(
            check_chip_set_on_board(
                Board::Fire28D,
                ChipType::Chip27512,
                ChipSetType::Multi,
                2,
                default_cs_config(ChipType::Chip27512),
            )
            .is_ok(),
            "a CE/OE chip should be servable as a multi pair"
        );
    }
}
