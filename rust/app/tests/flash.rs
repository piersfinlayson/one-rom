// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for splitting an image file into the flash operations that program
//! it.

use onerom_app::{FlashPlan, FlashPlanError, FlashStep};
use onerom_config::hw::BoardSize;
use onerom_config::mcu::Variant;
use onerom_gen::FlashChips;
use onerom_metadata::{FLASH_CS0_BASE_ADDR, FLASH_CS1_BASE_ADDR};

const KB: usize = 1024;
const MB: usize = 1024 * KB;

const FIRST_CHIP: u32 = FLASH_CS0_BASE_ADDR;
const SECOND_CHIP: u32 = FLASH_CS1_BASE_ADDR;

fn chips(size: BoardSize) -> FlashChips {
    FlashChips::new(Variant::RP2350, size)
}

/// An image of `len` bytes, each its offset's low byte, so a slice of it
/// shows where it came from.
fn image(len: usize) -> Vec<u8> {
    (0..len).map(|n| n as u8).collect()
}

/// An image on the first chip alone is erased and written there, on either
/// board size.
#[test]
fn an_image_on_the_first_chip_alone_is_erased_and_written_there() {
    let data = image(64 * KB);
    for size in [BoardSize::M, BoardSize::L] {
        let plan = FlashPlan::new(&data, &chips(size)).unwrap();
        assert_eq!(
            plan.steps(),
            [
                FlashStep::Erase {
                    addr: FIRST_CHIP,
                    len: 64 * KB as u32
                },
                FlashStep::Write {
                    addr: FIRST_CHIP,
                    data: &data
                },
            ],
            "{size}"
        );
    }
}

/// An image using the second chip erases the whole first chip, then erases
/// and writes the second chip, then writes the first. An interrupted run
/// leaves the first chip without firmware.
#[test]
fn an_image_using_the_second_chip_erases_the_first_chip_first() {
    let data = image(2 * MB + 100 * KB);
    let plan = FlashPlan::new(&data, &chips(BoardSize::L)).unwrap();
    assert_eq!(
        plan.steps(),
        [
            FlashStep::Erase {
                addr: FIRST_CHIP,
                len: 2 * MB as u32
            },
            FlashStep::Erase {
                addr: SECOND_CHIP,
                len: 100 * KB as u32
            },
            FlashStep::Write {
                addr: SECOND_CHIP,
                data: &data[2 * MB..]
            },
            FlashStep::Write {
                addr: FIRST_CHIP,
                data: &data[..2 * MB]
            },
        ]
    );
}

/// An erase covers whole 4KB sectors, on either chip.
#[test]
fn an_erase_rounds_up_to_whole_sectors() {
    for (len, erased) in [(1, 4096), (4095, 4096), (4096, 4096), (4097, 8192)] {
        let data = image(len);
        let plan = FlashPlan::new(&data, &chips(BoardSize::M)).unwrap();
        assert_eq!(
            plan.steps()[0],
            FlashStep::Erase {
                addr: FIRST_CHIP,
                len: erased
            },
            "{len}"
        );

        let data = image(2 * MB + len);
        let plan = FlashPlan::new(&data, &chips(BoardSize::L)).unwrap();
        assert_eq!(
            plan.steps()[1],
            FlashStep::Erase {
                addr: SECOND_CHIP,
                len: erased
            },
            "{len} on the second chip"
        );
    }
}

/// An image exactly the first chip's length doesn't use the second chip.
#[test]
fn an_image_filling_the_first_chip_stays_on_it() {
    let data = image(2 * MB);
    let plan = FlashPlan::new(&data, &chips(BoardSize::M)).unwrap();
    assert_eq!(
        plan.steps(),
        [
            FlashStep::Erase {
                addr: FIRST_CHIP,
                len: 2 * MB as u32
            },
            FlashStep::Write {
                addr: FIRST_CHIP,
                data: &data
            },
        ]
    );
}

#[test]
fn an_image_longer_than_the_first_chip_needs_a_second() {
    let data = image(2 * MB + 1);
    assert_eq!(
        FlashPlan::new(&data, &chips(BoardSize::M)),
        Err(FlashPlanError::SecondChipRequired)
    );
}

#[test]
fn an_image_longer_than_both_chips_is_too_large() {
    let data = image(4 * MB);
    assert!(FlashPlan::new(&data, &chips(BoardSize::L)).is_ok());

    let data = image(4 * MB + 1);
    assert_eq!(
        FlashPlan::new(&data, &chips(BoardSize::L)),
        Err(FlashPlanError::TooLarge)
    );
}
