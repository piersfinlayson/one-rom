// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use onerom_config::chip::ChipType;
use onerom_config::fw::{FirmwareProperties, FirmwareVersion};
use onerom_config::mcu::Family;
use onerom_metadata::{
    CURRENT_METADATA_VERSION, METADATA_BASE, METADATA_SIZE, ONEROM_METADATA_MAGIC,
    OneromMetadataHeader, OneromRomInfo, OneromRomSlot, Pointer, RomSlotType, serialize,
};

use crate::v2::firmware_config::{build_firmware_config, build_firmware_overrides};
use crate::v2::hardware_info::build_hardware_info;
use crate::v2::rom_image::build_rom_image;
use crate::v2::rom_info::truncate_filename;
use crate::v2::rom_slot::build_rom_slot;
use crate::{
    Chip, ChipConfig, ChipSet, ChipSetType, Config, CsConfig, CsLogic, Error, FileData, FileSpec,
    FireServeMode, License, MetadataWriter, Result,
};
use crate::{
    FIRMWARE_SIZE, MAX_METADATA_LEN, MAX_SUPPORTED_FIRMWARE_VERSION_V1,
    MAX_SUPPORTED_FIRMWARE_VERSION_V2, MIN_SUPPORTED_FIRMWARE_VERSION_V1,
    MIN_SUPPORTED_FIRMWARE_VERSION_V2, Metadata, SUPPORTED_CHIP_TYPES_V1, SUPPORTED_CHIP_TYPES_V2,
    UNSUPPORTED_FIRMWARE_VERSIONS_V1, UNSUPPORTED_FIRMWARE_VERSIONS_V2,
};
use crate::image::requires_half_select_cs1;

