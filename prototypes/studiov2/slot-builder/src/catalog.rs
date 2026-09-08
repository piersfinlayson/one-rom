// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Everything the UI offers the user, read out of `onerom-config` and
//! `onerom-gen`.
//!
//! Nothing here is hand-listed: the board list, the chip types a board can
//! serve, the flash each image costs, the file formats and the physical jumper
//! header all come from the crates that own them, so a new board or chip type
//! reaches this prototype without an edit.

use std::fmt;
use std::path::{Path, PathBuf};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::{Board, HeaderRole, HeaderSlot, JumperHeader, Model};
use onerom_config::mcu::Variant;
use onerom_gen::compat::supported_chips;
use onerom_gen::{ChipSetType, FileFormat, SizeHandling};

/// The MCU every Fire board runs. A/B silicon share the RP2350 firmware, so the
/// builder has no MCU picker.
pub const MCU: Variant = Variant::RP2350;

/// Flash a plugin occupies once padded to its slot.
pub const PLUGIN_SLOT_BYTES: u64 = 64 * 1024;

/// Firmware plus metadata: the fixed reservation at the bottom of flash.
pub fn reserved_bytes() -> u64 {
    (onerom_gen::FIRMWARE_SIZE + onerom_gen::MAX_METADATA_LEN) as u64
}

/// Total flash the build has to fit into.
pub fn flash_bytes() -> u64 {
    MCU.flash_storage_bytes() as u64
}

// ---------------------------------------------------------------- boards ----

/// A One ROM the builder can target: a USB-capable Fire board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardChoice(pub Board);

impl fmt::Display for BoardChoice {
    /// `fire-24-f` reads as `Fire 24 F`, matching the site's picker.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for part in self.0.name().split('-') {
            if !first {
                f.write_str(" ")?;
            }
            first = false;
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    write!(f, "{}", c.to_ascii_uppercase())?;
                    f.write_str(chars.as_str())?;
                }
                None => continue,
            }
        }
        Ok(())
    }
}

/// Every board the builder offers: Fire, and USB-programmable.
///
/// The tab is Fire-only because the flash tally and the v2 image sizes exist
/// only for the 0.7.0 schema, which Ice never reaches.
pub fn boards() -> Vec<BoardChoice> {
    let mut boards: Vec<BoardChoice> = Model::Fire
        .boards()
        .iter()
        .filter(|board| board.has_usb())
        .map(|board| BoardChoice(*board))
        .collect();
    boards.sort_by_key(|choice| choice.to_string());
    boards
}

/// The board's short code for the wireframe: `fire-24-f` is a `24F`.
pub fn board_short_code(board: Board) -> String {
    board
        .name()
        .split('-')
        .skip(1)
        .collect::<String>()
        .to_uppercase()
}

/// How many image-select jumpers this board has, and so how many slots its
/// jumpers can address.
pub fn max_slots(board: Board) -> usize {
    1 << board.sel_pins().len()
}

// ------------------------------------------------------------ chip types ----

/// One emulated chip a board can serve, and what it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipChoice {
    /// The name the user picks it by — a part number they can read off a chip.
    pub alias: &'static str,
    /// The chip type behind the alias.
    pub chip_type: ChipType,
    /// The chip's own capacity.
    pub rom_bytes: u32,
    /// The flash the served image occupies, which can exceed `rom_bytes`.
    pub image_bytes: u32,
    /// Chip-select lines whose polarity the user configures.
    pub chip_selects: usize,
}

impl fmt::Display for ChipChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.alias)
    }
}

/// Every chip type `board` can serve one at a time, with its image footprint.
///
/// `supported_chips` runs the real v2 layout derivation, so this is the same
/// list and the same sizes `docs/COMPATIBILITY.md` publishes.
pub fn chip_types(board: Board) -> Vec<ChipChoice> {
    let mut chips: Vec<ChipChoice> = supported_chips(board, ChipSetType::Single, 1)
        .into_iter()
        .map(|entry| ChipChoice {
            alias: entry.alias,
            chip_type: entry.chip_type,
            rom_bytes: entry.rom_size_bytes,
            image_bytes: entry.result.slot_size_bytes,
            chip_selects: configurable_lines(entry.chip_type),
        })
        .collect();
    chips.sort_by_key(|chip| (chip.rom_bytes, chip.alias));
    chips
}

