// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Contains Firmware Config objects

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// Required for custom schemas implementations
#[cfg(feature = "schemars")]
use alloc::borrow::Cow;
#[cfg(feature = "schemars")]
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

/// Top level configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FirmwareConfig {
    /// Optional Ice specific configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ice: Option<IceConfig>,

    /// Optional Fire specific configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire: Option<FireConfig>,

    /// Optional LED configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub led: Option<LedConfig>,

    /// Optional Debug configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swd: Option<DebugConfig>,

    /// Optional serving algorithm parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_alg_params: Option<ServeAlgParams>,
}

impl FirmwareConfig {
    /// Deserialize 64-byte onerom_firmware_overrides_t structure into FirmwareConfig
    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 64 {
            return Err(format!("Buffer too small: {} bytes", buf.len()));
        }

        let mut offset = 0;

        // Read override_present (8 bytes)
        let override_present = &buf[offset..offset + 8];
        offset += 8;

        // Read frequencies (2 bytes each as u16)
        let ice_freq = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
        offset += 2;
        let fire_freq = u16::from_le_bytes([buf[offset], buf[offset + 1]]);
        offset += 2;

        // Read fire_vreq (1 byte)
        let fire_vreq = buf[offset];
        offset += 1;

        // Skip pad1 (3 bytes)
        offset += 3;

        // Read override_value (8 bytes)
        let override_value = &buf[offset..offset + 8];
        // offset += 8; // Rest is padding

        // Reconstruct FirmwareConfig
        let ice_config =
            if ((override_present[0] & (1 << 0)) != 0) || ((override_present[0] & (1 << 1)) != 0) {
                let mut ice_config = IceConfig::default();
                if (override_present[0] & (1 << 0)) != 0 {
                    ice_config.cpu_freq = Some(
                        ice_freq
                            .try_into()
                            .map_err(|_| format!("Invalid ice_freq: {}", ice_freq))?,
                    );
                }
                if (override_present[0] & (1 << 1)) != 0 {
                    ice_config.overclock = Some((override_value[0] & (1 << 0)) != 0);
                }
                Some(ice_config)
            } else {
                None
            };

        let fire_config = if ((override_present[0] & (1 << 2)) != 0)
            || ((override_present[0] & (1 << 3)) != 0)
            || ((override_present[0] & (1 << 4)) != 0)
            || ((override_present[0] & (1 << 7)) != 0)
        {
            let mut fire_config = FireConfig::default();
            if (override_present[0] & (1 << 2)) != 0 {
                fire_config.cpu_freq = Some(
                    fire_freq
                        .try_into()
                        .map_err(|_| format!("Invalid fire_freq: {}", fire_freq))?,
                );
            }
            if (override_present[0] & (1 << 3)) != 0 {
                fire_config.overclock = Some((override_value[0] & (1 << 1)) != 0);
            }
            if (override_present[0] & (1 << 4)) != 0 {
                fire_config.vreg = Some(
                    fire_vreq
                        .try_into()
                        .map_err(|_| format!("Invalid fire_vreq: {}", fire_vreq))?,
                );
            }
            if (override_present[0] & (1 << 7)) != 0 {
                fire_config.serve_mode = Some(if (override_value[0] & (1 << 4)) != 0 {
                    FireServeMode::Pio
                } else {
                    FireServeMode::Cpu
                });
            }
            if (override_present[1] & (1 << 0)) != 0 {
                fire_config.rom_dma_preload = (override_value[0] & (1 << 5)) != 0;
            }
            if (override_present[1] & (1 << 1)) != 0 {
                fire_config.force_16_bit = (override_value[0] & (1 << 6)) != 0;
            }
            Some(fire_config)
        } else {
            None
        };

        let led = if (override_present[0] & (1 << 5)) != 0 {
            Some(LedConfig {
                enabled: (override_value[0] & (1 << 2)) != 0,
            })
        } else {
            None
        };