/// Main Builder object
///
/// Model is to create the builder from a JSON config, retrieve the list of
/// files that need to be loaded, call `add_file` for each file once loaded,
/// then call `build` to generate the metadata and ROM images.
///
/// The legacy (firmware < 0.7.0) or v2 (firmware >= 0.7.0) build path is
/// selected automatically based on the firmware version passed to
/// `from_json`.
///
/// # Example
/// ```no_run
/// use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
/// use onerom_config::hw::Board;
/// use onerom_config::mcu::{Family, Variant as McuVariant};
/// # use onerom_gen::Error;
/// use onerom_gen::{Builder, FileData, License};
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Builder {
    version: FirmwareVersion,
    config: Config,
    files: BTreeMap<usize, alloc::vec::Vec<u8>>,
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
        if version > MAX_SUPPORTED_FIRMWARE_VERSION_V2 {
            return Err(Error::FirmwareTooNew {
                version,
                maximum: MAX_SUPPORTED_FIRMWARE_VERSION_V2,
            });
        }

        let config: Config = serde_json::from_str(json)?;

        if version >= MIN_SUPPORTED_FIRMWARE_VERSION_V2 {
            validate_config_v2(&version, &mcu_family, &config)?;
        } else {
            validate_config_v1(&version, &mcu_family, &config)?;
        }

        let mut builder = Self {
            version,
            config,
            files: BTreeMap::new(),
            licenses: BTreeMap::new(),
            file_id_map: BTreeMap::new(),
        };

        build_file_id_map(&builder.config, &mut builder.file_id_map);

        Ok(builder)
    }

    /// Get reference to config
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get list of files that need to be loaded
    pub fn file_specs(&self) -> alloc::vec::Vec<FileSpec> {
        file_specs(self.config(), self.file_id_map())
    }

    /// Get description of config for display in UI
    pub fn description(&self) -> String {
        description(self.config(), self.num_chip_sets(), self.num_roms())
    }

    /// Get number of chip sets
    pub fn num_chip_sets(&self) -> usize {
        self.config().chip_sets.len()
    }

    /// Get number of ROMs
    pub fn num_roms(&self) -> usize {
        self.config()
            .chip_sets
            .iter()
            .map(|set| set.chips.len())
            .sum()
    }

    /// Get list of categories this config belongs to, for display in UI
    pub fn categories(&self) -> alloc::vec::Vec<String> {
        categories(self.config())
    }

    /// Get total number of unique files that need to be loaded
    pub fn total_file_count(&self) -> usize {
        total_file_count(self.file_id_map())
    }

    /// Mark a license as validated
    pub fn accept_license(&mut self, license: &License) -> Result<()> {
        accept_license(self.licenses_mut(), license)
    }

    /// Add a file to be included in the build
    pub fn add_file(&mut self, file: FileData) -> Result<()> {
        add_file(&mut self.files, file, &self.file_id_map)
    }

    /// Get list of licenses that need to be validated
    pub fn licenses(&mut self) -> alloc::vec::Vec<License> {
        licenses(&self.config, &mut self.licenses)
    }

    /// Get mutable reference to licenses map (mapping from license ID to
    /// License)
    pub fn licenses_mut(&mut self) -> &mut BTreeMap<usize, License> {
        &mut self.licenses
    }

    /// Get file ID map (mapping from ROM index to file index)
    pub fn file_id_map(&self) -> &BTreeMap<usize, usize> {
        &self.file_id_map
    }

    /// Check that the config can be built
    pub fn build_validation(&self, props: &FirmwareProperties) -> Result<()> {
        check_all_files_loaded(&self.files, self.total_file_count())?;
        check_all_licenses_validated(&self.licenses)?;
        validate_plugins(&self.config, &self.files, &self.file_id_map, props)?;

        Ok(())
    }

    /// Generate metadata and ROM images once all files loaded
    ///
    /// Returns (metadata, Chip images)
    pub fn build(
        &self,
        props: FirmwareProperties,
    ) -> Result<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)> {
        if props.version() > MAX_SUPPORTED_FIRMWARE_VERSION_V2 {
            return Err(Error::FirmwareTooNew {
                version: props.version(),
                maximum: MAX_SUPPORTED_FIRMWARE_VERSION_V2,
            });
        }

        self.build_validation(&props)?;

        if self.version > MAX_SUPPORTED_FIRMWARE_VERSION_V1 {
            self.build_v2(props)
        } else {
            self.build_v1(props)
        }
    }

    /// Legacy (pre-0.7.0) metadata/ROM image build
    fn build_v1(
        &self,
        props: FirmwareProperties,
    ) -> Result<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)> {
        // Reject chip types the target board does not support. This is the same
        // board-level test the CLI applies when parsing --slot, but here it is
        // enforced unconditionally: V1 firmware genuinely cannot serve an
        // unsupported chip type, so the CLI's --allow-unsupported-chip-type
        // override deliberately has no effect on the V1 path. Plugin chip types
        // are gated separately and skipped here.
        let board = props.board();
        for chip_set in self.config.chip_sets.iter() {
            for chip in chip_set.chips.iter() {
                if chip.chip_type.is_plugin() {
                    continue;
                }
                if !board.allows_chip_type(chip.chip_type) {
                    return Err(Error::UnsupportedBoardChipType {
                        board,
                        chip_type: chip.chip_type,
                    });
                }
            }
        }

        // Fire-CPU-serve-mode validation (v1-specific - PIO/CPU board
        // distinction doesn't apply to v2).
        for chip_set_config in self.config.chip_sets.iter() {
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
        }

        let chip_sets = build_chip_sets(&self.config, &self.files, &self.file_id_map, &props)?;

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
        let mut metadata_buf = alloc::vec![0u8; metadata_size];
        let mut rom_data_buf = alloc::vec![0u8; rom_data_size];
        let mut rom_data_ptrs = alloc::vec![0u32; set_count];

        // Write metadata
        metadata.write_all(&mut metadata_buf, &mut rom_data_ptrs)?;
        // Note rom_data_ptrs unused here - absolute flash addresses.

        // Write ROM data
        metadata.write_roms(&mut rom_data_buf)?;

        // Done - return the two buffers
        Ok((metadata_buf, rom_data_buf))
    }

    /// v2 (0.7.0+, RP2350, PIO-only) metadata/ROM image build
    fn build_v2(
        &self,
        props: FirmwareProperties,
    ) -> Result<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)> {
        let board = props.board();
        let chip_sets = build_chip_sets(&self.config, &self.files, &self.file_id_map, &props)?;

        let mut rom_slots = alloc::vec::Vec::with_capacity(chip_sets.len());
        let mut layouts = alloc::vec::Vec::with_capacity(chip_sets.len());
        for chip_set in &chip_sets {
            let firmware_overrides = chip_set
                .firmware_overrides
                .as_ref()
                .map(build_firmware_overrides);

            if chip_set.chips[0].chip_type().is_plugin() {
                let chip = &chip_set.chips[0];
                let slot_type = match chip.chip_type() {
                    ChipType::SystemPlugin => RomSlotType::RomSlotTypePluginSystem,
                    ChipType::UserPlugin => RomSlotType::RomSlotTypePluginUser,
                    ChipType::PioPlugin => RomSlotType::RomSlotTypePluginPio,
                    _ => unreachable!(),
                };
                let slot = OneromRomSlot {
                    data: Pointer::Null,
                    size: chip.data().map(|d| d.len() as u32).unwrap_or(0),
                    roms: alloc::vec![OneromRomInfo {
                        rom_type: chip.chip_type().name().to_string(),
                        filename: truncate_filename(chip.filename()),
                        pin_map: None,
                        chip_size: chip.chip_type().size_bytes() as u32,
                        rbcp_rom_type: chip.chip_type().rbcp_chip_type(),
                    }],
                    rom_count: 1,
                    slot_type,
                    alg: None,
                    firmware_overrides,
                };
                rom_slots.push(slot);
                layouts.push(None);
            } else {
                let force_16_bit = chip_set
                    .firmware_overrides
                    .as_ref()
                    .and_then(|o| o.fire.as_ref())
                    .is_some_and(|fire| fire.force_16_bit);
                let (slot, addr_layout, cs_data_layout, _pref) = build_rom_slot(
                    board,
                    chip_set.set_type,
                    &chip_set.chips,
                    0,
                    firmware_overrides,
                    force_16_bit,
                )
                .map_err(Error::from)?;
                rom_slots.push(slot);
                layouts.push(Some((addr_layout, cs_data_layout)));
            }
        }

        const ROM_DATA_BASE: u32 = METADATA_BASE + METADATA_SIZE as u32;
        let mut rom_data_offset: u32 = 0;
        for slot in &mut rom_slots {
            slot.data = Pointer::Addr32(ROM_DATA_BASE + rom_data_offset);
            rom_data_offset += slot.size;
        }

        let mut rom_data_buf = alloc::vec::Vec::with_capacity(rom_data_offset as usize);
        for ((chip_set, slot), layout) in chip_sets.iter().zip(rom_slots.iter()).zip(layouts.iter())
        {
            let image = match layout {
                Some((addr_layout, cs_data_layout)) => build_rom_image(
                    addr_layout,
                    cs_data_layout,
                    chip_set.set_type,
                    &chip_set.chips,
                    &slot
                        .alg
                        .as_ref()
                        .expect("non-plugin slot must have alg config")
                        .alg_dma,
                )?,
                None => chip_set.chips[0].data().unwrap_or(&[]).to_vec(),
            };
            debug_assert_eq!(image.len() as u32, slot.size);
            rom_data_buf.extend_from_slice(&image);
        }

        let hw = build_hardware_info(board);
        let fw = build_firmware_config(&self.config);

        let mut magic = [0u8; 16];
        magic[..ONEROM_METADATA_MAGIC.len()].copy_from_slice(ONEROM_METADATA_MAGIC.as_bytes());

        let header = OneromMetadataHeader {
            magic,
            version: CURRENT_METADATA_VERSION,
            hw,
            fw,
            rom_slot_count: rom_slots.len() as u8,
            boot_logging: self.config.boot_logging as u8,
            swd_enabled: self.config.swd_enabled as u8,
            turbo_boot: self.config.turbo_boot as u8,
            rom_slots,
        };

        let mut metadata_buf = alloc::vec![0u8; METADATA_SIZE];
        serialize(&header, METADATA_BASE, &mut metadata_buf)?;

        Ok((metadata_buf, rom_data_buf))
    }
}

