// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

const MAX_ADDRESS_LINES: usize = 20;
const VALID_DATA_LINE_COUNTS: &[usize] = &[8, 16];
const MIN_PIN_NUMBER: u8 = 1;
const VALID_PIN_COUNTS: &[u8] = &[24, 28, 32, 40];
const VALID_READ_STATES: &[&str] = &["vcc", "high", "low", "chip_select", "x", "word_size"];
const VALID_CONTROL_LINES: &[&str] = &[
    "cs1", "cs2", "cs3", "cs4", "ce", "oe", "byte", "write", "busy",
];

/// Chip select lines.
///
/// A CS line's polarity may be mask-programmed at manufacture
/// (`configurable`, e.g. the 23xx series), in which case it is supplied by
/// the user's ChipConfig; or fixed by the silicon (`fixed_active_low` /
/// `fixed_active_high`, e.g. the HM7641), in which case the user has no say
/// in it.  A chip type may mix the two across its CS lines.
const CS_CONTROL_LINES: &[&str] = &["cs1", "cs2", "cs3", "cs4"];

/// Control lines which are always active low.
///
/// Unlike the CS lines, the `ce`/`oe` names denote the JEDEC-standard enables
/// of the 27xx/28xx families and so carry their polarity: `fixed_active_low`
/// is the only valid type for them.  A part whose enables are active high, or
/// whose enables differ in polarity, uses CS lines instead.
const FIXED_ACTIVE_LOW_CONTROL_LINES: &[&str] = &["ce", "oe"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlLineType {
    Configurable,
    FixedActiveLow,
    FixedActiveHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlLine {
    pub pin: u8,
    #[serde(rename = "type")]
    pub line_type: ControlLineType,
    /// Whether this line may be set to Ignore in a ChipConfig without the
    /// explicit allow_cs_ignore flag.  Set only for lines where the chip
    /// datasheet explicitly defines a don't-care state (e.g. 23C1001 cs1/cs2).
    #[serde(default)]
    pub allow_ignore: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgrammingPin {
    pub pin: u8,
    pub read_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgrammingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpp: Option<ProgrammingPin>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pgm: Option<ProgrammingPin>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pe: Option<ProgrammingPin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerPin {
    pub name: String,
    pub pin: u8,
    pub voltage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
pub enum ChipFunction {
    #[serde(rename = "ROM")]
    Rom,
    #[serde(rename = "RAM")]
    Ram,
    Plugin,
}

impl ChipFunction {
    pub fn is_plugin(&self) -> bool {
        matches!(self, ChipFunction::Plugin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipType {
    pub description: String,
    #[serde(default)]
    pub supported: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub function: ChipFunction,

    /// RBCP wire protocol chip type value.  Must match the corresponding
    /// `onerom_rom_type_t` enum variant in the firmware metadata schema.
    pub rbcp_chip_type: u8,

    /// If true, this chip type shares its rbcp_chip_type value with another
    /// chip type of which it is an electrical alias.  Alias chips participate
    /// in rbcp_chip_type() (forward lookup) but are excluded from
    /// try_from_rbcp_u8() (reverse lookup) so that the canonical chip type
    /// is always returned.
    #[serde(default)]
    pub rbcp_alias: bool,

    pub bit_modes: Vec<u8>,
    pub pins: u8,

    pub size: usize,
    pub address: Vec<u8>,
    pub data: Vec<u8>,
    pub control: BTreeMap<String, ControlLine>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub programming: Option<ProgrammingConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<Vec<PowerPin>>,
    pub deselect_when_address_all_high: Option<Vec<u8>>,

    /// Permit this chip type to have both configurable CS lines (cs1/cs2/cs3)
    /// and fixed CE/OE lines simultaneously in its control map.  Most chip
    /// types are either CS-style or CE/OE-style; only chips like 23C1001 are
    /// both.  Replaces the old per-chip-name special case in validation.
    #[serde(default)]
    pub allow_mixed_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipTypesConfig {
    pub chip_types: BTreeMap<String, ChipType>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    JsonParseError(String),
    InvalidPinNumber {
        chip_type: String,
        pin: u8,
        max: u8,
    },
    AddressSizeMismatch {
        chip_type: String,
        address_lines: usize,
        expected_size: usize,
        actual_size: usize,
    },
    InvalidDataLineCount {
        chip_type: String,
        count: usize,
    },
    DuplicatePin {
        chip_type: String,
        pin: u8,
    },
    InvalidReadState {
        chip_type: String,
        pin_name: String,
        state: String,
    },
    InvalidPackagePinCount {
        chip_type: String,
        pins: u8,
    },
    TooManyAddressLines {
        chip_type: String,
        count: usize,
    },
    IncompatibleControlLines {
        chip_type: String,
        combination: String,
    },
    UnknownControlLine {
        chip_type: String,
        line_name: String,
    },
    /// A control line was declared with a polarity type its name does not
    /// permit - for example a `cs1` line declared `fixed_active_low`, or an
    /// `oe` line declared `fixed_active_high`.
    InvalidControlLinePolarity {
        chip_type: String,
        line_name: String,
        expected: &'static [&'static str],
    },
    /// Two non-alias chip types share the same rbcp_chip_type value.
    DuplicateRbcpChipType {
        chip_type_a: String,
        chip_type_b: String,
        value: u8,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::JsonParseError(msg) => {
                write!(f, "JSON parse error: {}", msg)
            }
            ValidationError::InvalidPinNumber {
                chip_type,
                pin,
                max,
            } => {
                write!(
                    f,
                    "ROM type '{}': pin {} is out of range (valid: {}-{})",
                    chip_type, pin, MIN_PIN_NUMBER, max
                )
            }
            ValidationError::AddressSizeMismatch {
                chip_type,
                address_lines,
                expected_size,
                actual_size,
            } => {
                write!(
                    f,
                    "ROM type '{}': {} address lines should give {} bytes, but size is {}",
                    chip_type, address_lines, expected_size, actual_size
                )
            }

            ValidationError::InvalidDataLineCount { chip_type, count } => {
                write!(
                    f,
                    "ROM type '{}': must have one of the valid data line counts {:?}, found {}",
                    chip_type, VALID_DATA_LINE_COUNTS, count
                )
            }
            ValidationError::DuplicatePin { chip_type, pin } => {
                write!(
                    f,
                    "ROM type '{}': pin {} is used multiple times",
                    chip_type, pin
                )
            }
            ValidationError::InvalidReadState {
                chip_type,
                pin_name,
                state,
            } => {
                write!(
                    f,
                    "ROM type '{}': invalid read state '{}' for pin '{}' (valid: {:?})",
                    chip_type, state, pin_name, VALID_READ_STATES
                )
            }
            ValidationError::InvalidPackagePinCount { chip_type, pins } => {
                write!(
                    f,
                    "ROM type '{}': invalid pin count {} (valid: {:?})",
                    chip_type, pins, VALID_PIN_COUNTS
                )
            }
            ValidationError::TooManyAddressLines { chip_type, count } => {
                write!(
                    f,
                    "ROM type '{}': {} address lines exceeds maximum of {}",
                    chip_type, count, MAX_ADDRESS_LINES
                )
            }
            ValidationError::IncompatibleControlLines {
                chip_type,
                combination,
            } => {
                let cs = CS_CONTROL_LINES.join("/");
                let ce_oe = FIXED_ACTIVE_LOW_CONTROL_LINES.join("/");
                write!(
                    f,
                    "ROM type '{}': incompatible chip select line combination: {}.\n\
                     CS lines ({cs}) cannot be used with the JEDEC enables ({ce_oe}) \
                     unless 'allow_mixed_control' is set.",
                    chip_type, combination
                )
            }
            ValidationError::UnknownControlLine {
                chip_type,
                line_name,
            } => {
                let valid_lines = VALID_CONTROL_LINES.join(", ");
                write!(
                    f,
                    "ROM type '{}': unrecognised control line name '{}'.\nValid names are: {valid_lines}",
                    chip_type, line_name
                )
            }
            ValidationError::InvalidControlLinePolarity {
                chip_type,
                line_name,
                expected,
            } => {
                write!(
                    f,
                    "ROM type '{}': control line '{}' has an invalid type (valid: {:?})",
                    chip_type, line_name, expected
                )
            }
            ValidationError::DuplicateRbcpChipType {
                chip_type_a,
                chip_type_b,
                value,
            } => {
                write!(
                    f,
                    "ROM types '{}' and '{}' share rbcp_chip_type value {} (0x{:02X}); \
                     mark one as 'rbcp_alias: true' if they are electrically equivalent",
                    chip_type_a, chip_type_b, value, value
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

impl ChipTypesConfig {
    pub fn from_json(json: &str) -> Result<Self, ValidationError> {
        let config: ChipTypesConfig = serde_json::from_str(json)
            .map_err(|e| ValidationError::JsonParseError(e.to_string()))?;

        config.validate()?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        for (type_name, chip_type) in &self.chip_types {
            chip_type.validate(type_name)?;
        }

        // Global uniqueness check: no two non-alias chips may share an rbcp_chip_type value.
        // BTreeMap iteration is alphabetical, giving deterministic error messages.
        let mut seen: BTreeMap<u8, &str> = BTreeMap::new();
        for (type_name, chip_type) in &self.chip_types {
            if chip_type.rbcp_alias {
                continue;
            }
            if let Some(existing) = seen.get(&chip_type.rbcp_chip_type) {
                return Err(ValidationError::DuplicateRbcpChipType {
                    chip_type_a: existing.to_string(),
                    chip_type_b: type_name.to_string(),
                    value: chip_type.rbcp_chip_type,
                });
            }
            seen.insert(chip_type.rbcp_chip_type, type_name);
        }

        Ok(())
    }
}

impl ChipType {
    fn validate_plugin_type(&self, type_name: &str) -> Result<(), ValidationError> {
        if self.size != 65536 {
            return Err(ValidationError::AddressSizeMismatch {
                chip_type: type_name.to_string(),
                address_lines: self.address.len(),
                expected_size: 65536,
                actual_size: self.size,
            });
        }
        if !self.bit_modes.is_empty() {
            return Err(ValidationError::InvalidDataLineCount {
                chip_type: type_name.to_string(),
                count: self.bit_modes.len(),
            });
        }
        if self.pins != 0 {
            return Err(ValidationError::InvalidPackagePinCount {
                chip_type: type_name.to_string(),
                pins: self.pins,
            });
        }
        if !self.address.is_empty() {
            return Err(ValidationError::TooManyAddressLines {
                chip_type: type_name.to_string(),
                count: self.address.len(),
            });
        }
        if !self.data.is_empty() {
            return Err(ValidationError::InvalidDataLineCount {
                chip_type: type_name.to_string(),
                count: self.data.len(),
            });
        }
        if !self.control.is_empty() {
            return Err(ValidationError::UnknownControlLine {
                chip_type: type_name.to_string(),
                line_name: "control lines should be empty".to_string(),
            });
        }
        Ok(())
    }

    pub fn validate(&self, type_name: &str) -> Result<(), ValidationError> {
        if self.function.is_plugin() {
            return self.validate_plugin_type(type_name);
        }

        if !VALID_PIN_COUNTS.contains(&self.pins) {
            return Err(ValidationError::InvalidPackagePinCount {
                chip_type: type_name.to_string(),
                pins: self.pins,
            });
        }

        if self.address.len() > MAX_ADDRESS_LINES {
            return Err(ValidationError::TooManyAddressLines {
                chip_type: type_name.to_string(),
                count: self.address.len(),
            });
        }

        let expected_size = if type_name != "23QL384" {
            1usize << self.address.len()
        } else {
            49152
        };

        if expected_size != self.size {
            return Err(ValidationError::AddressSizeMismatch {
                chip_type: type_name.to_string(),
                address_lines: self.address.len(),
                expected_size,
                actual_size: self.size,
            });
        }

        if !VALID_DATA_LINE_COUNTS.contains(&self.data.len()) {
            return Err(ValidationError::InvalidDataLineCount {
                chip_type: type_name.to_string(),
                count: self.data.len(),
            });
        }

        let mut used_pins = Vec::new();

        for &pin in &self.address {
            self.validate_pin_number(type_name, pin)?;
            self.check_duplicate_pin(type_name, pin, &mut used_pins)?;
        }

        for &pin in &self.data {
            self.validate_pin_number(type_name, pin)?;
            match self.check_duplicate_pin(type_name, pin, &mut used_pins) {
                Ok(_) => {}
                Err(e) => {
                    if self.pins == 40 {
                        // In 40-pin packages, data pins can overlap with address pins.
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        for control in self.control.values() {
            self.validate_pin_number(type_name, control.pin)?;
            self.check_duplicate_pin(type_name, control.pin, &mut used_pins)?;
        }

        if let Some(ref prog) = self.programming {
            if let Some(ref vpp) = prog.vpp {
                self.validate_pin_number(type_name, vpp.pin)?;
                self.validate_read_state(type_name, "vpp", &vpp.read_state)?;
                // Don't check duplicates - programming pins can overlap
            }
            if let Some(ref pgm) = prog.pgm {
                self.validate_pin_number(type_name, pgm.pin)?;
                self.validate_read_state(type_name, "pgm", &pgm.read_state)?;
                // Don't check duplicates
            }
            if let Some(ref pe) = prog.pe {
                self.validate_pin_number(type_name, pe.pin)?;
                self.validate_read_state(type_name, "pe", &pe.read_state)?;
                // Don't check duplicates
            }
        }

        // Validate power pins
        if let Some(ref power_pins) = self.power {
            for power_pin in power_pins {
                self.validate_pin_number(type_name, power_pin.pin)?;
                self.check_duplicate_pin(type_name, power_pin.pin, &mut used_pins)?;
            }
        }

        for (line_name, control) in &self.control {
            let line_name = line_name.as_str();

            // Check for unrecognised chip select line names.
            if !VALID_CONTROL_LINES.contains(&line_name) {
                return Err(ValidationError::UnknownControlLine {
                    chip_type: type_name.to_string(),
                    line_name: line_name.to_string(),
                });
            }

            // And unexpected line types.  The line's name determines which
            // polarity types it may declare; see the const definitions above.
            self.validate_control_line_polarity(type_name, line_name, &control.line_type)?;
        }

        // Check for incompatible chip select line combinations.
        // CS lines (cs1-cs4) and the JEDEC enables (ce/oe) may only coexist if
        // the chip type explicitly declares allow_mixed_control in
        // chip_types.json.
        let control_lines: Vec<&str> = self.control.keys().map(|s| s.as_str()).collect();
        let has_cs = control_lines
            .iter()
            .any(|name| CS_CONTROL_LINES.contains(name));
        let has_ce_oe = control_lines
            .iter()
            .any(|name| FIXED_ACTIVE_LOW_CONTROL_LINES.contains(name));
        #[allow(clippy::collapsible_if)]
        if has_cs && has_ce_oe {
            if !self.allow_mixed_control {
                return Err(ValidationError::IncompatibleControlLines {
                    chip_type: type_name.to_string(),
                    combination: format!("{:?}", control_lines),
                });
            }
        }

        Ok(())
    }

    /// Check that a control line's declared polarity type is permitted for its
    /// name.
    ///
    /// - `ce`/`oe` name the JEDEC-standard enables, so must be
    ///   `fixed_active_low`.
    /// - `cs1`-`cs4` may be `configurable`, `fixed_active_low` or
    ///   `fixed_active_high`: a CS line's polarity may be mask-programmed or
    ///   fixed by the silicon, and the name says nothing either way.
    /// - All other names (`byte`, `write`, `busy`) are unconstrained.
    fn validate_control_line_polarity(
        &self,
        type_name: &str,
        line_name: &str,
        line_type: &ControlLineType,
    ) -> Result<(), ValidationError> {
        if FIXED_ACTIVE_LOW_CONTROL_LINES.contains(&line_name)
            && *line_type != ControlLineType::FixedActiveLow
        {
            return Err(ValidationError::InvalidControlLinePolarity {
                chip_type: type_name.to_string(),
                line_name: line_name.to_string(),
                expected: &["fixed_active_low"],
            });
        }

        Ok(())
    }

    fn validate_pin_number(&self, type_name: &str, pin: u8) -> Result<(), ValidationError> {
        if pin < MIN_PIN_NUMBER || pin > self.pins {
            return Err(ValidationError::InvalidPinNumber {
                chip_type: type_name.to_string(),
                pin,
                max: self.pins,
            });
        }
        Ok(())
    }

    fn check_duplicate_pin(
        &self,
        type_name: &str,
        pin: u8,
        used_pins: &mut Vec<u8>,
    ) -> Result<(), ValidationError> {
        if used_pins.contains(&pin) {
            return Err(ValidationError::DuplicatePin {
                chip_type: type_name.to_string(),
                pin,
            });
        }
        used_pins.push(pin);
        Ok(())
    }

    fn validate_read_state(
        &self,
        type_name: &str,
        pin_name: &str,
        state: &str,
    ) -> Result<(), ValidationError> {
        if !VALID_READ_STATES.contains(&state) {
            return Err(ValidationError::InvalidReadState {
                chip_type: type_name.to_string(),
                pin_name: pin_name.to_string(),
                state: state.to_string(),
            });
        }
        Ok(())
    }
}