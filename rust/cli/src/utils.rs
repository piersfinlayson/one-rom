// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crossterm::event::{self, Event, KeyEvent};
use crossterm::terminal;
use log::debug;
use std::io::Write;

use crate::args::CommandTrait;
use onerom_cli::{Device, DeviceState, Error, LogLevel, Options};
use onerom_cli::{LIVE_ROM_BASE, LIVE_ROM_MAX_OFFSET};
use onerom_config::chip::ChipType;
use onerom_config::hw::{Board, Model};

/// The board types the CLI can act on, comma-separated.
///
/// Fire (RP2350) only. See [`check_fire_board`] for why the Ice boards are not
/// in here, and [`get_reference_boards`] for what does still accept them.
pub fn get_supported_boards() -> String {
    join_boards(Model::Fire.boards())
}

/// The board types the CLI recognises but cannot act on, comma-separated.
///
/// The Ice (STM32) boards. They remain fully described by the commands that
/// only *report* hardware - `board header`, `board socket`, `chips` and
/// `firmware releases` - none of which needs to build an image or reach a
/// device.
pub fn get_reference_boards() -> String {
    join_boards(Model::Ice.boards())
}

fn join_boards(boards: &[Board]) -> String {
    boards
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reject a board this CLI cannot act on.
///
/// The firmware paths compose images for `Variant::RP2350` and nothing else,
/// and the device paths speak picoboot, which is the RP2350 bootloader - so an
/// Ice (STM32) board cannot be programmed, downloaded for, or scanned. Checked
/// up front, where the user named the board, rather than left to fail deeper
/// down as a release the manifest does not have.
pub fn check_fire_board(board: &Board) -> Result<(), Error> {
    match board.model() {
        Model::Fire => Ok(()),
        Model::Ice => Err(Error::IceBoardUnsupported(board.name().to_string())),
    }
}

/// [`check_fire_board`] for the commands whose board is optional.
///
/// A board that could not be resolved at all is a separate matter, left to the
/// caller - it may well be survivable, whereas a board that resolved to an Ice
/// is not.
pub fn check_fire_board_optional(board: &Option<Board>) -> Result<(), Error> {
    match board {
        Some(board) => check_fire_board(board),
        None => Ok(()),
    }
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
///
/// A command that does *not* require a device is not an error without one -
/// there is simply nothing to check, so the run-capable test is skipped rather
/// than applied to a device that is not there.
pub fn check_device(
    options: &Options,
    args: &impl CommandTrait,
    must_be_run_capable: bool,
) -> Result<(), Error> {
    let Some(device) = options.device.as_ref() else {
        return if args.requires_device() {
            Err(Error::NoDevice)
        } else {
            Ok(())
        };
    };
    if must_be_run_capable && !device.usb_can_run {
        return Err(Error::CannotRun(device.to_string()));
    }
    Ok(())
}

/// Checks that a device is present and **currently running**.
///
/// [`check_device`] with `must_be_run_capable` tests `usb_can_run`, which asks
/// whether the flashed firmware and system plugin *could* serve. That is true of
/// a stopped device sitting in the RP2350 bootloader, and so is not enough for
/// anything that talks to One ROM's own picoboot command handler: that handler
/// lives in the USB system plugin, and while the device is stopped the boot ROM
/// answers picoboot instead, with no One ROM commands at all.
pub fn check_device_running(options: &Options, args: &impl CommandTrait) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    if !device.is_running() {
        return Err(Error::DeviceNotRunning(device.to_string()));
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

/// A colour for the RGB LED, as the device takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Colour {
    /// The three components, in the order the wire wants them.
    pub fn rgb(&self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

impl std::fmt::Display for Colour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// The colours nameable by word, rather than by hex.
///
/// A short list on purpose: it covers what someone marking one One ROM apart
/// from another reaches for, and anything else is expressible as hex.
const NAMED_COLOURS: &[(&str, (u8, u8, u8))] = &[
    ("red", (0xFF, 0x00, 0x00)),
    ("green", (0x00, 0xFF, 0x00)),
    ("blue", (0x00, 0x00, 0xFF)),
    ("white", (0xFF, 0xFF, 0xFF)),
    ("yellow", (0xFF, 0xFF, 0x00)),
    ("cyan", (0x00, 0xFF, 0xFF)),
    ("magenta", (0xFF, 0x00, 0xFF)),
    ("orange", (0xFF, 0x60, 0x00)),
    ("purple", (0x80, 0x00, 0xFF)),
    ("pink", (0xFF, 0x40, 0x60)),
];

/// The name for a colour, where it has one.
///
/// The reverse of the table [`parse_colour`] reads, so a colour a user could
/// have asked for by name is shown back to them by that name. Exact matches
/// only - a colour a shade off one of these has no name, and saying it did
/// would name a colour the LED is not showing.
pub fn colour_name(r: u8, g: u8, b: u8) -> Option<&'static str> {
    NAMED_COLOURS
        .iter()
        .find(|(_, rgb)| *rgb == (r, g, b))
        .map(|(name, _)| *name)
}

/// Parse a colour named by word, or written as `#RRGGBB` or `0xRRGGBB`.
pub fn parse_colour(s: &str) -> Result<Colour, String> {
    let lower = s.to_ascii_lowercase();

    if let Some((_, (r, g, b))) = NAMED_COLOURS.iter().find(|(name, _)| *name == lower) {
        return Ok(Colour {
            r: *r,
            g: *g,
            b: *b,
        });
    }

    let hex = lower
        .strip_prefix('#')
        .or_else(|| lower.strip_prefix("0x"))
        .unwrap_or(&lower);

    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let value = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
        return Ok(Colour {
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        });
    }

    let names = NAMED_COLOURS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "'{s}' is not a colour. Give one of {names}, or a hex colour such as #FF8000."
    ))
}

