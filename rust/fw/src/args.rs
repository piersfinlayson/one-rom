// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use clap::Parser as ClapParser;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw_parser::{Parser, readers::MemoryReader};

use crate::Error;
use crate::net::Releases;

#[derive(ClapParser, Debug)]
#[clap(
    name = "onerom-fw",
    about = "One ROM's CLI Firmware Generator",
    long_about = "One ROM's Command Line Firmware Generator\n\nAlternatively use https://onerom.org/studio/",
    version
)]
pub struct Args {
    /// List board types
    #[clap(
        long,
        long_help = "List supported board types",
        conflicts_with_all = &["board", "mcu", "rom", "out"],
        alias = "boards",
    )]
    pub list_boards: bool,

    /// List MCU variants
    #[clap(
        long,
        long_help = "List supported MCU variants",
        conflicts_with_all = &["board", "mcu", "rom", "out"],
        alias = "mcus",
    )]
    pub list_mcus: bool,

    /// List firmware versions
    #[clap(
        long,
        long_help = "List available firmware versions",
        conflicts_with_all = &["board", "mcu", "rom", "out"],
        alias = "versions",
    )]
    pub list_fw_versions: bool,

    /// Firmware image file to use as base
    #[clap(
        long,
        long_help = "Firmware image file to use as base.\nBoard type, MCU variant, and firmware version will be detected from the image",
        value_parser,
        conflicts_with_all = &["board", "mcu", "fw"],
    )]
    pub fw_image: Option<String>,

    /// Board type and revision
    #[clap(
        long,
        value_parser=board_value_parser,
        required_unless_present_any = &["list_boards", "list_mcus", "list_fw_versions", "fw_image"],
    )]
    pub board: Option<Board>,

    /// MCU variant
    #[clap(
        long,
        value_parser=mcu_value_parser,
        required_unless_present_any = &["list_boards", "list_mcus", "list_fw_versions", "fw_image"],
    )]
    pub mcu: Option<McuVariant>,

    /// Firmware version
    #[clap(
        short,
        long,
        long_help = "Firmware version to use (default latest)",
        value_parser=firmware_value_parser,
    )]
    pub fw: Option<FirmwareVersion>,

    /// ROM configuration JSON file
    #[clap(
        short,
        long,
        long_help = "ROM configuration JSON file.\nWithout this, a default firmware with no metadata or ROMs is generated.\nThis One ROM can then be updated later",
        value_parser,
        alias = "json"
    )]
    pub rom: Option<String>,

    /// Output firmware binary filename
    #[clap(
        short,
        long,
        long_help = "Output firmware binary filename.\nIf not specified, a default name is used",
        value_parser
    )]
    pub out: Option<String>,

    /// Verbose output
    #[clap(short, long, action)]
    pub verbose: bool,
}

impl Args {
    pub async fn validate(&mut self) -> Result<bool, Error> {
        // If listing, just list and exit
        let mut listed = false;
        if self.list_boards {
            listed = true;
            println!("Supported One ROM Boards: {}", board_values());
        }
        if self.list_mcus {
            listed = true;
            println!("Supported MCU Variants: {}", mcu_values());
        }
        if self.list_fw_versions {
            listed = true;
            let releases = Releases::from_network()?;
            let releases_str = releases
                .releases()
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "Available Firmware Versions (latest = {}): {}",
                releases.latest(),
                releases_str
            );
        }

        if listed {
            return Ok(true);
        }

        // Generate a default output filename if not specified
        if self.out.is_none() {
            self.out = Some("onerom-fw.bin".to_string());
        }

