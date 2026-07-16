// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! V2 builder implementation, for firmwares v0.7.0+

use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;

pub const MIN_FW_VERSION: FirmwareVersion = FirmwareVersion::new(0, 7, 0, 0);
pub const MAX_FW_VERSION: FirmwareVersion = FirmwareVersion::new(0, 7, 999, 999);
pub const UNSUPPORTED_FIRMWARE_VERSIONS: &[FirmwareVersion] = &[];

pub const SUPPORTED_CHIP_TYPES: &[ChipType; 35] = &[
    ChipType::Chip2316,
    ChipType::Chip2716,
    // ChipType::Chip6116,
    ChipType::Chip2332,
    ChipType::Chip2732,
    ChipType::Chip2364,
    ChipType::Chip2764,
    ChipType::Chip23128,
    ChipType::Chip27128,
    ChipType::Chip23256,
    ChipType::Chip27256,
    ChipType::Chip23512,
    ChipType::Chip27512,
    ChipType::Chip231024,
    ChipType::Chip27C400,
    ChipType::Chip27C010,
    ChipType::Chip27C020,
    ChipType::Chip27C040,
    ChipType::Chip27C080,
    ChipType::Chip27C301,
    ChipType::Chip2704,
    ChipType::Chip2708,
    ChipType::SystemPlugin,
    ChipType::UserPlugin,
    ChipType::PioPlugin,
    ChipType::Chip28C16,
    ChipType::Chip28C64,
    ChipType::Chip28C256,
    ChipType::Chip28C512,
    ChipType::Chip23C1001,
    ChipType::Chip23C1010,
    ChipType::Chip23QL512,
    ChipType::Chip23QL384,
    ChipType::Chip27C200,
    ChipType::ChipSST39SF040,
    ChipType::ChipHM7641,
];