        let swd = if (override_present[0] & (1 << 6)) != 0 {
            Some(DebugConfig {
                swd_enabled: (override_value[0] & (1 << 3)) != 0,
            })
        } else {
            None
        };

        Ok(FirmwareConfig {
            ice: ice_config,
            fire: fire_config,
            led,
            swd,
            serve_alg_params: None, // Stored separately
        })
    }
}

/// Ice configuration structure
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct IceConfig {
    /// CPU frequency.  Only specific frequencies are supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_freq: Option<IceCpuFreq>,

    /// Whether overclocking is enabled
    #[serde(default)]
    #[serde(skip_serializing_if = "is_none_or_is_false")]
    pub overclock: Option<bool>,
}

fn is_none_or_is_false(v: &Option<bool>) -> bool {
    if let Some(val) = v { !val } else { true }
}

/// Fire configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FireConfig {
    /// CPU frequency.  Only specific frequencies are supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_freq: Option<FireCpuFreq>,

    /// Whether overclocking is enabled
    #[serde(default, skip_serializing_if = "is_none_or_is_false")]
    pub overclock: Option<bool>,

    /// Optional Vreg output voltage setting for RP2350 MCUs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vreg: Option<FireVreg>,

    /// Optional PIO/CPU override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_mode: Option<FireServeMode>,

    /// Optional DMA ROM preload enable/disable
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub rom_dma_preload: bool,

    /// Optional Force 16 bit mode.  Only supported on One ROM 40, and if set
    /// this _disables_ of the /BYTE pin to indicate 8-bit mode, forcing the
    /// ROM to always operate in 16-bit mode.  This is a higher performance
    /// mode, as the algorithm can read the address lines 33% more frequently,
    /// but obviously disables the used of 8-bit mode.
    #[serde(default, skip_serializing_if = "is_false")]
    pub force_16_bit: bool,
}

fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}

impl Default for FireConfig {
    fn default() -> Self {
        Self {
            cpu_freq: None,
            overclock: None,
            vreg: None,
            serve_mode: None,
            rom_dma_preload: true,
            force_16_bit: false,
        }
    }
}

/// Fire serve mode
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FireServeMode {
    /// PIO mode
    Pio,
    /// CPU mode
    Cpu,
}

impl core::fmt::Display for FireServeMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FireServeMode::Pio => write!(f, "PIO"),
            FireServeMode::Cpu => write!(f, "CPU"),
        }
    }
}

/// LED configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LedConfig {
    /// Whether the status LED is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Debug configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DebugConfig {
    /// Whether SWD debug interface is enabled
    #[serde(default = "default_true")]
    pub swd_enabled: bool,
}

/// Custom serving algorithm parameters
///
/// This is stored as unstructured parameters to allow for easy future
/// extension without breaking compatibility.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ServeAlgParams {
    pub params: Vec<u8>,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceCpuFreq(u16);

impl IceCpuFreq {
    pub const NONE: u16 = 0;
    pub const STOCK: u16 = 0xFFFF;
    pub const MIN_MHZ: u16 = 1;
    pub const MAX_MHZ: u16 = 450;

    pub fn none() -> Self {
        Self(Self::NONE)
    }

    pub fn stock() -> Self {
        Self(Self::STOCK)
    }

    pub fn mhz(freq: u16) -> Result<Self, InvalidFreq> {
        if (Self::MIN_MHZ..=Self::MAX_MHZ).contains(&freq) {
            Ok(Self(freq))
        } else {
            Err(InvalidFreq(freq))
        }
    }

    pub fn is_none(&self) -> bool {
        self.0 == Self::NONE
    }

    pub fn is_stock(&self) -> bool {
        self.0 == Self::STOCK
    }

    pub fn get(&self) -> u16 {
        self.0
    }
}

impl Default for IceCpuFreq {
    fn default() -> Self {
        Self::stock()
    }
}