pub(crate) fn validate_config_version(config: &Config, _version: &FirmwareVersion) -> Result<()> {
    if config.version != 1 {
        return Err(Error::UnsupportedConfigVersion {
            version: config.version,
        });
    }
    Ok(())
}

pub(crate) fn validate_firmware_version(
    version: &FirmwareVersion,
    min: &FirmwareVersion,
    max: &FirmwareVersion,
    unsupported: &[FirmwareVersion],
    feat: &'static str,
) -> Result<()> {
    if version < min {
        return Err(Error::FirmwareTooOld {
            feat,
            version: *version,
            minimum: *min,
        });
    }
    if version > max {
        return Err(Error::FirmwareTooNew {
            version: *version,
            maximum: *max,
        });
    }
    for unsupported_version in unsupported {
        if version == unsupported_version {
            return Err(Error::FirmwareUnsupported { version: *version });
        }
    }
    Ok(())
}

pub(crate) fn build_file_id_map(config: &Config, file_id_map: &mut BTreeMap<usize, usize>) {
    let mut seen_files: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
    let mut file_id = 0;
    let mut chip_id = 0;

    for chip_set in config.chip_sets.iter() {
        for chip in &chip_set.chips {
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

            file_id_map.insert(chip_id, assigned_file_id);
            chip_id += 1;
        }
    }
}

