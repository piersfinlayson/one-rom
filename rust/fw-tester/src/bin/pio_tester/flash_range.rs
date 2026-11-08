// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The firmware's check that a ROM slot lies within the flash before it reads
//! the slot.
//!
//! - The check at the edges of each chip and at the plugin regions on a first
//!   chip too small to hold them.
//! - Serving a slot on chip select 1 of an M board enters limp mode with
//!   `LIMP_MODE_INVALID_CONFIG`. On an L board the same slot serves where the
//!   board has a secondary flash chip select.
//! - A plugin slot outside the flash fails the firmware's plugin check.
//! - A valid plugin header passes the plugin check with its slot in its region
//!   and fails with the slot on chip select 1 of an M board or its entry point
//!   past its region.
//! - The boot plugin parse applies a VBUS detect override only from a plugin
//!   the firmware starts.
//! - A plugin's yield capability comes from its header only where its slot
//!   lies within the flash.

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::{
    Emulator, ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
    OraResult, ffi,
};
use onerom_metadata::otp::flash_size_bytes;
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, FLASH_CS1_BASE_ADDR, LimpModePattern,
    OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_BOOT_FLAGS0_ROW, OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT,
    OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT, OTP_FLASH_DEVINFO_ROW, OneromFlashSize, SYSTEM_PLUGIN_OFFSET,
    SYSTEM_PLUGIN_SIZE, USER_PLUGIN_OFFSET, USER_PLUGIN_SIZE,
};

use crate::report::TestReport;

/// The ROM slot the checks move. Set 0 serves from it, as the test configs
/// don't have plugin slots.
const SLOT: u8 = 0;

/// The chip sizes OTP configures for a case, or `None` for OTP unwritten, an
/// M board's.
type Chips = Option<(OneromFlashSize, OneromFlashSize)>;

/// An L board's chips.
const L_CHIPS: Chips = Some((OneromFlashSize::FlashSize2mb, OneromFlashSize::FlashSize2mb));

/// A plugin slot [`Emulator::install_plugin_slots`] makes. `index` is both its
/// ROM slot index and its plugin index.
pub(crate) struct Plugin {
    pub(crate) label: &'static str,
    pub(crate) index: u8,
    pub(crate) plugin_type: ffi::ora_plugin_type_t,
    pub(crate) region: u32,
    pub(crate) size: u32,
}

pub(crate) const PLUGINS: [Plugin; 2] = [
    Plugin {
        label: "system",
        index: 0,
        plugin_type: ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM,
        region: FLASH_CS0_BASE_ADDR + SYSTEM_PLUGIN_OFFSET,
        size: SYSTEM_PLUGIN_SIZE as u32,
    },
    Plugin {
        label: "user",
        index: 1,
        plugin_type: ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER,
        region: FLASH_CS0_BASE_ADDR + USER_PLUGIN_OFFSET,
        size: USER_PLUGIN_SIZE as u32,
    },
];

impl Plugin {
    /// An entry point at the last address in the plugin's region.
    fn entry(&self) -> u32 {
        self.region + self.size - 1
    }

    /// The plugin's region moved to chip select 1.
    fn on_cs1(&self) -> u32 {
        self.region - FLASH_CS0_BASE_ADDR + FLASH_CS1_BASE_ADDR
    }

    /// Write a valid header and place the slot in its region or on chip
    /// select 1.
    pub(crate) fn place(&self, in_region: bool, overrides1: u8, properties1: u8) {
        Emulator::set_plugin_header(self.index, self.entry(), overrides1, properties1);
        let addr = if in_region {
            self.region
        } else {
            self.on_cs1()
        };
        Emulator::set_rom_slot_flash_addr(self.index, addr);
    }
}

/// Check the flash slot calls with the plugin slots in their regions.
/// `num_slots` is the metadata's slot count.
pub(crate) fn check_plugin_flash_slots(emu: &Emulator, num_slots: u8) -> Result<(), String> {
    let counts = [
        ("all slots", 0, num_slots),
        ("plugins", ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS, 2),
        (
            "ROM slots",
            ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
            num_slots - 2,
        ),
    ];
    for (label, flags, expected) in counts {
        let found = emu.get_flash_slot_count(flags);
        if found != expected {
            return Err(format!("{label}: {found} flash slots, not {expected}"));
        }
    }

    let flags = ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS;
    for (p, name, chip_type) in [
        (&PLUGINS[0], c"system_plugin", ChipType::SystemPlugin),
        (&PLUGINS[1], c"user_plugin", ChipType::UserPlugin),
    ] {
        let rbcp = u32::from(chip_type.rbcp_chip_type());
        let (r, info) = emu.get_flash_slot_info(p.index, flags);
        let info = info.ok_or(format!("the {} plugin slot's info: {r:?}", p.label))?;
        if (info.rom_type, info.rom_count, info.name) != (rbcp, 1, None) {
            return Err(format!(
                "the {} plugin slot's info: type {}, {} ROMs, name {:?}",
                p.label, info.rom_type, info.rom_count, info.name
            ));
        }

        let (r, ext) = emu.get_flash_slot_ext_info(p.index, 0, flags);
        let ext = ext.ok_or(format!("the {} plugin slot's ROM: {r:?}", p.label))?;
        if (ext.rom_type, ext.chip_size, ext.rbcp_rom_type) != (Some(name), p.size, rbcp) {
            return Err(format!(
                "the {} plugin slot's ROM: {:?}, {} bytes, type {}",
                p.label, ext.rom_type, ext.chip_size, ext.rbcp_rom_type
            ));
        }
        let (r, _) = emu.get_flash_slot_ext_info(p.index, 1, flags);
        if r != OraResult::InvalidArg {
            return Err(format!("the {} plugin slot's second ROM: {r:?}", p.label));
        }

        let found = emu.get_flash_slot_addr(p.index, flags);
        if found != (OraResult::Ok, p.region, p.size) {
            return Err(format!(
                "the {} plugin slot's address: {:?} {:#010x} {:#x}",
                p.label, found.0, found.1, found.2
            ));
        }
    }
    Ok(())
}