impl TryFrom<u16> for IceCpuFreq {
    type Error = InvalidFreq;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            Self::NONE => Ok(Self::none()),
            Self::STOCK => Ok(Self::stock()),
            freq if (Self::MIN_MHZ..=Self::MAX_MHZ).contains(&freq) => Ok(Self(freq)),
            _ => Err(InvalidFreq(value)),
        }
    }
}

impl From<IceCpuFreq> for u16 {
    fn from(freq: IceCpuFreq) -> u16 {
        freq.0
    }
}

impl serde::Serialize for IceCpuFreq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self.0 {
            Self::NONE => "None".to_string(),
            Self::STOCK => "Stock".to_string(),
            freq => format!("{}MHz", freq),
        };
        serializer.serialize_str(&s)
    }
}

impl<'de> serde::Deserialize<'de> for IceCpuFreq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        match s.as_str() {
            "None" => Ok(Self::none()),
            "Stock" => Ok(Self::stock()),
            _ => {
                if let Some(freq_str) = s.strip_suffix("MHz") {
                    let freq = freq_str.parse::<u16>().map_err(|_| {
                        serde::de::Error::custom(format!("Invalid frequency: {}", s))
                    })?;
                    Self::mhz(freq).map_err(|_| {
                        serde::de::Error::custom(format!(
                            "Frequency must be between {}MHz and {}MHz",
                            Self::MIN_MHZ,
                            Self::MAX_MHZ
                        ))
                    })
                } else {
                    Err(serde::de::Error::custom(format!(
                        "Invalid frequency format: {}",
                        s
                    )))
                }
            }
        }
    }
}

#[cfg(feature = "schemars")]
impl JsonSchema for IceCpuFreq {
    fn schema_name() -> Cow<'static, str> {
        "IceCpuFreq".into()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": format!(
                "CPU frequency: 'None', 'Stock', or '{{n}}MHz' where n is {}-{}",
                Self::MIN_MHZ,
                Self::MAX_MHZ
            )
        })
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireCpuFreq(u16);

impl FireCpuFreq {
    pub const NONE: u16 = 0;
    pub const STOCK: u16 = 0xFFFF;
    pub const MIN_MHZ: u16 = 16;
    pub const MAX_MHZ: u16 = 800;

    pub fn none() -> Self {
        Self(Self::NONE)
    }

    pub fn stock() -> Self {
        Self(Self::STOCK)
    }

    pub fn mhz(freq: u16) -> Result<Self, InvalidFreq> {
        if (Self::MIN_MHZ..=Self::MAX_MHZ).contains(&freq) {
            Ok(Self(freq))
        } else {
            Err(InvalidFreq(freq))
        }
    }

    pub fn is_none(&self) -> bool {
        self.0 == Self::NONE
    }

    pub fn is_stock(&self) -> bool {
        self.0 == Self::STOCK
    }

    pub fn get(&self) -> u16 {
        self.0
    }

    pub fn stock_value() -> Self {
        Self(150u16)
    }
}

impl PartialOrd for FireCpuFreq {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.is_stock() || self.is_none() || other.is_stock() || other.is_none() {
            return None;
        }
        self.0.partial_cmp(&other.0)
    }
}

impl Default for FireCpuFreq {
    fn default() -> Self {
        Self::stock()
    }
}

impl TryFrom<u16> for FireCpuFreq {
    type Error = InvalidFreq;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            Self::NONE => Ok(Self::none()),
            Self::STOCK => Ok(Self::stock()),
            freq if (Self::MIN_MHZ..=Self::MAX_MHZ).contains(&freq) => Ok(Self(freq)),
            _ => Err(InvalidFreq(value)),
        }
    }
}

impl core::fmt::Display for FireCpuFreq {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Self::NONE => write!(f, "None"),
            Self::STOCK => write!(f, "Stock"),
            freq => write!(f, "{}MHz", freq),
        }
    }
}

