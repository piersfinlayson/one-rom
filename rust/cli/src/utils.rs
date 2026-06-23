// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crossterm::event::{self, Event, KeyEvent};
use crossterm::terminal;
use log::debug;
use std::io::Write;

use crate::args::CommandTrait;
use onerom_cli::{DeviceState, Error, LogLevel, Options};
use onerom_cli::{LIVE_ROM_BASE, LIVE_ROM_MAX_OFFSET};
use onerom_config::hw::Board;

pub fn get_supported_boards() -> String {
    onerom_config::hw::BOARDS
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn init_logging(options: &Options) {
    let log_level = &options.log_level;

    let mut log_builder = env_logger::Builder::from_default_env();

    match log_level {
        LogLevel::Warn => {
            log_builder.filter_level(log::LevelFilter::Warn);
        }
        LogLevel::Info => {
            log_builder.filter_level(log::LevelFilter::Info);
            // nusb is noisy at info level
            log_builder.filter_module("nusb", log::LevelFilter::Warn);
        }
        LogLevel::Debug => {
            log_builder.filter_level(log::LevelFilter::Debug);
            // nusb is very noisy at debug level
            log_builder.filter_module("nusb", log::LevelFilter::Info);
        }
        LogLevel::Trace => {
            log_builder.filter_level(log::LevelFilter::Trace);
        }
    }

    log_builder.format(|buf, record| {
        let level = format!("{}: ", record.level());
        writeln!(buf, "{:07}{}", level, record.args())
    });
    log_builder.init();
}

pub fn check_device_nand_board(options: &Options, board_arg: &Option<String>) -> Result<(), Error> {
    if options.device.is_some() && board_arg.is_some() {
        return Err(Error::DeviceAndBoard);
    }
    Ok(())
}

/// Checks that a device is required and present if the command needs one.
pub fn check_device(
    options: &Options,
    args: &impl CommandTrait,
    must_be_run_capable: bool,
) -> Result<(), Error> {
    if args.requires_device() && options.device.is_none() {
        return Err(Error::NoDevice);
    }
    let device = options.device.as_ref().unwrap();
    if must_be_run_capable && !device.usb_can_run {
        return Err(Error::CannotRun(device.to_string()));
    }
    Ok(())
}

pub fn parse_u32(s: &str) -> Result<u32, std::num::ParseIntError> {
    let s = s.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        s.parse::<u32>()
    }
}

#[allow(unused)]
pub fn parse_u16(s: &str) -> Result<u16, std::num::ParseIntError> {
    let s = s.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
    } else {
        s.parse::<u16>()
    }
}

pub fn parse_u16_hex_only(s: &str) -> Result<u16, std::num::ParseIntError> {
    let s = s.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
    } else {
        u16::from_str_radix(&s, 16)
    }
}

pub fn parse_u8(s: &str) -> Result<u8, std::num::ParseIntError> {
    let s = s.replace('_', "");
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        s.parse::<u8>()
    }
}

pub fn print_hex_dump(address: u32, data: &[u8]) {
    const BYTES_PER_ROW: usize = 16;
    const GROUP_SIZE: usize = 4;

    // Figure out how many nibbles/characters of the address to output
    let max_addr = address + data.len() as u32;
    let nibbles = (32 - max_addr.leading_zeros()).div_ceil(4).max(4) as usize;

    for (row_idx, row) in data.chunks(BYTES_PER_ROW).enumerate() {
        let row_addr = address + (row_idx * BYTES_PER_ROW) as u32;

        print!("0x{:0width$x}  ", row_addr, width = nibbles);

        // Hex bytes in groups of 4
        for (i, chunk) in row.chunks(GROUP_SIZE).enumerate() {
            for byte in chunk {
                print!("{:02x} ", byte);
            }
            // Pad if this chunk was short (last row)
            if chunk.len() < GROUP_SIZE {
                let missing = GROUP_SIZE - chunk.len();
                print!("{}", "   ".repeat(missing));
            }
            if i < (BYTES_PER_ROW / GROUP_SIZE) - 1 {
                print!(" ");
            }
        }

        // Pad if the whole row was short
        if row.len() < BYTES_PER_ROW {
            let missing_bytes = BYTES_PER_ROW - row.len();
            let missing_groups = missing_bytes / GROUP_SIZE;
            let _ = missing_groups; // already padded per-chunk above
        }

        // ASCII
        print!(" |");
        for byte in row {
            let ch = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            print!("{}", ch);
        }
        println!("|");
    }
}

