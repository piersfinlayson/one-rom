// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! sdrr-fw-parser
//!
//! Enums and types used in SDRR firmware parsing

use core::fmt;
use deku::prelude::*;

#[cfg(not(feature = "std"))]
use alloc::string::String;

use crate::SdrrInfo;

/// Enum used to identify the source of an information object
pub enum Source {
    Flash,
    Ram,
}

/// STM32F4 product line options
///
/// Reflects `stm_line_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u16", ctx = "endian: deku::ctx::Endian")]
pub enum McuLine {
    /// F401D/E - 96KB RAM
    #[deku(id = "0x0000")]
    F401DE,

    /// F405
    #[deku(id = "0x0001")]
    F405,

    /// F411
    #[deku(id = "0x0002")]
    F411,

    /// F446
    #[deku(id = "0x0003")]
    F446,

    /// F401B/C - 64KB RAM
    #[deku(id = "0x0004")]
    F401BC,

    /// RP2350
    #[deku(id = "0x0005")]
    Rp2350,
}

impl fmt::Display for McuLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McuLine::F401DE => write!(f, "STM32F401DE"),
            McuLine::F405 => write!(f, "STM32F405"),
            McuLine::F411 => write!(f, "STM32F411"),
            McuLine::F446 => write!(f, "STM32F446"),
            McuLine::F401BC => write!(f, "STM32F401BC"),
            McuLine::Rp2350 => write!(f, "RP2350"),
        }
    }
}

impl McuLine {
    /// Returns the amount of SRAM of the device (not including any CCM RAM)
    pub fn ram_kb(&self) -> &str {
        match self {
            McuLine::F401DE => "96",
            McuLine::F401BC => "64",
            McuLine::F405 | McuLine::F411 | McuLine::F446 => "128",
            McuLine::Rp2350 => "520",
        }
    }

    pub fn chip_suffix(&self) -> &str {
        match self {
            McuLine::F401DE => "f401",
            McuLine::F401BC => "f401",
            McuLine::F405 => "f405",
            McuLine::F411 => "f411",
            McuLine::F446 => "f446",
            McuLine::Rp2350 => "rp2350",
        }
    }
}

/// STM32F4 package flash storage code
///
/// For example "E" in STM32F401RET6 means 512KB of flash storage.
///
/// Reflects `stm_storage_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u16", ctx = "endian: deku::ctx::Endian")]
pub enum McuStorage {
    /// 8 = 64KB
    #[deku(id = "0")]
    Storage8,

    /// B = 128KB
    #[deku(id = "1")]
    StorageB,

    /// C = 256KB
    #[deku(id = "2")]
    StorageC,

    /// D = 384KB
    #[deku(id = "3")]
    StorageD,

    /// E = 512KB
    #[deku(id = "4")]
    StorageE,

    /// F = 768KB
    #[deku(id = "5")]
    StorageF,

    /// G = 1024KB
    #[deku(id = "6")]
    StorageG,

    /// 2MB
    #[deku(id = "7")]
    Storage2MB,
}

impl McuStorage {
    /// Returns storage size in KB as a usize
    pub fn size_kb(&self) -> usize {
        match self {
            McuStorage::Storage8 => 64,
            McuStorage::StorageB => 128,
            McuStorage::StorageC => 256,
            McuStorage::StorageD => 384,
            McuStorage::StorageE => 512,
            McuStorage::StorageF => 768,
            McuStorage::StorageG => 1024,
            McuStorage::Storage2MB => 2048,
        }
    }

    /// Returns the storage size in kilobytes
    pub fn kb(&self) -> &str {
        match self {
            McuStorage::Storage8 => "64",
            McuStorage::StorageB => "128",
            McuStorage::StorageC => "256",
            McuStorage::StorageD => "384",
            McuStorage::StorageE => "512",
            McuStorage::StorageF => "768",
            McuStorage::StorageG => "1024",
            McuStorage::Storage2MB => "2048",
        }
    }

    /// Returns the storage package code
    pub fn package_code(&self) -> &str {
        match self {
            McuStorage::Storage8 => "8",
            McuStorage::StorageB => "B",
            McuStorage::StorageC => "C",
            McuStorage::StorageD => "D",
            McuStorage::StorageE => "E",
            McuStorage::StorageF => "F",
            McuStorage::StorageG => "G",
            McuStorage::Storage2MB => "2MB",
        }
    }

