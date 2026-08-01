// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Unified parsed device representation.
//!
//! [`ParsedDevice`] wraps either an original-format ([`Sdrr`]) or
//! schema-format ([`OneRom`]) parse result behind one interface, so callers
//! that only need facts common to both — firmware version, board, ROM layout,
//! which slot is being served — never have to branch on firmware generation.
//! Format-specific data remains reachable via [`as_original`] and [`as_schema`]
//! for the cases that genuinely need it.
//!
//! # Recognition vs parse errors
//!
//! [`ParsedDevice::parse_device`] is infallible: it always yields a
//! `ParsedDevice`, even for data that is not One ROM firmware at all. Two
//! separate questions follow from it, and callers should not conflate them:
//!
//! - [`is_recognised`] answers "is this a One ROM?". It is the check to make
//!   before presenting a device or file to the user as a One ROM.
//! - [`parse_errors`] answers "what went wrong *within* a device we did
//!   recognise?". A recognised device may still carry non-fatal errors.
//!
//! # ROM layout
//!
//! [`ParsedDevice::slots`] gives a borrowed, allocation-free walk over the
//! device's ROM slots. Each [`SlotView`] is classified as a
//! [`SlotKind::Plugin`] or [`SlotKind::Rom`] — read from the parsed data, not
//! inferred from position — and carries two numberings that must not be
//! confused:
//!
//! - `slot_index` is the absolute position in flash, counting plugins. It is
//!   what the runtime's active-slot index refers to (see
//!   [`active_slot_index`]).
//! - `user_index` is the plugin-excluding number, assigned across `Rom` slots
//!   only. It is the number to show a user, who did not place the plugins and
//!   should not have to count past them; it is `None` for plugin slots.
//!
//! The slot list always comes from flash (the configured set). Only *which*
//! slot is active comes from runtime state, surfaced per-slot as
//! [`SlotView::active`] and never as a substitute slot list.
//!
//! A slot may hold more than one ROM ([`SlotView::roms`]) — multi-ROM sets and
//! banked images — so the walk is two levels: slots, each yielding one or more
//! [`RomView`]s.
//!
//! [`as_original`]: ParsedDevice::as_original
//! [`as_schema`]: ParsedDevice::as_schema
//! [`active_slot_index`]: ParsedDevice::active_slot_index
//! [`is_recognised`]: ParsedDevice::is_recognised
//! [`parse_errors`]: ParsedDevice::parse_errors
//! [`ParsedDevice::parse_device`]: crate::Parser::parse_device

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use std::borrow::Cow;

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_metadata::{OneromRomInfo, OneromRomSlot, RomSlotType};

use crate::ParseError;
use crate::info::{Sdrr, SdrrRomInfo, SdrrRomSet};
use crate::onerom::OneRom;
use crate::types::SdrrRomType;

/// The complete parsed state of a One ROM device, regardless of firmware
/// generation.
///
/// Constructed by [`Parser::parse_device`], which detects the firmware format
/// and delegates to the appropriate parser.  Callers that need
/// format-specific fields can downcast via [`as_original`] or [`as_schema`].
///
/// [`as_original`]: ParsedDevice::as_original
/// [`as_schema`]: ParsedDevice::as_schema
/// [`Parser::parse_device`]: crate::Parser::parse_device
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParsedDevice {
    /// Pre-v0.7.0 hand-crafted format.
    Original(Sdrr),

    /// v0.7.0+ schema-driven metadata format.
    Schema(OneRom),
}

impl ParsedDevice {
    /// Returns `true` if a recognisable One ROM was found.
    ///
    /// [`Parser::parse_device`] is infallible, so this is the check that
    /// distinguishes "this is a One ROM" from "this is some other data".
    /// Original-format devices are recognised if either the flash or the RAM
    /// region parsed; schema-format devices if the firmware info structure was
    /// located.
    ///
    /// This is deliberately distinct from [`parse_errors`](Self::parse_errors),
    /// which reports non-fatal problems *within* a device that was recognised.
    /// A device can be recognised and still carry parse errors.
    ///
    /// [`Parser::parse_device`]: crate::Parser::parse_device
    pub fn is_recognised(&self) -> bool {
        match self {
            Self::Original(sdrr) => sdrr.flash.is_some() || sdrr.ram.is_some(),
            Self::Schema(onerom) => onerom.info().is_some(),
        }
    }

