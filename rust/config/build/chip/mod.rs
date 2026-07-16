// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use std::fs;
use std::path::Path;

mod doc;
mod validation;

use validation::{ChipFunction, ChipType, ChipTypesConfig, ControlLineType};

/// A documentation grouping for chip types.
///
/// Families are inferred from the chip type name and pin count by
/// [`chip_family`], as the JSON carries no explicit family field.
pub struct ChipFamily {
    /// Stable key used to group chip types.
    pub key: &'static str,

    /// Heading used in the generated crate documentation (`chip/mod.rs`).
    pub lib_heading: &'static str,

    /// Heading used in the generated markdown documentation (`CHIP-TYPES.md`).
    pub doc_heading: &'static str,
}

/// Every chip family, in the order they are presented in documentation.
///
/// A family with no chip types is skipped rather than emitted empty, so entries
/// may be added here ahead of the chip types that will populate them.
pub const CHIP_FAMILIES: &[ChipFamily] = &[
    ChipFamily {
        key: "mask_24pin",
        lib_heading: "24-pin Mask ROMs (23xx series)",
        doc_heading: "24-pin Mask ROM Family (23xx)",
    },
    ChipFamily {
        key: "mask_28pin",
        lib_heading: "28-pin Mask ROMs (23xx series)",
        doc_heading: "28-pin Mask ROM Family (23xx)",
    },
    ChipFamily {
        key: "mask_32pin",
        lib_heading: "32-pin Mask ROMs (23xx series)",
        doc_heading: "32-pin Mask ROM Family (23xx)",
    },
    ChipFamily {
        key: "mask_40pin",
        lib_heading: "40-pin Mask ROMs (23xx series)",
        doc_heading: "40-pin Mask ROM Family (23xx)",
    },
    ChipFamily {
        key: "eprom_24pin",
        lib_heading: "24-pin EPROMs (27xx series)",
        doc_heading: "24-pin EPROM Family (27xx)",
    },
    ChipFamily {
        key: "eprom_28pin",
        lib_heading: "28-pin EPROMs (27xx series)",
        doc_heading: "28-pin EPROM Family (27xx)",
    },
    ChipFamily {
        key: "eprom_32pin",
        lib_heading: "32-pin EPROMs (27Cxx series)",
        doc_heading: "32-pin EPROM Family (27xx)",
    },
    ChipFamily {
        key: "eprom_40pin",
        lib_heading: "40-pin EPROMs (27Cxx series)",
        doc_heading: "40-pin EPROM Family (27xx)",
    },
    ChipFamily {
        key: "eeprom_24pin",
        lib_heading: "24-pin EEPROMs (28Cxx series)",
        doc_heading: "24-pin EEPROM Family (28Cxx)",
    },
    ChipFamily {
        key: "eeprom_28pin",
        lib_heading: "28-pin EEPROMs (28Cxx series)",
        doc_heading: "28-pin EEPROM Family (28Cxx)",
    },
    ChipFamily {
        key: "eeprom_32pin",
        lib_heading: "32-pin EEPROMs (28Cxx series)",
        doc_heading: "32-pin EEPROM Family (28Cxx)",
    },
    ChipFamily {
        key: "prom_24pin",
        lib_heading: "24-pin Bipolar PROMs (HM76xx series)",
        doc_heading: "24-pin Bipolar PROM Family (HM76xx)",
    },
    ChipFamily {
        key: "ram_chips",
        lib_heading: "RAM Chips",
        doc_heading: "RAM Chips",
    },
];

/// Classify a chip type into its documentation family.
///
/// Returns `None` for plugins, which are not chips and so belong to no family.
///
/// The family is inferred from the chip type's name, as the JSON carries no
/// explicit family field: `23`, `27` and `28` denote the mask ROM, EPROM and
/// EEPROM families respectively, and `SST39SF` and `HM76` are the exceptions to
/// that numeric convention.  RAM is identified by its function rather than its
/// name.
///
/// # Panics
///
/// Panics if the chip type's name matches no known family, or if its pin count
/// is not one this family is known to come in.  Both mean a new chip type has
/// been added to the JSON without being accounted for here, and so must fail the
/// build rather than be silently omitted from the documentation.
pub fn chip_family(type_name: &str, chip_type: &ChipType) -> Option<&'static ChipFamily> {
    if chip_type.function.is_plugin() {
        return None;
    }

    // Identified by function, as RAM part numbers follow no common convention.
    let name = if chip_type.function == ChipFunction::Ram {
        "ram_chips"
    } else {
        let family = if type_name.starts_with("23") {
            "mask"
        } else if type_name.starts_with("27") || type_name.starts_with("SST39SF") {
            "eprom"
        } else if type_name.starts_with("28") {
            "eeprom"
        } else if type_name.starts_with("HM76") {
            "prom"
        } else {
            panic!("Unsupported chip type {type_name} - needs adding to chip_family()");
        };

        match (family, chip_type.pins) {
            ("mask", 24) => "mask_24pin",
            ("mask", 28) => "mask_28pin",
            ("mask", 32) => "mask_32pin",
            ("mask", 40) => "mask_40pin",
            ("eprom", 24) => "eprom_24pin",
            ("eprom", 28) => "eprom_28pin",
            ("eprom", 32) => "eprom_32pin",
            ("eprom", 40) => "eprom_40pin",
            ("eeprom", 24) => "eeprom_24pin",
            ("eeprom", 28) => "eeprom_28pin",
            ("eeprom", 32) => "eeprom_32pin",
            ("prom", 24) => "prom_24pin",
            (family, pins) => {
                panic!("Unexpected pin count {pins} for {family} chip type {type_name}")
            }
        }
    };

    // A miss here means `name` above and CHIP_FAMILIES have drifted apart.  It
    // must not fall through to `None`, which callers read as "not a chip".
    Some(
        CHIP_FAMILIES
            .iter()
            .find(|family| family.key == name)
            .unwrap_or_else(|| panic!("Chip family '{name}' is missing from CHIP_FAMILIES")),
    )
}

pub const CHIP_TYPES_JSON_FILENAME: &str = "json/chip-types.json";
pub const CHIP_GENERATED_RS_FILENAME: &str = "chip/generated.rs";
pub const CHIP_MOD_RS_FILENAME: &str = "chip/mod.rs";
pub const CHIP_DOCS_MD_FILENAME: &str = "CHIP-TYPES.md";

