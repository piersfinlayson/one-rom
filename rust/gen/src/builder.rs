// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM generation Builder objects and functions

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use onerom_config::chip::{ChipFunction, ChipType};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::mcu::Family;
use onerom_config::hw::Board;

use crate::image::{Chip, ChipSet, ChipSetType, CsConfig, CsLogic, Location, SizeHandling};
use crate::meta::Metadata;
use crate::{Error, FIRMWARE_SIZE, MAX_METADATA_LEN, MIN_FIRMWARE_OVERRIDES_VERSION, Result};

pub const MAX_SUPPORTED_FIRMWARE_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 999, 0);

const UNSUPPORTED_FIRMWARE_VERSIONS: [FirmwareVersion; 1] = [FirmwareVersion::new(0, 6, 3, 0)];

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

pub(crate) use crate::firmware::*;

/// Main Builder object
///
/// Model is to create the builder from a JSON config, retrieve the list of
/// files that need to be loaded, call `add_file` for each file once loaded,
/// then call `build` to generate the metadata and ROM images.
///
/// # Example
/// ```no_run
/// use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
/// use onerom_config::hw::Board;
/// use onerom_config::mcu::{Family, Variant as McuVariant};
/// # use onerom_gen::Error;
/// use onerom_gen::builder::{Builder, FileData, License};
///
/// # fn fetch_file(url: &str) -> Result<Vec<u8>, Error> {
/// #     // Dummy implementation for doc test
/// #     Ok(vec![0u8; 8192])
/// # }
/// #
/// # fn accept_license(license: &License) -> Result<(), Error> {
/// #     // Dummy implementation for doc test
/// #     Ok(())
/// # }
/// #
/// let json = r#"{
///     "version": 1,
///     "description": "Example ROM configuration",
///     "chip_sets": [{
///         "type": "single",
///         "chips": [{
///             "file": "http://example.com/kernal.bin",
///             "type": "2764",
///             "cs1": 0
///         }]
///     }]
/// }"#;
///
/// // Create builder from JSON
/// let mut builder = Builder::from_json(FirmwareVersion::new(0, 6, 0, 0), Family::Stm32f4, json)?;
///
/// // Get list of licenses to be validated
/// let licenses = builder.licenses();
///
/// // Accept licenses as required
/// for license in licenses {
///     accept_license(&license)?; // Your implementation
///
///     builder.accept_license(&license)?; // Mark as validated
/// }
///
/// // Get list of files to load
/// let file_specs = builder.file_specs();
///
/// // Load each file (fetch or read from disk)
/// for spec in file_specs {
///     let data = fetch_file(&spec.source)?; // Your implementation
///     
///     builder.add_file(FileData {
///         id: spec.id,
///         data,
///     })?;
/// }
///
/// // Get config description (optional)
/// let description = builder.description();
///
/// // Define firmware properties
/// let props = FirmwareProperties::new(
///     FirmwareVersion::new(0, 5, 1, 0),
///     Board::Ice24UsbH,
///     McuVariant::F411RE,
///     ServeAlg::Default,
///     false,
/// ).unwrap();
///
/// // Validate ready to build (optional)
/// builder.build_validation(&props)?;
///
/// // Build metadata and ROM images
/// let (metadata_buf, rom_images_buf) = builder.build(props)?;
/// // Buffers ready to flash at appropriate offsets
/// # Ok::<(), onerom_gen::Error>(())
/// ```
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Builder {
    version: FirmwareVersion,
    config: Config,
    files: BTreeMap<usize, Vec<u8>>,
    licenses: BTreeMap<usize, License>,
    file_id_map: BTreeMap<usize, usize>,
}

impl Builder {
    /// Create from JSON config
    ///
    /// Arguments:
    /// - `version`: Firmware version this config is for
    /// - `mcu_family`: MCU family this config is for
    /// - `json`: JSON string
    pub fn from_json(version: FirmwareVersion, mcu_family: Family, json: &str) -> Result<Self> {
        if version > MAX_SUPPORTED_FIRMWARE_VERSION {
            return Err(Error::FirmwareTooNew {
                version,
                maximum: MAX_SUPPORTED_FIRMWARE_VERSION,
            });
        }

        let config: Config = serde_json::from_str(json).map_err(|e| Error::InvalidConfig {
            error: e.to_string(),
        })?;

        Self::validate_config(&version, &mcu_family, &config)?;

        let mut builder = Self {
            version,
            config,
            files: BTreeMap::new(),
            licenses: BTreeMap::new(),
            file_id_map: BTreeMap::new(),
        };

        builder.build_file_id_map();

        Ok(builder)
    }