    pub fn stm32_suffix(&self) -> &str {
        match self {
            McuStorage::Storage8 => "R8",
            McuStorage::StorageB => "RB",
            McuStorage::StorageC => "RC",
            McuStorage::StorageD => "RD",
            McuStorage::StorageE => "RE",
            McuStorage::StorageF => "RF",
            McuStorage::StorageG => "RG",
            McuStorage::Storage2MB => "",
        }
    }
}

impl fmt::Display for McuStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.package_code())
    }
}

/// Type of ROMs supported by SDRR
///
/// Reflects `sdrr_rom_type_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u8")]
pub enum SdrrRomType {
    /// 2316 ROM, 11-bit address, 3 CS lines, 2KB size
    #[deku(id = "0")]
    Rom2316,

    /// 2332 ROM, 12-bit address, 2 CS lines, 4KB size
    #[deku(id = "1")]
    Rom2332,

    /// 2364 ROM, 13-bit address, 1 CS line, 8KB size
    #[deku(id = "2")]
    Rom2364,

    /// 23128 ROM, 14-bit address, 3 CS lines, 16KB size
    #[deku(id = "3")]
    Rom23128,

    /// 23256 ROM, 15-bit address, 2 CS lines, 32KB size
    #[deku(id = "4")]
    Rom23256,

    /// 23512 ROM, 16-bit address, 1 CS line, 64KB size
    #[deku(id = "5")]
    Rom23512,

    /// 2704 ROM, 9-bit address, CE/OE lines, 0.5KB size
    #[deku(id = "6")]
    Rom2704,

    /// 2708 ROM, 10-bit address, CE/OE lines, 1KB size
    #[deku(id = "7")]
    Rom2708,

    /// 2716 ROM, 11-bit address, CE/OE lines, 2KB size
    #[deku(id = "8")]
    Rom2716,

    /// 2732 ROM, 12-bit address, CE/OE lines, 4KB size
    #[deku(id = "9")]
    Rom2732,

    /// 2764 ROM, 13-bit address, CE/OE lines, 8KB size
    #[deku(id = "10")]
    Rom2764,

    /// 27128 ROM, 14-bit address, CE/OE lines, 16KB size
    #[deku(id = "11")]
    Rom27128,

    /// 27256 ROM, 15-bit address, CE/OE lines, 32KB size
    #[deku(id = "12")]
    Rom27256,

    /// 27512 ROM, 16-bit address, CE/OE lines, 64KB size
    #[deku(id = "13")]
    Rom27512,

    // 231024
    #[deku(id = "14")]
    Rom231024,

    // 27C010
    #[deku(id = "15")]
    Rom27C010,

    // 27C020
    #[deku(id = "16")]
    Rom27C020,

    // 27C040
    #[deku(id = "17")]
    Rom27C040,

    // 27C080
    #[deku(id = "18")]
    Rom27C080,

    // 27C400
    #[deku(id = "19")]
    Rom27C400,

    // 6116 (RAM)
    #[deku(id = "20")]
    Ram6116,

    // 27C301
    #[deku(id = "21")]
    Rom27C301,

    // System Plugin
    #[deku(id = "22")]
    SystemPlugin,

    // User Plugin
    #[deku(id = "23")]
    UserPlugin,

    // PIO Plugin
    #[deku(id = "24")]
    PioPlugin,

    // SST39SF040
    #[deku(id = "25")]
    RomSst39sf040,

    // 28C16
    #[deku(id = "26")]
    Rom28C16,

    // 28C64
    #[deku(id = "27")]
    Rom28C64,

    // 28C256
    #[deku(id = "28")]
    Rom28C256,

    // 28C512
    #[deku(id = "29")]
    Rom28C512,

    // 23QL512
    #[deku(id = "30")]
    Rom23QL512,

    // 23QL384
    #[deku(id = "31")]
    Rom23QL384,

    // 23C1001
    #[deku(id = "32")]
    Rom23C1001,

    // 27C200
    #[deku(id = "33")]
    Rom27C200,
}

