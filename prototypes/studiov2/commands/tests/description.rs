// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What the CLI's argument definitions must still say.
//!
//! Nothing else checks the description: it is written by a build script that
//! reads another crate's source, so a change over there that this reader gets
//! wrong produces a smaller description and no error at all.  These tests are
//! the thing that notices.
//!
//! They are deliberately specific.  A count and a named list fail when the CLI
//! moves under the reader, and the failure says what went missing.

use studiov2_commands::{COMMANDS, Command, GLOBALS, Group, Kind, Opt};

/// Every option the CLI has, counted by hand from `rust/cli/src/args/`.
///
/// 167 belong to a command and 6 are global.  A reader that quietly stops
/// understanding an attribute drops options rather than failing, and this is
/// what turns that into a test failure.
const TOTAL_OPTIONS: usize = 173;

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

fn choices(opt: &Opt) -> &[&str] {
    match &opt.kind {
        Kind::Choice(values) => values,
        _ => panic!("--{} is not a choice", opt.long),
    }
}

/// The group of that name on a command.
fn group<'a>(command: &'a Command, name: &str) -> &'a Group {
    command
        .groups
        .iter()
        .find(|group| group.name == name)
        .unwrap_or_else(|| panic!("onerom {} has no {name} group", command.path.join(" ")))
}

#[test]
fn every_path_is_named_and_unique() {
    let mut seen: Vec<&[&str]> = Vec::new();

    for command in COMMANDS {
        assert!(!command.path.is_empty(), "a command has no path");
        assert!(
            !seen.contains(&command.path),
            "onerom {} is described twice",
            command.path.join(" ")
        );
        seen.push(command.path);
    }
}

#[test]
fn every_command_says_what_it_does() {
    for command in COMMANDS {
        assert!(
            !command.about.is_empty(),
            "onerom {} has no description",
            command.path.join(" ")
        );
    }
}

#[test]
fn the_commands_that_must_be_there_are() {
    for path in [
        ["scan"].as_slice(),
        &["program"],
        &["image", "convert"],
        &["image", "swap-bytes"],
        &["image", "deinterleave"],
        &["control", "rgb", "on"],
        &["control", "pin"],
        &["control", "erase"],
        &["inspect", "gpio"],
        &["inspect", "peek", "live"],
        &["firmware", "build"],
        &["monitor", "log"],
        &["board", "socket"],
        &["self", "download"],
    ] {
        command(path);
    }
}

#[test]
fn a_command_reached_twice_is_described_once() {
    // Each of these is a second spelling of a command described elsewhere: the
    // CLI's own top-level shortcuts, and `program` repeated under `firmware`.
    for (spelling, described) in [
        (["peek"].as_slice(), ["inspect", "peek", "live"].as_slice()),
        (&["poke"], &["control", "poke", "live"]),
        (&["reboot"], &["control", "reboot"]),
        (&["chips"], &["firmware", "chips"]),
        (&["firmware", "program"], &["program"]),
    ] {
        assert!(
            !COMMANDS.iter().any(|command| command.path == spelling),
            "onerom {} is described as well as onerom {}",
            spelling.join(" "),
            described.join(" ")
        );
        command(described);
    }
}

#[test]
fn a_hidden_command_is_not_described() {
    assert!(
        !COMMANDS
            .iter()
            .any(|command| command.path == ["update", "otp"]),
        "onerom update otp is hidden in the CLI and should not be described"
    );
}

#[test]
fn image_convert_takes_its_five_options() {
    let convert = command(&["image", "convert"]);
    let longs: Vec<&str> = convert.opts.iter().map(|opt| opt.long).collect();
    assert_eq!(longs, ["from", "to", "input", "output", "load-address"]);

    for long in ["from", "to"] {
        assert_eq!(choices(opt(convert, long)), ["binary", "ihex", "srec"]);
    }

    let load_address = opt(convert, "load-address");
    assert!(load_address.optional);
    assert!(matches!(load_address.kind, Kind::Domain("LoadAddress")));
}

#[test]
fn control_rgb_on_knows_the_american_spelling() {
    let colour = opt(command(&["control", "rgb", "on"]), "colour");
    assert_eq!(colour.aliases, ["color"]);
    assert_eq!(colour.default, Some("red"));
}

#[test]
fn a_global_is_not_repeated_on_a_command() {
    for command in COMMANDS {
        for opt in command.opts {
            assert!(
                !GLOBALS.iter().any(|global| global.long == opt.long),
                "onerom {} repeats the global --{}",
                command.path.join(" "),
                opt.long
            );
        }
    }
}

#[test]
fn the_globals_are_the_six_the_cli_declares() {
    let longs: Vec<&str> = GLOBALS.iter().map(|opt| opt.long).collect();
    assert_eq!(
        longs,
        [
            "serial",
            "vid-pid",
            "unrecognised",
            "yes",
            "verbose",
            "log-level"
        ]
    );
}