    /// Returns `true` if the device was actively running when parsed.
    ///
    /// For original-format devices this means RAM info was successfully read.
    /// For schema-format devices this means runtime info was present.
    ///
    /// Note this reports what the *parse* found, which requires the reader to
    /// have been able to reach RAM. A caller that only read flash will always
    /// see `false` here, and should determine liveness by other means.
    pub fn is_running(&self) -> bool {
        match self {
            Self::Original(sdrr) => sdrr.is_running(),
            Self::Schema(onerom) => onerom.runtime().is_some(),
        }
    }

    /// Returns the firmware version, if it could be determined.
    pub fn version(&self) -> Option<FirmwareVersion> {
        match self {
            Self::Original(sdrr) => sdrr.flash.as_ref().map(|f| f.version),
            Self::Schema(onerom) => {
                let info = onerom.info()?;
                Some(FirmwareVersion::new(
                    info.major_version,
                    info.minor_version,
                    info.patch_version,
                    info.build_number,
                ))
            }
        }
    }

    /// Returns whether explicit firmware metadata is present, or `None` if it
    /// could not be determined.
    ///
    /// Always `Some(true)` for schema-format devices: the metadata region is
    /// what they are parsed from, so its presence is implied by the format.
    pub fn metadata_present(&self) -> Option<bool> {
        match self {
            Self::Original(sdrr) => sdrr.flash.as_ref().map(|f| f.metadata_present),
            Self::Schema(_) => Some(true),
        }
    }

    /// Returns non-fatal parse errors encountered during parsing.
    ///
    /// These describe problems *within* a device; see
    /// [`is_recognised`](Self::is_recognised) for whether a One ROM was found
    /// at all.
    pub fn parse_errors(&self) -> &[ParseError] {
        match self {
            Self::Original(sdrr) => sdrr
                .flash
                .as_ref()
                .map(|f| f.parse_errors.as_slice())
                .unwrap_or(&[]),
            Self::Schema(onerom) => onerom.parse_errors(),
        }
    }

    /// Returns the original-format data, or `None` if this is schema-format.
    pub fn as_original(&self) -> Option<&Sdrr> {
        if let Self::Original(sdrr) = self {
            Some(sdrr)
        } else {
            None
        }
    }

    /// Returns the schema-format data, or `None` if this is original-format.
    pub fn as_schema(&self) -> Option<&OneRom> {
        if let Self::Schema(onerom) = self {
            Some(onerom)
        } else {
            None
        }
    }

    /// Returns the board type, if it could be determined.
    pub fn get_board(&self) -> Option<Board> {
        match self {
            Self::Original(sdrr) => sdrr.flash.as_ref().and_then(|f| f.board),
            Self::Schema(onerom) => {
                let hw_rev = &onerom.info()?.metadata.as_ref()?.hw.hw_rev;
                Board::try_from_str(hw_rev)
            }
        }
    }

    /// Returns whether the device is USB capable
    pub fn is_usb_run_capable(&self) -> bool {
        match self {
            Self::Original(sdrr) => sdrr
                .flash
                .as_ref()
                .map(|f| f.is_usb_run_capable())
                .unwrap_or(false),
            Self::Schema(onerom) => {
                // Get the first ROM set if it exists
                let slot = match onerom.metadata() {
                    Some(metadata) => match metadata.rom_slots.first() {
                        Some(slot) => slot,
                        None => return false,
                    },
                    None => return false,
                };

                let rom_info = match slot.roms.first() {
                    Some(info) => info,
                    None => return false,
                };

                rom_info.rbcp_rom_type
                    == onerom_config::chip::ChipType::SystemPlugin.rbcp_chip_type()
            }
        }
    }