// Returns number of non-plugin ROM slots.
// Validates structural constraints only — chip counts, firmware version
// compatibility, file specs, plugin rules.  CS/CE/OE validation is handled
// separately by check_cs_v1 (V1) or check_cs_v2 (V2).
pub(crate) fn check_chip_sets(
    version: &FirmwareVersion,
    config: &Config,
    supported_chip_types: &[ChipType],
    mcu_family: &Family,
) -> Result<usize> {
    let mut chip_num = 0;
    let mut num_non_plugin_slots = 0;
    for (set_id, set) in config.chip_sets.iter().enumerate() {
        if set.chips.is_empty() {
            return Err(Error::NoChips { id: set_id });
        }

        // FirmwareConfig only supported from 0.6.0 firmware onwards
        #[allow(clippy::collapsible_if)]
        if set.firmware_overrides.is_some() {
            if version < &crate::MIN_FIRMWARE_OVERRIDES_VERSION {
                return Err(Error::FirmwareTooOld {
                    feat: "firmware overrides",
                    version: *version,
                    minimum: crate::MIN_FIRMWARE_OVERRIDES_VERSION,
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

            #[allow(clippy::collapsible_if)]
            if set.set_type == ChipSetType::Banked {
                if set.chips.len() > 4 {
                    return Err(Error::TooManyChips {
                        id: set_id,
                        expected: 4,
                        actual: set.chips.len(),
                    });
                }
            }
        }

        let mut is_plugin = false;
        for chip in set.chips.iter() {
            let chip0 = &set.chips[0];

            // Check chip_type is supported
            if !supported_chip_types.contains(&chip.chip_type) {
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
            if chip.file.is_empty()
                && chip.chip_type.chip_function() != onerom_config::chip::ChipFunction::Ram
            {
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

            // Validate location if present
            if let Some(location) = &chip.location {
                if location.length == 0 {
                    return Err(Error::InvalidConfig {
                        error: format!("Chip {} location length must be non-zero", chip_num),
                    });
                }

                if location.start.checked_add(location.length).is_none() {
                    return Err(Error::InvalidConfig {
                        error: format!("Chip {} location start + length overflows", chip_num),
                    });
                }
            }

            // Check for plugins on Ice (not supported)
            if chip.chip_type.is_plugin() {
                if matches!(mcu_family, Family::Stm32f4) {
                    return Err(Error::InvalidConfig {
                        error: format!("Plugins are not supported on Ice (Chip {})", chip_num),
                    });
                }
                is_plugin = true;
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
                    if config.chip_sets[0].chips[0].chip_type != ChipType::SystemPlugin {
                        return Err(Error::InvalidConfig {
                            error: "User plugins must be in the second slot, and the first slot must be a system plugin".to_string()
                        });
                    }
                }
            }

            chip_num += 1;
        }

        if !is_plugin {
            num_non_plugin_slots += 1;
        }
    }

    Ok(num_non_plugin_slots)
}

/// V1 CS validation — validates cs1/cs2/cs3 for all chip sets.
/// Behaviour is identical to pre-split check_chip_sets; ce/oe are not
/// configurable in V1 and are treated as always Ignore.
pub(crate) fn check_cs_v1(config: &Config) -> Result<()> {
    let mut chip_num = 0usize;

    for set in config.chip_sets.iter() {
        for chip in set.chips.iter() {
            // Check that required CS lines are specified
            for line in chip.chip_type.control_lines() {
                let cs = match line.name {
                    "cs1" => chip.cs1,
                    "cs2" => chip.cs2,
                    "cs3" => chip.cs3,
                    // V1 does not configure ce/oe — treat as always active
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

            // CS1 cannot be Ignore if CS2/CS3 are active
            if !cs1_active && (cs2_active || cs3_active) {
                return Err(Error::InvalidConfig {
                    error: "CS1 cannot be ignore when CS2 or CS3 are active".to_string(),
                });
            }
            // CS2 cannot be Ignore if CS3 is active
            if !cs2_active && cs3_active {
                return Err(Error::InvalidConfig {
                    error: "CS2 cannot be ignore when CS3 is active".to_string(),
                });
            }

            // Check that the correct CS lines are specified for the Chip type
            let mut required_cs_lines: BTreeSet<&str> = chip
                .chip_type
                .control_lines()
                .iter()
                .filter(|line| matches!(line.name, "cs1" | "cs2" | "cs3"))
                .map(|line| line.name)
                .collect();
            // V1 special case: 27C080 has no CS lines but we treat A19 as CS1
            // so it can switch between two One ROMs each serving half.
            if chip.chip_type == ChipType::Chip27C080 {
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
                // Single-chip sets: no CS line may be Ignore
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
                // For multi ROM sets, CS1 must be active
                if set.set_type == ChipSetType::Multi && !cs1_active {
                    return Err(Error::InvalidConfig {
                        error: format!(
                            "CS1 cannot be ignore for multi-ROM sets (Chip {})",
                            chip_num
                        ),
                    });
                }
            }

            chip_num += 1;
        }

        // CS polarity consistency across chips in multi/banked sets
        #[allow(clippy::collapsible_if)]
        if set.set_type == ChipSetType::Multi || set.set_type == ChipSetType::Banked {
            if set.chips.len() > 1 {
                let first_cs1 = set.chips[0].cs1;
                let first_cs2 = set.chips[0].cs2;
                let first_cs3 = set.chips[0].cs3;

                for (idx, rom) in set.chips.iter().enumerate().skip(1) {
                    if rom.cs1 != first_cs1 || rom.cs2 != first_cs2 || rom.cs3 != first_cs3 {
                        if (rom.cs2 != first_cs2)
                            && let Some(cs) = rom.cs2
                            && (cs == CsLogic::Ignore)
                        {
                            // Ignore difference if cs2 is ignore
                            continue;
                        }
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "{:?} set requires all ROMs to have identical CS configuration. \
                                 ROM 0 has cs1={:?}/cs2={:?}/cs3={:?}, but ROM {} has \
                                 cs1={:?}/cs2={:?}/cs3={:?}",
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


/// V2 CS/CE/OE validation.
///
/// Handles:
/// - ce/oe override fields (only valid for chip types that have those lines)
/// - CS lines against the chip type: configurable lines must be specified,
///   fixed lines must not have their polarity stated
/// - allow_cs_ignore flag and the ignore permission rules:
///   (a) chips[1+] in a multi-ROM set — implicit free pass
///   (b) lines with allow_ignore in chip_types.json — implicit free pass
///   (c) all other cases — require allow_cs_ignore on ChipConfig
/// - Polarity consistency across chips in multi/banked sets
pub(crate) fn check_cs_v2(config: &Config) -> Result<()> {
    use onerom_config::chip::ControlLineType;
 
    for (set_id, set) in config.chip_sets.iter().enumerate() {
        // Plugin sets have no CS lines — skip
        if set.chips.iter().all(|c| c.chip_type.is_plugin()) {
            continue;
        }
 
        for (chip_idx, chip) in set.chips.iter().enumerate() {
            if chip.chip_type.is_plugin() {
                continue;
            }
 
            let is_multi_secondary = set.set_type == ChipSetType::Multi && chip_idx > 0;
 
            // ---- ce/oe fields only valid for chip types that have those lines ----
            let has_ce = chip
                .chip_type
                .control_lines()
                .iter()
                .any(|l| l.name == "ce");
            let has_oe = chip
                .chip_type
                .control_lines()
                .iter()
                .any(|l| l.name == "oe");
 
            if chip.ce.is_some() && !has_ce {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "ce specified for chip type {} which has no /CE line (set {}, chip {})",
                        chip.chip_type.name(),
                        set_id,
                        chip_idx
                    ),
                });
            }
            if chip.oe.is_some() && !has_oe {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "oe specified for chip type {} which has no /OE line (set {}, chip {})",
                        chip.chip_type.name(),
                        set_id,
                        chip_idx
                    ),
                });
            }
 
            // ---- Both CE and OE may not both be Ignore ----
            let ce_logic = chip.ce.unwrap_or(CsLogic::ActiveLow);
            let oe_logic = chip.oe.unwrap_or(CsLogic::ActiveLow);
            if has_ce && has_oe && ce_logic == CsLogic::Ignore && oe_logic == CsLogic::Ignore {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "Both CE and OE cannot be Ignore simultaneously for chip type {} \
                         (set {}, chip {})",
                        chip.chip_type.name(),
                        set_id,
                        chip_idx
                    ),
                });
            }
 
            // ---- Validate each CS line against the chip type ----
            //
            // A CS line's polarity is either mask-programmed at manufacture
            // (Configurable — the user must state it, there is no default) or
            // fixed by the silicon (FixedActiveLow/FixedActiveHigh — the user
            // must not state it). `Ignore` is not a polarity: it says this One
            // ROM does not monitor the line, so it remains available on fixed
            // lines and is policed by the allow_cs_ignore rules below.
            //
            // A chip whose address space exceeds MAX_IMAGE_SIZE has no cs1
            // control line, but requires cs1 to select which half of it this
            // One ROM serves - cs1 there names the excess top address line,
            // not a pin. Derived from the chip type's address line count
            // rather than named explicitly, so any future oversized chip type
            // works without touching this. See `requires_half_select_cs1`.
            let half_select_cs1 = requires_half_select_cs1(&chip.chip_type);
            let cs_values = [
                ("cs1", chip.cs1),
                ("cs2", chip.cs2),
                ("cs3", chip.cs3),
                ("cs4", chip.cs4),
            ];
 
            for (name, user) in cs_values {
                let spec = chip
                    .chip_type
                    .control_lines()
                    .iter()
                    .find(|l| l.name == name);
                let virtual_cs1 = half_select_cs1 && name == "cs1";
 
                match (spec, user) {
                    // Line the chip doesn't have.
                    (None, Some(_)) if !virtual_cs1 => {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "{} specified for chip type {} which does not use it \
                                 (set {}, chip {})",
                                name.to_uppercase(),
                                chip.chip_type.name(),
                                set_id,
                                chip_idx
                            ),
                        });
                    }
                    // Oversized chip's half-select.
                    (None, None) if virtual_cs1 => {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "Chip type {} requires cs1 (the half-select) to be \
                                 specified (set {}, chip {})",
                                chip.chip_type.name(),
                                set_id,
                                chip_idx
                            ),
                        });
                    }
                    // Configurable polarity: the user must state it.
                    (Some(s), None) if s.line_type == ControlLineType::Configurable => {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "Chip type {} requires configurable CS line {} to be \
                                 specified (set {}, chip {})",
                                chip.chip_type.name(),
                                name,
                                set_id,
                                chip_idx
                            ),
                        });
                    }
                    // Fixed polarity: the user must not state it.
                    (Some(s), Some(logic))
                        if s.line_type != ControlLineType::Configurable
                            && logic != CsLogic::Ignore =>
                    {
                        return Err(Error::InvalidConfig {
                            error: format!(
                                "{} polarity is fixed by chip type {} and cannot be \
                                 configured; only 'ignore' may be specified \
                                 (set {}, chip {})",
                                name.to_uppercase(),
                                chip.chip_type.name(),
                                set_id,
                                chip_idx
                            ),
                        });
                    }
                    _ => {}
                }
            }
 
            // ---- Ignore permission check ----
            //
            // Ignore is implicitly permitted:
            //   (a) for any line on a multi-ROM secondary chip (chips[1+])
            //   (b) for lines with allow_ignore: true in chip_types.json
            //
            // All other uses of Ignore require allow_cs_ignore on ChipConfig.
            let check_ignore = |line_name: &str, logic: CsLogic| -> Result<()> {
                if logic != CsLogic::Ignore {
                    return Ok(());
                }
                // (a) secondary chip in multi set
                if is_multi_secondary {
                    return Ok(());
                }
                // (b) per-line allow_ignore in chip_types.json
                let line_spec = chip
                    .chip_type
                    .control_lines()
                    .iter()
                    .find(|l| l.name == line_name);
                if line_spec.is_some_and(|l| l.allow_ignore) {
                    return Ok(());
                }
                // (c) explicit user opt-in
                if chip.allow_cs_ignore {
                    return Ok(());
                }
                Err(Error::InvalidConfig {
                    error: format!(
                        "{} is set to Ignore for chip type {} (set {}, chip {}) but \
                         allow_cs_ignore is not set. Ignoring a control line can cause \
                         bus contention — set allow_cs_ignore: true to confirm this is \
                         intentional.",
                        line_name.to_uppercase(),
                        chip.chip_type.name(),
                        set_id,
                        chip_idx
                    ),
                })
            };
 
            for (name, user) in cs_values {
                if let Some(logic) = user {
                    check_ignore(name, logic)?;
                }
            }
            if has_ce {
                check_ignore("ce", ce_logic)?;
            }
            if has_oe {
                check_ignore("oe", oe_logic)?;
            }
 
            // ---- CS ordering rules for configurable-CS chips ----
            // When allow_cs_ignore is NOT set, the natural expectation is that
            // CS lines are used from CS1 outward — e.g. CS1 only, or CS1+CS2,
            // or CS1+CS2+CS3. Any other ordering (e.g. CS1 ignored but CS2
            // active) must be explicitly opted into with allow_cs_ignore.
            // Multi secondary chips are exempt (they may ignore any combination).
            if !has_ce && !has_oe && !is_multi_secondary && !chip.allow_cs_ignore {
                let cs1_active = chip.cs1.is_some_and(|l| l != CsLogic::Ignore);
                let cs2_active = chip.cs2.is_some_and(|l| l != CsLogic::Ignore);
                let cs3_active = chip.cs3.is_some_and(|l| l != CsLogic::Ignore);
                let cs4_active = chip.cs4.is_some_and(|l| l != CsLogic::Ignore);
 
                if !cs1_active && (cs2_active || cs3_active || cs4_active) {
                    return Err(Error::InvalidConfig {
                        error: alloc::format!(
                            "CS1 cannot be Ignore when CS2, CS3 or CS4 are active for chip \
                             type {} (set {}, chip {}) unless allow_cs_ignore is set",
                            chip.chip_type.name(),
                            set_id,
                            chip_idx
                        ),
                    });
                }
                if !cs2_active && (cs3_active || cs4_active) {
                    return Err(Error::InvalidConfig {
                        error: alloc::format!(
                            "CS2 cannot be Ignore when CS3 or CS4 are active for chip type {} \
                             (set {}, chip {}) unless allow_cs_ignore is set",
                            chip.chip_type.name(),
                            set_id,
                            chip_idx
                        ),
                    });
                }
                if !cs3_active && cs4_active {
                    return Err(Error::InvalidConfig {
                        error: alloc::format!(
                            "CS3 cannot be Ignore when CS4 is active for chip type {} \
                             (set {}, chip {}) unless allow_cs_ignore is set",
                            chip.chip_type.name(),
                            set_id,
                            chip_idx
                        ),
                    });
                }
            }
        }
 
        // ---- Multi set consistency validation ----
        //
        // For Multi sets with chips having N > 1 control lines, chips[1+]
        // must each have exactly 1 active line (the per-chip select, fly-leaded
        // to X1/X2) and ignore the remaining N-1 (the commoned lines). All
        // chips[1+] must ignore the same set of lines. Chip[0] must not ignore
        // its per-chip select line.
        //
        // Only the Ignore/not-Ignore distinction matters here, so the
        // ActiveLow default used for unspecified lines is safe even where the
        // real polarity is fixed active-high (the line is active either way).
        if set.set_type == ChipSetType::Multi && set.chips.len() >= 2 {
            let chip1 = &set.chips[1];
            let chip_type = chip1.chip_type;
            let control_lines = chip_type.control_lines();
 
            let line_logic = |chip: &ChipConfig, name: &str| -> CsLogic {
                match name {
                    "ce" => chip.ce.unwrap_or(CsLogic::ActiveLow),
                    "oe" => chip.oe.unwrap_or(CsLogic::ActiveLow),
                    "cs1" => chip.cs1.unwrap_or(CsLogic::ActiveLow),
                    "cs2" => chip.cs2.unwrap_or(CsLogic::ActiveLow),
                    "cs3" => chip.cs3.unwrap_or(CsLogic::ActiveLow),
                    "cs4" => chip.cs4.unwrap_or(CsLogic::ActiveLow),
                    _ => CsLogic::ActiveLow,
                }
            };
 
            // Collect (name, logic) for all control lines of chips[1],
            // using defaults (ActiveLow) for unspecified lines.
            let line_logics: alloc::vec::Vec<(&str, CsLogic)> = control_lines
                .iter()
                .filter(|l| matches!(l.name, "ce" | "oe" | "cs1" | "cs2" | "cs3" | "cs4"))
                .map(|l| (l.name, line_logic(chip1, l.name)))
                .collect();
 
            let num_lines = line_logics.len();
            let active: alloc::vec::Vec<&str> = line_logics
                .iter()
                .filter(|(_, l)| *l != CsLogic::Ignore)
                .map(|(n, _)| *n)
                .collect();
 
            // For single-line chips, no ignores needed.
            if num_lines > 1 && active.len() != 1 {
                return Err(Error::InvalidConfig {
                    error: alloc::format!(
                        "Multi set secondary chips must have exactly 1 active control line \
                         (the per-chip select) and ignore the rest. Chip type {} (set {}) \
                         has {} active lines: {:?}",
                        chip_type.name(),
                        set_id,
                        active.len(),
                        active
                    ),
                });
            }
 
            // All chips[1+] must agree on which lines are active/ignored.
            for (idx, chip) in set.chips.iter().enumerate().skip(2) {
                for &(name, ref_logic) in &line_logics {
                    let chip_is_ignore = line_logic(chip, name) == CsLogic::Ignore;
                    let ref_is_ignore = ref_logic == CsLogic::Ignore;
                    if chip_is_ignore != ref_is_ignore {
                        return Err(Error::InvalidConfig {
                            error: alloc::format!(
                                "Multi set secondary chips must all ignore the same control \
                                 lines. Chip {} (set {}, chip {}) differs from chip 1 on \
                                 line '{}'",
                                chip_type.name(),
                                set_id,
                                idx,
                                name
                            ),
                        });
                    }
                }
            }
 
            // Chip[0] must not ignore the per-chip select line.
            if let Some(&per_chip_select_name) = active.first() {
                let chip0 = &set.chips[0];
                if line_logic(chip0, per_chip_select_name) == CsLogic::Ignore {
                    return Err(Error::InvalidConfig {
                        error: alloc::format!(
                            "Multi set chip 0 cannot ignore '{}', which is the per-chip \
                             select line for this set (set {})",
                            per_chip_select_name,
                            set_id
                        ),
                    });
                }
            }
        }
 
        // ---- CS polarity consistency for Banked sets only ----
        //
        // All chips in a Banked set share the same physical CS line on the board,
        // so they must agree on its polarity: the same voltage level can't be
        // simultaneously "active" for one bank and "inactive" for another.
        //
        // Multi sets are exempt: each chip's per-chip select is a different
        // physical GPIO (chip[0] uses CS1/CE/OE, chips[1+] use X1/X2 fly-leads).
        // Those are independent signals with independent GpioOverInvert handling,
        // so there is no physical requirement for their polarities to match.
        if set.set_type == ChipSetType::Banked && set.chips.len() > 1 {
            let primary = cs_primary_polarity(&set.chips[0]);
            for chip in set.chips.iter().skip(1) {
                let polarity = cs_primary_polarity(chip);
                if polarity != primary {
                    return Err(Error::InconsistentCsLogic {
                        first: primary.unwrap_or(CsLogic::ActiveLow),
                        other: polarity.unwrap_or(CsLogic::ActiveLow),
                    });
                }
            }
        }
    }
 
    Ok(())
}