pub fn build(manifest_path: &Path) {
    // Construct path to JSON config
    let json_path = manifest_path.join(CHIP_TYPES_JSON_FILENAME);

    // Tell Cargo to rerun only if the JSON config changes
    println!("cargo:rerun-if-changed={}", json_path.display());

    // Read and validate the configuration
    let json = fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", json_path.display(), e));

    let config = ChipTypesConfig::from_json(&json)
        .unwrap_or_else(|e| panic!("Failed to parse or validate {}: {}", json_path.display(), e));

    // Generate Rust code for types and implementations
    let generated_code = generate_rust_code(&config);

    // Generate lib.rs with documentation
    let lib_code = generate_lib_rs(&config);

    // Generate markdown docs
    let markdown = doc::generate_chip_types_markdown(&config);

    // Write src/chip/generated.rs
    let src_path = manifest_path.join("src").join(CHIP_GENERATED_RS_FILENAME);
    fs::write(&src_path, &generated_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", src_path.display(), e));

    // Write src/chip/mod.rs
    let mod_path = manifest_path.join("src").join(CHIP_MOD_RS_FILENAME);
    fs::write(&mod_path, &lib_code)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", mod_path.display(), e));

    // Write docs/chip-types.md
    let docs_path = manifest_path
        .join("..")
        .join("..")
        .join("docs")
        .join(CHIP_DOCS_MD_FILENAME);
    fs::create_dir_all(docs_path.parent().unwrap())
        .unwrap_or_else(|e| panic!("Failed to create docs directory: {}", e));
    fs::write(&docs_path, &markdown)
        .unwrap_or_else(|e| panic!("Failed to write {}: {}", docs_path.display(), e));

    eprintln!("Documentation generated at {}", docs_path.display());
}

fn variant_name(type_name: &str, chip_type: &ChipType) -> String {
    if chip_type.function.is_plugin() {
        type_name.to_string()
    } else {
        format!("Chip{}", type_name)
    }
}

fn get_sorted_chip_types(config: &ChipTypesConfig) -> Vec<(&String, &ChipType)> {
    let mut types: Vec<_> = config.chip_types.iter().collect();
    // Sort by: pin count, then size, then name (for determinism)
    types.sort_by_key(|(name, chip_type)| (chip_type.pins, chip_type.size, *name));
    types
}

fn generate_lib_rs(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("// AUTO-GENERATED by build/main.rs - DO NOT EDIT MANUALLY\n");
    code.push_str("// Generated from hw-config/chip-types.json\n");
    code.push_str("//\n");
    code.push_str("// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>\n");
    code.push_str("// MIT License\n\n");

    code.push_str("//! Chip type configurations for One ROM\n");
    code.push_str("//!\n");
    code.push_str(
        "//! This module provides compile-time Chip chip specifications for retro computing\n",
    );
    code.push_str("//! systems. All data is generated at build time from JSON configuration and\n");
    code.push_str("//! embedded as const data - no runtime parsing or allocations needed.\n");
    code.push_str("//!\n");
    code.push_str(
        "//! It is primarily intended for use by the One ROM firmware tooling, but may be\n",
    );
    code.push_str("//! useful in other embedded or WASM projects related to One ROM, such as\n");
    code.push_str("//! Airfrog.\n");
    code.push_str("//!\n");
    code.push_str(
        "//! Note that the presence of Chip types in this crate does not imply that they are\n",
    );
    code.push_str(
        "//! supported by all (or even any!) One ROM hardware versions. Please check the\n",
    );
    code.push_str("//! One ROM documentation for supported Chip types.\n");
    code.push_str("//!\n");
    code.push_str("//! # Supported Chip Types\n");
    code.push_str("//!\n");

    // Group Chips by family for documentation
    let mut families: std::collections::BTreeMap<&'static str, Vec<String>> =
        std::collections::BTreeMap::new();

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            // Plugins have no family - they are not chips.
            let Some(family) = chip_family(type_name, chip_type) else {
                continue;
            };

            let mut entry = format!(
                "//! - **{}**: {} ({})\n",
                type_name,
                chip_type
                    .description
                    .split(" with ")
                    .next()
                    .unwrap_or(&chip_type.description),
                chip_type
                    .description
                    .split(" with ")
                    .nth(1)
                    .unwrap_or("see datasheet")
            );
            if let Some(aliases) = &chip_type.aliases
                && !aliases.is_empty()
            {
                entry.push_str(&format!("//!   Aliases: {}\n", aliases.join(", ")));
            }

            families.entry(family.key).or_default().push(entry);
        }
    }

    for family in CHIP_FAMILIES {
        if let Some(entries) = families.get(family.key) {
            code.push_str(&format!("//! ## {}\n", family.lib_heading));
            for entry in entries {
                code.push_str(entry);
            }
            code.push_str("//!\n");
        }
    }

    code.push_str("//! # Usage\n");
    code.push_str("//!\n");
    code.push_str("//! ```\n");
    code.push_str("//! use onerom_config::chip::{ChipType, ControlLineType};\n");
    code.push_str("//!\n");
    code.push_str("//! // Get Chip specifications\n");
    code.push_str("//! let chip = ChipType::Chip2364;\n");
    code.push_str("//! assert_eq!(chip.size_bytes(), 8192);\n");
    code.push_str("//! assert_eq!(chip.chip_pins(), 24);\n");
    code.push_str("//! assert_eq!(chip.num_addr_lines(), 13);\n");
    code.push_str("//!\n");
    code.push_str("//! // Get pin mappings\n");
    code.push_str("//! let addr_pins = chip.address_pins();\n");
    code.push_str("//! let data_pins = chip.data_pins();\n");
    code.push_str("//! println!(\"A0 is on pin {}\", addr_pins[0]);\n");
    code.push_str("//! println!(\"D0 is on pin {}\", data_pins[0]);\n");
    code.push_str("//!\n");
    code.push_str("//! // Check control lines\n");
    code.push_str("//! let control = chip.control_lines();\n");
    code.push_str("//! assert_eq!(control.len(), 1);\n");
    code.push_str("//! assert_eq!(control[0].name, \"cs1\");\n");
    code.push_str("//! assert_eq!(control[0].line_type, ControlLineType::Configurable);\n");
    code.push_str("//!\n");
    code.push_str("//! // Parse from string\n");
    code.push_str("//! if let Some(chip) = ChipType::try_from_str(\"27128\") {\n");
    code.push_str("//!     println!(\"Found Chip: {}\", chip.name());\n");
    code.push_str("//! }\n");
    code.push_str("//! ```\n");
    code.push_str("//!\n");
    code.push_str("//! # Features\n");
    code.push_str("//!\n");
    code.push_str("//! - **Zero runtime cost**: All data is const, compiled into your binary\n");
    code.push_str("//! - **no_std and no allocations**: Perfect for embedded systems and WASM\n");
    code.push_str("//! - **Type safe**: Enum-based API prevents invalid Chip type references\n");
    code.push_str("//! - **Validated**: Build fails if JSON config is invalid\n");
    code.push_str("//!\n");
    code.push_str("//! # Architecture\n");
    code.push_str("//!\n");
    code.push_str("//! This crate uses the build/main.rs script to:\n");
    code.push_str("//! 1. Read `hw-config/chip-types.json` from the repository root\n");
    code.push_str("//! 2. Validate all Chip specifications at build time\n");
    code.push_str("//! 3. Generate Rust const data structures\n");
    code.push_str("//! 4. Fail the build if validation errors occur\n");
    code.push_str("//!\n");
    code.push_str("//! The generated code is pure Rust with no dependencies, making it suitable\n");
    code.push_str("//! for use in no_std environments, WASM, and any Rust project.\n\n");

    code.push_str("#![deny(missing_docs)]\n");
    code.push_str("#![deny(unsafe_code)]\n\n");

    code.push_str("mod generated;\n\n");
    code.push_str("pub use generated::*;\n");

    code
}