    /// Returns a display MCU name, if determinable.
    ///
    /// Original devices report their specific MCU variant (e.g. "F411RE");
    /// schema devices report the board's MCU family (e.g. "RP2350").
    pub fn mcu_name(&self) -> Option<String> {
        match self {
            Self::Original(sdrr) => sdrr
                .flash
                .as_ref()
                .and_then(|f| f.mcu_variant)
                .map(|v| v.to_string()),
            Self::Schema(_) => self.get_board().map(|b| b.mcu_family().to_string()),
        }
    }

    /// The absolute index of the slot currently being served, or `None` if the
    /// device is not running.
    ///
    /// Neutralises the original `ram.rom_set_index` and the schema
    /// `runtime.rom_slot_index`; both count every slot, plugins included, and
    /// so line up with [`SlotView::slot_index`].
    pub fn active_slot_index(&self) -> Option<usize> {
        match self {
            Self::Original(sdrr) => sdrr.ram.as_ref().map(|r| r.rom_set_index as usize),
            Self::Schema(onerom) => onerom.runtime().map(|r| r.rom_slot_index as usize),
        }
    }

    /// Iterates the device's ROM slots in flash order, classified and numbered.
    ///
    /// The slot list always comes from flash (the configured set), never from
    /// the single runtime slot; [`SlotView::active`] marks which one is live.
    /// Yields nothing when the relevant region failed to parse (no flash / no
    /// metadata) — corruption surfaces via [`parse_errors`](Self::parse_errors),
    /// not here.
    pub fn slots(&self) -> Slots<'_> {
        let active = self.active_slot_index();
        let inner = match self {
            Self::Original(sdrr) => match sdrr.flash.as_ref() {
                Some(info) => SlotsInner::Original(info.rom_sets.iter()),
                None => SlotsInner::Empty,
            },
            Self::Schema(onerom) => match onerom.metadata() {
                Some(md) => SlotsInner::Schema(md.rom_slots.iter()),
                None => SlotsInner::Empty,
            },
        };
        Slots {
            inner,
            active,
            idx: 0,
            user_idx: 0,
        }
    }
}

// ===========================================================================
// Neutral slot / ROM view
// ===========================================================================

/// High-level classification of a ROM slot.
///
/// Deliberately coarse: callers presenting a device summary care whether a slot
/// is a *user* ROM or firmware infrastructure, not which specific plugin it is.
/// The distinction is read from the parsed data (original: the ROM type;
/// schema: the slot type), not inferred from slot position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// A plugin slot (system, user, or PIO). Not counted in the user-facing ROM
    /// numbering.
    Plugin,
    /// A ROM slot the user placed.
    Rom,
}

/// A single ROM image within a slot, borrowed from the parse.
#[derive(Debug)]
pub struct RomView<'a> {
    /// Human-readable ROM type (e.g. "27C512", "System Plugin"). Borrowed on
    /// the schema side; materialised from the enum on the original side.
    pub rom_type: Cow<'a, str>,
    /// Source filename or URL, if the firmware recorded one.
    pub filename: Option<&'a str>,
    /// Logical chip size in bytes.
    pub size: usize,
}

/// One ROM slot, borrowed from the parse.
///
/// See the [module documentation](self) for the distinction between
/// `slot_index` (absolute, counts plugins) and `user_index` (plugin-excluding).
#[derive(Debug)]
pub struct SlotView<'a> {
    /// Absolute slot index (counts plugins).
    pub slot_index: usize,
    /// User-facing ROM index (counts ROM slots only), or `None` for plugins.
    pub user_index: Option<usize>,
    /// Whether this slot is a plugin or a user ROM.
    pub kind: SlotKind,
    /// Whether this slot is the one currently being served (running devices
    /// only; always `false` when stopped).
    pub active: bool,
    roms: RomsBacking<'a>,
}

impl<'a> SlotView<'a> {
    /// Iterates the ROM images in this slot. A slot may hold more than one
    /// (multi-ROM sets, banked images).
    pub fn roms(&self) -> RomsIter<'a> {
        match self.roms {
            RomsBacking::Original(s) => RomsIter::Original(s.iter()),
            RomsBacking::Schema(s) => RomsIter::Schema(s.iter()),
        }
    }
}