impl fmt::Display for SdrrRomType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdrrRomType::Rom2316 => write!(f, "2316"),
            SdrrRomType::Rom2332 => write!(f, "2332"),
            SdrrRomType::Rom2364 => write!(f, "2364"),
            SdrrRomType::Rom23128 => write!(f, "23128"),
            SdrrRomType::Rom23256 => write!(f, "23256"),
            SdrrRomType::Rom23512 => write!(f, "23512"),
            SdrrRomType::Rom2704 => write!(f, "2704"),
            SdrrRomType::Rom2708 => write!(f, "2708"),
            SdrrRomType::Rom2716 => write!(f, "2716"),
            SdrrRomType::Rom2732 => write!(f, "2732"),
            SdrrRomType::Rom2764 => write!(f, "2764"),
            SdrrRomType::Rom27128 => write!(f, "27128"),
            SdrrRomType::Rom27256 => write!(f, "27256"),
            SdrrRomType::Rom27512 => write!(f, "27512"),
            SdrrRomType::Rom231024 => write!(f, "231024"),
            SdrrRomType::Rom27C010 => write!(f, "27C010"),
            SdrrRomType::Rom27C020 => write!(f, "27C020"),
            SdrrRomType::Rom27C040 => write!(f, "27C040"),
            SdrrRomType::Rom27C080 => write!(f, "27C080"),
            SdrrRomType::Rom27C400 => write!(f, "27C400"),
            SdrrRomType::Ram6116 => write!(f, "6116 (RAM)"),
            SdrrRomType::Rom27C301 => write!(f, "27C301"),
            SdrrRomType::SystemPlugin => write!(f, "System Plugin"),
            SdrrRomType::UserPlugin => write!(f, "User Plugin"),
            SdrrRomType::PioPlugin => write!(f, "PIO Plugin"),
            SdrrRomType::RomSst39sf040 => write!(f, "SST39SF040"),
            SdrrRomType::Rom28C16 => write!(f, "28C16"),
            SdrrRomType::Rom28C64 => write!(f, "28C64"),
            SdrrRomType::Rom28C256 => write!(f, "28C256"),
            SdrrRomType::Rom28C512 => write!(f, "28C512"),
            SdrrRomType::Rom23QL512 => write!(f, "23QL512"),
            SdrrRomType::Rom23QL384 => write!(f, "23QL384"),
            SdrrRomType::Rom23C1001 => write!(f, "23C1001"),
            SdrrRomType::Rom27C200 => write!(f, "27C200"),
        }
    }
}

impl SdrrRomType {
    /// Returns the size of the ROM in bytes
    pub fn rom_size(&self) -> usize {
        if self == &SdrrRomType::Rom2704 {
            return 512;
        }
        self.rom_size_kb() * 1024
    }

    /// Returns the size of the ROM in KB
    pub fn rom_size_kb(&self) -> usize {
        match self {
            SdrrRomType::Rom2316 => 2,
            SdrrRomType::Rom2332 => 4,
            SdrrRomType::Rom2364 => 8,
            SdrrRomType::Rom23128 => 16,
            SdrrRomType::Rom23256 => 32,
            SdrrRomType::Rom23512 => 64,
            SdrrRomType::Rom2704 => 0,
            SdrrRomType::Rom2708 => 1,
            SdrrRomType::Rom2716 => 2,
            SdrrRomType::Rom2732 => 4,
            SdrrRomType::Rom2764 => 8,
            SdrrRomType::Rom27128 => 16,
            SdrrRomType::Rom27256 => 32,
            SdrrRomType::Rom27512 => 64,
            SdrrRomType::Rom231024 => 128,
            SdrrRomType::Rom27C010 => 128,
            SdrrRomType::Rom27C020 => 256,
            SdrrRomType::Rom27C040 => 512,
            SdrrRomType::Rom27C080 => 1024,
            SdrrRomType::Rom27C400 => 512,
            SdrrRomType::Ram6116 => 2,
            SdrrRomType::Rom27C301 => 128,
            SdrrRomType::SystemPlugin => 64,
            SdrrRomType::UserPlugin => 64,
            SdrrRomType::PioPlugin => 64,
            SdrrRomType::RomSst39sf040 => 512,
            SdrrRomType::Rom28C16 => 2,
            SdrrRomType::Rom28C64 => 8,
            SdrrRomType::Rom28C256 => 32,
            SdrrRomType::Rom28C512 => 64,
            SdrrRomType::Rom23QL512 => 64,
            SdrrRomType::Rom23QL384 => 48,
            SdrrRomType::Rom23C1001 => 128,
            SdrrRomType::Rom27C200 => 256,
        }
    }