    /// Get a reference to the config
    pub fn config(&self) -> &Config {
        &self.config
    }

    fn validate_config(
        version: &FirmwareVersion,
        mcu_family: &Family,
        config: &Config,
    ) -> Result<()> {
        // Validate version
        if config.version != 1 {
            return Err(Error::UnsupportedConfigVersion {
                version: config.version,
            });
        }

        // Check the firmware release is not one we explicitly do not support.
        for unsupported_version in UNSUPPORTED_FIRMWARE_VERSIONS.iter() {
            if unsupported_version.matches_release(version) {
                return Err(Error::FirmwareUnsupported { version: *version });
            }
        }

        /// Check firmware supports this ROM type
        const MIN_FW_CHIP_TYPE_231024: FirmwareVersion = FirmwareVersion::new(0, 6, 3, 0);
        if *version < MIN_FW_CHIP_TYPE_231024 {
            for set in config.chip_sets.iter() {
                for chip in set.chips.iter() {
                    if matches!(chip.chip_type, ChipType::Chip231024) {
                        return Err(Error::FirmwareTooOld {
                            feat: "231024 ROMs",
                            version: *version,
                            minimum: MIN_FW_CHIP_TYPE_231024,
                        });
                    }
                }
            }
        }

        // Validate each rom set has roms
        let mut chip_num = 0;
        for (set_id, set) in config.chip_sets.iter().enumerate() {
            if set.chips.is_empty() {
                return Err(Error::NoChips { id: set_id });
            }

            // FirmwareConfig only supported from 0.6.0 firmware onwards
            #[allow(clippy::collapsible_if)]
            if set.firmware_overrides.is_some() {
                if version < &MIN_FIRMWARE_OVERRIDES_VERSION {
                    return Err(Error::FirmwareTooOld {
                        feat: "firmware overrides",
                        version: *version,
                        minimum: MIN_FIRMWARE_OVERRIDES_VERSION,
                    });
                }
            }

            if set.chips.len() > 1 {
                if set.set_type == ChipSetType::Single {
                    return Err(Error::TooManyChips {
                        id: set_id,
                        expected: 1,
                        actual: set.chips.len(),
                    });
                }

                if set.chips.len() > 3 && set.set_type == ChipSetType::Multi {
                    return Err(Error::TooManyChips {
                        id: set_id,
                        expected: 3,
                        actual: set.chips.len(),
                    });
                }

                if set.chips.len() > 4 && set.set_type == ChipSetType::Banked {
                    return Err(Error::TooManyChips {
                        id: set_id,
                        expected: 4,
                        actual: set.chips.len(),
                    });
                }
            }

            for chip in set.chips.iter() {
                let chip0 = &set.chips[0];

                // Check chip_type is supported
                if !SUPPORTED_CHIP_TYPES.contains(&chip.chip_type) {
                    return Err(Error::UnsupportedToolChipType {
                        chip_type: chip.chip_type,
                    });
                }

                // Check chip type is supported for this version of firmware
                if let Some(min_version) = chip.chip_type.min_supported_firmware_version() {
                    if version < &min_version {
                        return Err(Error::FirmwareTooOld {
                            feat: chip.chip_type.name(),
                            version: *version,
                            minimum: min_version,
                        });
                    }
                } else {
                    return Err(Error::UnsupportedToolChipType {
                        chip_type: chip.chip_type,
                    });
                }

                // Check filename specified for ROMs
                if chip.file.is_empty() && chip.chip_type.chip_function() != ChipFunction::Ram {
                    return Err(Error::InvalidConfig {
                        error: format!("Chip {} file name is empty", chip_num),
                    });
                }

                // Check all Chips in a bank are same type
                if set.set_type == ChipSetType::Banked && chip.chip_type != chip0.chip_type {
                    return Err(Error::InvalidConfig {
                        error: format!(
                            "All Chips in a banked set must be of the same type ({} != {})",
                            chip.chip_type.name(),
                            chip0.chip_type.name()
                        ),
                    });
                }

                // Check that required CS lines are specified
                for line in chip.chip_type.control_lines() {
                    let cs = match line.name {
                        "cs1" => chip.cs1,
                        "cs2" => chip.cs2,
                        "cs3" => chip.cs3,
                        // Clumsy code to ignore these
                        "ce" | "oe" => Some(CsLogic::Ignore),
                        "write" | "byte" | "busy" => Some(CsLogic::Ignore),
                        _ => {
                            return Err(Error::InvalidConfig {
                                error: format!("Unknown control line {}", line.name),
                            });
                        }
                    };
                    if cs.is_none() {
                        return Err(Error::MissingCsConfig {
                            chip_type: chip.chip_type,
                            line: line.name,
                        });
                    }
                }

                // Check that invalid CS lines are NOT specified
                let has_cs2 = chip
                    .chip_type
                    .control_lines()
                    .iter()
                    .any(|line| line.name == "cs2");
                let has_cs3 = chip
                    .chip_type
                    .control_lines()
                    .iter()
                    .any(|line| line.name == "cs3");

                if chip.cs2.is_some() && !has_cs2 {
                    return Err(Error::InvalidConfig {
                        error: format!(
                            "CS2 specified for Chip type {} which does not use CS2",
                            chip.chip_type.name()
                        ),
                    });
                }

                if chip.cs3.is_some() && !has_cs3 {
                    return Err(Error::InvalidConfig {
                        error: format!(
                            "CS3 specified for Chip type {} which does not use CS3",
                            chip.chip_type.name()
                        ),
                    });
                }

                let cs1_active = chip.cs1.is_some() && chip.cs1.unwrap() != CsLogic::Ignore;
                let cs2_active = chip.cs2.is_some() && chip.cs2.unwrap() != CsLogic::Ignore;
                let cs3_active = chip.cs3.is_some() && chip.cs3.unwrap() != CsLogic::Ignore;

                // Check that CS1 cannot be ignore if other CS lines are active
                if !cs1_active && (cs2_active || cs3_active) {
                    return Err(Error::InvalidConfig {
                        error: "CS1 cannot be ignore when CS2 or CS3 are active".to_string(),
                    });
                }

                // Check that CS2 is not ignore if CS3 is active
                if !cs2_active && cs3_active {
                    return Err(Error::InvalidConfig {
                        error: "CS2 cannot be ignore when CS3 is active".to_string(),
                    });
                }

                // Check that the correct CS lines are specified for the Chip
                // type
                let mut required_cs_lines: BTreeSet<&str> = chip
                    .chip_type
                    .control_lines()
                    .iter()
                    .filter(|line| matches!(line.name, "cs1" | "cs2" | "cs3"))
                    .map(|line| line.name)
                    .collect();
                if chip.chip_type == ChipType::Chip27C080 {
                    // Sepcial handling for 27C080.  The chip itself
                    // doesn't have CS lines, but we consider A19 to be
                    // CS1, so we can use that to switch between two One
                    // ROMs, each servig half
                    required_cs_lines.insert("cs1");
                }

                let specified_cs_lines: BTreeSet<&str> = {
                    let mut lines = BTreeSet::new();
                    if chip.cs1.is_some() {
                        lines.insert("cs1");
                    }
                    if chip.cs2.is_some() {
                        lines.insert("cs2");
                    }
                    if chip.cs3.is_some() {
                        lines.insert("cs3");
                    }
                    lines
                };

                if required_cs_lines != specified_cs_lines {
                    return Err(Error::InvalidConfig {
                        error: format!(
                            "Chip type {} requires CS lines {:?}, but specified CS lines are {:?}",
                            chip.chip_type.name(),
                            required_cs_lines,
                            specified_cs_lines
                        ),
                    });
                }

                // Check no extra CS lines specified
                for line in &["cs1", "cs2", "cs3"] {
                    if !required_cs_lines.contains(line) && specified_cs_lines.contains(line) {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "Chip type {} does not use {}, but it is specified",
                                chip.chip_type.name(),
                                line
                            ),
                        });
                    }
                }

