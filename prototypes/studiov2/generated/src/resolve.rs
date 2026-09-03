// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Where an option's values come from, turned into the values.
//!
//! [`Source`] names a set and deliberately does not carry it.  This is the one
//! place that turns a name into values, and it reads them from the crates that
//! own them — `onerom-config` for boards and chip types, `onerom-cli` for pins,
//! colours and plugin types.  A board added to the tree reaches every pane that
//! takes one with nobody typing anything here.
//!
//! Nothing in this module knows what a widget is.  It answers with an [`Offer`]
//! and [`crate::ui::value_widget`] decides what that looks like, which is the
//! same split the description itself draws.
//!
//! ## What this build makes up
//!
//! Three answers cannot be read here and are written out instead, all named in
//! [`FAKED`] so the screen can say so: the released versions, which the shipped
//! app fetches from `images.onerom.org`, the attached devices, which the
//! shipped app enumerates over USB, and [`browse_path`], because there is no
//! file dialog in this prototype.

use onerom_cli::colour::{RgbColour, parse_colour};
use onerom_cli::pin::parse_pin;
use onerom_cli::plugin::PluginType;
use onerom_config::chip::CHIP_TYPES;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::{Board, Model};
use studiov2_commands::Source;
use studiov2_shared::Shared;

/// What this screen makes up rather than reads, in one sentence.
///
/// Shown on the page.  A prototype that quietly invented a device list would be
/// a prototype nobody could trust the rest of.
pub const FAKED: &str = "Faked here: Browse fills in a sample path instead of \
                         opening a dialog, the version lists are written out \
                         rather than fetched, and the devices are invented.";

/// The pad names `--pin` spells, from `gpio<N>` aside.
///
/// `onerom_cli::pin` documents `sel_a`..`sel_e`, `x1` and `x2` as the whole
/// namespace, and the board decides which of them it actually has — that is
/// what [`Context::pins`] asks it.
const PAD_NAMES: [&str; 7] = ["sel_a", "sel_b", "sel_c", "sel_d", "sel_e", "x1", "x2"];

/// The firmware versions offered for a `--version`.
///
/// FAKED: the shipped app reads `releases.json` from `images.onerom.org`, and
/// this prototype does no network I/O.  Written as numbers rather than strings
/// so the spelling on screen is `FirmwareVersion`'s own.
const RELEASES: [(u16, u16, u16); 4] = [(0, 7, 2), (0, 7, 1), (0, 7, 0), (0, 6, 14)];

/// The CLI releases offered for a `self download --version`.
///
/// FAKED, and a second list rather than a filter over [`RELEASES`], because the
/// CLI and the firmware are versioned apart.  `onerom-cli` publishes its own
/// version to Cargo and not to a caller, so even the one this build links
/// against is out of reach here.
const CLI_RELEASES: [(u16, u16, u16); 3] = [(0, 4, 0), (0, 3, 1), (0, 3, 0)];

/// What the app knows that a set of values can depend on.
///
/// One field today, and a struct rather than that field because the dependency
/// is the point: a pin means nothing without a board, and a screen that passed
/// the board around loose would have nowhere to put the next such fact.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// The board in front of the user, where one is selected.
    board: Option<Board>,
}

impl Context {
    /// Reads the board off the selected device.
    ///
    /// The shared device carries its board as a string on purpose — see
    /// [`studiov2_shared::device`] — so this is where it becomes a `Board`.  A
    /// name no board answers to leaves it unknown, which is the same state as
    /// no device at all and draws the same way.
    pub fn new(shared: &Shared) -> Self {
        Self {
            board: shared
                .device
                .as_ref()
                .and_then(|device| Board::try_from_str(&device.board)),
        }
    }

    /// Every header pad this board has, spelled the way `--pin` takes it.
    ///
    /// Asked of the pin module rather than worked out from the board's pin
    /// arrays: a pad that parses and resolves is a pad the CLI would accept,
    /// and anything else here would be this screen's opinion of the rule.
    fn pins(board: Board) -> Vec<String> {
        PAD_NAMES
            .into_iter()
            .filter(|name| parse_pin(name).is_ok_and(|pin| pin.resolve(Some(&board)).is_ok()))
            .map(str::to_owned)
            .collect()
    }
}

/// What a Browse button is picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browse {
    /// A file that has to exist.
    Open,
    /// A path to write.
    Save,
    /// A directory.
    Directory,
}