fn placement(in_region: bool) -> &'static str {
    if in_region {
        "in its region"
    } else {
        "on chip select 1 of an M board"
    }
}

/// Runs the checks. They need a ROM slot, so they run only where the config
/// has one.
pub fn run(board: Board, report: &mut TestReport) {
    let addr = Emulator::rom_slot_flash_addr(SLOT);
    report.add_check("flash range: slot placement", check_placement(board, addr));
    report.add_check(
        "flash range: serving from chip select 1",
        check_serving(board),
    );
    report.add_check("flash range: plugin check", check_plugin());
    Emulator::set_rom_slot_flash_addr(SLOT, addr);
    Emulator::clear_otp();
}

/// Runs the checks on plugin slots with valid headers. The plugin slots read
/// their flash addresses from the config's table so the checks run only where
/// the config has at least two ROM slots.
pub fn run_plugins(report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    set_chips(None);
    Emulator::install_plugin_slots();
    report.add_check("flash range: valid plugin check", check_valid_plugin());
    report.add_check("flash range: boot plugin parse", check_plugin_parse());
    report.add_check("flash range: plugin yield capability", check_yield());
    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

/// Put `chips` in OTP.
fn set_chips(chips: Chips) {
    Emulator::clear_otp();
    if let Some((cs0, cs1)) = chips {
        Emulator::set_otp_raw(
            OTP_BOOT_FLAGS0_ROW,
            &[OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE; 3],
        );
        let devinfo = ((cs0 as u16) << OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT)
            | ((cs1 as u16) << OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT);
        Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[devinfo]);
    }
}

fn check_placement(board: Board, generated: u32) -> Result<(), String> {
    let size = Emulator::rom_slot_size(SLOT);
    let mb2 = flash_size_bytes(OneromFlashSize::FlashSize2mb) as u32;
    let cs0_end = FLASH_CS0_BASE_ADDR + mb2;
    let cs1_end = FLASH_CS1_BASE_ADDR + mb2;
    let cs1_counts = board.external_flash_cs_pin().is_some();
    let small = Some((
        OneromFlashSize::FlashSize128kb,
        OneromFlashSize::FlashSizeNone,
    ));

    let cases: [(&str, Chips, u32, bool); 10] = [
        ("where the generator put it", None, generated, true),
        ("ending at the first chip's end", None, cs0_end - size, true),
        (
            "a byte past the first chip's end",
            None,
            cs0_end - size + 1,
            false,
        ),
        (
            "ending at the first chip's start",
            None,
            FLASH_CS0_BASE_ADDR - size,
            false,
        ),
        (
            "on chip select 1 of an M board",
            None,
            FLASH_CS1_BASE_ADDR,
            false,
        ),
        (
            "on chip select 1 of an L board",
            L_CHIPS,
            FLASH_CS1_BASE_ADDR,
            cs1_counts,
        ),
        (
            "ending at chip select 1's end",
            L_CHIPS,
            cs1_end - size,
            cs1_counts,
        ),
        (
            "a byte past chip select 1's end",
            L_CHIPS,
            cs1_end - size + 1,
            false,
        ),
        (
            "at the system plugin region of a 128KB first chip",
            small,
            FLASH_CS0_BASE_ADDR + SYSTEM_PLUGIN_OFFSET,
            size as usize <= SYSTEM_PLUGIN_SIZE,
        ),
        (
            "at the user plugin region of a 128KB first chip",
            small,
            FLASH_CS0_BASE_ADDR + USER_PLUGIN_OFFSET,
            false,
        ),
    ];

    for (label, chips, addr, expected) in cases {
        set_chips(chips);
        Emulator::set_rom_slot_flash_addr(SLOT, addr);
        let found = Emulator::rom_slot_in_flash(SLOT);
        if found != expected {
            return Err(format!(
                "a {size} byte slot at {addr:#010x}, {label}: the firmware found it \
                 {}within the flash",
                if found { "" } else { "not " }
            ));
        }
    }
    Ok(())
}

