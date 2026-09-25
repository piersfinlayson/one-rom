// tests/plugin_headers.rs
//
// Tests for what the two plugin-facing headers say about the release each
// thing in them arrived in.  A plugin author sets min_fw_version from it, and
// api.h carries the same line by hand for what it declares.
//
// The generators are called directly, because what they emit from this
// crate's own schema is the thing under test.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata_gen::schema::Schema;
use onerom_metadata_gen::{c_gen, constants_gen, keys_gen};

const SINCE: &str = "// @since firmware ";

/// The schema the firmware and its plugins are really built from.
fn shipped() -> Schema {
    Schema::parse(include_str!("../metadata_schema.toml"))
        .expect("the shipped schema should have been accepted")
}

/// The line above `line` in `header`, or a message naming what was looked for.
fn line_above<'a>(header: &'a str, line: &str) -> &'a str {
    let lines: Vec<&str> = header.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.trim_start().starts_with(line))
        .unwrap_or_else(|| panic!("the header carries no line opening '{line}':\n{header}"));
    assert!(at > 0, "'{line}' opens the header");
    lines[at - 1].trim_start()
}

// ===========================================================================
// The constants header
// ===========================================================================

/// Every constant, not a chosen one: a constant reaching the plugin API
/// without its release is exactly the thing an author cannot work around.
#[test]
fn every_plugin_constant_names_the_release_it_arrived_in() {
    let schema = shipped();
    let header = constants_gen::generate(&schema);

    for constant in schema.ora_constants() {
        let release = constant
            .first_release
            .as_deref()
            .expect("an ora_api constant carries a first_release");
        assert_eq!(
            line_above(&header, &format!("#define {}", constant.ora_name())),
            format!("{SINCE}{release}"),
            "{} does not name the release it arrived in:\n{header}",
            constant.ora_name()
        );
    }
}

/// The line sits below the constant's own comment, the way api.h closes a doc
/// block with it.
#[test]
fn the_since_line_closes_the_constants_comment() {
    let header = constants_gen::generate(&shipped());
    assert!(
        header.contains(
            "// Sentinel: no GPIO is connected to this pin position\n\
             // @since firmware 0.7.2\n\
             #define ORA_GPIO_NONE"
        ),
        "unexpected header:\n{header}"
    );
}

/// The release a constant reached the plugin API in is for whoever sets
/// min_fw_version.  The firmware's own header is rebuilt from this schema
/// whenever it changes, so it has no version to ask for.
#[test]
fn the_firmware_header_carries_no_since_lines() {
    let header = c_gen::generate(&shipped());
    assert!(!header.contains("@since"), "unexpected header:\n{header}");
}

// ===========================================================================
// The key header
// ===========================================================================

#[test]
fn every_plugin_key_names_the_release_it_arrived_in() {
    let schema = shipped();
    let header = keys_gen::generate(&schema);

    for entry in schema.plugin_keys() {
        assert_eq!(
            line_above(&header, &format!("ORA_METADATA_KEY_{} ", entry.key.name)),
            format!("{SINCE}{}", entry.key.first_release),
            "ORA_METADATA_KEY_{} does not name the release it arrived in:\n{header}",
            entry.key.name
        );
    }
}

/// The two sentinels are not metadata and arrived with the header itself, so
/// neither carries a release of its own.
#[test]
fn the_sentinels_carry_no_since_line() {
    let header = keys_gen::generate(&shipped());
    for sentinel in ["ORA_METADATA_KEY_NONE", "ORA_METADATA_KEY_INVALID"] {
        assert!(
            !line_above(&header, &format!("{sentinel} ")).starts_with(SINCE),
            "{sentinel} names a release:\n{header}"
        );
    }
}