/// Parse a brightness percentage, 1 to 100.
pub fn parse_brightness(s: &str) -> Result<u8, String> {
    let value = s
        .parse::<u16>()
        .map_err(|_| "Brightness must be a number".to_string())?;

    if (1..=100).contains(&value) {
        Ok(value as u8)
    } else {
        Err("Brightness is a percentage, 1 to 100".to_string())
    }
}

/// The longest hold a One ROM accepts for an LED, in milliseconds.
///
/// Mirrors `ONEROM_LED_MAX_HOLD_MS` in the plugin's `usb_custom_pbx.h`, and the
/// engine's own bound beneath it. Checked here so a value too large is a parse
/// error rather than a refusal from the device.
pub const LED_MAX_HOLD_MS: u32 = 60_000;

/// Parse a hold in milliseconds, bounded by what a device accepts.
pub fn parse_hold_ms(s: &str) -> Result<u32, String> {
    let value = parse_u32(s).map_err(|_| "Hold must be a number of milliseconds".to_string())?;

    // Zero on the wire means "hold until something changes it", which is what
    // omitting --hold asks for, so it is not a hold this can express.
    if value == 0 {
        return Err("Minimum hold is 1ms".to_string());
    }

    if value > LED_MAX_HOLD_MS {
        return Err(format!("Maximum hold is {LED_MAX_HOLD_MS}ms"));
    }

    Ok(value)
}

/// The shortest period each repeating mode accepts, in milliseconds.
///
/// Mirrors `LED_*_MIN_PERIOD_MS` in the firmware's `pioled.c`, which is where
/// they are enforced for every caller. They are checked here too so a value the
/// device would refuse fails before the command is sent, naming the mode.
///
/// Each comes from how many steps the mode divides a repetition into: below
/// these the engine would have to run frames closer together than it schedules
/// them, so it could not repeat at the period asked for.
pub const CYCLE_MIN_PERIOD_MS: u16 = 1000;
pub const BREATHE_MIN_PERIOD_MS: u16 = 1000;
pub const BLINK_MIN_PERIOD_MS: u16 = 50;
pub const BEACON_MIN_PERIOD_MS: u16 = 50;
pub const FLAME_MIN_PERIOD_MS: u16 = 500;

/// Parse a period in milliseconds, bounded below by what the mode can run at.
///
/// The upper bound is what fits the wire's `u16` rather than a judgement about
/// what is useful.
fn parse_period_min(s: &str, min: u16) -> Result<u16, String> {
    let value = parse_u32(s).map_err(|_| "Period must be a number of milliseconds".to_string())?;

    // Zero is below every mode's minimum, so it needs no message of its own.
    // On the wire it means "the mode's own default", which is what omitting
    // --period asks for.
    let value = u16::try_from(value).map_err(|_| format!("Maximum period is {}ms", u16::MAX))?;

    if value < min {
        return Err(format!("Minimum period is {min}ms"));
    }

    Ok(value)
}

/// Parse a `cycle` period.
pub fn parse_cycle_period(s: &str) -> Result<u16, String> {
    parse_period_min(s, CYCLE_MIN_PERIOD_MS)
}

/// Parse a `breathe` period.
pub fn parse_breathe_period(s: &str) -> Result<u16, String> {
    parse_period_min(s, BREATHE_MIN_PERIOD_MS)
}

/// Parse a `blink` period.
pub fn parse_blink_period(s: &str) -> Result<u16, String> {
    parse_period_min(s, BLINK_MIN_PERIOD_MS)
}

/// Parse a `beacon` period.
pub fn parse_beacon_period(s: &str) -> Result<u16, String> {
    parse_period_min(s, BEACON_MIN_PERIOD_MS)
}