/// Returns the polarity of the primary chip-select line for a chip.
///
/// For CE/OE chips: the non-Ignore line (CE takes precedence if both active).
/// For configurable-CS chips: cs1.
pub(crate) fn cs_primary_polarity(chip: &ChipConfig) -> Option<CsLogic> {
    let has_ce = chip
        .chip_type
        .control_lines()
        .iter()
        .any(|l| l.name == "ce");
    if has_ce {
        let ce = chip.ce.unwrap_or(CsLogic::ActiveLow);
        let oe = chip.oe.unwrap_or(CsLogic::ActiveLow);
        if ce != CsLogic::Ignore {
            Some(ce)
        } else {
            Some(oe)
        }
    } else {
        chip.cs1
    }
}

pub(crate) fn file_specs(
    config: &Config,
    file_id_map: &BTreeMap<usize, usize>,
) -> alloc::vec::Vec<FileSpec> {
    let mut specs = alloc::vec::Vec::new();
    let mut seen_files: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
    let mut rom_id = 0;

    for (chip_set_num, chip_set) in config.chip_sets.iter().enumerate() {
        for rom in &chip_set.chips {
            if rom.file.is_empty() {
                rom_id += 1;
                continue;
            }
            let key = (rom.file.clone(), rom.extract.clone());
            let file_id = *file_id_map.get(&rom_id).unwrap();

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
                    cs4: rom.cs4,
                    ce: rom.ce,
                    oe: rom.oe,
                    set_id: chip_set_num,
                    set_type: chip_set.set_type,
                    set_description: chip_set.description.clone(),
                });
                file_id
            });

            rom_id += 1;
        }
    }

    specs
}