fn generate_rust_code(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    // File header
    code.push_str("// AUTO-GENERATED by build/main.rs - DO NOT EDIT MANUALLY\n");
    code.push_str("// Generated from hw-config/chip-types.json\n");
    code.push('\n');
    code.push_str("// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>\n");
    code.push_str("//\n");
    code.push_str("// MIT License\n\n");
    code.push_str("#![allow(dead_code)]\n\n");
    code.push_str("use crate::fw::FirmwareVersion;\n\n");

    // Generate chip function type enum
    code.push_str(generate_chip_function_enum());
    code.push_str("\n\n");

    // Generate control line type enum
    code.push_str(generate_control_line_type_enum());
    code.push_str("\n\n");

    // Generate control line spec struct
    code.push_str(generate_control_line_spec_struct());
    code.push_str("\n\n");

    // Generate programming pin spec struct
    code.push_str(generate_programming_pin_spec_struct());
    code.push_str("\n\n");

    // Generate power pin spec struct
    code.push_str(generate_power_pin_spec_struct());
    code.push_str("\n\n");

    // Generate RBCP constants
    code.push_str(generate_rbcp_constants());
    code.push_str("\n\n");

    // Generate ChipType enum
    code.push_str(&generate_chip_type_enum(config));
    code.push_str("\n\n");

    // Generate ChipType implementation
    code.push_str(&generate_chip_type_impl(config));

    // Generate lists of ChipType variants per pin counts
    code.push_str(&generate_chip_type_names(config));

    code
}

fn generate_chip_function_enum() -> &'static str {
    r#"/// Chip function type
///
/// Defines the function of this chip (currently ROM or RAM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ChipFunction {
    /// Read-Only Memory (ROM) chip
    #[serde(rename = "ROM")]
    Rom,
    
    /// Random-Access Memory (RAM) chip
    #[serde(rename = "RAM")]
    Ram,

    /// One ROM Plugin (not a chip)
    Plugin,
}
    
impl ChipFunction {
    /// Check if this ChipFunction is a plugin type
    pub const fn is_plugin(&self) -> bool {
        matches!(self, ChipFunction::Plugin)
    }
}
"#
}

fn generate_control_line_type_enum() -> &'static str {
    r#"/// Control line behavior type
///
/// Defines whether a control line's polarity is user-configurable
/// (mask-programmable) or fixed by the silicon, and if fixed, which polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ControlLineType {
    /// CS line with user-configurable polarity (23xxx series mask ROMs)
    ///
    /// These lines can be mask-programmed as either active-high or active-low
    /// during manufacturing. The user must specify the polarity in their configuration.
    Configurable,
    
    /// Fixed active-low control line (27xxx series EPROMs)
    ///
    /// These lines follow the JEDEC standard and are always active-low (/CE, /OE).
    FixedActiveLow,

    /// Fixed active-high control line
    ///
    /// The polarity is fixed by the silicon, as for `FixedActiveLow`, but the
    /// line is asserted high. Used by parts whose chip selects are not
    /// mask-programmable and are not all active low - the HM7641, for
    /// instance, has CS1/CS2 fixed active low and CS3/CS4 fixed active high.
    /// The user has no say in the polarity of these lines.
    FixedActiveHigh,
}

impl ControlLineType {
    /// Check whether this control line's polarity is fixed by the silicon
    ///
    /// Returns `false` only for [`ControlLineType::Configurable`], whose
    /// polarity must be supplied by the user's chip configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use onerom_config::chip::ControlLineType;
    ///
    /// assert!(ControlLineType::FixedActiveLow.is_fixed());
    /// assert!(ControlLineType::FixedActiveHigh.is_fixed());
    /// assert!(!ControlLineType::Configurable.is_fixed());
    /// ```
    pub const fn is_fixed(&self) -> bool {
        matches!(
            self,
            ControlLineType::FixedActiveLow | ControlLineType::FixedActiveHigh
        )
    }

    /// Get the fixed active level of this control line
    ///
    /// Returns `Some(false)` for an active-low line, `Some(true)` for an
    /// active-high line, and `None` for a configurable line, whose active level
    /// is not known until the user configures it.
    ///
    /// # Examples
    ///
    /// ```
    /// use onerom_config::chip::ControlLineType;
    ///
    /// assert_eq!(ControlLineType::FixedActiveLow.fixed_active_level(), Some(false));
    /// assert_eq!(ControlLineType::FixedActiveHigh.fixed_active_level(), Some(true));
    /// assert_eq!(ControlLineType::Configurable.fixed_active_level(), None);
    /// ```
    pub const fn fixed_active_level(&self) -> Option<bool> {
        match self {
            ControlLineType::FixedActiveLow => Some(false),
            ControlLineType::FixedActiveHigh => Some(true),
            ControlLineType::Configurable => None,
        }
    }
}"#
}