/// Parse a `flame` period.
pub fn parse_flame_period(s: &str) -> Result<u16, String> {
    parse_period_min(s, FLAME_MIN_PERIOD_MS)
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

/// Resolves the target board type, where not knowing it is survivable.
///
/// The GPIO commands use the board to *name* things - a pin's ROM function, the
/// pad it surfaces on, whether it is 5V-tolerant - and to resolve a `--pin` pad
/// name. None of that is worth failing a command over when the user named a
/// GPIO directly, so a board this build cannot infer costs a name rather than
/// the operation, and a `--pin` pad name reports the missing board itself (see
/// [`Pin::resolve`](onerom_cli::pin::Pin::resolve)).
///
/// An *explicit* `--board` is different: the user asked for a specific board, so
/// a name this build does not know is an error rather than something to shrug
/// off and then blame on the device.
pub fn resolve_board_optional(
    options: &Options,
    board_arg: &Option<String>,
) -> Result<Option<Board>, Error> {
    if board_arg.is_some() {
        resolve_board(options, board_arg)
    } else {
        Ok(resolve_board(options, &None).ok().flatten())
    }
}

/// The chip type of the ROM the device is currently serving.
///
/// The device records a human-readable ROM type per slot rather than an enum,
/// so this resolves that label back to a [`ChipType`]. `None` when the device is
/// not running, has no readable metadata, or names a type this build does not
/// know - all of which cost the caller a name, not an operation.
pub fn active_chip_type(device: &Device) -> Option<ChipType> {
    ChipType::try_from_str(&device.get_active_rom_type()?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::hw::BOARDS;

    /// Every board must fall into exactly one of the two lists the CLI shows -
    /// a board in neither would be invisible to `board list`, and one in both
    /// would be claimed as usable and unusable at once.
    #[test]
    fn the_two_board_lists_partition_every_board() {
        let supported = get_supported_boards();
        let reference = get_reference_boards();
        for board in BOARDS {
            let name = board.name();
            let in_supported = supported.split(", ").any(|b| b == name);
            let in_reference = reference.split(", ").any(|b| b == name);
            assert!(
                in_supported != in_reference,
                "{name} must appear in exactly one list"
            );
            // And the list a board is in must agree with whether it passes the
            // guard the device and firmware commands apply.
            assert_eq!(
                check_fire_board(&board).is_ok(),
                in_supported,
                "{name} listing disagrees with check_fire_board"
            );
        }
    }

    #[test]
    fn ice_boards_are_rejected_by_the_guard() {
        let ice = Board::try_from_str("ice-24-d").unwrap();
        let fire = Board::try_from_str("fire-24-f").unwrap();
        assert!(matches!(
            check_fire_board(&ice),
            Err(Error::IceBoardUnsupported(_))
        ));
        // The message names the board and what the command does support. It
        // speaks for the command the user ran and nothing else: what another
        // command accepts is that command's to report, and it promises nothing
        // about later releases either way.
        let msg = check_fire_board(&ice).unwrap_err().to_string();
        assert!(msg.contains("ice-24-d"), "{msg}");
        assert!(msg.contains("Fire (RP2350)"), "{msg}");
        for forecast in ["not yet", "yet supported", "later", "never"] {
            assert!(!msg.contains(forecast), "says '{forecast}': {msg}");
        }
        for other in ["board header", "board socket", "onerom chips", "releases"] {
            assert!(!msg.contains(other), "names '{other}': {msg}");
        }
        assert!(check_fire_board(&fire).is_ok());
        // Optional form: an unresolved board is the caller's business, not a
        // failure here.
        assert!(check_fire_board_optional(&None).is_ok());
        assert!(check_fire_board_optional(&Some(fire)).is_ok());
        assert!(check_fire_board_optional(&Some(ice)).is_err());
    }
}

#[cfg(test)]
mod colour_tests {
    use super::*;

    #[test]
    fn every_name_survives_the_round_trip() {
        // Each name parses to a colour, and that colour names itself again.
        // Written over the table rather than a hand-copied list, so a colour
        // added there is covered without touching this.
        for (name, _) in NAMED_COLOURS {
            let parsed = parse_colour(name).expect("a listed name parses");
            assert_eq!(
                colour_name(parsed.r, parsed.g, parsed.b),
                Some(*name),
                "{name} did not name itself again"
            );
        }
    }

    #[test]
    fn the_names_are_distinct_colours() {
        // The reverse lookup answers with the first match, so two names on one
        // colour would make it silently pick one.  They must not collide.
        for (i, (name, rgb)) in NAMED_COLOURS.iter().enumerate() {
            for (other, other_rgb) in NAMED_COLOURS.iter().skip(i + 1) {
                assert_ne!(rgb, other_rgb, "{name} and {other} are the same colour");
            }
        }
    }

    #[test]
    fn a_colour_off_by_one_has_no_name() {
        // Exact matches only.  #FE0000 is not red, and calling it red would
        // name a colour the LED is not showing.
        assert_eq!(colour_name(0xFF, 0x00, 0x00), Some("red"));
        assert_eq!(colour_name(0xFE, 0x00, 0x00), None);
        assert_eq!(colour_name(0x00, 0x00, 0x00), None);
        assert_eq!(colour_name(0x12, 0x34, 0x56), None);
    }
}