pub(crate) fn add_file(
    files: &mut BTreeMap<usize, alloc::vec::Vec<u8>>,
    file: FileData,
    file_id_map: &BTreeMap<usize, usize>,
) -> Result<()> {
    if files.contains_key(&file.id) {
        return Err(Error::DuplicateFile { id: file.id });
    }

    let total_files = total_file_count(file_id_map);
    if file.id >= total_files {
        return Err(Error::InvalidFile {
            id: file.id,
            total: total_files,
        });
    }

    files.insert(file.id, file.data);
    Ok(())
}

pub(crate) fn licenses(
    config: &Config,
    licenses: &mut BTreeMap<usize, License>,
) -> alloc::vec::Vec<License> {
    let mut licenses_vec = alloc::vec::Vec::new();

    let mut license_id = 0;
    let mut rom_id = 0;
    for chip_set in config.chip_sets.iter() {
        for rom in &chip_set.chips {
            if let Some(ref url) = rom.license {
                let license = License::new(license_id, rom_id, url.clone());
                licenses_vec.push(license.clone());
                licenses.insert(license_id, license);
                license_id += 1;
            }
            rom_id += 1;
        }
    }

    licenses_vec
}

pub(crate) fn accept_license(
    licenses: &mut BTreeMap<usize, License>,
    license: &License,
) -> Result<()> {
    let own_license = licenses
        .get_mut(&license.id)
        .ok_or(Error::InvalidLicense { id: license.id })?;

    own_license.validated = true;
    Ok(())
}