/// Checks an address offset and length for validity against this particular
/// device.
///
/// Checks the device is running and can accept live reads/writes.
/// Checks that the offset is valid for the ROM currently being served by
/// the devce.
///
/// Returns the actual device start address to read/write and length.
pub fn check_live_read_write(
    options: &Options,
    offset: u32,
    length: Option<u32>,
    args: &impl CommandTrait,
) -> Result<(u32, u32), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();

    if device.state != DeviceState::Running {
        return Err(Error::NotRunning);
    }

    let rom_type = device.get_active_rom_type().ok_or(Error::UnknownRomType)?;
    let rom_size = device.get_active_rom_size().ok_or(Error::UnknownRomType)?;

    let length = if let Some(len) = length {
        len
    } else {
        // If length is not specified (read only) read to the end of the ROM
        // image
        if offset as usize >= rom_size {
            return Err(Error::LiveOutOfBounds(rom_type, rom_size));
        }
        (rom_size as u32) - offset
    };

    let end_offset = offset + length;
    assert!(rom_size <= LIVE_ROM_MAX_OFFSET as usize);
    if end_offset as usize > rom_size {
        return Err(Error::LiveOutOfBounds(rom_type, rom_size));
    }

    Ok((LIVE_ROM_BASE + offset, length))
}

/// Resolves the target board type.
///
/// If `board_arg` is provided, it takes precedence. Otherwise the board
/// is inferred from the connected device. Returns `None` if neither is
/// available, leaving it to the caller to decide whether that's an error.
pub fn resolve_board(
    options: &Options,
    board_arg: &Option<String>,
) -> Result<Option<Board>, Error> {
    if let Some(board) = board_arg {
        debug!("Resolving board from argument: {board}");
        Ok(Some(
            onerom_config::hw::Board::try_from_str(board)
                .ok_or_else(|| Error::InvalidBoard(board.clone(), get_supported_boards()))?,
        ))
    } else if let Some(device) = options.device.as_ref() {
        debug!("Resolving board from connected device");
        let board = device
            .onerom
            .as_ref()
            .and_then(|o| o.get_board())
            .ok_or(Error::NoBoardFromDevice(device.to_string()))?;
        Ok(Some(board))
    } else {
        debug!("No board argument or device available to resolve board");
        Ok(None)
    }
}

/// Figures out the firmware output filename to use
pub fn resolve_firmware_output(
    output: &Option<String>,
    path: &Option<String>,
    board: &Board,
    version: Option<&str>,
    config: Option<&str>,
) -> String {
    let version_part = version.map(|v| format!("_v{v}")).unwrap_or_default();

    let config_suffix = config
        .map(|c| {
            std::path::Path::new(c)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(c)
        })
        .map(|s| format!("_{s}"))
        .unwrap_or_default();

    let default_filename = format!(
        "onerom_{}{version_part}{config_suffix}.bin",
        board.name().to_ascii_lowercase(),
    );
    if let Some(output) = output {
        output.clone()
    } else if let Some(path) = path {
        format!("{}/{}", path.trim_end_matches('/'), default_filename)
    } else {
        default_filename
    }
}

pub fn read_char() -> Result<KeyEvent, Error> {
    terminal::enable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    let key = loop {
        if let Event::Key(key) = event::read().unwrap() {
            break key;
        }
    };
    terminal::disable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    Ok(key)
}