fn generate_control_line_spec_struct() -> &'static str {
    r#"/// Specification for a single control line
///
/// Defines the physical pin number and behavior type for control signals
/// like chip select (CS), chip enable (CE), and output enable (OE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ControlLineSpec {
    /// Signal name (e.g., "cs1", "ce", "oe")
    pub name: &'static str,
 
    /// Physical pin number on the Chip package
    pub pin: u8,
 
    /// Behavior type (configurable or fixed active-low)
    pub line_type: ControlLineType,
 
    /// Whether this line may be set to Ignore in a ChipConfig without the
    /// explicit allow_cs_ignore flag.  True only for lines where the chip
    /// datasheet defines a don't-care state (e.g. 23C1001 cs1/cs2).
    pub allow_ignore: bool,
}"#
}

fn generate_programming_pin_spec_struct() -> &'static str {
    r#"/// Programming pin read state specification
///
/// Defines the required state for programming-related pins (Vpp, /PGM)
/// during normal read operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ProgrammingPinState {
    /// Pin must be at Vcc (5V)
    Vcc,
    
    /// Pin must be logic high
    High,
    
    /// Pin must be logic low  
    Low,
    
    /// Pin generates chip select (output enable) signal
    ///
    /// Used for shared /OE/VPP pins (e.g., 2732 pin 20) where the pin
    /// serves as output enable during read and VPP during programming.
    ChipSelect,

    /// Pin is ignored during read operations
    Ignored,

    /// Pin indicates word size (e.g., 8-bit vs 16-bit)
    WordSize,
}

/// Programming pin specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProgrammingPinSpec {
    /// Pin name: one of "vpp", "pgm" or "pe"
    pub name: &'static str,
    
    /// Physical pin number on the Chip package
    pub pin: u8,
    
    /// Required state during read operations
    pub read_state: ProgrammingPinState,
}"#
}

fn generate_power_pin_spec_struct() -> &'static str {
    r#"/// Power pin specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PowerPinSpec {
    /// Pin name ("vcc" or "gnd")
    pub name: &'static str,
    
    /// Physical pin number on the Chip package
    pub pin: u8,
}"#
}

fn generate_rbcp_constants() -> &'static str {
    r#"/// Sentinel value for an invalid or unset chip type in the RBCP wire protocol.
///
/// Matches `INVALID_CHIP_TYPE` in the OneROM C firmware metadata schema.
/// `ChipType::try_from_rbcp_u8` returns `None` for this value.
pub const INVALID_RBCP_CHIP_TYPE: u8 = 0xFF;"#
}

fn generate_chip_type_enum(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("/// Chip type\n");
    code.push_str("///\n");
    code.push_str("/// Supported retrochip types with their pinouts and characteristics.\n");
    code.push_str("/// Includes mask ROMs (23xx series), EPROMs (27xx series) and RAM chips.\n");
    code.push_str("///\n");
    code.push_str("/// # Examples\n");
    code.push_str("///\n");
    code.push_str("/// ```\n");
    code.push_str("/// use onerom_config::chip::ChipType;\n");
    code.push_str("///\n");
    code.push_str("/// let chip = ChipType::Chip2364;\n");
    code.push_str("/// assert_eq!(chip.size_bytes(), 8192);\n");
    code.push_str("/// assert_eq!(chip.chip_pins(), 24);\n");
    code.push_str("/// assert_eq!(chip.num_addr_lines(), 13);\n");
    code.push_str("/// ```\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]\n");
    code.push_str("#[cfg_attr(feature = \"schemars\", derive(schemars::JsonSchema))]\n");
    code.push_str("pub enum ChipType {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            let vname = variant_name(type_name, chip_type);
            code.push_str(&format!(
                "    /// {} - {} bytes, {}-pin package\n",
                chip_type.description, chip_type.size, chip_type.pins
            ));
            if !chip_type.function.is_plugin() {
                code.push_str(&format!(
                    "    #[cfg_attr(feature = \"schemars\", schemars(rename = \"{type_name}\"))]\n"
                ));
            } else {
                let snake_name = type_name.chars().fold(String::new(), |mut acc, c| {
                    if c.is_uppercase() && !acc.is_empty() {
                        acc.push('_');
                    }
                    acc.push(c.to_ascii_lowercase());
                    acc
                });
                code.push_str(&format!(
                    "    #[cfg_attr(feature = \"schemars\", schemars(rename = \"{snake_name}\"))]\n"
                ));
                code.push_str(&format!("    #[serde(rename = \"{snake_name}\")]\n"));
            }
            code.push_str(&format!("    {},\n", vname));
        }
    }

    code.push_str("}\n\n");

    code.push_str("impl<'de> serde::Deserialize<'de> for ChipType {\n");
    code.push_str("    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>\n");
    code.push_str("    where\n");
    code.push_str("        D: serde::Deserializer<'de>,\n");
    code.push_str("    {\n");
    code.push_str("        let s = <&str>::deserialize(deserializer)?;\n");
    code.push_str("        match s {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            let vname = variant_name(type_name, chip_type);
            if vname == *type_name {
                // Plugin: also accept snake_case form
                let snake_name = type_name.chars().fold(String::new(), |mut acc, c| {
                    if c.is_uppercase() && !acc.is_empty() {
                        acc.push('_');
                    }
                    acc.push(c.to_ascii_lowercase());
                    acc
                });
                code.push_str(&format!(
                    "            \"{}\" | \"{}\" => Ok(ChipType::{}),\n",
                    type_name, snake_name, vname
                ));
            } else {
                // Non-plugin: accept "Chip2364", "2364", and all aliases
                let mut patterns = vec![format!("\"{}\"", vname), format!("\"{}\"", type_name)];
                if let Some(aliases) = &chip_type.aliases {
                    for alias in aliases {
                        let aliased = format!("\"{}\"", alias);
                        if !patterns.contains(&aliased) {
                            patterns.push(aliased);
                        }
                    }
                }
                code.push_str(&format!(
                    "            {} => Ok(ChipType::{}),\n",
                    patterns.join(" | "),
                    vname
                ));
            }
        }
    }

    code.push_str("            _ => Err(serde::de::Error::unknown_variant(\n");
    code.push_str("                s,\n");
    code.push_str("                &[");

    let type_names: Vec<String> = get_sorted_chip_types(config)
        .iter()
        .filter_map(|(type_name, _)| {
            if config.chip_types.contains_key(type_name.as_str()) {
                Some(format!("\"{}\"", type_name))
            } else {
                None
            }
        })
        .collect();
    code.push_str(&type_names.join(", "));

    code.push_str("],\n");
    code.push_str("            )),\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    code.push_str("/// All supported Chip types\n");
    code.push_str("pub const CHIP_TYPES: &[ChipType] = &[\n");
    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            code.push_str(&format!(
                "    ChipType::{},\n",
                variant_name(type_name, chip_type)
            ));
        }
    }
    code.push_str("];\n");

    code
}