pub(crate) fn total_file_count(file_id_map: &BTreeMap<usize, usize>) -> usize {
    file_id_map.values().collect::<BTreeSet<_>>().len()
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
pub(crate) fn description(config: &Config, num_chip_sets: usize, num_roms: usize) -> String {
    let mut desc = String::new();

    if let Some(name) = config.name.as_ref() {
        desc.push_str(name);
        desc.push('\n');
        desc.push_str(&"-".repeat(name.len()));
        desc.push_str("\n\n");
    }

    desc.push_str(&config.description);
    desc.push_str("\n\n");

    if let Some(detail) = &config.detail {
        desc.push_str(detail);
        desc.push_str("\n\n");
    }

    let multi_chip_sets = if num_chip_sets == num_roms {
        desc.push_str("Images:");
        false
    } else {
        desc.push_str("Sets:");
        true
    };
    desc.push('\n');

    let mut none = true;
    for (ii, set) in config.chip_sets.iter().enumerate() {
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

    if let Some(notes) = &config.notes {
        desc.push('\n');
        desc.push_str(notes);
    } else {
        desc.pop();
    }

    desc
}

pub(crate) fn categories(config: &Config) -> alloc::vec::Vec<String> {
    let mut categories = alloc::vec::Vec::new();
    if let Some(cats) = &config.categories {
        for cat in cats {
            categories.push(cat.clone());
        }
    }
    categories
}

pub(crate) fn check_all_files_loaded(
    files: &BTreeMap<usize, alloc::vec::Vec<u8>>,
    total_file_count: usize,
) -> Result<()> {
    for ii in 0..total_file_count {
        if !files.contains_key(&ii) {
            return Err(Error::MissingFile { id: ii });
        }
    }
    Ok(())
}

pub(crate) fn check_all_licenses_validated(licenses: &BTreeMap<usize, License>) -> Result<()> {
    for (id, license) in licenses.iter() {
        if !license.validated {
            return Err(Error::UnvalidatedLicense { id: *id });
        }
    }
    Ok(())
}

pub(crate) fn validate_plugins(
    config: &Config,
    files: &BTreeMap<usize, alloc::vec::Vec<u8>>,
    file_id_map: &BTreeMap<usize, usize>,
    props: &FirmwareProperties,
) -> Result<()> {
    let mut rom_id = 0;
    for set in config.chip_sets.iter() {
        for rom in set.chips.iter() {
            if matches!(
                rom.chip_type,
                ChipType::SystemPlugin | ChipType::UserPlugin | ChipType::PioPlugin
            ) {
                let file_id = file_id_map.get(&rom_id).unwrap();
                let data = files.get(file_id).unwrap();

                if data.len() < 256 {
                    return Err(Error::InvalidPluginImage {
                        plugin_type: rom.chip_type,
                        image_file: rom.file.clone(),
                        error:
                            "Plugin image is smaller than the required plugin header (256 bytes)."
                                .to_string(),
                    });
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
                            "Plugin requires at least firmware version {} which is newer than \
                             the firmware version being built for ({})",
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

/// Build `ChipSet`s (with `Chip`s, including loaded ROM image data) from
/// `config`, `files` and `file_id_map`.
///
/// Shared between v1 and v2. Any builder-specific pre-validation (e.g. v1's
/// Fire CPU-serve-mode checks) is the caller's responsibility, run
/// separately before calling this.
pub(crate) fn build_chip_sets(
    config: &Config,
    files: &BTreeMap<usize, alloc::vec::Vec<u8>>,
    file_id_map: &BTreeMap<usize, usize>,
    props: &FirmwareProperties,
) -> Result<alloc::vec::Vec<ChipSet>> {
    let mut chip_sets = alloc::vec::Vec::new();
    let mut chip_id = 0;

    for (set_id, chip_set_config) in config.chip_sets.iter().enumerate() {
        let mut set_roms = alloc::vec::Vec::new();

        for chip_config in &chip_set_config.chips {
            let data = if let Some(&file_id) = file_id_map.get(&chip_id) {
                Some(files.get(&file_id).unwrap())
            } else {
                None
            };

            let filename = chip_config.filename();

            // Resolve the chip's control line configuration against its chip
            // type: fixed-polarity CS lines take their polarity from the
            // silicon, configurable ones from the user, and chip types with no
            // CS lines at all fall through to CE/OE.
            let cs_config = CsConfig::from_chip_type(
                &chip_config.chip_type,
                chip_config.cs1,
                chip_config.cs2,
                chip_config.cs3,
                chip_config.cs4,
                chip_config.ce,
                chip_config.oe,
            );

            let rom = Chip::from_raw_rom_image(
                chip_id,
                filename,
                chip_config.label.clone(),
                data.map(|v| &**v),
                alloc::vec![0u8; chip_config.chip_type.size_bytes()],
                &chip_config.chip_type,
                cs_config,
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
            chip_set_config.set_type,
            serve_alg,
            set_roms,
            chip_set_config.firmware_overrides.clone(),
        )?;
        chip_sets.push(chip_set);
    }

    Ok(chip_sets)
}

fn validate_config_v1(
    version: &FirmwareVersion,
    mcu_family: &Family,
    config: &Config,
) -> Result<()> {
    validate_config_version(config, version)?;
    validate_firmware_version(
        version,
        &MIN_SUPPORTED_FIRMWARE_VERSION_V1,
        &MAX_SUPPORTED_FIRMWARE_VERSION_V1,
        &UNSUPPORTED_FIRMWARE_VERSIONS_V1,
        "pre-0.7.0 firmware",
    )?;
    if config.version != 1 {
        return Err(Error::UnsupportedConfigVersion {
            version: config.version,
        });
    }

    for unsupported_version in UNSUPPORTED_FIRMWARE_VERSIONS_V1.iter() {
        if unsupported_version.matches_release(version) {
            return Err(Error::FirmwareUnsupported { version: *version });
        }
    }

    let _ = check_chip_sets(version, config, SUPPORTED_CHIP_TYPES_V1, mcu_family)?;
    check_cs_v1(config)?;

    // V1 does not support CE/OE overrides — reject them explicitly so users
    // get a clear error rather than silent ignore
    for (set_id, set) in config.chip_sets.iter().enumerate() {
        for (chip_idx, chip) in set.chips.iter().enumerate() {
            if chip.ce.is_some() {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "ce is not supported in V1 configurations (set {}, chip {})",
                        set_id, chip_idx
                    ),
                });
            }
            if chip.oe.is_some() {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "oe is not supported in V1 configurations (set {}, chip {})",
                        set_id, chip_idx
                    ),
                });
            }
        }
    }

    // Special ROM type handling for V1
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

    // Special config handling for V1
    #[allow(clippy::collapsible_if)]
    if config.instance_name.is_some() {
        return Err(Error::InvalidConfig {
            error: "instance_name is not supported by this firmware version".to_string(),
        });
    }
    if config.boot_logging {
        return Err(Error::InvalidConfig {
            error: "boot_logging is not supported by this firmware version".to_string(),
        });
    }
    if config.turbo_boot {
        return Err(Error::InvalidConfig {
            error: "turbo_boot is not supported by this firmware version".to_string(),
        });
    }
    if !config.swd_enabled {
        return Err(Error::InvalidConfig {
            error: "swd_enabled = false is not supported by this firmware version".to_string(),
        });
    }

    Ok(())
}

fn validate_config_v2(
    version: &FirmwareVersion,
    mcu_family: &Family,
    config: &Config,
) -> Result<()> {
    if !matches!(mcu_family, Family::Rp2350) {
        return Err(Error::UnsupportedMcuFamily {
            family: *mcu_family,
            version: *version,
        });
    }
    validate_config_version(config, version)?;
    validate_firmware_version(
        version,
        &MIN_SUPPORTED_FIRMWARE_VERSION_V2,
        &MAX_SUPPORTED_FIRMWARE_VERSION_V2,
        UNSUPPORTED_FIRMWARE_VERSIONS_V2,
        "v0.7.0+ firmware",
    )?;

    let num_non_plugin_slots =
        check_chip_sets(version, config, SUPPORTED_CHIP_TYPES_V2, &Family::Rp2350)?;
    check_cs_v2(config)?;

    // Special config handling for V2

    // V2 firmware (v0.7.0+) has no CPU-serving path - PIO serving only.
    for chip_set_config in config.chip_sets.iter() {
        if let Some(overrides) = chip_set_config.firmware_overrides.as_ref()
            && let Some(fire) = overrides.fire.as_ref()
            && fire.serve_mode == Some(FireServeMode::Cpu)
        {
            return Err(Error::InvalidConfig {
                error:
                    "Fire CPU serving mode is not supported by firmware v0.7.0+ (PIO serving only)"
                        .to_string(),
            });
        }
    }

    for chip_set_config in config.chip_sets.iter() {
        if let Some(overrides) = chip_set_config.firmware_overrides.as_ref() {
            if overrides.ice.is_some() {
                return Err(Error::InvalidConfig {
                    error: "Ice firmware overrides are not supported by firmware v0.7.0+ (Fire/RP2350 only)".to_string(),
                });
            }
            if let Some(fire) = overrides.fire.as_ref() {
                if fire.serve_mode == Some(FireServeMode::Cpu) {
                    return Err(Error::InvalidConfig {
                        error: "Fire CPU serving mode is not supported by firmware v0.7.0+ (PIO serving only)".to_string(),
                    });
                }
                if !fire.rom_dma_preload {
                    return Err(Error::InvalidConfig {
                        error: "Disabling ROM DMA preload (rom_dma_preload: false) has no effect and is not supported by firmware v0.7.0+ - remove this override".to_string(),
                    });
                }
            }
        }
    }

    if !config.swd_enabled && config.boot_logging {
        return Err(Error::InvalidConfig {
            error: "Boot logging cannot be enabled when SWD is disabled".to_string(),
        });
    }
    if config.turbo_boot && num_non_plugin_slots > 1 {
        return Err(Error::InvalidConfig {
            error: "Turbo boot cannot be enabled when there is more than one non-plugin ROM slot"
                .to_string(),
        });
    }

    Ok(())
}
