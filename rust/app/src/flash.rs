// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The flash operations that program an image file onto a One ROM.

use alloc::vec;
use alloc::vec::Vec;

use onerom_gen::FlashChips;

/// A flash sector, the smallest area flash erases.
const SECTOR_SIZE: u32 = 4096;

/// The flash operations that program an image file, in the order to run them.
///
/// An image file holds the first flash chip's contents. Where it's longer than
/// the first chip, the second chip's contents follow at the offset of the first
/// chip's length.
///
/// An image that uses the second chip erases the whole first chip before it
/// touches the second. A run that stops part way leaves the first chip without
/// firmware, so the One ROM stays in the bootloader. It never boots with
/// metadata pointing at another image's data on the second chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashPlan<'a> {
    steps: Vec<FlashStep<'a>>,
}

impl<'a> FlashPlan<'a> {
    /// Plans programming `image` onto a board with `chips`.
    pub fn new(image: &'a [u8], chips: &FlashChips) -> Result<Self, FlashPlanError> {
        let first = chips.first();
        let first_len = first.len();
        if image.len() <= first_len {
            return Ok(Self {
                steps: vec![
                    FlashStep::Erase {
                        addr: first.start,
                        len: sectors(image.len()),
                    },
                    FlashStep::Write {
                        addr: first.start,
                        data: image,
                    },
                ],
            });
        }

        let second = chips.second().ok_or(FlashPlanError::SecondChipRequired)?;
        if image.len() > first_len + second.len() {
            return Err(FlashPlanError::TooLarge);
        }
        let (first_data, second_data) = image.split_at(first_len);
        Ok(Self {
            steps: vec![
                FlashStep::Erase {
                    addr: first.start,
                    len: first.end - first.start,
                },
                FlashStep::Erase {
                    addr: second.start,
                    len: sectors(second_data.len()),
                },
                FlashStep::Write {
                    addr: second.start,
                    data: second_data,
                },
                FlashStep::Write {
                    addr: first.start,
                    data: first_data,
                },
            ],
        })
    }

    /// The operations, in the order to run them.
    pub fn steps(&self) -> &[FlashStep<'a>] {
        &self.steps
    }
}

/// `len` bytes rounded up to whole sectors. `len` is at most a chip's length.
fn sectors(len: usize) -> u32 {
    (len as u32).next_multiple_of(SECTOR_SIZE)
}

/// One flash operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlashStep<'a> {
    /// Erase whole sectors.
    Erase {
        /// The first address to erase.
        addr: u32,
        /// The bytes to erase, a multiple of the 4KB sector.
        len: u32,
    },
    /// Write to erased flash.
    Write {
        /// The first address to write.
        addr: u32,
        /// The bytes to write.
        data: &'a [u8],
    },
}

/// Why an image file can't be programmed onto a board.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlashPlanError {
    /// The image is longer than the first chip and the board doesn't have a
    /// second.
    #[error("the image requires a board size larger than M")]
    SecondChipRequired,

    /// The image is longer than the board's flash chips together.
    #[error("the image is larger than this board's flash")]
    TooLarge,
}