    /// How many total physical pins this ROM type has
    pub fn rom_pins(&self) -> usize {
        match self {
            SdrrRomType::Rom2316 => 24,
            SdrrRomType::Rom2332 => 24,
            SdrrRomType::Rom2364 => 24,
            SdrrRomType::Rom23128 => 28,
            SdrrRomType::Rom23256 => 28,
            SdrrRomType::Rom23512 => 28,
            SdrrRomType::Rom2704 => 24,
            SdrrRomType::Rom2708 => 24,
            SdrrRomType::Rom2716 => 24,
            SdrrRomType::Rom2732 => 24,
            SdrrRomType::Rom2764 => 28,
            SdrrRomType::Rom27128 => 28,
            SdrrRomType::Rom27256 => 28,
            SdrrRomType::Rom27512 => 28,
            SdrrRomType::Rom231024 => 28,
            SdrrRomType::Rom27C010 => 32,
            SdrrRomType::Rom27C020 => 32,
            SdrrRomType::Rom27C040 => 32,
            SdrrRomType::Rom27C080 => 32,
            SdrrRomType::Rom27C400 => 40,
            SdrrRomType::Ram6116 => 24,
            SdrrRomType::Rom27C301 => 32,
            SdrrRomType::SystemPlugin => 0,
            SdrrRomType::UserPlugin => 0,
            SdrrRomType::PioPlugin => 0,
            SdrrRomType::RomSst39sf040 => 32,
            SdrrRomType::Rom28C16 => 24,
            SdrrRomType::Rom28C64 => 28,
            SdrrRomType::Rom28C256 => 28,
            SdrrRomType::Rom28C512 => 32,
            SdrrRomType::Rom23QL512 => 28,
            SdrrRomType::Rom23QL384 => 28,
            SdrrRomType::Rom23C1001 => 32,
            SdrrRomType::Rom27C200 => 40,
        }
    }

    /// Returns the maximum addressable location in the ROM
    pub fn max_addr(&self) -> u32 {
        (self.rom_size() - 1) as u32
    }

    /// Checks if the ROM type supports the CS2 line
    pub fn supports_cs2(&self) -> bool {
        match self {
            SdrrRomType::Rom2316 => true,
            SdrrRomType::Rom2332 => true,
            SdrrRomType::Rom2364 => false,
            SdrrRomType::Rom23128 => true,
            SdrrRomType::Rom23256 => true,
            SdrrRomType::Rom23512 => false,
            SdrrRomType::Rom2704 => false,
            SdrrRomType::Rom2708 => false,
            SdrrRomType::Rom2716 => false,
            SdrrRomType::Rom2732 => false,
            SdrrRomType::Rom2764 => false,
            SdrrRomType::Rom27128 => false,
            SdrrRomType::Rom27256 => false,
            SdrrRomType::Rom27512 => false,
            SdrrRomType::Rom231024 => false,
            SdrrRomType::Rom27C010 => false,
            SdrrRomType::Rom27C020 => false,
            SdrrRomType::Rom27C040 => false,
            SdrrRomType::Rom27C080 => false,
            SdrrRomType::Rom27C400 => false,
            SdrrRomType::Ram6116 => false,
            SdrrRomType::Rom27C301 => false,
            SdrrRomType::SystemPlugin => false,
            SdrrRomType::UserPlugin => false,
            SdrrRomType::PioPlugin => false,
            SdrrRomType::RomSst39sf040 => false,
            SdrrRomType::Rom28C16 => false,
            SdrrRomType::Rom28C64 => false,
            SdrrRomType::Rom28C256 => false,
            SdrrRomType::Rom28C512 => false,
            SdrrRomType::Rom23QL512 => false,
            SdrrRomType::Rom23QL384 => false,
            SdrrRomType::Rom23C1001 => true,
            SdrrRomType::Rom27C200 => false,
        }
    }

