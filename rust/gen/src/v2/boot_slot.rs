// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Checks, metadata encoding and recovery of `boot_slot`.

use onerom_config::fw::FirmwareVersion;
use onerom_metadata::BOOT_SLOT_NOT_SET;

use crate::{Config, Error, MIN_BOOT_SLOT_VERSION, Result};

pub(crate) fn check_config(version: FirmwareVersion, config: &Config) -> Result<()> {
    let Some(boot_slot) = config.boot_slot else {
        return Ok(());
    };
    if version < MIN_BOOT_SLOT_VERSION {
        return Err(Error::FirmwareTooOld {
            feat: "boot_slot",
            version,
            minimum: MIN_BOOT_SLOT_VERSION,
        });
    }
    if !is_rom_slot(config, boot_slot) {
        return Err(Error::BootSlotNotRomSlot {
            boot_slot,
            first: config.chip_set_of_slot(0).unwrap_or(0),
            slots: config
                .chip_sets
                .iter()
                .filter(|set| !set.is_plugin())
                .count(),
        });
    }
    Ok(())
}

pub(crate) fn metadata_value(boot_slot: Option<u8>) -> u8 {
    debug_assert_ne!(boot_slot, Some(BOOT_SLOT_NOT_SET), "check_config checks it");
    boot_slot.unwrap_or(BOOT_SLOT_NOT_SET)
}

/// The `boot_slot` of `config` from its metadata `value`. `None` where `value`
/// isn't a ROM slot, as the firmware ignores it.
pub(crate) fn recover(config: &Config, value: u8) -> Option<u8> {
    is_rom_slot(config, value).then_some(value)
}

fn is_rom_slot(config: &Config, index: u8) -> bool {
    // The metadata's slot count is a u8 so this index is never a slot.
    index != BOOT_SLOT_NOT_SET
        && config
            .chip_sets
            .get(usize::from(index))
            .is_some_and(|set| !set.is_plugin())
}