#[derive(Debug)]
pub struct InvalidFreq(u16);

impl core::fmt::Display for InvalidFreq {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Invalid frequency: {}MHz (must be {}-{}MHz, 0 (None), or 0xFFFF (Stock))",
            self.0,
            FireCpuFreq::MIN_MHZ,
            FireCpuFreq::MAX_MHZ
        )
    }
}

impl serde::Serialize for FireCpuFreq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self.0 {
            Self::NONE => "None".to_string(),
            Self::STOCK => "Stock".to_string(),
            freq => format!("{}MHz", freq),
        };
        serializer.serialize_str(&s)
    }
}

impl<'de> serde::Deserialize<'de> for FireCpuFreq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        match s.as_str() {
            "None" => Ok(Self::none()),
            "Stock" => Ok(Self::stock()),
            _ => {
                if let Some(freq_str) = s.strip_suffix("MHz") {
                    let freq = freq_str.parse::<u16>().map_err(|_| {
                        serde::de::Error::custom(format!("Invalid frequency: {}", s))
                    })?;
                    Self::mhz(freq).map_err(|_| {
                        serde::de::Error::custom(format!(
                            "Frequency must be between {}MHz and {}MHz",
                            Self::MIN_MHZ,
                            Self::MAX_MHZ
                        ))
                    })
                } else {
                    Err(serde::de::Error::custom(format!(
                        "Invalid frequency format: {}",
                        s
                    )))
                }
            }
        }
    }
}

#[cfg(feature = "schemars")]
impl JsonSchema for FireCpuFreq {
    fn schema_name() -> Cow<'static, str> {
        "FireCpuFreq".into()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "description": format!(
                "CPU frequency: 'None', 'Stock', or '{{n}}MHz' where n is {}-{}",
                Self::MIN_MHZ,
                Self::MAX_MHZ
            )
        })
    }
}

/// Voltage regulator setting for RP2350 MCUs
#[repr(u8)]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FireVreg {
    #[serde(rename = "0.55V")]
    V0_55 = 0x00,
    #[serde(rename = "0.60V")]
    V0_60 = 0x01,
    #[serde(rename = "0.65V")]
    V0_65 = 0x02,
    #[serde(rename = "0.70V")]
    V0_70 = 0x03,
    #[serde(rename = "0.75V")]
    V0_75 = 0x04,
    #[serde(rename = "0.80V")]
    V0_80 = 0x05,
    #[serde(rename = "0.85V")]
    V0_85 = 0x06,
    #[serde(rename = "0.90V")]
    V0_90 = 0x07,
    #[serde(rename = "0.95V")]
    V0_95 = 0x08,
    #[serde(rename = "1.00V")]
    V1_00 = 0x09,
    #[serde(rename = "1.05V")]
    V1_05 = 0x0A,
    #[serde(rename = "1.10V")]
    V1_10 = 0x0B,
    #[serde(rename = "1.15V")]
    V1_15 = 0x0C,
    #[serde(rename = "1.20V")]
    V1_20 = 0x0D,
    #[serde(rename = "1.25V")]
    V1_25 = 0x0E,
    #[serde(rename = "1.30V")]
    V1_30 = 0x0F,
    #[serde(rename = "1.35V")]
    V1_35 = 0x10,
    #[serde(rename = "1.40V")]
    V1_40 = 0x11,
    #[serde(rename = "1.50V")]
    V1_50 = 0x12,
    #[serde(rename = "1.60V")]
    V1_60 = 0x13,
    #[serde(rename = "1.65V")]
    V1_65 = 0x14,
    #[serde(rename = "1.70V")]
    V1_70 = 0x15,
    #[serde(rename = "1.80V")]
    V1_80 = 0x16,
    #[serde(rename = "1.90V")]
    V1_90 = 0x17,
    #[serde(rename = "2.00V")]
    V2_00 = 0x18,
    #[serde(rename = "2.35V")]
    V2_35 = 0x19,
    #[serde(rename = "2.50V")]
    V2_50 = 0x1A,
    #[serde(rename = "2.65V")]
    V2_65 = 0x1B,
    #[serde(rename = "2.80V")]
    V2_80 = 0x1C,
    #[serde(rename = "3.00V")]
    V3_00 = 0x1D,
    #[serde(rename = "3.15V")]
    V3_15 = 0x1E,
    #[serde(rename = "3.30V")]
    V3_30 = 0x1F,
    #[default]
    Stock = 0xFF,
}