    /// Checks if the ROM type supports the CS3 line
    pub fn supports_cs3(&self) -> bool {
        match self {
            SdrrRomType::Rom2316 => true,
            SdrrRomType::Rom2332 => false,
            SdrrRomType::Rom2364 => false,
            SdrrRomType::Rom23128 => true,
            SdrrRomType::Rom23256 => false,
            SdrrRomType::Rom23512 => false,
            SdrrRomType::Rom2704 => false,
            SdrrRomType::Rom2708 => false,
            SdrrRomType::Rom2716 => false,
            SdrrRomType::Rom2732 => false,
            SdrrRomType::Rom2764 => false,
            SdrrRomType::Rom27128 => false,
            SdrrRomType::Rom27256 => false,
            SdrrRomType::Rom27512 => false,
            SdrrRomType::Rom231024 => false,
            SdrrRomType::Rom27C010 => false,
            SdrrRomType::Rom27C020 => false,
            SdrrRomType::Rom27C040 => false,
            SdrrRomType::Rom27C080 => false,
            SdrrRomType::Rom27C400 => false,
            SdrrRomType::Ram6116 => false,
            SdrrRomType::Rom27C301 => false,
            SdrrRomType::SystemPlugin => false,
            SdrrRomType::UserPlugin => false,
            SdrrRomType::PioPlugin => false,
            SdrrRomType::RomSst39sf040 => false,
            SdrrRomType::Rom28C16 => false,
            SdrrRomType::Rom28C64 => false,
            SdrrRomType::Rom28C256 => false,
            SdrrRomType::Rom28C512 => false,
            SdrrRomType::Rom23QL512 => false,
            SdrrRomType::Rom23QL384 => false,
            SdrrRomType::Rom23C1001 => false,
            SdrrRomType::Rom27C200 => false,
        }
    }
}

/// SDRR chip select active options
///
/// Reflects `sdrr_cs_state_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u8")]
pub enum SdrrCsState {
    /// Chip select line is active low
    #[deku(id = "0")]
    ActiveLow,

    /// Chip select line is active high
    #[deku(id = "1")]
    ActiveHigh,

    /// Chip select line is not used
    #[deku(id = "2")]
    NotUsed,
}

impl fmt::Display for SdrrCsState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdrrCsState::ActiveLow => write!(f, "Active Low"),
            SdrrCsState::ActiveHigh => write!(f, "Active High"),
            SdrrCsState::NotUsed => write!(f, "Not Used"),
        }
    }
}

/// SDRR serving algorithm options
///
/// Reflects `sdrr_serve_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u8")]
pub enum SdrrServe {
    /// Original algorithm - two CS checks for every address check, checks
    /// while CS is inactive as well as active
    #[deku(id = "0")]
    TwoCsOneAddr,

    /// Default algorithm from v0.2.1 as it is more performant in every case
    /// tested - checks address only on CS active, and at same frequency
    #[deku(id = "1")]
    AddrOnCs,

    /// Multi-ROM set algorithm - as `AddrOnCs`, but ROM is considered active
    /// when _any_ ROM's CS line is active
    #[deku(id = "2")]
    AddrOnAnyCs,
}

impl fmt::Display for SdrrServe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdrrServe::TwoCsOneAddr => write!(f, "A = Two CS checks for every address check"),
            SdrrServe::AddrOnCs => write!(f, "B = Check address only when CS active"),
            SdrrServe::AddrOnAnyCs => write!(f, "C = Check address on any CS active"),
        }
    }
}

/// SDRR STM32 port options
///
/// Reflects `sdrr_stm_port_t` from `sdrr/include/config_base.h`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize,
)]
#[deku(id_type = "u8")]
pub enum SdrrMcuPort {
    /// No port (pin set is not exposed/used)
    #[deku(id = "0x00")]
    None,

    /// Port A
    #[deku(id = "0x01")]
    PortA,

    /// Port B
    #[deku(id = "0x02")]
    PortB,

    /// Port C
    #[deku(id = "0x03")]
    PortC,

    /// Port D
    #[deku(id = "0x04")]
    PortD,

    /// Port 0 (RP2350 only)
    #[deku(id = "0x05")]
    Port0,
}

