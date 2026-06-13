// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Firmware objects

use core::fmt::Display;

use crate::Error;
use crate::hw::Board;
use crate::mcu::Variant as McuVariant;

/// Represents a One ROM Firmware Version
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FirmwareVersion {
    major: u16,
    minor: u16,
    patch: u16,
    build: u16,
}

impl Display for FirmwareVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            write!(
                f,
                "v{}.{}.{}.{}",
                self.major, self.minor, self.patch, self.build
            )
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch,)
        }
    }
}

impl FirmwareVersion {
    /// Create a new firmware version
    pub const fn new(major: u16, minor: u16, patch: u16, build: u16) -> Self {
        Self {
            major,
            minor,
            patch,
            build,
        }
    }

    pub const fn min() -> Self {
        Self::new(0, 0, 0, 0)
    }

    /// Whether this version matches another version, ignoring build number.
    pub fn matches_release(&self, other: &Self) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }

    /// Get the major version
    pub const fn major(&self) -> u16 {
        self.major
    }

    /// Get the minor version
    pub const fn minor(&self) -> u16 {
        self.minor
    }

    /// Get the patch version
    pub const fn patch(&self) -> u16 {
        self.patch
    }

    /// Get the build version
    pub const fn build(&self) -> u16 {
        self.build
    }

    // Create from a string like "1.2.3" or "v1.2.3.4"
    pub fn try_from_str(s: &str) -> Result<Self, Error> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.');

        let major = parts
            .next()
            .ok_or(Error::InvalidFirmwareVersion)?
            .parse::<u16>()
            .map_err(|_| Error::InvalidFirmwareVersion)?;

        let minor = parts
            .next()
            .ok_or(Error::InvalidFirmwareVersion)?
            .parse::<u16>()
            .map_err(|_| Error::InvalidFirmwareVersion)?;

        let patch = parts
            .next()
            .ok_or(Error::InvalidFirmwareVersion)?
            .parse::<u16>()
            .map_err(|_| Error::InvalidFirmwareVersion)?;

        let build = match parts.next() {
            Some(s) => s
                .parse::<u16>()
                .map_err(|_| Error::InvalidFirmwareVersion)?,
            None => 0,
        };

        // Ensure no extra parts
        if parts.next().is_some() {
            return Err(Error::InvalidFirmwareVersion);
        }

        Ok(Self::new(major, minor, patch, build))
    }
}

/// Chip serving algorithm.  Only valid for firmwares serving via CPU.  By
/// default this is Ice boards and Fire 24 A/B boards.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServeAlg {
    /// default
    #[default]
    Default,

    /// a
    TwoCsOneAddr,

    /// b
    AddrOnCs,

    /// Multi-ROM set only
    AddrOnAnyCs,
}

impl Display for ServeAlg {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ServeAlg::Default => write!(f, "default"),
            ServeAlg::TwoCsOneAddr => write!(f, "two_cs_one_addr"),
            ServeAlg::AddrOnCs => write!(f, "addr_on_cs"),
            ServeAlg::AddrOnAnyCs => write!(f, "addr_on_any_cs"),
        }
    }
}

impl ServeAlg {
    pub fn try_from_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("default") {
            Some(ServeAlg::Default)
        } else if s.eq_ignore_ascii_case("a") || s.eq_ignore_ascii_case("two_cs_one_addr") {
            Some(ServeAlg::TwoCsOneAddr)
        } else if s.eq_ignore_ascii_case("b") {
            Some(ServeAlg::AddrOnCs)
        } else {
            None
        }
    }

    pub fn c_value(&self) -> &str {
        match self {
            ServeAlg::TwoCsOneAddr => "SERVE_TWO_CS_ONE_ADDR",
            ServeAlg::Default => "SERVE_ADDR_ON_CS",
            ServeAlg::AddrOnCs => "SERVE_ADDR_ON_CS",
            ServeAlg::AddrOnAnyCs => "SERVE_ADDR_ON_ANY_CS",
        }
    }

    pub fn c_value_multi_rom_set(&self) -> &str {
        Self::AddrOnAnyCs.c_value()
    }

    pub fn c_enum_value(&self) -> u8 {
        match self {
            ServeAlg::TwoCsOneAddr => 0,
            ServeAlg::Default | ServeAlg::AddrOnCs => 1,
            ServeAlg::AddrOnAnyCs => 2,
        }
    }
}