fn generate_chip_type_impl(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("impl ChipType {\n");

    code.push_str(&generate_const_eq(config));
    code.push_str("\n\n");

    code.push_str(&generate_try_from_str(config));
    code.push_str("\n\n");

    code.push_str(&generate_name_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_aliases_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_chip_function_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_bit_modes_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_supports_bit_mode_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_c_enum_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_chip_pins_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_size_bytes_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_num_addr_lines_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_address_pins_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_data_pins_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_control_lines_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_programming_pins_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_power_pins_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_is_plugin_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_chip_type_is_supported_fn(config));
    code.push_str("\n\n");

    code.push_str(&generate_deselect_when_address_all_high_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_rbcp_chip_type_method(config));
    code.push_str("\n\n");

    code.push_str(&generate_try_from_rbcp_u8(config));
    code.push_str("\n\n");

    code.push_str("}\n");

    // Display impl
    code.push('\n');
    code.push_str("impl core::fmt::Display for ChipType {\n");
    code.push_str("    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n");
    code.push_str("        write!(f, \"{}\", self.name())\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    code
}

fn generate_const_eq(_config: &ChipTypesConfig) -> String {
    let mut code = String::new();
    code.push_str("    /// Const-compatible equality check\n");
    code.push_str("    pub const fn eq(&self, other: &ChipType) -> bool {\n");
    code.push_str("        *self as u8 == *other as u8\n");
    code.push_str("    }\n");
    code
}

fn generate_try_from_str(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Parse Chip type from string identifier\n");
    code.push_str("    ///\n");
    code.push_str("    /// Matching is case-insensitive and aliases are also accepted.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str(
        "    /// assert_eq!(ChipType::try_from_str(\"2364\"), Some(ChipType::Chip2364));\n",
    );
    code.push_str(
        "    /// assert_eq!(ChipType::try_from_str(\"27128\"), Some(ChipType::Chip27128));\n",
    );
    code.push_str("    /// assert_eq!(ChipType::try_from_str(\"2016\"), Some(ChipType::Chip6116)); // alias\n");
    code.push_str("    /// assert_eq!(ChipType::try_from_str(\"invalid\"), None);\n");
    code.push_str("    /// ```\n");

    code.push_str("    pub fn try_from_str(s: &str) -> Option<Self> {\n");

    let mut first = true;
    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            let vname = variant_name(type_name, chip_type);

            let mut names = vec![type_name.to_ascii_lowercase()];
            if let Some(aliases) = &chip_type.aliases {
                for alias in aliases {
                    let lower = alias.to_ascii_lowercase();
                    if !names.contains(&lower) {
                        names.push(lower);
                    }
                }
            }

            let condition = names
                .iter()
                .map(|n| format!("s.eq_ignore_ascii_case(\"{}\")", n))
                .collect::<Vec<_>>()
                .join(" || ");

            let keyword = if first { "if" } else { "} else if" };
            first = false;
            code.push_str(&format!("        {} {} {{\n", keyword, condition));
            code.push_str(&format!("            Some(ChipType::{})\n", vname));
        }
    }

    code.push_str("        } else {\n");
    code.push_str("            None\n");
    code.push_str("        }\n");
    code.push_str("    }\n");

    code
}