impl fmt::Display for SdrrMcuPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdrrMcuPort::None => write!(f, "None"),
            SdrrMcuPort::PortA => write!(f, "A"),
            SdrrMcuPort::PortB => write!(f, "B"),
            SdrrMcuPort::PortC => write!(f, "C"),
            SdrrMcuPort::PortD => write!(f, "D"),
            SdrrMcuPort::Port0 => write!(f, "0"),
        }
    }
}

// from u8
impl From<u8> for SdrrMcuPort {
    fn from(value: u8) -> Self {
        match value {
            0x00 => SdrrMcuPort::None,
            0x01 => SdrrMcuPort::PortA,
            0x02 => SdrrMcuPort::PortB,
            0x03 => SdrrMcuPort::PortC,
            0x04 => SdrrMcuPort::PortD,
            0x05 => SdrrMcuPort::Port0,
            _ => SdrrMcuPort::None,
        }
    }
}

/// Struct representing the state of the chip select (CS) lines in a given
/// logical address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdrrCsSet {
    /// The state of CS1 (1 = high, 0 = low)
    cs1: bool,

    /// The state of CS2 (1 = high, 0 = low) - 2332/2316 ROM types only
    cs2: Option<bool>,

    /// The state of CS3 (1 = high, 0 = low) - 2316 ROM type only
    cs3: Option<bool>,

    /// The state of X1 (1 = high, 0 = low) - rom sets only
    x1: Option<bool>,

    /// The state of X2 (1 = high, 0 = low) - rom sets only
    x2: Option<bool>,
}

impl SdrrCsSet {
    /// Constructs a new CS set
    pub fn new(
        cs1: bool,
        cs2: Option<bool>,
        cs3: Option<bool>,
        x1: Option<bool>,
        x2: Option<bool>,
    ) -> Self {
        Self {
            cs1,
            cs2,
            cs3,
            x1,
            x2,
        }
    }

    /// Returns the state of CS1
    pub fn cs1(&self) -> bool {
        self.cs1
    }

    /// Returns the state of CS2
    pub fn cs2(&self) -> Option<bool> {
        self.cs2
    }

    /// Returns the state of CS3
    pub fn cs3(&self) -> Option<bool> {
        self.cs3
    }

    /// Returns the state of X1
    pub fn x1(&self) -> Option<bool> {
        self.x1
    }

    /// Returns the state of X2
    pub fn x2(&self) -> Option<bool> {
        self.x2
    }
}

/// Struct representing the information required to construct a logical address
/// to access a byte in an SDRR rom set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdrrLogicalAddress {
    /// The logical address of the ROM to access - i.e the value of the address
    /// pins on the original ROM chip.
    addr: u32,

    /// The CS state associated with this address
    cs_set: SdrrCsSet,
}

impl SdrrLogicalAddress {
    /// Constructs a new logical address
    pub fn new(addr: u32, cs_set: SdrrCsSet) -> Self {
        Self { addr, cs_set }
    }

    /// Mangles the logical address into a raw address using the pin config from
    /// the given `SdrrInfo`.
    pub fn mangle(&self, info: &SdrrInfo) -> Result<u32, String> {
        info.mangle_address(self)
    }

    /// Returns the logical address pins state from the original ROM
    pub fn addr(&self) -> u32 {
        self.addr
    }

    /// Returns the CS set associated with this logical address
    pub fn cs_set(&self) -> &SdrrCsSet {
        &self.cs_set
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdrrAddress {
    /// Raw address - used to index directly into the ROM set
    Raw(u32),

    /// Logical address - the logical address on the original ROM, plus CS
    /// and X1/X2 lines
    Logical(SdrrLogicalAddress),
}

impl SdrrAddress {
    /// Creates a new raw address from the given value.
    pub fn from_raw(addr: u32) -> Self {
        Self::Raw(addr)
    }

    /// Creates a new logical address from the given components.
    pub fn from_logical(addr: u32, cs_set: &SdrrCsSet) -> Self {
        Self::Logical(SdrrLogicalAddress::new(addr, *cs_set))
    }
}

impl fmt::Display for SdrrAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                SdrrAddress::Raw(addr) => write!(f, "Raw address: 0x{:04X}", addr),
                SdrrAddress::Logical(logical) => write!(
                    f,
                    "Logical address: 0x{:04X} (CS1: {}, CS2: {:?}, CS3: {:?}, X1: {:?}, X2: {:?})",
                    logical.addr,
                    logical.cs_set.cs1(),
                    logical.cs_set.cs2(),
                    logical.cs_set.cs3(),
                    logical.cs_set.x1(),
                    logical.cs_set.x2()
                ),
            }
        } else {
            match self {
                SdrrAddress::Raw(addr) => write!(f, "0x{:04X}", addr),
                SdrrAddress::Logical(logical) => write!(f, "0x{:04X}", logical.addr),
            }
        }
    }
}

