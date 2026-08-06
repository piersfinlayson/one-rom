// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument-convention tests for the commands that describe hardware.
//!
//! The structural rules - one meaning per short flag, no positionals, no
//! self-aliases, every option documented - are asserted over the whole clap
//! tree in `main.rs`'s `cli_assert` module, which can walk it directly. These
//! tests cover what that walk cannot see: that a given spelling actually
//! reaches the right code path, and that the spellings deliberately removed are
//! genuinely gone rather than quietly still accepted.

mod common;
use common::{fails, onerom};
use std::process::Command;

/// Run and return stdout, asserting success.
fn stdout(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(cmd: &mut Command) -> String {
    String::from_utf8_lossy(&cmd.output().unwrap().stderr).into_owned()
}

//
// board header / board socket: --board, not a positional
//

/// The board name reaches the renderer, and picks the board actually named.
///
/// Discriminating on the rendered title rather than on exit status: a command
/// that ignored --board and drew some other board would still exit 0.
#[test]
fn board_header_draws_the_board_named_by_the_option() {
    let out = stdout(onerom().args(["board", "header", "--board", "fire-24-f"]));
    assert!(out.contains("Fire 24"), "{out}");
    assert!(out.contains("Pin header"), "{out}");

    let other = stdout(onerom().args(["board", "header", "--board", "fire-28-c"]));
    assert!(other.contains("Fire 28"), "{other}");
    assert_ne!(out, other, "--board made no difference to the diagram");
}

#[test]
fn board_header_accepts_the_b_short_form() {
    let long = stdout(onerom().args(["board", "header", "--board", "fire-24-f"]));
    let short = stdout(onerom().args(["board", "header", "-b", "fire-24-f"]));
    assert_eq!(long, short);
}

#[test]
fn board_socket_draws_the_board_and_chip_named_by_the_options() {
    let gpios = stdout(onerom().args(["board", "socket", "--board", "fire-24-f"]));
    let functions = stdout(onerom().args([
        "board",
        "socket",
        "--board",
        "fire-24-f",
        "--chip-type",
        "2364",
    ]));
    assert_ne!(gpios, functions, "--chip-type made no difference");
    // The function view labels pins with ROM signals; the GPIO view does not.
    assert!(functions.contains("A0"), "{functions}");

    let short = stdout(onerom().args(["board", "socket", "-b", "fire-24-f", "-c", "2364"]));
    assert_eq!(functions, short, "short forms differ from long");
}

/// The positional form is gone, not merely undocumented.
#[test]
fn board_views_reject_a_positional_board() {
    fails(onerom().args(["board", "header", "fire-24-f"]));
    fails(onerom().args(["board", "socket", "fire-24-f"]));
}

/// Omitting the board with nothing connected must give advice the command can
/// actually take. This used to name --board while the command had no such
/// option.
#[test]
fn board_views_without_a_board_name_an_option_that_exists() {
    for view in ["header", "socket"] {
        let err = stderr(onerom().args(["board", view]));
        assert!(err.contains("--board"), "{view}: {err}");
        // The advice is only good if the option is real.
        let help = stdout(onerom().args(["board", view, "--help"]));
        assert!(help.contains("--board"), "{view}: {help}");
    }
}

//
// inspect header / inspect socket: --board overrides, it does not substitute
//

/// `--board` on the device-side views is an override, not a way to run without
/// a One ROM. With nothing connected the command must still fail, and must
/// point at the `board` form rather than at --board, which would loop.
#[test]
fn inspect_views_still_need_a_device_when_board_is_given() {
    for view in ["header", "socket"] {
        let err = stderr(onerom().args(["inspect", view, "--board", "fire-24-f"]));
        assert!(
            err.contains("No One ROM") || err.contains("board"),
            "{view}: {err}"
        );
        fails(onerom().args(["inspect", view, "--board", "fire-24-f"]));
    }
}

/// The option exists and is spelled the same as everywhere else.
#[test]
fn inspect_views_advertise_board_and_chip_type() {
    let header = stdout(onerom().args(["inspect", "header", "--help"]));
    assert!(header.contains("--board"), "{header}");

    let socket = stdout(onerom().args(["inspect", "socket", "--help"]));
    assert!(socket.contains("--board"), "{socket}");
    assert!(socket.contains("--chip-type"), "{socket}");
}

/// An unknown board name is rejected by the option itself, before any device
/// lookup - so the error names the bad board rather than the missing One ROM.
#[test]
fn an_unknown_board_name_is_rejected() {
    let err = stderr(onerom().args(["board", "header", "--board", "not-a-board"]));
    assert!(err.contains("not-a-board"), "{err}");
}

//
// Short flags removed on purpose
//

/// These letters were reclaimed so each means one thing CLI-wide. A command
/// that still accepted the old spelling would defeat that silently.
#[test]
fn reclaimed_short_flags_are_rejected() {
    // -b was --byte on poke; it is --board everywhere now.
    fails(onerom().args(["control", "poke", "live", "-b", "0xEA"]));
    // -l was --slot; it is --length only.
    fails(onerom().args(["control", "select", "-l", "2"]));
    fails(onerom().args(["update", "commit", "-l", "2"]));
    // -o was --offset on erase; it is --output only.
    fails(onerom().args(["control", "erase", "-o", "0x20000", "--length", "0x1000"]));
    // -m was --image on update slot; it is --msd only.
    fails(onerom().args(["update", "slot", "--slot", "0", "-m", "rom.bin"]));
}

/// The long forms those shorts belonged to still work.
#[test]
fn the_long_forms_of_reclaimed_shorts_still_work() {
    for (args, flag) in [
        (vec!["control", "poke", "live", "--help"], "--byte"),
        (vec!["control", "select", "--help"], "--slot"),
        (vec!["control", "erase", "--help"], "--offset"),
        (vec!["update", "slot", "--help"], "--image"),
    ] {
        let help = stdout(onerom().args(&args));
        assert!(help.contains(flag), "{args:?}: {help}");
    }
}

/// `-i` moved from the global --vid-pid to --input, and --vid-pid keeps its
/// --id alias so nothing is left unreachable.
#[test]
fn short_i_is_input_and_vid_pid_keeps_a_long_alias() {
    let help = stdout(onerom().args(["image", "swap-bytes", "--help"]));
    assert!(help.contains("-i, --input"), "{help}");
    // The global option no longer claims -i ...
    let root = stdout(onerom().arg("--help"));
    assert!(!root.contains("-i, --vid-pid"), "{root}");
    // ... and is still reachable by both long spellings.
    fails(onerom().args(["--vid-pid", "bad", "scan"]));
    fails(onerom().args(["--id", "bad", "scan"]));
}

//
// control erase: --stopped / --running, matching reboot and program
//

#[test]
fn control_erase_uses_the_same_reboot_flag_names_as_reboot_and_program() {
    let help = stdout(onerom().args(["control", "erase", "--help"]));
    assert!(help.contains("--stopped"), "{help}");
    assert!(help.contains("--running"), "{help}");
    // The old spellings are a clean break, not aliases.
    assert!(!help.contains("--reboot-stopped"), "{help}");
    assert!(!help.contains("--reboot-running"), "{help}");
    fails(onerom().args(["control", "erase", "--all", "--reboot-stopped"]));
}

//
// --config as the primary spelling
//

#[test]
fn config_is_the_primary_spelling_and_the_old_ones_still_parse() {
    let help = stdout(onerom().args(["program", "--help"]));
    assert!(help.contains("--config <FILE>"), "{help}");
    assert!(
        help.contains("--config-file"),
        "expected alias listed: {help}"
    );

    // Every spelling reaches the same option: each fails on the missing file,
    // not on an unknown argument.
    for spelling in ["--config", "--config-file", "--config-json", "--json"] {
        let err = stderr(onerom().args(["firmware", "build", spelling, "/nonexistent.json"]));
        assert!(
            !err.contains("unexpected argument"),
            "{spelling} not accepted: {err}"
        );
    }
}

//
// image convert: formats validated at parse time, from onerom-gen's list
//

#[test]
fn image_convert_rejects_an_unknown_format_before_touching_files() {
    let err = stderr(onerom().args([
        "image",
        "convert",
        "--from",
        "nope",
        "--to",
        "binary",
        "--input",
        "/nonexistent",
        "--output",
        "/nonexistent",
    ]));
    assert!(err.contains("nope"), "{err}");
    assert!(err.contains("binary"), "expected the value list: {err}");
    assert!(err.contains("ihex"), "expected the value list: {err}");
    // Parse-time, so the unreadable --input never got as far as being opened.
    assert!(!err.contains("No such file"), "{err}");
}

#[test]
fn image_convert_lists_its_formats_in_help() {
    let help = stdout(onerom().args(["image", "convert", "--help"]));
    assert!(help.contains("[possible values: binary, ihex]"), "{help}");
}

/// The aliases predate the value parser and must survive it - a plain list of
/// possible values would have accepted only the canonical names.
#[test]
fn image_convert_still_accepts_format_aliases() {
    for alias in ["bin", "raw", "hex", "intel-hex", "intelhex", "BINARY"] {
        let err = stderr(onerom().args([
            "image",
            "convert",
            "--from",
            alias,
            "--to",
            "binary",
            "--input",
            "/nonexistent",
            "--output",
            "/nonexistent",
        ]));
        // Accepted by the parser: it got far enough to try opening the file.
        assert!(
            err.contains("No such file") || err.contains("input/output"),
            "alias '{alias}' rejected: {err}"
        );
    }
}

/// --load-address is validated by the same parser the config file uses, so it
/// fails at parse time and accepts the config's `$`-prefixed hex.
#[test]
fn image_convert_load_address_is_parsed_at_parse_time() {
    let err = stderr(onerom().args([
        "image",
        "convert",
        "--from",
        "binary",
        "--to",
        "ihex",
        "--input",
        "/nonexistent",
        "--output",
        "/nonexistent",
        "--load-address",
        "wibble",
    ]));
    assert!(err.contains("wibble"), "{err}");
    assert!(
        !err.contains("No such file"),
        "should fail before I/O: {err}"
    );

    // The config file's spellings all reach the conversion.
    for spelling in ["$E000", "0xE000", "57344"] {
        let err = stderr(onerom().args([
            "image",
            "convert",
            "--from",
            "binary",
            "--to",
            "ihex",
            "--input",
            "/nonexistent",
            "--output",
            "/nonexistent",
            "--load-address",
            spelling,
        ]));
        assert!(
            err.contains("No such file") || err.contains("input/output"),
            "'{spelling}' rejected: {err}"
        );
    }
}

//
// --slot spec keys
//

/// `size-handling` is spelled like every other multi-word slot key. It was the
/// only one missing its kebab form.
#[test]
fn slot_size_handling_accepts_the_kebab_spelling() {
    for spelling in ["size-handling", "size_handling", "size"] {
        let err = stderr(onerom().args([
            "firmware",
            "build",
            "--board",
            "fire-24-e",
            "--slot",
            &format!("file=/nonexistent.bin,type=2364,cs1=active_low,{spelling}=pad"),
        ]));
        assert!(
            !err.contains("Unrecognised slot key"),
            "'{spelling}' rejected: {err}"
        );
    }
}

/// The advertised key list is what a user is told to use, so every key in it
/// must parse. A kebab-vs-snake slip here is invisible until someone copies the
/// error message's own advice and it fails.
#[test]
fn every_advertised_slot_key_is_accepted() {
    let err = stderr(onerom().args([
        "firmware",
        "build",
        "--board",
        "fire-24-e",
        "--slot",
        "file=/nonexistent.bin,type=2364,cs1=active_low,nonsense=1",
    ]));
    let listed = err
        .split("Supported keys:")
        .nth(1)
        .unwrap_or_else(|| panic!("no key list in: {err}"));
    let keys: Vec<&str> = listed
        .lines()
        .next()
        .unwrap()
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    assert!(keys.len() > 10, "key list looks truncated: {keys:?}");

    for key in keys {
        // A value that is wrong for the key still proves the key was known:
        // the complaint is about the value, never "Unrecognised slot key".
        let err = stderr(onerom().args([
            "firmware",
            "build",
            "--board",
            "fire-24-e",
            "--slot",
            &format!("file=/nonexistent.bin,type=2364,cs1=active_low,{key}=zzz"),
        ]));
        assert!(
            !err.contains("Unrecognised slot key"),
            "advertised key '{key}' is not accepted: {err}"
        );
    }
}

/// Chip-select values take the CLI's kebab spelling as well as the config
/// file's snake one, and `ignore` reaches the rule that governs it.
///
/// `cs1` was the last multi-word slot value that took only snake_case, while
/// `format` and `transform` already took both.
#[test]
fn slot_cs_values_accept_both_spellings_and_ignore() {
    for spelling in ["active-low", "active_low", "0"] {
        let err = stderr(onerom().args([
            "firmware",
            "build",
            "--board",
            "fire-24-e",
            "--slot",
            &format!("file=/nonexistent.bin,type=2364,cs1={spelling}"),
        ]));
        assert!(
            !err.contains("Invalid CS logic"),
            "cs1={spelling} rejected: {err}"
        );
    }

    // `ignore` now parses, so the answer comes from the rule that governs it
    // rather than from the value parser.
    let err = stderr(onerom().args([
        "firmware",
        "build",
        "--board",
        "fire-24-e",
        "--slot",
        "file=/nonexistent.bin,type=2332,cs1=active-low,cs2=ignore",
    ]));
    assert!(!err.contains("Invalid CS logic"), "{err}");
    assert!(err.contains("ignore"), "{err}");
}

/// A bad chip-select value still fails, and lists every accepted value.
#[test]
fn slot_rejects_an_unknown_cs_value_listing_the_alternatives() {
    let err = stderr(onerom().args([
        "firmware",
        "build",
        "--board",
        "fire-24-e",
        "--slot",
        "file=/nonexistent.bin,type=2364,cs1=sideways",
    ]));
    assert!(err.contains("Invalid CS logic"), "{err}");
    for value in ["active-low", "active-high", "ignore"] {
        assert!(err.contains(value), "{value} missing from: {err}");
    }
}