                if set.chips.len() == 1 {
                    // Check none are ignore
                    for line in &["cs1", "cs2", "cs3"] {
                        let cs = match *line {
                            "cs1" => &chip.cs1,
                            "cs2" => &chip.cs2,
                            "cs3" => &chip.cs3,
                            _ => unreachable!(),
                        };
                        #[allow(clippy::collapsible_if)]
                        if let Some(cs_logic) = cs {
                            if *cs_logic == CsLogic::Ignore {
                                return Err(Error::InvalidConfig {
                                    error: format!(
                                        "{} cannot be ignore for single-ROM sets (Chip {})",
                                        line.to_uppercase(),
                                        chip_num
                                    ),
                                });
                            }
                        }
                    }
                } else {
                    // For multi ROM sets, not all CS lines can be ignore
                    if !cs1_active {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "CS1 cannot be ignore for multi-ROM sets (Chip {})",
                                chip_num
                            ),
                        });
                    }
                }

                // Validate location if present
                if let Some(location) = &chip.location {
                    // Check length is non-zero
                    if location.length == 0 {
                        return Err(Error::InvalidConfig {
                            error: format!("Chip {} location length must be non-zero", chip_num),
                        });
                    }

                    // Check for overflowing a usize (!)
                    if location.start.checked_add(location.length).is_none() {
                        return Err(Error::InvalidConfig {
                            error: format!("Chip {} location start + length overflows", chip_num),
                        });
                    }
                }

                // Check for plugins on Ice (not supported)
                if chip.chip_type.is_plugin() && *mcu_family == Family::Stm32f4 {
                    return Err(Error::InvalidConfig {
                        error: format!("Plugins are not supported on Ice (Chip {})", chip_num),
                    });
                }

                if chip.chip_type == ChipType::SystemPlugin && set_id != 0 {
                    return Err(Error::InvalidConfig {
                        error: "System plugins must be in the first slot".to_string(),
                    });
                }
                if chip.chip_type == ChipType::UserPlugin {
                    if set_id != 1 {
                        return Err(Error::InvalidConfig {
                            error: "User plugins must be in the second slot".to_string(),
                        });
                    } else {
                        // System plugin must be slot 0
                        if config.chip_sets[0].chips[0].chip_type != ChipType::SystemPlugin {
                            return Err(Error::InvalidConfig {
                                error: "User plugins must be in the second slot, and the first slot must be a system plugin".to_string()
                            });
                        }
                    }
                }

                chip_num += 1;
            }

            // After the loop: validate CS consistency for multi/banked sets
            #[allow(clippy::collapsible_if)]
            if set.set_type == ChipSetType::Multi || set.set_type == ChipSetType::Banked {
                if set.chips.len() > 1 {
                    // Get CS configuration from first ROM
                    let first_cs1 = set.chips[0].cs1;
                    let first_cs2 = set.chips[0].cs2;
                    let first_cs3 = set.chips[0].cs3;

                    // Check all other ROMs have the same CS configuration
                    for (idx, rom) in set.chips.iter().enumerate().skip(1) {
                        if rom.cs1 != first_cs1 || rom.cs2 != first_cs2 || rom.cs3 != first_cs3 {
                            if (rom.cs2 != first_cs2)
                                && let Some(cs) = rom.cs2
                                && (cs == CsLogic::Ignore)
                            {
                                // Ignore difference if cs2 is ignore
                                // If there are 3 CS lines on ROM 1, cs2 must
                                // be the same, but we don't support that yet
                                continue;
                            }
                            // Should do a similar test for CS3, but we don't support that yet
                            return Err(Error::InvalidConfig {
                                error: format!(
                                    "{:?} set requires all ROMs to have identical CS configuration. ROM 0 has cs1={:?}/cs2={:?}/cs3={:?}, but ROM {} has cs1={:?}/cs2={:?}/cs3={:?}",
                                    set.set_type,
                                    first_cs1,
                                    first_cs2,
                                    first_cs3,
                                    idx,
                                    rom.cs1,
                                    rom.cs2,
                                    rom.cs3
                                ),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn build_file_id_map(&mut self) {
        let mut seen_files: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
        let mut file_id = 0;
        let mut chip_id = 0;

        for chip_set in self.config.chip_sets.iter() {
            for chip in &chip_set.chips {
                // Skip chips with no file (e.g., RAM)
                if chip.file.is_empty() {
                    chip_id += 1;
                    continue;
                }

                let key = (chip.file.clone(), chip.extract.clone());

                let assigned_file_id = if let Some(&existing_id) = seen_files.get(&key) {
                    existing_id
                } else {
                    seen_files.insert(key, file_id);
                    let id = file_id;
                    file_id += 1;
                    id
                };

                self.file_id_map.insert(chip_id, assigned_file_id);
                chip_id += 1;
            }
        }
    }

    /// Get list of files that need to be loaded
    pub fn file_specs(&self) -> Vec<FileSpec> {
        let mut specs = Vec::new();
        let mut seen_files: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
        let mut rom_id = 0;

        for (chip_set_num, chip_set) in self.config.chip_sets.iter().enumerate() {
            for rom in &chip_set.chips {
                if rom.file.is_empty() {
                    // No file to load for this ROM
                    rom_id += 1;
                    continue;
                }
                let key = (rom.file.clone(), rom.extract.clone());
                let file_id = *self.file_id_map.get(&rom_id).unwrap();

                seen_files.entry(key).or_insert_with(|| {
                    specs.push(FileSpec {
                        id: file_id,
                        description: rom.description.clone(),
                        source: rom.file.clone(),
                        extract: rom.extract.clone(),
                        size_handling: rom.size_handling.clone(),
                        chip_type: rom.chip_type,
                        rom_size: rom.chip_type.size_bytes(),
                        cs1: rom.cs1,
                        cs2: rom.cs2,
                        cs3: rom.cs3,
                        set_id: chip_set_num,
                        set_type: chip_set.set_type.clone(),
                        set_description: chip_set.description.clone(),
                    });
                    file_id
                });

                rom_id += 1;
            }
        }

        specs
    }

    /// Add a loaded file - called multiple times, once for each file that
    /// has been loaded
    pub fn add_file(&mut self, file: FileData) -> Result<()> {
        // Check if already added
        if self.files.contains_key(&file.id) {
            return Err(Error::DuplicateFile { id: file.id });
        }

        // Validate id is in range
        let total_files = self.total_file_count();
        if file.id >= total_files {
            return Err(Error::InvalidFile {
                id: file.id,
                total: total_files,
            });
        }

        self.files.insert(file.id, file.data);
        Ok(())
    }

    /// Get list of licenses that need to be validated
    pub fn licenses(&mut self) -> Vec<License> {
        let mut licenses = Vec::new();

        let mut license_id = 0;
        let mut rom_id = 0;
        for chip_set in self.config.chip_sets.iter() {
            for rom in &chip_set.chips {
                if let Some(ref url) = rom.license {
                    let license = License::new(license_id, rom_id, url.clone());
                    licenses.push(license.clone());
                    self.licenses.insert(license_id, license);
                    license_id += 1;
                }
                rom_id += 1;
            }
        }

        licenses
    }

    /// Mark a license as validated
    pub fn accept_license(&mut self, license: &License) -> Result<()> {
        let own_license = self
            .licenses
            .get_mut(&license.id)
            .ok_or(Error::InvalidLicense { id: license.id })?;

        own_license.validated = true;
        Ok(())
    }

    fn total_file_count(&self) -> usize {
        self.file_id_map.values().collect::<BTreeSet<_>>().len()
    }

    /// Validate whether ready to build
    pub fn build_validation(&self, props: &FirmwareProperties) -> Result<()> {
        // Check all files loaded
        for ii in 0..self.total_file_count() {
            if !self.files.contains_key(&ii) {
                return Err(Error::MissingFile { id: ii });
            }
        }

        // Check all licenses validated
        for (id, license) in self.licenses.iter() {
            if !license.validated {
                return Err(Error::UnvalidatedLicense { id: *id });
            }
        }

        // Validate all plugins are built for at least the firmware version
        // we're building for.  The major, minor and patch fw versions are in
        // little endian format, at offsets 24, 26 and 28 in the image.  Also
        // header the magic ("ORA ") at offset 0, and version (1) at offset 4.
        let mut rom_id = 0;
        for set in self.config.chip_sets.iter() {
            for rom in set.chips.iter() {
                if matches!(
                    rom.chip_type,
                    ChipType::SystemPlugin | ChipType::UserPlugin | ChipType::PioPlugin
                ) {
                    let file_id = self.file_id_map.get(&rom_id).unwrap();
                    let data = self.files.get(file_id).unwrap();

                    if data.len() < 256 {
                        return Err(Error::InvalidPluginImage { plugin_type: rom.chip_type, image_file: rom.file.clone(), error: "Plugin image is smaller than the required plugin header (256 bytes).".to_string() });
                    }

                    if &data[0..4] != b"ORA " {
                        return Err(Error::InvalidPluginImage {
                            plugin_type: rom.chip_type,
                            image_file: rom.file.clone(),
                            error: "Invalid magic value in plugin header.".to_string(),
                        });
                    }
                    let api_version = u32::from_le_bytes(data[4..8].try_into().unwrap());
                    if api_version != 1 {
                        return Err(Error::InvalidPluginImage {
                            plugin_type: rom.chip_type,
                            image_file: rom.file.clone(),
                            error: format!(
                                "Invalid API version {api_version} in plugin header - must be 1."
                            ),
                        });
                    }

                    let plugin_fw_major = u16::from_le_bytes([data[24], data[25]]);
                    let plugin_fw_minor = u16::from_le_bytes([data[26], data[27]]);
                    let plugin_fw_patch = u16::from_le_bytes([data[28], data[29]]);
                    let plugin_fw_version =
                        FirmwareVersion::new(plugin_fw_major, plugin_fw_minor, plugin_fw_patch, 0);

                    if plugin_fw_version > props.version() {
                        return Err(Error::InvalidPluginImage {
                            plugin_type: rom.chip_type,
                            image_file: rom.file.clone(),
                            error: format!(
                                "Plugin requires at least firmware version {} which is newer than the firmware version being built for ({})",
                                plugin_fw_version,
                                props.version()
                            ),
                        });
                    }
                }
                rom_id += 1;
            }
        }

        Ok(())
    }

    /// Generate metadata and ROM images once all files loaded
    ///
    /// Returns (metadata, Chip images)
    pub fn build(&self, props: FirmwareProperties) -> Result<(Vec<u8>, Vec<u8>)> {
        if props.version() > MAX_SUPPORTED_FIRMWARE_VERSION {
            return Err(Error::FirmwareTooNew {
                version: props.version(),
                maximum: MAX_SUPPORTED_FIRMWARE_VERSION,
            });
        }

        // Validate ready to build
        self.build_validation(&props)?;

        // Build Chip and ChipSet objects together
        let mut chip_sets = Vec::new();
        let mut chip_id = 0;

        for (set_id, chip_set_config) in self.config.chip_sets.iter().enumerate() {
            let mut set_roms = Vec::new();

            if let Some(overrides) = chip_set_config.firmware_overrides.as_ref()
                && let Some(fire) = overrides.fire.as_ref()
                && fire.serve_mode == Some(FireServeMode::Cpu)
            {
                if props.board().chip_pins() != 24 {
                    return Err(Error::InvalidConfig {
                        error: "Fire CPU serving mode is only supported on One ROM 24".to_string(),
                    });
                } else if chip_set_config.chips[0].chip_type == ChipType::Chip28C16 {
                    return Err(Error::InvalidConfig {
                        error: "Fire CPU serving mode is not supported with 28C16".to_string(),
                    });
                }
            }

            for chip_config in &chip_set_config.chips {
                let data = if let Some(&file_id) = self.file_id_map.get(&chip_id) {
                    // Has a file - use loaded data
                    Some(self.files.get(&file_id).unwrap())
                } else {
                    // No file (RAM chip) - use empty slice
                    None
                };

                let filename = chip_config.filename();

                let chip_config = if matches!(props.board(), Board::Fire32A) && matches!(chip_config.chip_type, ChipType::ChipSST39SF040) {
                    // Rewrite the SST39SF040 for Fire32A to be a 27C040, assuming that
                    // a shim will be used (as this variant doesn't support the
                    // SST39SF040 natively).
                    let mut new_cc = chip_config.clone();
                    new_cc.chip_type = ChipType::Chip27C040;
                    new_cc
                } else {
                    chip_config.clone()
                };
                let rom = Chip::from_raw_rom_image(
                    chip_id,
                    filename,
                    chip_config.label.clone(),
                    data.map(|v| &**v),
                    vec![0u8; chip_config.chip_type.size_bytes()],
                    &chip_config.chip_type,
                    CsConfig::new(chip_config.cs1, chip_config.cs2, chip_config.cs3),
                    &chip_config.size_handling,
                    chip_config.location,
                )?;
                set_roms.push(rom);
                chip_id += 1;
            }

            let serve_alg = if let Some(alg) = chip_set_config.serve_alg {
                alg
            } else {
                props.serve_alg()
            };
            let chip_set = ChipSet::new(
                set_id,
                chip_set_config.set_type.clone(),
                serve_alg,
                set_roms,
                chip_set_config.firmware_overrides.clone(),
            )?;
            chip_sets.push(chip_set);
        }

        // Build Metadata
        let metadata = Metadata::new(
            props.board(),
            chip_sets,
            props.boot_logging(),
            props.board().mcu_pio(),
            props.version(),
        );

        // Get buffer sizes
        let metadata_size = metadata.metadata_len();
        let rom_data_size: usize = metadata.rom_images_size();
        let set_count = metadata.total_set_count();

        // Check the board has enough space
        let mcu_variant = props.mcu_variant();
        let flash_size = mcu_variant.flash_storage_bytes();
        let rom_space = flash_size - FIRMWARE_SIZE - MAX_METADATA_LEN;
        assert!(rom_space > 0);

        // Figure out the ROM data size
        if rom_data_size > rom_space {
            return Err(Error::BufferTooSmall {
                location: "Flash",
                expected: rom_data_size,
                actual: rom_space,
            });
        }

        // Allocate buffers
        let mut metadata_buf = vec![0u8; metadata_size];
        let mut rom_data_buf = vec![0u8; rom_data_size];
        let mut rom_data_ptrs = vec![0u32; set_count];

        // Write metadata
        metadata.write_all(&mut metadata_buf, &mut rom_data_ptrs)?;
        // Note rom_data_ptrs unused here - absolute flash addresses.

        // Write ROM data
        metadata.write_roms(&mut rom_data_buf)?;

        // Done - return the two buffers
        Ok((metadata_buf, rom_data_buf))
    }

    fn num_chip_sets(&self) -> usize {
        self.config.chip_sets.len()
    }

    fn num_roms(&self) -> usize {
        self.config
            .chip_sets
            .iter()
            .map(|set| set.chips.len())
            .sum()
    }

    /// Build config description
    ///
    /// Returns a string like:
    ///
    /// No multi-set/banked ROMS:
    ///
    /// ```text
    /// Name of config
    /// --------------
    ///
    /// Description of config
    ///
    /// Detailed description
    ///
    /// Images:
    /// 0: Image 0
    /// 1: Image 1
    ///
    /// Notes```
    ///
    /// Multi-set/banked ROMs:
    ///
    /// ```text
    /// Description of config
    ///
    /// Detailed description
    ///
    /// Sets:
    /// 0: Image 0
    /// 1: Image 1
    ///
    /// Notes```
    pub fn description(&self) -> String {
        let mut desc = String::new();

        if let Some(name) = self.config.name.as_ref() {
            desc.push_str(name);
            desc.push('\n');
            desc.push_str(&"-".repeat(name.len()));
            desc.push_str("\n\n");
        }

        desc.push_str(&self.config.description);
        desc.push_str("\n\n");

        if let Some(detail) = &self.config.detail {
            desc.push_str(detail);
            desc.push_str("\n\n");
        }

        let multi_chip_sets = if self.num_chip_sets() == self.num_roms() {
            desc.push_str("Images:");
            false
        } else {
            desc.push_str("Sets:");
            true
        };
        desc.push('\n');

        let mut none = true;
        for (ii, set) in self.config.chip_sets.iter().enumerate() {
            none = false;
            desc.push_str(&format!("{ii}:"));
            if multi_chip_sets {
                desc.push_str(&format!(" {:?}", set.set_type));
                if let Some(ref set_desc) = set.description {
                    desc.push_str(&format!(", {set_desc}"));
                }
                desc.push('\n');
            } else {
                desc.push(' ');
            }

            for (jj, rom) in set.chips.iter().enumerate() {
                if multi_chip_sets {
                    desc.push_str(&format!("  {jj}: "));
                }
                if let Some(rom_desc) = &rom.description {
                    desc.push_str(rom_desc);
                } else {
                    desc.push_str(&rom.file);
                }
                desc.push('\n');
            }
        }

        if none {
            desc.push_str("  None\n");
        }

        if let Some(notes) = &self.config.notes {
            desc.push('\n');
            desc.push_str(notes);
        } else {
            // Strip trailing \n
            desc.pop();
        }

        desc
    }

    /// Get categories for this config
    pub fn categories(&self) -> Vec<String> {
        let mut categories = Vec::new();
        if let Some(cats) = &self.config.categories {
            for cat in cats {
                categories.push(cat.clone());
            }
        }
        categories
    }
}

/// License details for validation by caller
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct License {
    /// License ID provided for information only.
    pub id: usize,

    /// File ID that this license applies to, provided for information only.
    pub file_id: usize,

    /// License URL/identifier.  Used by caller to retrieve and present to user
    /// for acceptance.
    pub url: String,

    // Whether this license has been validated by the caller
    validated: bool,
}

impl License {
    /// Create new license
    pub fn new(id: usize, file_id: usize, url: String) -> Self {
        Self {
            id,
            file_id,
            url,
            validated: false,
        }
    }
}

/// Details about a file to be loaded by the caller
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileSpec {
    /// File ID to be used when adding the loaded file to the builder
    pub id: usize,

    /// Optional description for this file.  Provided for information only.
    pub description: Option<String>,

    /// Filename or URL of the ROM image to be loaded
    pub source: String,

    /// Optional extract path within an archive (zip/tar) if the file pointed
    /// to is an archive.  If extract is present, the file at that path within
    /// the archive should be extracted before returning the data to the
    /// builder.
    pub extract: Option<String>,

    /// Size handling configuration for this ROM.  Provided for information
    /// only.
    pub size_handling: SizeHandling,

    /// Type of Chip.  Provided for information only.
    pub chip_type: ChipType,

    /// Size of the ROM in bytes.  Provided for information only.
    pub rom_size: usize,

    /// Optional Chip Select 1 logic - only valid for ROM Types that have CS1.
    /// Provided for information only.
    pub cs1: Option<CsLogic>,

    /// Optional Chip Select 2 logic - only valid for ROM Types that have CS2.
    /// Provided for information only.
    pub cs2: Option<CsLogic>,

    /// Optional Chip Select 3 logic - only valid for ROM Types that have CS3.
    /// Provided for information only.
    pub cs3: Option<CsLogic>,

    /// ROM Set ID that this file belongs to.  Provided for information only.
    pub set_id: usize,

    /// ROM Set type that this file belongs to.  Provided for information only.
    pub set_type: ChipSetType,

    /// Optional ROM Set description that this file belongs to.  Provided for
    /// information only.
    pub set_description: Option<String>,
}

/// File data loaded by the caller, passed back to the builder.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FileData {
    /// File ID as per FileSpec
    pub id: usize,

    /// File data
    pub data: Vec<u8>,
}

/// One ROM chip configuration format.
///
/// Used to indicate:
/// - What ROM chips, RAM chips and any other devices to emulate
/// - What ROM images to include
/// - Any overrides for the firmware build-time setting
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(title = "One ROM Configuration"))]
pub struct Config {
    /// Configuration format version.
    #[cfg_attr(feature = "schemars", schemars(schema_with = "version_schema"))]
    pub version: u32,

    /// Optional name for this configuration.  Is included in the description
    /// output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Mandatory description for this configuration.  This is included in the
    /// description output by the builder, following the name.
    pub description: String,

    /// Optional detailed description for this configuration.  This is included
    /// in the description output by the builder, following name and
    /// description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,

    /// Array of chip set configurations.  Note that even if not using complex
    /// features like dynamic banking and multi-ROM sets, each ROM image, or
    /// other chip types is in its own set.
    ///
    /// The builder description output lists either "Images" or "Sets"
    /// depending on whether there are any multi-set or banked sets in use.
    #[serde(alias = "rom_sets")]
    pub chip_sets: Vec<ChipSetConfig>,

    /// Optional notes for this configuration.  This is included in the
    /// description output by the builder, following the list of images/sets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    /// Optional categories for this configuration, to aid in grouping,
    /// sorting, and searching of configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
}

#[cfg(feature = "schemars")]
fn version_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "const": 1
    })
}

