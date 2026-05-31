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
const VALID_CONTROL_LINES: &[&str] = &["cs1", "cs2", "cs3", "ce", "oe", "byte", "write", "busy"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlLineType {
    Configurable,
    FixedActiveLow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlLine {
    pub pin: u8,
    #[serde(rename = "type")]
    pub line_type: ControlLineType,
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
                write!(
                    f,
                    "ROM type '{}': incompatible chip select line combination: {}.\nCS1/2/3 cannot be used with CE/OE.",
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

        for line_name in self.control.keys() {
            // Check for unrecognised chip select line names.
            if !VALID_CONTROL_LINES.contains(&line_name.as_str()) {
                return Err(ValidationError::UnknownControlLine {
                    chip_type: type_name.to_string(),
                    line_name: line_name.to_string(),
                });
            }

            // And unexpected line types
            #[allow(clippy::collapsible_if)]
            if line_name == "ce" || line_name == "oe" {
                if self.control[line_name].line_type != ControlLineType::FixedActiveLow {
                    return Err(ValidationError::IncompatibleControlLines {
                        chip_type: type_name.to_string(),
                        combination: format!("{} must be of type 'fixed_active_low'", line_name),
                    });
                }
            }
        }

        // Check for incompatible chip select line combinations.
        let cs_lines: Vec<&str> = self.control.keys().map(|s| s.as_str()).collect();
        if (cs_lines.contains(&"cs1") || cs_lines.contains(&"cs2") || cs_lines.contains(&"cs3"))
            && (cs_lines.contains(&"ce") || cs_lines.contains(&"oe"))
        {
            // 23C1001 is the only chip type to mix CE/OE and CS lines.
            if type_name != "23C1001" {
                return Err(ValidationError::IncompatibleControlLines {
                    chip_type: type_name.to_string(),
                    combination: format!("{:?}", cs_lines),
                });
            }
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
