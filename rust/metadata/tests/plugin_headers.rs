// tests/plugin_headers.rs
//
// Tests for what the two plugin-facing headers say about the release each
// thing in them arrived in.  A plugin author sets min_fw_version from it, and
// api.h has the same line, written by hand, for what it declares.  It also
// tests the enum values in the constants header.
//
// The generators are called directly, because what they emit from this
// crate's own schema is the thing under test.  A fixture holds the kinds of
// enum value the schema doesn't put in the plugin API.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::{OneromBoardSize, Rp235xVariant};
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

#[test]
fn every_plugin_enum_value_says_the_release_it_arrived_in() {
    let schema = shipped();
    let header = constants_gen::generate(&schema);

    for e in schema.ora_enums() {
        for v in &e.variants {
            let release = e
                .value_release(v)
                .expect("an ora_api enum has a first_release");
            assert_eq!(
                line_above(&header, &format!("#define {} ", v.ora_name())),
                format!("{SINCE}{release}"),
                "{} doesn't say the release it arrived in:\n{header}",
                v.ora_name()
            );
        }
    }
}

/// A plugin compares what the BOARD_SIZE and RP_VARIANT keys return with
/// these, so each must hold the number the firmware records.
#[test]
fn the_board_size_and_variant_values_hold_the_firmwares_numbers() {
    let header = constants_gen::generate(&shipped());
    let values = [
        (
            "ORA_BOARD_SIZE_UNKNOWN",
            OneromBoardSize::BoardSizeUnknown as u8,
        ),
        ("ORA_BOARD_SIZE_M", OneromBoardSize::BoardSizeM as u8),
        ("ORA_BOARD_SIZE_L", OneromBoardSize::BoardSizeL as u8),
        (
            "ORA_BOARD_SIZE_OTHER",
            OneromBoardSize::BoardSizeOther as u8,
        ),
        ("ORA_RP235XB", Rp235xVariant::Rp235xb as u8),
        ("ORA_RP235XA", Rp235xVariant::Rp235xa as u8),
    ];
    for (name, value) in values {
        let number = match value {
            0..=9 => value.to_string(),
            _ => format!("0x{value:02X}"),
        };
        let define = format!("#define {name} ((uint8_t){number})\n");
        assert!(header.contains(&define), "no '{define}' in:\n{header}");
    }
}

/// The schema's plugin enums are one byte and don't have a sentinel or
/// deprecated value, so a fixture stands in for those.
const ENUM_FIXTURE: &str = r#"
[schema]
format_version = 1
firmware_release = "0.8.0"
name = "Test"
description = "Plugin header fixture"
flash_base = 0x10000000
root_struct = "header_t"

[[versions]]
struct_name = "header_t"
version = 1
first_release = "0.8.0"

[[constants]]
name = "HEADER_VERSION"
type = "u32"
value = 1

[[enums]]
name = "mode_t"
size = 2
packed = true
ora_api = true
first_release = "0.8.0"

[[enums.variants]]
name = "MODE_SLOW"
value = 0
comment = "Slow"

[[enums.variants]]
name = "MODE_OLD"
value = 0x1234
comment = "Old"
deprecated_release = "0.9.0"

[[enums.variants]]
name = "MODE_NONE"
value = 0xFFFF
sentinel = true
comment = "No mode"

[[enums.variants]]
name = "MODE_FAST"
value = 1
comment = "Fast"
first_release = "0.8.1"

[[structs]]
name = "header_t"
generate = "parse"
root = true
version_field = "version"
version_constant = "HEADER_VERSION"
generation_slot = "metadata"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"
"#;

/// The cast is the enum's own size, and every value is there: a sentinel
/// is a value a device can hold, and a deprecated value keeps its name for
/// good.
#[test]
fn every_value_of_a_plugin_enum_reaches_the_header() {
    let schema = Schema::parse(ENUM_FIXTURE).expect("the fixture should have been accepted");
    let header = constants_gen::generate(&schema);
    for define in [
        "#define ORA_MODE_SLOW ((uint16_t)0)\n",
        "#define ORA_MODE_OLD ((uint16_t)0x1234)\n",
        "#define ORA_MODE_NONE ((uint16_t)0xFFFF)\n",
    ] {
        assert!(header.contains(define), "no '{define}' in:\n{header}");
    }
    // The deprecation note sits above the @since line.
    let lines: Vec<&str> = header.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("#define ORA_MODE_OLD "))
        .expect("the header contains ORA_MODE_OLD");
    assert!(
        lines[at - 2].contains("0.9.0"),
        "the deprecated value doesn't say so:\n{header}"
    );
}

/// A value added after its enum reached the plugin API states the release it
/// arrived in, and the rest take the enum's.
#[test]
fn a_value_with_a_release_of_its_own_says_so() {
    let schema = Schema::parse(ENUM_FIXTURE).expect("the fixture should have been accepted");
    let header = constants_gen::generate(&schema);
    assert_eq!(
        line_above(&header, "#define ORA_MODE_FAST "),
        format!("{SINCE}0.8.1")
    );
    assert_eq!(
        line_above(&header, "#define ORA_MODE_SLOW "),
        format!("{SINCE}0.8.0")
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
