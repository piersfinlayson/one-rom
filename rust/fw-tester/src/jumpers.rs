// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Image select jumper states for each image.

use onerom_config::hw::Board;
use onerom_config::pin::HeaderPin;
use onerom_fw_emulator::Emulator;
use onerom_gen::{Config, MIN_RESERVED_PINS_VERSION};
use onerom_metadata::metadata_generation_for;

/// The image select jumpers for a config on a board.
///
/// Reserved pins are closed so firmware that reads one selects the wrong
/// image.
pub struct Jumpers {
    /// The image select pins the firmware reads, lowest bit first, as indices
    /// from SEL_A.
    pub read: Vec<u8>,
    reserved: u8,
}

impl Jumpers {
    /// Panics where a pin in `config`'s `reserved_pins` isn't on `board`. Such
    /// a config fails to build.
    pub fn new(board: Board, config: &Config) -> Self {
        let reserved = config
            .reserved_pins_on(board)
            .expect("the config's reserved_pins are pins this board has");
        // Every pin read is an image select pin.
        let read = reserved
            .select_pins_read(&board)
            .filter_map(|pin| {
                if let HeaderPin::Select(index) = pin {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();
        Self {
            read,
            reserved: reserved.select_bits(),
        }
    }

    pub fn images(&self) -> usize {
        1 << self.read.len()
    }

    /// The jumper state that selects `image` and the state the firmware reads,
    /// each a bit per pin from SEL_A.
    ///
    /// Reserved pins are left open where the metadata predates reserved pins,
    /// as the firmware then reads every pin.
    pub fn for_image(&self, image: u8) -> (u8, u8) {
        let read: u8 = self
            .read
            .iter()
            .enumerate()
            .filter(|(bit, _)| image & (1 << bit) != 0)
            .fold(0, |jumpers, (_, pin)| jumpers | (1 << pin));
        let first = metadata_generation_for(MIN_RESERVED_PINS_VERSION)
            .expect("the reservation fields arrived in a metadata generation");
        if Emulator::metadata_generation() >= first {
            (read | self.reserved, read)
        } else {
            (read, read)
        }
    }
}