/// Chip Set configuration structure
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ChipSetConfig {
    /// Type of ROM set
    #[serde(rename = "type")]
    #[cfg_attr(feature = "schemars", schemars(default))]
    pub set_type: ChipSetType,

    /// Optional description for this chip set.  This is included in the
    /// description output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Array of chip configurations in this set.  Contains 1 member for single
    /// chip sets, and multiple members for multi-ROM and banked ROM sets.
    #[serde(alias = "roms")]
    pub chips: Vec<ChipConfig>,

    /// Optional serving algorithm override for this chip set.  Only valid
    /// when using CPU serving - Ice boards and Fire 24 A/B by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_alg: Option<ServeAlg>,

    /// Optional firmware overrides when serving this chip set.  Takes
    /// precedence over any global configuration firmware overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_overrides: Option<FirmwareConfig>,
}

/// Chip configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ChipConfig {
    /// Filename or URL of any ROM image - filename is only valid if using a
    /// generator tool with local file access.  This is passed to the generator
    /// tool to retrieve the ROM image.
    #[serde(default)]
    pub file: String,

    /// Optional license URL/identifier for the ROM.  This is passed to the
    /// generator tool to retrieve and ask the user to accept before building.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Optional description for this configuration.  This is included in the
    /// description output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Type of ROM
    #[serde(rename = "type")]
    pub chip_type: ChipType,

    /// Optional Chip Select 1 logic - only valid for Chip Types that have CS1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs1: Option<CsLogic>,

    /// Optional Chip Select 2 logic - only valid for Chip Types that have CS2
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs2: Option<CsLogic>,

    /// Optional Chip Select 3 logic - only valid for Chip Types that have CS3
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs3: Option<CsLogic>,

    /// Optional size handling configuration for this Chip.  Used to specify
    /// handling when the image supplied isn't the correct size for this Chip
    /// type.
    #[serde(default, skip_serializing_if = "SizeHandling::is_none")]
    pub size_handling: SizeHandling,

    /// Optional extract path within an archive (zip/tar) if the file pointed
    /// to is an archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<String>,

    /// Optional label for this ROM image.  If specified, this is used in
    /// metadata instead of the filename (which itself can be complex if
    /// extracting a file from an image and providing location information)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Optional location within a larger image file.  Used to specify start
    /// offset and length within the file.  Useful when multiple ROM images
    /// are concatenated into a single file and one needs to be extracted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

impl ChipConfig {
    // Constructs the filename string for metadata.  Note label will be used
    // in metadata instead if specified.
    fn filename(&self) -> String {
        if let Some(label) = &self.label {
            // Return label if we have one
            return label.clone();
        }

        // Base of filename is "file|extract" or just "file"
        let filename_base = if let Some(extract) = &self.extract {
            format!("{}|{}", self.file, extract)
        } else {
            self.file.clone()
        };

        // If location specified, append "|start=0x...,length=0x..."
        if let Some(location) = &self.location {
            format!(
                "{}|start={:#X},length={:#X}",
                filename_base, location.start, location.length
            )
        } else {
            filename_base
        }
    }
}
