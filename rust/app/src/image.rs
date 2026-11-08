// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Assembling an image file from a build, and checking an image file before it
//! is programmed.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use onerom_config::fw::FirmwareProperties;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant;
use onerom_fw_parser::{ImageFileError, ParseError, ParsedDevice, parse_image_file};
use onerom_gen::{FIRMWARE_SIZE, FlashChips, MAX_METADATA_LEN};

/// An erased flash byte, which fills the gaps between an image's parts.
const ERASED_FLASH: u8 = 0xFF;

/// An image file, its parse and what [`check_image`] found wrong with it.
#[derive(Debug)]
#[non_exhaustive]
pub struct CheckedImage {
    /// The image file.
    pub image: Vec<u8>,
    /// The image's parse.
    pub device: ParsedDevice,
    /// What's wrong with the image, in the order checked. Empty for an image
    /// fit to program.
    pub problems: Vec<ImageProblem>,
}

/// Assembles an image file from base firmware and the metadata and ROM data
/// a build produced, then checks it for the board in `fw_props`.
///
/// `metadata` and `roms` are as [`Builder::build`](onerom_gen::Builder::build)
/// returns them. An empty part is left out of the image.
pub async fn assemble_image(
    fw_props: &FirmwareProperties,
    firmware: Vec<u8>,
    metadata: Vec<u8>,
    roms: Vec<u8>,
) -> Result<CheckedImage, ImageTooLarge> {
    ImageTooLarge::check(ImagePart::Firmware, firmware.len(), FIRMWARE_SIZE)?;
    ImageTooLarge::check(ImagePart::Metadata, metadata.len(), MAX_METADATA_LEN)?;
    let image = join(firmware, metadata, roms);
    let chips = FlashChips::new(fw_props.mcu_variant(), fw_props.board_size());
    let flash = chips.first().len() + chips.second().map_or(0, |chip| chip.len());
    ImageTooLarge::check(ImagePart::Image, image.len(), flash)?;
    Ok(check_image(image, Some(fw_props.board())).await)
}

/// The image file holding `firmware`, `metadata` and `roms`, each at its fixed
/// offset. The firmware and metadata must fit their regions.
fn join(mut image: Vec<u8>, metadata: Vec<u8>, roms: Vec<u8>) -> Vec<u8> {
    if metadata.is_empty() && roms.is_empty() {
        return image;
    }
    image.resize(FIRMWARE_SIZE, ERASED_FLASH);
    image.extend_from_slice(&metadata);
    if !roms.is_empty() {
        image.resize(FIRMWARE_SIZE + MAX_METADATA_LEN, ERASED_FLASH);
        image.extend_from_slice(&roms);
    }
    image
}

/// Checks an image file for programming onto `board`, or onto any board where
/// `board` is `None`.
///
/// A file longer than the first flash chip holds the second chip's contents
/// after the first chip's.
pub async fn check_image(image: Vec<u8>, board: Option<Board>) -> CheckedImage {
    let first = FlashChips::first_for(Variant::RP2350);
    let device = parse_image_file(&image, first.clone()).await;
    let mut problems = Vec::new();
    if let (Some(expected), Some(image_board)) = (board, device.get_board())
        && image_board != expected
    {
        problems.push(ImageProblem::BoardMismatch {
            image: image_board,
            expected,
        });
    }
    if !device.parse_errors().is_empty() {
        problems.push(ImageProblem::ParseErrors(device.parse_errors().to_vec()));
    }
    if let Err(e) = device.check_image_file(image.len(), first) {
        problems.push(ImageProblem::ImageFile(e));
    }
    CheckedImage {
        image,
        device,
        problems,
    }
}

/// Something [`check_image`] found wrong with an image file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageProblem {
    /// The image is for another board.
    BoardMismatch {
        /// The image's board.
        image: Board,
        /// The board the image was checked for.
        expected: Board,
    },

    /// The image doesn't parse.
    ParseErrors(Vec<ParseError>),

    /// The image's slots don't match its length or the flash chips.
    ImageFile(ImageFileError),
}

impl fmt::Display for ImageProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BoardMismatch { image, expected } => write!(
                f,
                "Firmware board type '{}' does not match the expected board type '{}'.",
                image.name(),
                expected.name()
            ),
            Self::ParseErrors(errors) => {
                write!(f, "Assembled firmware has parse errors:")?;
                for error in errors {
                    write!(f, "\n    {error}")?;
                }
                write!(f, "\n  This is likely a bug.  Please report it.")
            }
            Self::ImageFile(error) => f.write_str(&image_file_text(error)),
        }
    }
}

/// The text for an image file whose slots don't match its length or the flash
/// chips.
pub fn image_file_text(error: &ImageFileError) -> String {
    let (fault, detail) = match *error {
        ImageFileError::TooShort { short_by } => {
            ("too short", format!("It is {short_by} bytes short"))
        }
        ImageFileError::BadAddress { slot, addr } => (
            "damaged",
            format!("Slot {slot} is at {addr:#010x}, which isn't a valid address"),
        ),
        ImageFileError::TooLong { too_long_by } => {
            ("too long", format!("It is {too_long_by} bytes too long"))
        }
    };
    format!("Cannot use this image file because it is {fault}.\n  {detail}.")
}

/// A part of an image, or the whole image, too large for its space in flash.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}\n  {size} bytes vs {max} bytes maximum.", .part.too_large())]
#[non_exhaustive]
pub struct ImageTooLarge {
    /// The part that is too large.
    pub part: ImagePart,
    /// The part's size in bytes.
    pub size: usize,
    /// The part's space in bytes.
    pub max: usize,
}

impl ImageTooLarge {
    fn check(part: ImagePart, size: usize, max: usize) -> Result<(), Self> {
        if size > max {
            Err(Self { part, size, max })
        } else {
            Ok(())
        }
    }
}

/// A part of an image file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImagePart {
    /// The base firmware.
    Firmware,
    /// The metadata.
    Metadata,
    /// The whole image.
    Image,
}

impl ImagePart {
    fn too_large(self) -> &'static str {
        match self {
            Self::Firmware => "The base firmware is too large.",
            Self::Metadata => "The metadata is too large.",
            Self::Image => "The firmware image is larger than the board's flash.",
        }
    }
}