/// The chip's control lines the user sets the polarity of.
fn configurable_lines(chip_type: ChipType) -> usize {
    chip_type
        .control_lines()
        .iter()
        .filter(|line| line.line_type == ControlLineType::Configurable)
        .count()
}

// ------------------------------------------------------------- pick lists ---

/// A file format wrapped for the format picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatChoice(pub FileFormat);

impl fmt::Display for FormatChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.display_name())
    }
}

/// Every format `onerom-gen` decodes, in its own order.
pub fn formats() -> Vec<FormatChoice> {
    FileFormat::supported_values()
        .iter()
        .map(|format| FormatChoice(*format))
        .collect()
}

/// A size-handling mode wrapped for its picker.
///
/// `SizeHandling` is `#[non_exhaustive]` and derives neither `Copy` nor
/// `PartialEq`, both of which a `pick_list` option needs, so the comparison is
/// by discriminant here.
#[derive(Debug, Clone)]
pub struct SizingChoice(pub SizeHandling);

impl PartialEq for SizingChoice {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(&self.0) == std::mem::discriminant(&other.0)
    }
}

impl Eq for SizingChoice {}

impl fmt::Display for SizingChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            SizeHandling::None => "None",
            SizeHandling::Duplicate => "Duplicate",
            SizeHandling::Truncate => "Truncate",
            SizeHandling::Pad => "Pad",
            _ => "Unknown",
        };
        f.write_str(name)
    }
}

/// The four ways an image is fitted to its chip.
pub fn sizings() -> Vec<SizingChoice> {
    SizeHandling::supported_values()
        .iter()
        .map(|handling| SizingChoice(handling.clone()))
        .collect()
}

/// A chip-select polarity wrapped for its picker.
///
/// `Ignore` is left out: reaching it needs `allow_cs_ignore`, which this
/// builder never sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolarityChoice(pub bool);

impl fmt::Display for PolarityChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0 { "Active High" } else { "Active Low" })
    }
}

/// The two polarities a configurable chip select accepts.
pub const POLARITIES: [PolarityChoice; 2] = [PolarityChoice(false), PolarityChoice(true)];

// ---------------------------------------------------------------- plugins ---

/// A plugin binary sitting in the repo's `plugins/dist`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginChoice {
    /// The name shown in the picker.
    pub display: String,
    /// The binary, named in the config and read when building.
    pub path: PathBuf,
    /// Whether it occupies the system or the user plugin slot.
    pub system: bool,
}

impl fmt::Display for PluginChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

impl PluginChoice {
    /// The chip type of the slot this plugin occupies.
    pub fn chip_type(&self) -> ChipType {
        if self.system {
            ChipType::SystemPlugin
        } else {
            ChipType::UserPlugin
        }
    }

    /// The spelling the config uses for that chip type.
    ///
    /// `ChipType::try_from_str` accepts only the PascalCase name, while the
    /// config's own serde rename is snake_case, so the two are kept apart
    /// rather than one being parsed back out of the other.
    pub fn config_chip_type(&self) -> &'static str {
        if self.system {
            "system_plugin"
        } else {
            "user_plugin"
        }
    }
}

/// A plugin picker entry, including the `None` at the head of the list.
///
/// The pickers need a type that can print "None", and `Option` cannot carry
/// that itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSlot(pub Option<PluginChoice>);

impl fmt::Display for PluginSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(plugin) => f.write_str(&plugin.display),
            None => f.write_str("None"),
        }
    }
}

/// The plugins built in this tree, from `plugins/dist/<kind>/<name>/plugin.bin`.
///
/// An empty list is a normal outcome — nobody has run the plugin build — and
/// leaves the pickers showing None alone.
pub fn plugins() -> Vec<PluginChoice> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugins/dist")
        .canonicalize();
    let Ok(root) = root else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for (kind, system) in [("system", true), ("user", false)] {
        let Ok(entries) = std::fs::read_dir(root.join(kind)) else {
            continue;
        };
        for entry in entries.flatten() {
            let binary = entry.path().join("plugin.bin");
            if !binary.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            found.push(PluginChoice {
                display: display_name(&entry.path()).unwrap_or(name),
                path: binary,
                system,
            });
        }
    }
    found.sort_by(|a, b| a.display.cmp(&b.display));
    found
}