        // Check firmware image file exists if specified
        if let Some(ref fw_image_file) = self.fw_image {
            if !std::path::Path::new(fw_image_file).exists() {
                return Err(Error::config(format!(
                    "Firmware image file `{}` does not exist",
                    fw_image_file
                )));
            }

            // Extract metadata and populate fields
            let (version, board, mcu) = load_firmware_metadata(fw_image_file).await?;
            self.fw = Some(version);
            self.board = Some(board);
            self.mcu = Some(mcu);

            debug!("Extracted firmware metadata from image:");
            debug!("  Board: {:?}", self.board.as_ref().unwrap());
            debug!("  MCU: {:?}", self.mcu.as_ref().unwrap());
            debug!("  Firmware Version: {:?}", self.fw.as_ref().unwrap());
        }

        // Check required arguments
        if self.board.is_none() {
            return Err(Error::config("Board type is required".to_string()));
        }
        if self.mcu.is_none() {
            return Err(Error::config("MCU variant is required".to_string()));
        }

        // Check the board and MCU are compatible
        if self.board.as_ref().map(|b| b.mcu_family()) != self.mcu.as_ref().map(|m| m.family()) {
            return Err(Error::config(
                "MCU variant is not supported by the selected board".to_string(),
            ));
        }

        // Check the file exists if specified
        #[allow(clippy::collapsible_if)]
        if let Some(ref rom_file) = self.rom {
            if !std::path::Path::new(rom_file).exists() {
                return Err(Error::config(format!(
                    "ROM configuration file `{}` does not exist",
                    rom_file
                )));
            }
        }

        // Check the release exists, assuming the firmware image wasn't supplied
        if self.fw_image.is_none() {
            let releases = Releases::from_network()?;
            if let Some(version) = self.fw {
                debug!("Firmware version specified: {:?}", version);
                if releases.release(&version).is_none() {
                    let error_message = format!(
                        "Firmware version `{}.{}.{}` not available.\n  Check {}\n  Available releases: {}\n  Latest release: {}",
                        version.major(),
                        version.minor(),
                        version.patch(),
                        Releases::manifest_url(),
                        releases.releases_str(),
                        releases.latest(),
                    );
                    return Err(Error::config(error_message));
                }
            } else {
                let latest = releases.latest();
                debug!("Firmware version not specified, using latest: {}", latest);
                self.fw =
                    Some(FirmwareVersion::try_from_str(latest).map_err(Error::firmware_version)?);
            }
        }

        Ok(false)
    }
}

fn board_value_parser(s: &str) -> Result<Board, String> {
    Board::try_from_str(s).ok_or("Invalid board type".to_string())
}

fn mcu_value_parser(s: &str) -> Result<McuVariant, String> {
    McuVariant::try_from_str(s).ok_or("Invalid MCU variant".to_string())
}

fn firmware_value_parser(s: &str) -> Result<FirmwareVersion, String> {
    FirmwareVersion::try_from_str(s).map_err(|_| "Invalid firmware version".to_string())
}

fn board_values() -> String {
    onerom_config::hw::BOARDS
        .iter()
        .map(|b| b.name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn mcu_values() -> String {
    onerom_config::mcu::MCU_VARIANTS
        .iter()
        .map(|m| m.to_string().to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

async fn load_firmware_metadata(path: &str) -> Result<(FirmwareVersion, Board, McuVariant), Error> {
    // Load the binary file
    let data = std::fs::read(path).map_err(|e| Error::read(path.to_string(), e))?;

    // Create a memory reader and parser (0x0800_0000 is the base address
    // for STM32F4 flash - but the reader will cope with RP2350 images as well)
    let mut reader = MemoryReader::new(data, 0x0800_0000);
    let mut parser = Parser::new(&mut reader);

    // Parse the firmware
    let fw_info = parser
        .parse_flash()
        .await
        .map_err(|e| Error::config(format!("Failed to parse firmware image: {}", e)))?;

    // Extract version
    let version = fw_info.version;

    // Extract MCU variant
    let mcu = fw_info.mcu_variant.ok_or_else(|| {
        Error::config("Failed to determine MCU variant from firmware".to_string())
    })?;

    // Extract board
    let board = fw_info
        .board
        .ok_or_else(|| Error::config("Failed to determine board type from firmware".to_string()))?;

    Ok((version, board, mcu))
}