/// One ROM Firmware Properties
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct FirmwareProperties {
    version: FirmwareVersion,
    board: Board,
    mcu_variant: McuVariant,
    serve_alg: ServeAlg,
    boot_logging: bool,
}

impl FirmwareProperties {
    /// Create a new firmware properties object
    pub fn new(
        version: FirmwareVersion,
        board: Board,
        mcu_variant: McuVariant,
        serve_alg: ServeAlg,
        boot_logging: bool,
    ) -> Result<Self, Error> {
        let mcu_family = mcu_variant.family();
        let board_mcu_family = board.mcu_family();
        if mcu_family != board_mcu_family {
            return Err(Error::InvalidMcuVariant {
                variant: mcu_variant,
            });
        }
        Ok(Self {
            version,
            board,
            mcu_variant,
            serve_alg,
            boot_logging,
        })
    }

    /// Get the firmware version
    pub const fn version(&self) -> FirmwareVersion {
        self.version
    }

    /// Get the board
    pub const fn board(&self) -> Board {
        self.board
    }

    /// Get the ROM serving algorithm
    pub const fn serve_alg(&self) -> ServeAlg {
        self.serve_alg
    }

    /// Does this firmware support boot logging?
    pub const fn boot_logging(&self) -> bool {
        self.boot_logging
    }

    /// Get the MCU variant
    pub const fn mcu_variant(&self) -> McuVariant {
        self.mcu_variant
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;

    #[test]
    fn test_version_equality() {
        let v1 = FirmwareVersion::new(1, 2, 3, 4);
        let v2 = FirmwareVersion::new(1, 2, 3, 4);
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_major_version_comparison() {
        let v1 = FirmwareVersion::new(1, 0, 0, 0);
        let v2 = FirmwareVersion::new(2, 0, 0, 0);
        assert!(v1 < v2);
        assert!(v2 > v1);
    }

    #[test]
    fn test_minor_version_comparison() {
        let v1 = FirmwareVersion::new(0, 1, 0, 0);
        let v2 = FirmwareVersion::new(0, 2, 0, 0);
        assert!(v1 < v2);
        assert!(v2 > v1);
    }

    #[test]
    fn test_patch_version_comparison() {
        let v1 = FirmwareVersion::new(1, 2, 3, 0);
        let v2 = FirmwareVersion::new(1, 2, 4, 0);
        assert!(v1 < v2);
        assert!(v2 > v1);
    }

    #[test]
    fn test_build_version_comparison() {
        let v1 = FirmwareVersion::new(1, 2, 3, 100);
        let v2 = FirmwareVersion::new(1, 2, 3, 200);
        assert!(v1 < v2);
        assert!(v2 > v1);
    }

    #[test]
    fn test_major_overrides_minor() {
        let v1 = FirmwareVersion::new(1, 99, 99, 99);
        let v2 = FirmwareVersion::new(2, 0, 0, 0);
        assert!(v1 < v2);
    }

    #[test]
    fn test_minor_overrides_patch() {
        let v1 = FirmwareVersion::new(1, 1, 99, 99);
        let v2 = FirmwareVersion::new(1, 2, 0, 0);
        assert!(v1 < v2);
    }

    #[test]
    fn test_patch_overrides_build() {
        let v1 = FirmwareVersion::new(1, 2, 3, 999);
        let v2 = FirmwareVersion::new(1, 2, 4, 0);
        assert!(v1 < v2);
    }

    #[test]
    fn test_version_ordering() {
        let mut versions = [
            FirmwareVersion::new(2, 0, 0, 0),
            FirmwareVersion::new(1, 2, 3, 4),
            FirmwareVersion::new(1, 2, 3, 5),
            FirmwareVersion::new(1, 3, 0, 0),
            FirmwareVersion::new(1, 2, 4, 0),
        ];

        versions.sort();

        assert_eq!(versions[0], FirmwareVersion::new(1, 2, 3, 4));
        assert_eq!(versions[1], FirmwareVersion::new(1, 2, 3, 5));
        assert_eq!(versions[2], FirmwareVersion::new(1, 2, 4, 0));
        assert_eq!(versions[3], FirmwareVersion::new(1, 3, 0, 0));
        assert_eq!(versions[4], FirmwareVersion::new(2, 0, 0, 0));
    }
}