impl TryFrom<u8> for FireVreg {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::V0_55),
            0x01 => Ok(Self::V0_60),
            0x02 => Ok(Self::V0_65),
            0x03 => Ok(Self::V0_70),
            0x04 => Ok(Self::V0_75),
            0x05 => Ok(Self::V0_80),
            0x06 => Ok(Self::V0_85),
            0x07 => Ok(Self::V0_90),
            0x08 => Ok(Self::V0_95),
            0x09 => Ok(Self::V1_00),
            0x0A => Ok(Self::V1_05),
            0x0B => Ok(Self::V1_10),
            0x0C => Ok(Self::V1_15),
            0x0D => Ok(Self::V1_20),
            0x0E => Ok(Self::V1_25),
            0x0F => Ok(Self::V1_30),
            0x10 => Ok(Self::V1_35),
            0x11 => Ok(Self::V1_40),
            0x12 => Ok(Self::V1_50),
            0x13 => Ok(Self::V1_60),
            0x14 => Ok(Self::V1_65),
            0x15 => Ok(Self::V1_70),
            0x16 => Ok(Self::V1_80),
            0x17 => Ok(Self::V1_90),
            0x18 => Ok(Self::V2_00),
            0x19 => Ok(Self::V2_35),
            0x1A => Ok(Self::V2_50),
            0x1B => Ok(Self::V2_65),
            0x1C => Ok(Self::V2_80),
            0x1D => Ok(Self::V3_00),
            0x1E => Ok(Self::V3_15),
            0x1F => Ok(Self::V3_30),
            0xFF => Ok(Self::Stock),
            _ => Err(value),
        }
    }
}

impl core::fmt::Display for FireVreg {
    #[allow(clippy::wildcard_enum_match_arm)]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Stock => write!(f, "Stock"),
            _ => {
                let s = serde_json::to_string(self).map_err(|_| core::fmt::Error)?;
                write!(f, "{}", s.trim_matches('"'))
            }
        }
    }
}

impl FireVreg {
    pub fn is_stock(&self) -> bool {
        matches!(self, Self::Stock)
    }

    pub fn stock_value() -> Self {
        Self::V1_10
    }

    pub fn supported_levels() -> &'static [Self] {
        &[
            Self::V0_55,
            Self::V0_60,
            Self::V0_65,
            Self::V0_70,
            Self::V0_75,
            Self::V0_80,
            Self::V0_85,
            Self::V0_90,
            Self::V0_95,
            Self::V1_00,
            Self::V1_05,
            Self::V1_10,
            Self::V1_15,
            Self::V1_20,
            Self::V1_25,
            Self::V1_30,
            Self::V1_35,
            Self::V1_40,
            Self::V1_50,
            Self::V1_60,
            Self::V1_65,
            Self::V1_70,
            Self::V1_80,
            Self::V1_90,
            Self::V2_00,
            Self::V2_35,
            Self::V2_50,
            Self::V2_65,
            Self::V2_80,
            Self::V3_00,
            Self::V3_15,
            Self::V3_30,
        ]
    }
}

impl PartialOrd for FireVreg {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.is_stock() || other.is_stock() {
            return None;
        }
        (self.clone() as u8).partial_cmp(&(other.clone() as u8))
    }
}