fn check_serving(board: Board) -> Result<(), String> {
    let cs1_counts = board.external_flash_cs_pin().is_some();
    for (label, chips, serves) in [
        ("an M board", None, false),
        ("an L board", L_CHIPS, cs1_counts),
    ] {
        set_chips(chips);
        Emulator::set_rom_slot_flash_addr(SLOT, FLASH_CS1_BASE_ADDR);
        Emulator::set_rp_variant(board.rp_variant());
        Emulator::set_sel_image(0);
        let emulator = Emulator::boot();

        let limp = emulator.limp_mode_pattern();
        if serves {
            if emulator.limp_mode() {
                return Err(format!("{label}: the firmware entered limp mode {limp}"));
            }
            if !emulator.pios_enabled() {
                return Err(format!(
                    "{label}: PIO state machines not enabled after boot"
                ));
            }
        } else if limp != LimpModePattern::LimpModeInvalidConfig as u8 {
            return Err(format!(
                "{label}: the firmware's limp mode is {limp}, not {}",
                LimpModePattern::LimpModeInvalidConfig as u8
            ));
        }
    }
    Ok(())
}

fn check_plugin() -> Result<(), String> {
    set_chips(None);
    Emulator::set_rom_slot_flash_addr(SLOT, FLASH_CS1_BASE_ADDR);
    let plugins = [
        ("system", ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM, 0),
        ("user", ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER, 1),
    ];
    for (label, plugin_type, index) in plugins {
        if Emulator::check_plugin_valid(SLOT, plugin_type, index) {
            return Err(format!(
                "a {label} plugin slot on chip select 1 of an M board passed the plugin check"
            ));
        }
    }
    Ok(())
}

fn check_valid_plugin() -> Result<(), String> {
    for p in &PLUGINS {
        let cases = [
            ("in its region", p.entry(), p.region, true),
            (
                "on chip select 1 of an M board",
                p.entry(),
                p.on_cs1(),
                false,
            ),
            (
                "with its entry point past its region",
                p.region + p.size,
                p.region,
                false,
            ),
        ];
        for (label, entry, addr, expected) in cases {
            Emulator::set_plugin_header(p.index, entry, 0, 0);
            Emulator::set_rom_slot_flash_addr(p.index, addr);
            if Emulator::check_plugin_valid(p.index, p.plugin_type, p.index) != expected {
                return Err(format!(
                    "the {} plugin {label} {} the plugin check",
                    p.label,
                    if expected { "failed" } else { "passed" }
                ));
            }
        }
    }
    Ok(())
}

fn check_plugin_parse() -> Result<(), String> {
    let off = ffi::ORA_OVERRIDE1_DISABLE_VBUS_DETECT as u8;
    let cases = [
        (
            "the system plugin disabling VBUS detect, both in their regions",
            [(true, off), (true, 0)],
            true,
        ),
        (
            "the user plugin disabling VBUS detect, both in their regions",
            [(true, 0), (true, off)],
            true,
        ),
        (
            "both disabling VBUS detect on chip select 1 of an M board",
            [(false, off), (false, off)],
            false,
        ),
        (
            "the user plugin disabling VBUS detect, the system plugin on chip select 1",
            [(false, 0), (true, off)],
            false,
        ),
    ];
    for (label, placements, disabled) in cases {
        for (p, (in_region, overrides1)) in PLUGINS.iter().zip(placements) {
            p.place(in_region, overrides1, 0);
        }
        let (plugins, count, found) = Emulator::initial_plugin_parse();
        if (plugins, count, found) != (0b11, 2, disabled) {
            return Err(format!(
                "{label}: the parse found plugins {plugins:#04b}, {count} of them, with \
                 VBUS detect {}",
                if found { "disabled" } else { "enabled" }
            ));
        }
    }
    Ok(())
}

fn check_yield() -> Result<(), String> {
    let yields = ffi::ORA_PROPERTY1_SUPPORTS_YIELD as u8;
    let [system, user] = &PLUGINS;
    let cases = [
        (0, system, true, yields, 1),
        (0, system, true, 0, -1),
        (0, system, false, yields, 0),
        (1, user, true, yields, 1),
        (1, user, true, 0, -1),
        (1, user, false, 0, 1),
    ];
    for (core, p, in_region, properties1, expected) in cases {
        p.place(in_region, 0, properties1);
        let found = Emulator::other_core_yield_capability(core);
        if found != expected {
            return Err(format!(
                "from core {core}, the {} plugin {} {}: yield capability {found}, not \
                 {expected}",
                p.label,
                if properties1 == 0 {
                    "without yield"
                } else {
                    "with yield"
                },
                placement(in_region)
            ));
        }
    }
    Ok(())
}