#[test]
fn every_option_says_what_it_is_for() {
    for opt in GLOBALS.iter().chain(COMMANDS.iter().flat_map(|c| c.opts)) {
        assert!(!opt.help.is_empty(), "--{} has no help", opt.long);
    }
}

#[test]
fn nothing_has_gone_missing() {
    let counted: usize = COMMANDS.iter().map(|command| command.opts.len()).sum();
    assert_eq!(counted + GLOBALS.len(), TOTAL_OPTIONS);
}

#[test]
fn a_repeatable_option_is_marked_repeatable() {
    let erase = command(&["control", "erase"]);
    for long in ["offset", "address", "length"] {
        let opt = opt(erase, long);
        assert!(opt.multiple, "--{long} can be given more than once");
        assert!(matches!(opt.kind, Kind::Number));
    }
    assert!(matches!(opt(erase, "all").kind, Kind::Flag));
}

#[test]
fn control_erase_says_you_must_pick_one_target() {
    let erase = command(&["control", "erase"]);
    let target = group(erase, "erase_target");

    assert_eq!(target.opts, ["all", "offset", "address"]);
    assert!(target.required, "erase_target has to be given");
    assert!(!target.multiple, "erase_target takes one of the three");
}

#[test]
fn control_reboot_will_not_go_two_ways_at_once() {
    let msd = opt(command(&["control", "reboot"]), "msd");
    assert_eq!(msd.conflicts, ["running"]);
}

#[test]
fn firmware_build_config_rules_out_the_hand_written_form() {
    let config = opt(command(&["firmware", "build"]), "config");
    assert_eq!(
        config.conflicts,
        [
            "slot",
            "config-name",
            "config-description",
            "save-config",
            "no-config"
        ]
    );
}

#[test]
fn every_name_in_a_relationship_points_at_something() {
    for command in COMMANDS {
        let named = |name: &str| {
            command.opts.iter().any(|opt| opt.long == name)
                || command.groups.iter().any(|group| group.name == name)
        };

        for opt in command.opts {
            for name in opt.conflicts.iter().chain(opt.requires) {
                assert!(
                    named(name),
                    "onerom {} says --{} relates to {name}, which it does not have",
                    command.path.join(" "),
                    opt.long
                );
            }
        }

        for group in command.groups {
            assert!(!group.opts.is_empty(), "a group has no members");
            for name in group.opts {
                assert!(
                    command.opts.iter().any(|opt| opt.long == *name),
                    "onerom {}'s {} group holds {name}, which is not an option of it",
                    command.path.join(" "),
                    group.name
                );
            }
        }
    }
}

#[test]
fn a_relationship_names_the_option_a_user_types() {
    // clap knows `--config` by its field name, `config_file`, and that is what
    // the conflicts in the CLI's source are written in.  A pane has only the
    // long name to match against.
    let build = command(&["firmware", "build"]);
    assert_eq!(opt(build, "config-name").conflicts, ["config"]);
    assert!(
        !build.opts.iter().any(|opt| opt.long == "config-file"),
        "config-file is an alias of --config, not an option in its own right"
    );
}

#[test]
fn nothing_shows_a_user_a_name_it_could_not_resolve() {
    // Two shapes of unresolved value have reached a pane: a `{CONSTANT}` left
    // where a firmware number belongs, and a `LogLevel::Warn` left where a
    // typed value belongs.  Both are Rust that escaped into a user's face, and
    // both are caught by refusing either spelling anywhere.
    for opt in GLOBALS.iter().chain(COMMANDS.iter().flat_map(|c| c.opts)) {
        for (what, text) in [("help", opt.help), ("default", opt.default.unwrap_or(""))] {
            assert!(
                !text.contains('{'),
                "--{}'s {what} holds an unresolved constant: {text}",
                opt.long
            );
            assert!(
                !text.contains("::"),
                "--{}'s {what} holds a Rust path: {text}",
                opt.long
            );
        }
    }
}

#[test]
fn a_default_is_a_value_a_user_could_type() {
    // The firmware chose 100ms, and the schema is where that is written down.
    assert_eq!(
        opt(command(&["control", "reset"]), "hold").default,
        Some("100")
    );

    let log_level = GLOBALS
        .iter()
        .find(|opt| opt.long == "log-level")
        .expect("--log-level is global");
    assert_eq!(log_level.default, Some("warn"));
    assert!(
        choices(log_level).contains(&"warn"),
        "a default has to be one of the option's own values"
    );
}

#[test]
fn a_help_quoting_the_firmware_quotes_the_number() {
    let blink = opt(command(&["control", "led", "blink"]), "period");
    assert_eq!(
        blink.help,
        "Milliseconds for one on and off. Defaults to 1000."
    );
}

#[test]
fn an_option_a_user_must_fill_in_says_so() {
    let convert = command(&["image", "convert"]);
    assert!(opt(convert, "input").must_supply());
    assert!(!opt(convert, "load-address").must_supply());

    // A default is as good as a value already supplied.
    assert!(!opt(command(&["control", "rgb", "on"]), "colour").must_supply());
}