fn generate_name_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get the Chip type name as a string\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.name(), \"2364\");\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn name(&self) -> &'static str {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            code.push_str(&format!(
                "            ChipType::{} => \"{}\",\n",
                variant_name(type_name, chip_type),
                type_name
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_aliases_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str(
        "    /// Get all names for this Chip type, including the primary name and any aliases\n",
    );
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip6116.aliases(), &[\"6116\", \"2016\"]);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.aliases(), &[\"2364\", \"4764\", \"MCM68764\", \"MCM68A764\", \"MCM68364\", \"MCM68A364\", \"MM52164\", \"MK36000\"]);\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn aliases(&self) -> &'static [&'static str] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let mut all_names = vec![format!("\"{}\"", type_name)];
            if let Some(aliases) = &chip_type.aliases {
                for alias in aliases {
                    all_names.push(format!("\"{}\"", alias));
                }
            }
            code.push_str(&format!(
                "            ChipType::{} => &[{}],\n",
                variant_name(type_name, chip_type),
                all_names.join(", ")
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}
fn generate_chip_function_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get the function type of this chip (ROM or RAM)\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::{ChipType, ChipFunction};\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.chip_function(), ChipFunction::Rom);\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn chip_function(&self) -> ChipFunction {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => ChipFunction::{},\n",
                variant_name(type_name, chip_type),
                match chip_type.function {
                    ChipFunction::Rom => "Rom",
                    ChipFunction::Ram => "Ram",
                    ChipFunction::Plugin => "Plugin",
                }
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_bit_modes_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get supported bit modes for this Chip type\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.bit_modes(), &[8]);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip27C400.bit_modes(), &[8, 16]);\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn bit_modes(&self) -> &'static [u8] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let modes = chip_type
                .bit_modes
                .iter()
                .map(|mode| mode.to_string())
                .collect::<Vec<String>>()
                .join(", ");
            code.push_str(&format!(
                "            ChipType::{} => &[{}],\n",
                variant_name(type_name, chip_type),
                modes
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_supports_bit_mode_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Check if this Chip type supports the given bit mode\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert!(ChipType::Chip27C400.supports_bit_mode(16));\n");
    code.push_str("    /// assert!(!ChipType::Chip2364.supports_bit_mode(16));\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn supports_bit_mode(&self, mode: u8) -> bool {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let modes = chip_type
                .bit_modes
                .iter()
                .map(|mode| mode.to_string())
                .collect::<Vec<String>>()
                .join(" | ");
            if modes.is_empty() {
                code.push_str(&format!(
                    "            ChipType::{} => false,\n",
                    variant_name(type_name, chip_type)
                ));
            } else {
                code.push_str(&format!(
                    "            ChipType::{} => matches!(mode, {}),\n",
                    variant_name(type_name, chip_type),
                    modes
                ));
            }
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_c_enum_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get the C enum name for this Chip type\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.c_enum_name(), \"CHIP_TYPE_2364\");\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn c_enum_name(&self) -> &'static str {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name.as_str()) {
            if !chip_type.function.is_plugin() {
                code.push_str(&format!(
                    "            ChipType::{} => \"CHIP_TYPE_{}\",\n",
                    variant_name(type_name, chip_type),
                    type_name
                ));
            } else {
                let caps_snake_name = type_name.chars().fold(String::new(), |mut acc, c| {
                    if c.is_uppercase() && !acc.is_empty() {
                        acc.push('_');
                    }
                    acc.push(c.to_ascii_uppercase());
                    acc
                });
                code.push_str(&format!(
                    "            ChipType::{} => \"CHIP_TYPE_{}\",\n",
                    variant_name(type_name, chip_type),
                    caps_snake_name
                ));
            }
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_chip_pins_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get the number of pins in the Chip package\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.chip_pins(), 24);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip27128.chip_pins(), 28);\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn chip_pins(&self) -> u8 {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                chip_type.pins
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_size_bytes_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get Chip capacity in bytes\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2316.size_bytes(), 2048);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip27512.size_bytes(), 65536);\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn size_bytes(&self) -> usize {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                chip_type.size
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_num_addr_lines_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get number of address lines\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.num_addr_lines(), 13); // 2^13 = 8192\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn num_addr_lines(&self) -> usize {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                chip_type.address.len()
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_address_pins_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get address line pin mapping\n");
    code.push_str("    ///\n");
    code.push_str(
        "    /// Returns an array where index is the logical address line number (A0, A1, ...)\n",
    );
    code.push_str("    /// and the value is the physical pin number on the Chip package.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// let pins = ChipType::Chip2364.address_pins();\n");
    code.push_str("    /// assert_eq!(pins[0], 8);  // A0 is on pin 8\n");
    code.push_str("    /// assert_eq!(pins[12], 21); // A12 is on pin 21\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn address_pins(&self) -> &'static [u8] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let pins_str = chip_type
                .address
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            code.push_str(&format!(
                "            ChipType::{} => &[{}],\n",
                variant_name(type_name, chip_type),
                pins_str
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_data_pins_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get data line pin mapping\n");
    code.push_str("    ///\n");
    code.push_str("    /// Returns an array where index is the logical data line number (D0-D7)\n");
    code.push_str("    /// and the value is the physical pin number on the Chip package.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// let pins = ChipType::Chip2364.data_pins();\n");
    code.push_str("    /// assert_eq!(pins.len(), 8);\n");
    code.push_str("    /// assert_eq!(pins[0], 9);  // D0 is on pin 9\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn data_pins(&self) -> &'static [u8] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let pins_str = chip_type
                .data
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            code.push_str(&format!(
                "            ChipType::{} => &[{}],\n",
                variant_name(type_name, chip_type),
                pins_str
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_control_lines_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get control line specifications\n");
    code.push_str("    ///\n");
    code.push_str("    /// Returns control line specs for CS (chip select), CE (chip enable),\n");
    code.push_str("    /// and OE (output enable) signals.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::{ChipType, ControlLineType};\n");
    code.push_str("    ///\n");
    code.push_str("    /// let lines = ChipType::Chip2364.control_lines();\n");
    code.push_str("    /// assert_eq!(lines.len(), 1);\n");
    code.push_str("    /// assert_eq!(lines[0].name, \"cs1\");\n");
    code.push_str("    /// assert_eq!(lines[0].line_type, ControlLineType::Configurable);\n");
    code.push_str("    ///\n");
    code.push_str("    /// let lines = ChipType::Chip27128.control_lines();\n");
    code.push_str("    /// assert_eq!(lines.len(), 2);\n");
    code.push_str("    /// assert!(lines.iter().any(|l| l.name == \"ce\"));\n");
    code.push_str("    /// assert!(lines.iter().any(|l| l.name == \"oe\"));\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn control_lines(&self) -> &'static [ControlLineSpec] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => &[\n",
                variant_name(type_name, chip_type)
            ));

            let mut control_lines: Vec<_> = chip_type.control.iter().collect();
            control_lines.sort_by_key(|(name, _)| *name);

            for (name, control) in control_lines {
                let line_type = match control.line_type {
                    ControlLineType::Configurable => "ControlLineType::Configurable",
                    ControlLineType::FixedActiveLow => "ControlLineType::FixedActiveLow",
                    ControlLineType::FixedActiveHigh => "ControlLineType::FixedActiveHigh",
                };
                code.push_str(&format!(
                    "                ControlLineSpec {{ name: \"{}\", pin: {}, line_type: {}, allow_ignore: {} }},\n",
                    name, control.pin, line_type, control.allow_ignore
                ));
            }

            code.push_str("            ],\n");
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

/// Map a JSON read state to the generated `ProgrammingPinState` variant.
///
/// # Panics
///
/// Panics on an unrecognised read state.  Validation rejects these before code
/// generation is reached, so this indicates the two have drifted apart.
fn programming_pin_state(type_name: &str, pin_name: &str, read_state: &str) -> &'static str {
    match read_state {
        "vcc" => "ProgrammingPinState::Vcc",
        "high" => "ProgrammingPinState::High",
        "low" => "ProgrammingPinState::Low",
        "chip_select" => "ProgrammingPinState::ChipSelect",
        "x" => "ProgrammingPinState::Ignored",
        "word_size" => "ProgrammingPinState::WordSize",
        _ => panic!(
            "Chip type '{type_name}': invalid read state '{read_state}' for programming pin '{pin_name}'"
        ),
    }
}

fn generate_programming_pins_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get programming pin specifications\n");
    code.push_str("    ///\n");
    code.push_str(
        "    /// Returns specifications for programming-related pins (Vpp, /PGM) and their\n",
    );
    code.push_str(
        "    /// required states during normal read operations. Returns None if the Chip type\n",
    );
    code.push_str("    /// has no programming pins (e.g., 27512 where pin 1 is A15).\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::{ChipType, ProgrammingPinState};\n");
    code.push_str("    ///\n");
    code.push_str("    /// let pins = ChipType::Chip27128.programming_pins().unwrap();\n");
    code.push_str("    /// assert_eq!(pins.len(), 2);\n");
    code.push_str("    /// let vpp = pins.iter().find(|p| p.name == \"vpp\").unwrap();\n");
    code.push_str("    /// assert_eq!(vpp.read_state, ProgrammingPinState::Vcc);\n");
    code.push_str("    /// ```\n");
    code.push_str(
        "    pub const fn programming_pins(&self) -> Option<&'static [ProgrammingPinSpec]> {\n",
    );
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let vname = variant_name(type_name, chip_type);
            if let Some(ref prog) = chip_type.programming {
                let mut specs = Vec::new();

                for (pin_name, pin) in [
                    ("vpp", prog.vpp.as_ref()),
                    ("pgm", prog.pgm.as_ref()),
                    ("pe", prog.pe.as_ref()),
                ] {
                    let Some(pin) = pin else { continue };
                    specs.push(format!(
                        "ProgrammingPinSpec {{ name: \"{}\", pin: {}, read_state: {} }}",
                        pin_name,
                        pin.pin,
                        programming_pin_state(type_name, pin_name, &pin.read_state)
                    ));
                }

                if !specs.is_empty() {
                    code.push_str(&format!("            ChipType::{} => Some(&[\n", vname));
                    for spec in specs {
                        code.push_str(&format!("                {},\n", spec));
                    }
                    code.push_str("            ]),\n");
                } else {
                    code.push_str(&format!("            ChipType::{} => None,\n", vname));
                }
            } else {
                code.push_str(&format!("            ChipType::{} => None,\n", vname));
            }
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_power_pins_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get power pin specifications\n");
    // ... (keep the existing doc comments) ...
    code.push_str("    pub const fn power_pins(&self) -> &'static [PowerPinSpec] {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => &[\n",
                variant_name(type_name, chip_type)
            ));

            if let Some(ref power_pins) = chip_type.power {
                for power_pin in power_pins {
                    let name = power_pin.name.to_lowercase();
                    if name == "gnd" || name == "vcc" {
                        code.push_str(&format!(
                            "                PowerPinSpec {{ name: \"{}\", pin: {} }},\n",
                            name, power_pin.pin
                        ));
                    }
                }
            }

            code.push_str("            ],\n");
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_is_plugin_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Check if this ChipType is a plugin\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert!(!ChipType::Chip2364.is_plugin());\n");
    code.push_str("    /// assert!(ChipType::SystemPlugin.is_plugin());\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn is_plugin(&self) -> bool {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                chip_type.function.is_plugin()
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");

    code
}

fn generate_chip_type_is_supported_fn(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Check if this ChipType is supported by the library\n");
    code.push_str("    ///\n");
    code.push_str("    /// This checks if the ChipType is one of the known variants defined in this module.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert!(ChipType::Chip2364.is_supported());\n");
    code.push_str("    /// assert!(!ChipType::try_from_str(\"unknown\").is_some());\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn is_supported(&self) -> bool {\n");
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                chip_type.supported.is_some()
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");

    code.push('\n');

    // Now generate min_supported_firmware_version method
    code.push_str("    /// Get the minimum firmware version that supports this ChipType\n");
    code.push_str("    ///\n");
    code.push_str("    /// Returns `None` if the ChipType is not supported.");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    /// use onerom_config::fw::FirmwareVersion;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.min_supported_firmware_version(), Some(FirmwareVersion::new(0, 4, 4, 0)));\n");
    code.push_str("    /// assert_eq!(ChipType::try_from_str(\"unknown\").and_then(|t| t.min_supported_firmware_version()), None);\n");
    code.push_str("    /// ```\n");
    code.push_str(
        "    pub fn min_supported_firmware_version(&self) -> Option<FirmwareVersion> {\n",
    );
    code.push_str("        match self {\n");

    for (type_name, _chip_type) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let version_str = if let Some(version_string) = &chip_type.supported {
                format!(
                    "Some(FirmwareVersion::try_from_str(\"{}\").unwrap())",
                    version_string
                )
            } else {
                "None".to_string()
            };
            code.push_str(&format!(
                "            ChipType::{} => {},\n",
                variant_name(type_name, chip_type),
                version_str
            ));
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");

    code
}

fn generate_chip_type_names(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    // Collect names grouped by pin count, all non-plugin names, and plugin names
    let mut by_pin: std::collections::HashMap<u8, Vec<String>> = std::collections::HashMap::new();
    let mut all_names: Vec<String> = Vec::new();
    let mut plugin_names: Vec<String> = Vec::new();

    for (type_name, chip_type) in &config.chip_types {
        let mut names = vec![type_name.clone()];
        if let Some(aliases) = &chip_type.aliases {
            names.extend(aliases.iter().cloned());
        }

        if chip_type.supported.is_none() {
            continue; // Skip unsupported types
        }

        if chip_type.function.is_plugin() {
            for name in &names {
                if !plugin_names.contains(name) {
                    plugin_names.push(name.clone());
                }
            }
        } else {
            let entry = by_pin.entry(chip_type.pins).or_default();
            for name in &names {
                if !entry.contains(name) {
                    entry.push(name.clone());
                }
                if !all_names.contains(name) {
                    all_names.push(name.clone());
                }
            }
        }
    }

    all_names.sort_unstable();
    plugin_names.sort_unstable();

    // CHIP_TYPE_NAMES
    code.push_str("/// All chip type names and aliases, alphabetically sorted.\n");
    code.push_str("///\n");
    code.push_str(
        "/// Includes primary names and all known aliases for every supported chip type.\n",
    );
    code.push_str("/// Does not include plugins.\n");
    code.push_str("pub const CHIP_TYPE_NAMES: &[&str] = &[\n");
    for name in &all_names {
        code.push_str(&format!("    \"{name}\",\n"));
    }
    code.push_str("];\n\n");

    // CHIP_TYPE_NAMES_PLUGINS
    code.push_str("/// All plugin type names and aliases, alphabetically sorted.\n");
    code.push_str("pub const CHIP_TYPE_NAMES_PLUGINS: &[&str] = &[\n");
    for name in &plugin_names {
        code.push_str(&format!("    \"{name}\",\n"));
    }
    code.push_str("];\n\n");

    // Per-pin-count arrays
    let mut pin_counts: Vec<u8> = by_pin.keys().copied().collect();
    pin_counts.sort_unstable();

    for &pins in &pin_counts {
        let names = by_pin.get_mut(&pins).unwrap();
        names.sort_unstable();

        code.push_str(&format!(
            "/// All chip type names and aliases for {pins}-pin chips, alphabetically sorted.\n"
        ));
        code.push_str(&format!(
            "pub const CHIP_TYPE_NAMES_{pins}_PIN: &[&str] = &[\n"
        ));
        for name in names {
            code.push_str(&format!("    \"{name}\",\n"));
        }
        code.push_str("];\n\n");
    }

    // chip_type_names_for_pins function
    code.push_str("/// Return the chip type names and aliases for the given pin count.\n");
    code.push_str("///\n");
    code.push_str("/// Returns `None` if no chip types exist for the given pin count.\n");
    code.push_str(
        "pub const fn chip_type_names_for_pins(pins: u8) -> Option<&'static [&'static str]> {\n",
    );
    code.push_str("    match pins {\n");
    for &pins in &pin_counts {
        code.push_str(&format!(
            "        {pins} => Some(CHIP_TYPE_NAMES_{pins}_PIN),\n"
        ));
    }
    code.push_str("        _ => None,\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    code
}

fn generate_deselect_when_address_all_high_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get address line indices which, when all high, deselect the chip\n");
    code.push_str("    ///\n");
    code.push_str("    /// Indices are 0-based into `address_pins()`. Returns `Some` only for\n");
    code.push_str("    /// composite ROM types such as the 23QL384 which require the CS2\n");
    code.push_str("    /// (enable + address qualified) algorithm.\n");
    code.push_str(
        "    pub const fn deselect_when_address_all_high(&self) -> Option<&'static [u8]> {\n",
    );
    code.push_str("        match self {\n");

    for (type_name, _) in get_sorted_chip_types(config) {
        if let Some(chip_type) = config.chip_types.get(type_name) {
            let vname = variant_name(type_name, chip_type);
            match &chip_type.deselect_when_address_all_high {
                Some(indices) if !indices.is_empty() => {
                    let joined = indices
                        .iter()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    code.push_str(&format!(
                        "            ChipType::{} => Some(&[{}]),\n",
                        vname, joined
                    ));
                }
                _ => {
                    code.push_str(&format!("            ChipType::{} => None,\n", vname));
                }
            }
        }
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_rbcp_chip_type_method(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Get the RBCP wire protocol chip type value\n");
    code.push_str("    ///\n");
    code.push_str("    /// Returns the `u8` used to identify this chip type on the RBCP wire.\n");
    code.push_str("    /// Matches the corresponding `onerom_rom_type_t` enum value in the\n");
    code.push_str("    /// OneROM C firmware metadata schema.\n");
    code.push_str("    ///\n");
    code.push_str("    /// Alias chip types (e.g. `Chip23C1010`) return the same value as their\n");
    code.push_str("    /// canonical equivalent (`Chip27C010`), since they are electrically\n");
    code.push_str("    /// identical and share the same wire representation.\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::ChipType;\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::Chip2364.rbcp_chip_type(), 2);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip27C010.rbcp_chip_type(), 15);\n");
    code.push_str("    /// assert_eq!(ChipType::Chip23C1010.rbcp_chip_type(), 15); // alias\n");
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn rbcp_chip_type(&self) -> u8 {\n");
    code.push_str("        match self {\n");

    let mut sorted: Vec<_> = config.chip_types.iter().collect();
    sorted.sort_by_key(|(_, chip_type)| chip_type.rbcp_chip_type);
    for (type_name, chip_type) in &sorted {
        code.push_str(&format!(
            "            ChipType::{} => {},\n",
            variant_name(type_name, chip_type),
            chip_type.rbcp_chip_type
        ));
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}

fn generate_try_from_rbcp_u8(config: &ChipTypesConfig) -> String {
    let mut code = String::new();

    code.push_str("    /// Resolve an RBCP wire protocol chip type value to a `ChipType`\n");
    code.push_str("    ///\n");
    code.push_str("    /// Returns `None` for unrecognised values, including\n");
    code.push_str("    /// `INVALID_RBCP_CHIP_TYPE` (0xFF). Alias chip types are not returned;\n");
    code.push_str("    /// the canonical type is returned instead (e.g. value 15 yields\n");
    code.push_str("    /// `Chip27C010`, not `Chip23C1010`).\n");
    code.push_str("    ///\n");
    code.push_str("    /// # Examples\n");
    code.push_str("    ///\n");
    code.push_str("    /// ```\n");
    code.push_str("    /// use onerom_config::chip::{ChipType, INVALID_RBCP_CHIP_TYPE};\n");
    code.push_str("    ///\n");
    code.push_str("    /// assert_eq!(ChipType::try_from_rbcp_u8(2), Some(ChipType::Chip2364));\n");
    code.push_str(
        "    /// assert_eq!(ChipType::try_from_rbcp_u8(15), Some(ChipType::Chip27C010));\n",
    );
    code.push_str(
        "    /// assert_eq!(ChipType::try_from_rbcp_u8(INVALID_RBCP_CHIP_TYPE), None);\n",
    );
    code.push_str("    /// ```\n");
    code.push_str("    pub const fn try_from_rbcp_u8(val: u8) -> Option<Self> {\n");
    code.push_str("        match val {\n");

    let mut sorted: Vec<_> = config.chip_types.iter().collect();
    sorted.sort_by_key(|(_, chip_type)| chip_type.rbcp_chip_type);
    for (type_name, chip_type) in &sorted {
        if !chip_type.rbcp_alias {
            code.push_str(&format!(
                "            {} => Some(ChipType::{}),\n",
                chip_type.rbcp_chip_type,
                variant_name(type_name, chip_type)
            ));
        }
    }

    code.push_str("            _ => None,\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    code
}