#[repr(u8)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, DekuRead, DekuWrite,
)]
#[deku(id_type = "u8")]
pub enum FireVreg {
    #[deku(id = "0x00")]
    V0_55 = 0x00,
    #[deku(id = "0x01")]
    V0_60 = 0x01,
    #[deku(id = "0x02")]
    V0_65 = 0x02,
    #[deku(id = "0x03")]
    V0_70 = 0x03,
    #[deku(id = "0x04")]
    V0_75 = 0x04,
    #[deku(id = "0x05")]
    V0_80 = 0x05,
    #[deku(id = "0x06")]
    V0_85 = 0x06,
    #[deku(id = "0x07")]
    V0_90 = 0x07,
    #[deku(id = "0x08")]
    V0_95 = 0x08,
    #[deku(id = "0x09")]
    V1_00 = 0x09,
    #[deku(id = "0x0A")]
    V1_05 = 0x0A,
    #[deku(id = "0x0B")]
    V1_10 = 0x0B,
    #[deku(id = "0x0C")]
    V1_15 = 0x0C,
    #[deku(id = "0x0D")]
    V1_20 = 0x0D,
    #[deku(id = "0x0E")]
    V1_25 = 0x0E,
    #[deku(id = "0x0F")]
    V1_30 = 0x0F,
    #[deku(id = "0x10")]
    V1_35 = 0x10,
    #[deku(id = "0x11")]
    V1_40 = 0x11,
    #[deku(id = "0x12")]
    V1_50 = 0x12,
    #[deku(id = "0x13")]
    V1_60 = 0x13,
    #[deku(id = "0x14")]
    V1_65 = 0x14,
    #[deku(id = "0x15")]
    V1_70 = 0x15,
    #[deku(id = "0x16")]
    V1_80 = 0x16,
    #[deku(id = "0x17")]
    V1_90 = 0x17,
    #[deku(id = "0x18")]
    V2_00 = 0x18,
    #[deku(id = "0x19")]
    V2_35 = 0x19,
    #[deku(id = "0x1A")]
    V2_50 = 0x1A,
    #[deku(id = "0x1B")]
    V2_65 = 0x1B,
    #[deku(id = "0x1C")]
    V2_80 = 0x1C,
    #[deku(id = "0x1D")]
    V3_00 = 0x1D,
    #[deku(id = "0x1E")]
    V3_15 = 0x1E,
    #[deku(id = "0x1F")]
    V3_30 = 0x1F,
    #[deku(id = "0xFE")]
    None = 0xFE,
    #[deku(id = "0xFF")]
    Stock = 0xFF,
}

/// Data bus width mode
#[repr(u8)]
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, DekuRead, DekuWrite,
)]
#[deku(id_type = "u8")]
pub enum BitMode {
    #[deku(id = "0x01")]
    Mode8 = 0x01,
    #[deku(id = "0x02")]
    Mode16 = 0x02,
}

/// Whether the RP2350 serves using the CPU or PIO
#[repr(u8)]
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, DekuRead, DekuWrite,
)]
#[deku(id_type = "u8")]
pub enum FireServeMode {
    #[deku(id = "0x00")]
    Cpu = 0x00,
    #[deku(id = "0x01")]
    Pio = 0x01,
}

/// Limp mode pattern
#[repr(u8)]
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, DekuRead, DekuWrite,
)]
#[deku(id_type = "u8")]
pub enum LimpMode {
    #[deku(id = "0x00")]
    None = 0x00,
    #[deku(id = "0x01")]
    NoRoms = 0x01,
    #[deku(id = "0x02")]
    InvalidConfig = 0x02,
    #[deku(id = "0x03")]
    InvalidBuild = 0x03,
}