/// What an option's values look like, once the source has been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Values to pick from, and whether the CLI takes others besides.
    ///
    /// `open` is what stops a pick list narrowing the option: `--colour` takes
    /// ten names *and* any hex colour, and offering the names alone would take
    /// the hex away.  An open set gets a box beside its list.
    Pick {
        /// The values, in the order they should be offered.
        choices: Vec<String>,
        /// Whether a value outside the list is still legal.
        open: bool,
    },

    /// A path, with the button that fills it in.
    Browse(Browse),

    /// Nothing this build can enumerate, so the option keeps its text box.
    Free,
}

/// The values behind a source, in the context the app is in.
///
/// An option with no source answers [`Offer::Free`], which is what leaves every
/// option the description says nothing about exactly as it was.
pub fn offer(source: Option<Source>, context: &Context) -> Offer {
    let Some(source) = source else {
        return Offer::Free;
    };

    match source {
        Source::OpenFile => Offer::Browse(Browse::Open),
        Source::SaveFile => Offer::Browse(Browse::Save),
        Source::Directory => Offer::Browse(Browse::Directory),

        // Fire only.  Studio v2 talks to a One ROM over USB, and Ice is the
        // legacy STM32F4 hardware, so an Ice board here would be a board this
        // app could never reach.
        Source::Board => closed(
            Model::Fire
                .boards()
                .iter()
                .map(|board| board.name().to_owned()),
        ),

        // Plugin slots rather than a variant list: slot 0 is the system plugin
        // and slot 1 the user plugin, and asking which type owns each slot is
        // how the type itself says how many there are.
        Source::PluginType => closed(
            (0..)
                .map_while(PluginType::from_slot_index)
                .map(|kind| kind.short().to_owned()),
        ),

        Source::ChipType => closed(
            CHIP_TYPES
                .iter()
                .filter(|chip| !chip.is_plugin())
                .map(|chip| chip.name().to_owned()),
        ),

        // Open, because a colour is also a hex value.
        Source::Colour => open(RgbColour::names().map(str::to_owned)),

        // Open, because `gpio<N>` is legal for any N the device has, and how
        // many that is comes from the device rather than from the board.
        Source::Pin => match context.board {
            Some(board) => open(Context::pins(board).into_iter()),
            None => Offer::Free,
        },

        // FAKED, and open, because the real set is whatever has been released.
        Source::Version => open(versions(&RELEASES)),
        Source::CliVersion => open(versions(&CLI_RELEASES)),

        // FAKED: `studiov2_shared::device::attached` invents its devices.
        Source::Serial => closed(
            studiov2_shared::device::attached()
                .into_iter()
                .map(|device| device.serial),
        ),
    }
}

/// A set the CLI accepts nothing outside of.
fn closed(choices: impl Iterator<Item = String>) -> Offer {
    Offer::Pick {
        choices: choices.collect(),
        open: false,
    }
}

/// A set the CLI takes other values besides.
fn open(choices: impl Iterator<Item = String>) -> Offer {
    Offer::Pick {
        choices: choices.collect(),
        open: true,
    }
}

/// Version numbers spelled the way `FirmwareVersion` spells them.
fn versions(releases: &[(u16, u16, u16)]) -> impl Iterator<Item = String> + '_ {
    releases
        .iter()
        .map(|(major, minor, patch)| FirmwareVersion::new(*major, *minor, *patch, 0).to_string())
}

/// The colour a value stands for, where the source deals in colours.
///
/// Takes the raw value rather than a chosen one so a hex colour typed by hand
/// shows its swatch too, which is the half a pick list cannot reach.
pub fn swatch(source: Option<Source>, value: &str) -> Option<(u8, u8, u8)> {
    match source {
        Some(Source::Colour) => parse_colour(value).ok().map(|colour| colour.rgb()),
        _ => None,
    }
}

/// The path a Browse button fills in.
///
/// FAKED: there is no file dialog here, and adding one would put a windowing
/// dependency into a prototype about generated layout.  The answer is a sample
/// path under the user's home directory, so the pane shows what a chosen path
/// looks like on the row and on the command line.
pub fn browse_path(browse: Browse) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/you".to_owned());
    match browse {
        Browse::Open => format!("{home}/one-rom/kernal.rom"),
        Browse::Save => format!("{home}/one-rom/onerom.bin"),
        Browse::Directory => format!("{home}/one-rom"),
    }
}
