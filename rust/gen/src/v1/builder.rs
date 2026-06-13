// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM generation Builder objects and functions

use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;

pub const MIN_SUPPORTED_FIRMWARE_VERSION: FirmwareVersion = FirmwareVersion::new(0, 2, 0, 0);
pub const MAX_SUPPORTED_FIRMWARE_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 999, 0);

pub const UNSUPPORTED_FIRMWARE_VERSIONS: [FirmwareVersion; 1] = [FirmwareVersion::new(0, 6, 3, 0)];

pub const SUPPORTED_CHIP_TYPES: &[ChipType; 35] = &[
    ChipType::Chip2316,
    ChipType::Chip2716,
    ChipType::Chip6116,
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
    ChipType::Chip23C1010,
    ChipType::Chip27C080,
    ChipType::Chip23QL512,
    ChipType::Chip23QL384,
    ChipType::Chip27C200,
    ChipType::ChipSST39SF040,
];
