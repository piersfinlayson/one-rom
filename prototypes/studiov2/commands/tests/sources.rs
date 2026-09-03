// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Where the description says an option's values come from.
//!
//! Two things can go wrong and neither shows up as a build failure.  A
//! placeholder the CLI renames stops deriving, quietly turning a picker back
//! into a text box.  And an entry in `sources.rs` can be right about the
//! command and wrong about what the option does, which nothing mechanical can
//! catch.
//!
//! The same `sources.rs` the build reads is pasted in here, so these are tests
//! of the table itself and not of a copy of it.

use studiov2_commands::{COMMANDS, Command, GLOBALS, Kind, Opt, Source};

include!("../sources.rs");

/// How many options the placeholder alone settles.
///
/// It is the number that says whether the approach pays for itself: 36 of the
/// 62 come from a rule that needed nobody to type anything, and 25 of the 26
/// entries written by hand are the `FILE` options.  The odd one is `self
/// download --version`, where a placeholder derives and derives wrongly.
const DERIVED_OPTIONS: usize = 36;

/// The command the CLI puts at that path.
fn command(path: &[&str]) -> &'static Command {
    COMMANDS
        .iter()
        .find(|command| command.path == path)
        .unwrap_or_else(|| panic!("no command at onerom {}", path.join(" ")))
}

/// The option of that name on a command.
fn opt<'a>(command: &'a Command, long: &str) -> &'a Opt {
    command
        .opts
        .iter()
        .find(|opt| opt.long == long)
        .unwrap_or_else(|| panic!("onerom {} has no --{long}", command.path.join(" ")))
}

/// Every option there is, with the command it belongs to.
fn all() -> impl Iterator<Item = (String, &'static Opt)> {
    let globals = GLOBALS.iter().map(|opt| (String::new(), opt));
    let commands = COMMANDS
        .iter()
        .flat_map(|command| command.opts.iter().map(|opt| (command.path.join(" "), opt)));
    globals.chain(commands)
}

#[test]
fn image_convert_reads_one_file_and_writes_the_other() {
    let convert = command(&["image", "convert"]);
    assert_eq!(opt(convert, "input").source, Some(Source::OpenFile));
    assert_eq!(opt(convert, "output").source, Some(Source::SaveFile));
}

#[test]
fn a_board_is_a_board_wherever_it_appears() {
    let boards: Vec<&str> = all()
        .filter(|(_, opt)| opt.long == "board")
        .map(|(path, opt)| {
            assert_eq!(
                opt.source,
                Some(Source::Board),
                "onerom {path} --board does not offer the boards"
            );
            opt.long
        })
        .collect();

    assert_eq!(boards.len(), 14, "the CLI has 14 --board options");
}

#[test]
fn control_rgb_on_offers_the_colours() {
    let on = command(&["control", "rgb", "on"]);
    assert_eq!(opt(on, "colour").source, Some(Source::Colour));
}

#[test]
fn inspect_gpio_offers_the_pins() {
    let gpio = command(&["inspect", "gpio"]);
    assert_eq!(opt(gpio, "pin").source, Some(Source::Pin));
}

#[test]
fn every_annotation_names_a_real_command_and_option() {
    for (path, long, _) in ANNOTATIONS {
        let opts = if path.is_empty() {
            GLOBALS
        } else {
            command(&path.split(' ').collect::<Vec<&str>>()).opts
        };
        assert!(
            opts.iter().any(|opt| opt.long == *long),
            "sources.rs annotates onerom {path} --{long}, which is not an option of it"
        );
    }
}

#[test]
fn every_derivation_entry_reaches_something() {
    for (value_name, _) in DERIVED {
        assert!(
            all().any(|(_, opt)| opt.value_name == Some(value_name)),
            "sources.rs derives from {value_name}, which no option shows"
        );
    }
}

#[test]
fn an_annotation_never_repeats_what_a_placeholder_says() {
    // An entry may correct a placeholder, and none may agree with one.  A
    // table that says again what a rule already says is a table that will one
    // day disagree with it, and nothing would notice which had moved.
    for (path, long, source) in ANNOTATIONS {
        let opts = if path.is_empty() {
            GLOBALS
        } else {
            command(&path.split(' ').collect::<Vec<&str>>()).opts
        };
        let opt = opts
            .iter()
            .find(|opt| opt.long == *long)
            .expect("the entry names a real option");

        let derived = DERIVED
            .iter()
            .find(|(value_name, _)| opt.value_name == Some(value_name))
            .map(|(_, derived)| derived);

        assert_ne!(
            derived,
            Some(source),
            "onerom {path} --{long} is annotated with what its placeholder already gives"
        );
    }
}

#[test]
fn the_placeholder_does_most_of_the_work() {
    let sourced = all().filter(|(_, opt)| opt.source.is_some()).count();

    assert_eq!(
        sourced,
        DERIVED_OPTIONS + ANNOTATIONS.len(),
        "an option is sourced twice, or by neither table"
    );
    assert_eq!(sourced - ANNOTATIONS.len(), DERIVED_OPTIONS);
}

#[test]
fn a_serial_a_user_invents_is_not_a_serial_to_pick_from() {
    // `--serial DEVICE` picks one of the One ROMs on the bus.  `--serial-
    // override SERIAL` is the serial a user is about to give a One ROM, and
    // nothing can offer a list of those.
    let global = GLOBALS
        .iter()
        .find(|opt| opt.long == "serial")
        .expect("--serial is global");
    assert_eq!(global.source, Some(Source::Serial));

    let program = opt(command(&["program"]), "serial-override");
    assert_eq!(program.value_name, Some("SERIAL"));
    assert_eq!(program.source, None);
}

#[test]
fn the_cli_and_the_firmware_are_two_release_lists() {
    // Both options show `VERSION`, so the rule alone would offer a user
    // firmware releases to pick a CLI download from.  This is the one option
    // an entry corrects rather than reaches, and it is what proves an entry
    // beats a placeholder rather than only filling a gap one left.
    let cli = opt(command(&["self", "download"]), "version");
    assert_eq!(cli.value_name, Some("VERSION"));
    assert_eq!(cli.source, Some(Source::CliVersion));

    let firmware = opt(command(&["firmware", "download"]), "version");
    assert_eq!(firmware.value_name, Some("VERSION"));
    assert_eq!(firmware.source, Some(Source::Version));
}

#[test]
fn a_flag_takes_no_value_and_so_has_no_source() {
    for (path, opt) in all() {
        if matches!(opt.kind, Kind::Flag) {
            assert_eq!(
                opt.source, None,
                "onerom {path} --{} is a flag with a value source",
                opt.long
            );
        }
    }
}