/// The plugin's own display name from its `plugin-meta.json`.
fn display_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("plugin-meta.json")).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(meta.get("display_name")?.as_str()?.to_owned())
}

// ----------------------------------------------------------- jumper header --

/// One drawn position of the board's jumper header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderPosition {
    /// The 5V/GND pair, marked with a red cross and never jumpered.
    Power,
    /// An image-select pair, gold and lettered. `bit` 0 is jumper A, the least
    /// significant bit of the slot number.
    Select {
        /// The slot-number bit this jumper carries.
        bit: u8,
        /// The silkscreen letter, `A` for bit 0.
        letter: char,
    },
    /// A populated pad that is not a slot select — leave it open.
    Other,
    /// A position with no pad, drawn as a gap so the columns either side keep
    /// their real spacing.
    Gap,
}

/// The board's header, column by column from its left edge.
///
/// `None` where the board carries no header descriptor, which is the signal to
/// fall back to text rather than draw a wireframe that would be a guess.
pub fn header_positions(board: Board) -> Option<Vec<HeaderPosition>> {
    let header = board.jumper_header()?;
    let last = header.columns.iter().map(|column| column.col).max()?;

    Some(
        (1..=last)
            .map(|col| {
                header
                    .columns
                    .iter()
                    .find(|column| column.col == col)
                    .map_or(HeaderPosition::Gap, classify)
            })
            .collect(),
    )
}

/// What one physical column is for.
///
/// A select role wins over power: every select column grounds its bottom pad,
/// so `gnd` alone would classify half the header as power. 5V is the tell.
fn classify(column: &onerom_config::hw::HeaderColumn) -> HeaderPosition {
    let rows = [Some(column.row1), Some(column.row2), column.row3];
    let roles = rows.iter().flatten().flat_map(|slot| match slot {
        HeaderSlot::Roles(roles) => *roles,
        _ => &[][..],
    });

    let mut has_power = false;
    for role in roles {
        match role {
            HeaderRole::Select(bit) => {
                return HeaderPosition::Select {
                    bit: *bit,
                    letter: (b'A' + bit) as char,
                };
            }
            HeaderRole::Power5V => has_power = true,
            _ => {}
        }
    }

    if has_power {
        return HeaderPosition::Power;
    }
    if matches!(column.row1, HeaderSlot::NotPopulated)
        && matches!(column.row2, HeaderSlot::NotPopulated)
    {
        return HeaderPosition::Gap;
    }
    HeaderPosition::Other
}

/// The image-select letters this board offers, `A` first.
///
/// Taken from the header where the board has one, and synthesised from the
/// select-pin count otherwise, so a board with no wireframe still gets the
/// right letters in its slot hints.
pub fn select_letters(board: Board) -> Vec<char> {
    let from_header = board.jumper_header().map(|header: JumperHeader| {
        let mut letters: Vec<char> = header
            .columns
            .iter()
            .filter_map(|column| match classify(column) {
                HeaderPosition::Select { letter, .. } => Some(letter),
                _ => None,
            })
            .collect();
        letters.sort_unstable();
        letters
    });

    from_header.unwrap_or_else(|| {
        (0..board.sel_pins().len())
            .map(|bit| (b'A' + bit as u8) as char)
            .collect()
    })
}

/// Which jumpers to close to boot slot `slot`, in words.
pub fn jumper_hint(board: Board, slot: usize) -> String {
    let closed: Vec<char> = (0..board.sel_pins().len())
        .filter(|bit| (slot >> bit) & 1 == 1)
        .map(|bit| (b'A' + bit as u8) as char)
        .collect();

    match closed.len() {
        0 => "all jumpers open".to_owned(),
        1 => format!("close jumper {}", closed[0]),
        _ => format!(
            "close jumpers {}",
            closed
                .iter()
                .map(char::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

// ------------------------------------------------------------- formatting ---

/// A byte count the way the site writes it: `16KB`, `1.5KB`, `2MB`.
pub fn kb(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        let mb = bytes as f64 / (1024.0 * 1024.0);
        if bytes.is_multiple_of(1024 * 1024) {
            format!("{mb:.0}MB")
        } else {
            format!("{mb:.2}MB")
        }
    } else {
        let kb = bytes as f64 / 1024.0;
        if bytes.is_multiple_of(1024) {
            format!("{kb:.0}KB")
        } else {
            format!("{kb:.1}KB")
        }
    }
}
