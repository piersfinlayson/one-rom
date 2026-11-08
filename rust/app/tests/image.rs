// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for assembling an image file from a build, and for what checking one
//! reports.

use onerom_app::{ImagePart, ImageProblem, ImageTooLarge, assemble_image};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::Variant;
use onerom_fw_parser::{ImageFileError, ParseError};
use onerom_gen::{FIRMWARE_SIZE, MAX_METADATA_LEN};

const MB: usize = 1024 * 1024;

fn props(size: BoardSize) -> FirmwareProperties {
    FirmwareProperties::new(
        FirmwareVersion::new(0, 8, 0, 0),
        Board::Fire32B,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap()
    .with_board_size(size)
}

/// What assembling these parts for a `size` board fails on, if anything.
async fn too_large(
    size: BoardSize,
    firmware: usize,
    metadata: usize,
    roms: usize,
) -> Option<ImageTooLarge> {
    assemble_image(
        &props(size),
        vec![0; firmware],
        vec![0; metadata],
        vec![0; roms],
    )
    .await
    .err()
}

/// ROM data follows the firmware and metadata regions and can fill the rest of
/// the flash.
#[tokio::test]
async fn an_m_board_takes_rom_data_to_the_end_of_its_one_chip() {
    let space = 2 * MB - FIRMWARE_SIZE - MAX_METADATA_LEN;
    assert_eq!(
        too_large(BoardSize::M, 1024, MAX_METADATA_LEN, space).await,
        None
    );
    let error = too_large(BoardSize::M, 1024, MAX_METADATA_LEN, space + 1)
        .await
        .unwrap();
    assert_eq!(error.part, ImagePart::Image);
    assert_eq!((error.size, error.max), (2 * MB + 1, 2 * MB));
}

#[tokio::test]
async fn an_l_board_takes_rom_data_to_the_end_of_its_second_chip() {
    let space = 4 * MB - FIRMWARE_SIZE - MAX_METADATA_LEN;
    assert_eq!(
        too_large(BoardSize::L, 1024, MAX_METADATA_LEN, space).await,
        None
    );
    assert!(
        too_large(BoardSize::L, 1024, MAX_METADATA_LEN, space + 1)
            .await
            .is_some()
    );
    assert!(
        too_large(BoardSize::M, 1024, MAX_METADATA_LEN, space)
            .await
            .is_some()
    );
}

/// Firmware and metadata each fit their own region, whatever the board size.
#[tokio::test]
async fn firmware_and_metadata_fit_their_regions() {
    let error = too_large(BoardSize::L, FIRMWARE_SIZE + 1, 0, 0)
        .await
        .unwrap();
    assert_eq!(error.part, ImagePart::Firmware);
    let error = too_large(BoardSize::L, 1024, MAX_METADATA_LEN + 1, 1)
        .await
        .unwrap();
    assert_eq!(error.part, ImagePart::Metadata);
}

/// Each part is at its fixed offset, with erased flash between them.
#[tokio::test]
async fn each_part_is_at_its_offset() {
    let checked = assemble_image(
        &props(BoardSize::M),
        vec![1; 100],
        vec![2; 100],
        vec![3; 100],
    )
    .await
    .unwrap();
    let image = checked.image;
    let metadata = FIRMWARE_SIZE;
    let roms = FIRMWARE_SIZE + MAX_METADATA_LEN;
    assert_eq!(image.len(), roms + 100);
    assert!(image[..100].iter().all(|&b| b == 1));
    assert!(image[100..metadata].iter().all(|&b| b == 0xFF));
    assert!(image[metadata..metadata + 100].iter().all(|&b| b == 2));
    assert!(image[metadata + 100..roms].iter().all(|&b| b == 0xFF));
    assert!(image[roms..].iter().all(|&b| b == 3));
}

/// An image without ROM data ends with its metadata. One without metadata or ROM
/// data is the firmware alone.
#[tokio::test]
async fn an_image_ends_with_its_last_part() {
    let props = props(BoardSize::M);
    let checked = assemble_image(&props, vec![1; 100], vec![2; 100], vec![])
        .await
        .unwrap();
    assert_eq!(checked.image.len(), FIRMWARE_SIZE + 100);
    let checked = assemble_image(&props, vec![1; 100], vec![], vec![])
        .await
        .unwrap();
    assert_eq!(checked.image, vec![1; 100]);
}

/// A problem's text contains the values that identify it.
#[test]
fn a_problems_text_contains_its_values() {
    let text = ImageProblem::BoardMismatch {
        image: Board::Fire28A,
        expected: Board::Fire40A,
    }
    .to_string();
    assert!(text.contains("fire-28-a"), "{text}");
    assert!(text.contains("fire-40-a"), "{text}");

    let text = ImageProblem::ParseErrors(vec![
        ParseError::new("hw_rev", "first reason"),
        ParseError::new("pins", "second reason"),
    ])
    .to_string();
    assert!(text.contains("first reason"), "{text}");
    assert!(text.contains("second reason"), "{text}");

    let text = ImageProblem::ImageFile(ImageFileError::TooShort { short_by: 1111 }).to_string();
    assert!(text.contains("1111"), "{text}");
}