// The `'a` slices are `Copy`, so `SlotView::roms` can hand back an iterator
// bound to the underlying parse (`'a`) rather than to the `&self` borrow — the
// views outlive the call.
#[derive(Clone, Copy, Debug)]
enum RomsBacking<'a> {
    Original(&'a [SdrrRomInfo]),
    Schema(&'a [OneromRomInfo]),
}

/// Iterator over the [`RomView`]s of a slot, over either backing.
pub enum RomsIter<'a> {
    #[doc(hidden)]
    Original(core::slice::Iter<'a, SdrrRomInfo>),
    #[doc(hidden)]
    Schema(core::slice::Iter<'a, OneromRomInfo>),
}

impl<'a> Iterator for RomsIter<'a> {
    type Item = RomView<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            RomsIter::Original(it) => it.next().map(|r| RomView {
                rom_type: Cow::Owned(r.rom_type.to_string()),
                filename: r.filename.as_deref(),
                size: r.rom_type.rom_size(),
            }),
            RomsIter::Schema(it) => it.next().map(|r| RomView {
                rom_type: Cow::Borrowed(r.rom_type.as_str()),
                filename: r.filename.as_deref(),
                size: r.chip_size as usize,
            }),
        }
    }
}

/// Iterator over a device's [`SlotView`]s, over either backing.
///
/// Returned by [`ParsedDevice::slots`].
pub struct Slots<'a> {
    inner: SlotsInner<'a>,
    active: Option<usize>,
    idx: usize,
    user_idx: usize,
}

enum SlotsInner<'a> {
    Original(core::slice::Iter<'a, SdrrRomSet>),
    Schema(core::slice::Iter<'a, OneromRomSlot>),
    Empty,
}

impl<'a> Iterator for Slots<'a> {
    type Item = SlotView<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (kind, roms) = match &mut self.inner {
            SlotsInner::Original(it) => {
                let set = it.next()?;
                (sdrr_set_kind(set), RomsBacking::Original(&set.roms))
            }
            SlotsInner::Schema(it) => {
                let slot = it.next()?;
                (schema_slot_kind(slot), RomsBacking::Schema(&slot.roms))
            }
            SlotsInner::Empty => return None,
        };

        let slot_index = self.idx;
        self.idx += 1;

        let user_index = match kind {
            SlotKind::Rom => {
                let u = self.user_idx;
                self.user_idx += 1;
                Some(u)
            }
            SlotKind::Plugin => None,
        };

        Some(SlotView {
            slot_index,
            user_index,
            kind,
            active: self.active == Some(slot_index),
            roms,
        })
    }
}

/// Classify an original-format ROM set. A set is a plugin set iff its ROM is a
/// plugin type; plugin sets carry exactly one ROM.
fn sdrr_set_kind(set: &SdrrRomSet) -> SlotKind {
    match set.roms.first().map(|r| r.rom_type) {
        Some(SdrrRomType::SystemPlugin | SdrrRomType::UserPlugin | SdrrRomType::PioPlugin) => {
            SlotKind::Plugin
        }
        _ => SlotKind::Rom,
    }
}

/// Classify a schema-format slot by its declared slot type.
fn schema_slot_kind(slot: &OneromRomSlot) -> SlotKind {
    match slot.slot_type {
        RomSlotType::RomSlotTypePluginSystem
        | RomSlotType::RomSlotTypePluginUser
        | RomSlotType::RomSlotTypePluginPio => SlotKind::Plugin,
        // A RAM slot is user-placed and counted in the user-facing numbering,
        // so it belongs here rather than with the plugins.
        RomSlotType::RomSlotTypeSingleRom
        | RomSlotType::RomSlotTypeBankedRom
        | RomSlotType::RomSlotTypeMultiRom
        | RomSlotType::RomSlotTypeSingleRam => SlotKind::Rom,
    }
}
