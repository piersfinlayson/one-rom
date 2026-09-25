// tests/generation_gating.rs
//
// Tests for what the generators emit for a field carrying a generation
// marker.  Each generator is called with a schema built here, since what it
// emits is the thing under test.
//
// No field of One ROM's own schema carries a marker - nothing has been added
// since v0.7.0 - so a fixture schema is the only way to reach any of this.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata_gen::schema::Schema;
use onerom_metadata_gen::{c_gen, rust_gen, serialize_gen};

// ===========================================================================
// The fixture
// ===========================================================================

/// A schema carrying all three versioned structures, with the marked fields
/// under test on `onerom_hardware_info_t` - a structure the metadata header
/// points at, which is where a field is most likely to be added.
///
/// `hw_attrs` is appended to `onerom_hardware_info_t.board_id`, `hdr_attrs` to
/// `onerom_metadata_header_t.turbo_boot`, and `mode_attrs` to
/// `onerom_hardware_info_t.mode`, the one enum-typed field.
fn fixture(hw_attrs: &str, hdr_attrs: &str, mode_attrs: &str) -> String {
    fixture_with(hw_attrs, hdr_attrs, mode_attrs, "")
}

/// [`fixture`], plus `extra_hdr` as further fields of the metadata header.
///
/// The kinds whose accessor is not a number need a field each, added here
/// rather than to every fixture.
///
/// No struct states a `size`, so the header carries no `STATIC_ASSERT` on one.
/// The compile tests below build it for a host, where a pointer is eight bytes
/// and a device's sizes do not hold.
fn fixture_with(hw_attrs: &str, hdr_attrs: &str, mode_attrs: &str, extra_hdr: &str) -> String {
    format!(
        r#"
[schema]
format_version = 1
firmware_release = "0.8.0"
name = "Test"
description = "Generation gating fixture"
flash_base = 0x10000000
metadata_base = 0x1000C000
metadata_size = 16384
root_struct = "onerom_metadata_header_t"

[[versions]]
struct_name = "onerom_info_t"
version = 2
first_release = "0.7.0"

[[versions]]
struct_name = "onerom_metadata_header_t"
version = 2
first_release = "0.7.0"

[[versions]]
struct_name = "onerom_metadata_header_t"
version = 3
first_release = "0.8.0"

[[versions]]
struct_name = "onerom_runtime_info_t"
version = 2
first_release = "0.7.0"

[[constants]]
name = "ONEROM_INFO_VERSION"
type = "u32"
value = 2

[[constants]]
name = "CURRENT_METADATA_VERSION"
type = "u32"
value = 3

[[constants]]
name = "RUNTIME_INFO_VERSION"
type = "u32"
value = 2

[[enums]]
name = "onerom_mode_t"
size = 1
strip_prefix = "MODE_"

[[enums.variants]]
name = "MODE_SLOW"
value = 0

[[enums.variants]]
name = "MODE_FAST"
value = 7

[[structs]]
name = "onerom_info_t"
generate = "parse"
version_field = "version"
version_constant = "ONEROM_INFO_VERSION"
generation_slot = "info"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"

[[structs.fields]]
name = "metadata"
kind = "struct_ptr"
type = "onerom_metadata_header_t"
nullable = true

[[structs.fields]]
name = "runtime"
kind = "struct_ptr"
type = "onerom_runtime_info_t"
nullable = true

[[structs]]
name = "onerom_metadata_header_t"
generate = "both"
root = true
version_field = "version"
version_constant = "CURRENT_METADATA_VERSION"
generation_slot = "metadata"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"

[[structs.fields]]
name = "hw"
kind = "struct_ptr"
type = "onerom_hardware_info_t"
nullable = false

[[structs.fields]]
name = "turbo_boot"
kind = "scalar"
type = "u8"
{hdr_attrs}
{extra_hdr}

[[structs]]
name = "onerom_hardware_info_t"
generate = "both"

[[structs.fields]]
name = "board_id"
kind = "scalar"
type = "u16"
{hw_attrs}

[[structs.fields]]
name = "mode"
kind = "enum"
type = "onerom_mode_t"
{mode_attrs}

[[structs]]
name = "onerom_runtime_info_t"
generate = "parse"
version_field = "version"
version_constant = "RUNTIME_INFO_VERSION"
generation_slot = "runtime"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"

[[structs.fields]]
name = "image_sel"
kind = "scalar"
type = "u8"
"#
    )
}

/// `board_id` as a field that arrived in metadata generation 3, with a default
/// that is neither 0 nor 0xFF so a test cannot pass on a zeroed or an
/// unwritten byte.
const ARRIVED_IN_3: &str = "since_metadata_version = 3\ndefault_if_absent = 4660";

/// `mode` as an enum-typed field that arrived in metadata generation 3,
/// defaulting to the variant valued 7 rather than the one valued 0.
const GATED_ENUM: &str = "since_metadata_version = 3\ndefault_if_absent = 7";

/// The parsed fixture.
fn parsed(hw_attrs: &str, hdr_attrs: &str) -> Schema {
    Schema::parse(&fixture(hw_attrs, hdr_attrs, "")).expect("fixture should be valid")
}

fn c_of(hw_attrs: &str, hdr_attrs: &str) -> String {
    c_gen::generate(&parsed(hw_attrs, hdr_attrs))
}

fn rust_of(hw_attrs: &str, hdr_attrs: &str) -> String {
    rust_gen::generate(&parsed(hw_attrs, hdr_attrs))
}

fn serialize_of(hw_attrs: &str, hdr_attrs: &str) -> String {
    serialize_gen::generate(&parsed(hw_attrs, hdr_attrs))
}

/// Assert `haystack` contains `needle`, showing the whole output when it does
/// not - a generator's output is only readable in full.
fn contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected to find:\n{needle}\n\nin:\n{haystack}"
    );
}

fn lacks(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "expected NOT to find:\n{needle}\n\nin:\n{haystack}"
    );
}

// ===========================================================================
// C: the member a gated field's bytes sit in
// ===========================================================================

/// The bytes are declared under a name of their own, so hand-written firmware
/// naming the field itself does not compile.
#[test]
fn a_gated_field_declares_its_bytes_under_another_name() {
    let c = c_of(ARRIVED_IN_3, "");
    contains(&c, "const uint16_t board_id_stored;");
    lacks(&c, "const uint16_t board_id;");
}

#[test]
fn an_ungated_field_declares_its_bytes_under_its_own_name() {
    let c = c_of("", "");
    contains(&c, "const uint16_t board_id;");
    lacks(&c, "board_id_stored");
}

// ===========================================================================
// C: the accessor
// ===========================================================================

#[test]
fn a_gated_field_of_a_pointed_at_structure_takes_the_header_and_the_object() {
    contains(
        &c_of(ARRIVED_IN_3, ""),
        "static inline uint16_t onerom_hardware_info_board_id(const onerom_metadata_header_t \
         *header, const onerom_hardware_info_t *obj) {\n    return (header->version >= 3u) ? \
         obj->board_id_stored : (uint16_t)4660;\n}",
    );
}

/// A field of the header itself has the generation in the same structure, so
/// the accessor asks for nothing more.
#[test]
fn a_gated_field_of_the_header_takes_the_header_alone() {
    contains(
        &c_of("", "since_metadata_version = 3\ndefault_if_absent = 1"),
        "static inline uint8_t onerom_metadata_header_turbo_boot(const onerom_metadata_header_t \
         *header) {\n    return (header->version >= 3u) ? header->turbo_boot_stored : \
         (uint8_t)1;\n}",
    );
}

/// An enum default is written as the state's own name, so the header says
/// which state a reader of older metadata sees rather than a number to look up.
#[test]
fn an_enum_default_names_the_variant_rather_than_its_number() {
    let c = c_gen::generate(
        &Schema::parse(&fixture("", "", GATED_ENUM)).expect("fixture should be valid"),
    );
    contains(
        &c,
        "return (header->version >= 3u) ? obj->mode_stored : MODE_FAST;",
    );
}

#[test]
fn no_accessors_are_emitted_where_nothing_is_gated() {
    lacks(&c_of("", ""), "Generation-gated field accessors");
}

// ===========================================================================
// C: reading a gated field through a compiler
// ===========================================================================

/// Compile `program` against `header`, returning gcc's combined result.
/// Returns `None` where gcc is not available.
fn compile_c(header: &str, program: &str) -> Option<std::process::Output> {
    use std::process::Command;

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR is two levels below the project root");
    let firmware_include = project_root.join("firmware").join("include");

    let dir = std::env::temp_dir().join(format!(
        "onerom_gating_{}_{:p}",
        std::process::id(),
        program.as_ptr()
    ));
    std::fs::create_dir_all(&dir).expect("failed to create the temp directory");
    std::fs::write(dir.join("onerom_metadata.h"), header)
        .expect("failed to write the generated header");
    let c_path = dir.join("main.c");
    std::fs::write(&c_path, program).expect("failed to write the test program");
    let bin_path = dir.join("main");

    let result = Command::new("gcc")
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-I",
            &dir.to_string_lossy(),
            "-I",
            &firmware_include.to_string_lossy(),
            "-D",
            "TEST_BUILD",
            "-o",
            &bin_path.to_string_lossy(),
            &c_path.to_string_lossy(),
        ])
        .output();

    match result {
        Err(e) => {
            eprintln!("skipping - gcc not available: {e}");
            let _ = std::fs::remove_dir_all(&dir);
            None
        }
        Ok(output) => {
            if output.status.success() {
                let run = Command::new(&bin_path)
                    .output()
                    .expect("the compiled program should run");
                let _ = std::fs::remove_dir_all(&dir);
                return Some(run);
            }
            let _ = std::fs::remove_dir_all(&dir);
            Some(output)
        }
    }
}

/// The value an older header yields is the declared default, and a header of
/// the field's own generation yields the stored bytes.
#[test]
fn the_accessor_reads_the_default_below_its_generation_and_the_bytes_at_it() {
    let program = r#"
#include "onerom_metadata.h"

static const onerom_hardware_info_t HARDWARE = { .board_id_stored = 0x0505, .mode = MODE_FAST };
static const onerom_metadata_header_t OLD = { .version = 2, .hw = &HARDWARE, .turbo_boot = 0 };
static const onerom_metadata_header_t NEW = { .version = 3, .hw = &HARDWARE, .turbo_boot = 0 };

int main(void) {
    if (onerom_hardware_info_board_id(&OLD, &HARDWARE) != 4660) { return 1; }
    if (onerom_hardware_info_board_id(&NEW, &HARDWARE) != 0x0505) { return 2; }
    return 0;
}
"#;
    let Some(output) = compile_c(&c_of(ARRIVED_IN_3, ""), program) else {
        return;
    };
    assert!(
        output.status.success(),
        "the accessor did not behave as declared:\nstatus {:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Why the member is renamed: naming the field itself does not compile, so
/// nothing can read the bytes and skip the generation check.
#[test]
fn reading_a_gated_field_as_a_member_does_not_compile() {
    let program = r#"
#include "onerom_metadata.h"

static const onerom_hardware_info_t HARDWARE = { .board_id_stored = 0x0505, .mode = MODE_FAST };

int main(void) {
    return (int)HARDWARE.board_id;
}
"#;
    let Some(output) = compile_c(&c_of(ARRIVED_IN_3, ""), program) else {
        return;
    };
    assert!(
        !output.status.success(),
        "reading the member directly should not have compiled"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("board_id"),
        "the compiler should have named the member: {stderr}"
    );
}

// ===========================================================================
// C: the kinds whose value is not a number
// ===========================================================================

/// One gated field of every kind an accessor hands back by pointer.
///
/// The firmware has nowhere to copy an array or a string to, so these are
/// returned as a pointer - to the member where the metadata is new enough,
/// and to a static default otherwise.
const POINTER_RETURNING_FIELDS: &str = r#"
[[structs.fields]]
name = "pins"
kind = "inline_array"
element = "u8"
count = 4
since_metadata_version = 3
default_if_absent = [1, 2, 3, 4]

[[structs.fields]]
name = "matrix"
kind = "inline_array2d"
element = "u8"
rows = 2
cols = 3
since_metadata_version = 3
default_if_absent = 9

[[structs.fields]]
name = "build_name"
kind = "cstr_ptr"
nullable = true
since_metadata_version = 3
default_if_absent = "null"

[[structs.fields]]
name = "extra"
kind = "struct_ptr"
type = "onerom_hardware_info_t"
nullable = true
since_metadata_version = 3
default_if_absent = "null"

[[structs.fields]]
name = "blob"
kind = "opaque_ptr"
pointed_type = "u8"
since_metadata_version = 3
default_if_absent = "null"

[[structs.fields]]
name = "hook"
kind = "fn_ptr"
since_metadata_version = 3
default_if_absent = "null"
"#;

/// The C generated for those fields.
fn kinds_c() -> String {
    c_gen::generate(
        &Schema::parse(&fixture_with("", "", "", POINTER_RETURNING_FIELDS))
            .expect("fixture should be valid"),
    )
}

/// An array's bytes are in the structure whatever the generation, so what is
/// absent is anything having been written there, and the accessor hands back
/// a static object holding what the schema says instead.
#[test]
fn a_gated_array_is_handed_back_by_pointer_with_a_static_default() {
    let c = kinds_c();
    contains(
        &c,
        "static const uint8_t onerom_metadata_header_pins_default[4] = \
         { 0x01, 0x02, 0x03, 0x04 };\n\
         static inline const uint8_t *onerom_metadata_header_pins(const \
         onerom_metadata_header_t *header) {\n    \
         return (header->version >= 3u) ? header->pins_stored : \
         onerom_metadata_header_pins_default;\n}",
    );
}

/// A whole number fills the array, so a default need not be written out
/// element by element.
#[test]
fn a_two_dimensional_default_fills_from_one_number_and_keeps_its_rows() {
    let c = kinds_c();
    contains(
        &c,
        "static const uint8_t onerom_metadata_header_matrix_default[2][3] = \
         { { 0x09, 0x09, 0x09 }, { 0x09, 0x09, 0x09 } };",
    );
    // A function returning a pointer to a row, which is what indexing the
    // result as `[r][c]` needs.
    contains(
        &c,
        "static inline const uint8_t (*onerom_metadata_header_matrix(const \
         onerom_metadata_header_t *header))[3] {",
    );
}

/// A pointer that is not there is null, which needs no storage at all.
#[test]
fn a_gated_pointer_hands_back_null() {
    let c = kinds_c();
    for (ctype, name, member) in [
        ("const char *", "build_name", "build_name_stored"),
        ("const onerom_hardware_info_t *", "extra", "extra_stored"),
        ("const uint8_t *", "blob", "blob_stored"),
    ] {
        contains(
            &c,
            &format!(
                "static inline {ctype}onerom_metadata_header_{name}(const \
                 onerom_metadata_header_t *header) {{\n    \
                 return (header->version >= 3u) ? header->{member} : NULL;\n}}"
            ),
        );
    }
    contains(
        &c,
        "static inline void (*onerom_metadata_header_hook(const \
         onerom_metadata_header_t *header))(void) {\n    \
         return (header->version >= 3u) ? header->hook_stored : NULL;\n}",
    );
    lacks(&c, "build_name_default");
}

/// Every one of those declarators put to a compiler, and each accessor asked
/// what it hands back on either side of its generation.
#[test]
fn the_accessors_for_those_kinds_compile_and_read_as_declared() {
    let program = r#"
#include "onerom_metadata.h"

static const uint8_t BLOB[2] = { 1, 2 };
static void hook(void) {}
static const onerom_hardware_info_t HARDWARE = { .board_id = 0x0505, .mode = MODE_FAST };

#define STORED                                       \
    .hw = &HARDWARE,                                 \
    .pins_stored = { 10, 11, 12, 13 },               \
    .matrix_stored = { { 1, 2, 3 }, { 4, 5, 6 } },   \
    .build_name_stored = "fixture",                  \
    .extra_stored = &HARDWARE,                       \
    .blob_stored = BLOB,                             \
    .hook_stored = hook

static const onerom_metadata_header_t OLD = { .version = 2, STORED };
static const onerom_metadata_header_t NEW = { .version = 3, STORED };

static int check(const onerom_metadata_header_t *h, int old) {
    const uint8_t *pins = onerom_metadata_header_pins(h);
    const uint8_t (*matrix)[3] = onerom_metadata_header_matrix(h);
    if (pins[0] != (old ? 1 : 10)) { return 1; }
    if (matrix[1][2] != (old ? 9 : 6)) { return 2; }
    if (onerom_metadata_header_build_name(h) != (old ? NULL : NEW.build_name_stored)) { return 3; }
    if (onerom_metadata_header_extra(h) != (old ? NULL : &HARDWARE)) { return 4; }
    if (onerom_metadata_header_blob(h) != (old ? NULL : BLOB)) { return 5; }
    if (onerom_metadata_header_hook(h) != (old ? NULL : hook)) { return 6; }
    return 0;
}

int main(void) {
    int r = check(&OLD, 1);
    if (r != 0) { return r; }
    return check(&NEW, 0) + 10;
}
"#;
    let Some(output) = compile_c(&kinds_c(), program) else {
        return;
    };
    assert_eq!(
        output.status.code(),
        Some(10),
        "the accessors did not behave as declared:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

// ===========================================================================
// Deprecation
// ===========================================================================

/// `mode` deprecated from metadata generation 4, having been there from the
/// first.
const DEPRECATED_IN_4: &str = "deprecated_metadata_version = 4";

#[test]
fn a_deprecated_field_carries_the_note_on_the_member_it_is_read_through() {
    let c = c_of(DEPRECATED_IN_4, "");
    contains(&c, "#define ONEROM_DEPRECATED(reason)");
    contains(
        &c,
        "const uint16_t board_id ONEROM_DEPRECATED(\"board_id is deprecated from \
         onerom_metadata_header_t generation 4\");",
    );
}

/// A gated field is read through its accessor, so that is what carries the
/// note - the member is what a writer names, and writing is what deprecation
/// leaves alone.
#[test]
fn a_deprecated_gated_field_carries_the_note_on_its_accessor() {
    let c = c_of(&format!("{ARRIVED_IN_3}\n{DEPRECATED_IN_4}"), "");
    contains(
        &c,
        "static inline ONEROM_DEPRECATED(\"board_id is deprecated from \
         onerom_metadata_header_t generation 4\") uint16_t onerom_hardware_info_board_id(",
    );
    lacks(&c, "board_id_stored ONEROM_DEPRECATED");
}

#[test]
fn nothing_deprecated_leaves_the_macro_out() {
    lacks(&c_of("", ""), "ONEROM_DEPRECATED");
}

#[test]
fn a_deprecated_field_is_still_a_field_in_rust_and_says_it_is_deprecated() {
    let rust = rust_of(DEPRECATED_IN_4, "");
    contains(
        &rust,
        "#[deprecated(note = \"deprecated from onerom_metadata_header_t generation 4\")]\n    \
         pub board_id: u16,",
    );
    // The derives and the generated parser name every field, so the note they
    // would raise is allowed at the item.  Code outside still hears it.
    contains(&rust, "#[allow(deprecated)]\npub struct OneromHardwareInfo");
    contains(&rust, "#[allow(deprecated)]\nimpl OneromHardwareInfo");
}

// ===========================================================================
// Rust: the generated parser
// ===========================================================================

#[test]
fn the_parser_reads_the_bytes_only_at_the_generation_that_brought_the_field_in() {
    let rust = rust_of(ARRIVED_IN_3, "");
    contains(
        &rust,
        "let board_id = if generations.metadata >= 3u32 {\n            \
         let board_id = view.read_u16_le(offset)?; offset += 2;\n            \
         board_id\n        } else {\n            offset += 2;\n            4660u16\n        };",
    );
}

/// The offset moves on whether or not the bytes were read, so a field after a
/// gated one still lands where the layout puts it.
#[test]
fn a_skipped_field_still_moves_the_offset_on() {
    let rust = rust_of(ARRIVED_IN_3, "");
    contains(&rust, "offset += 2;\n            4660u16");
}

#[test]
fn an_ungated_field_is_read_with_no_test_at_all() {
    let rust = rust_of("", "");
    contains(
        &rust,
        "let board_id = view.read_u16_le(offset)?; offset += 2;",
    );
    lacks(&rust, "generations.metadata >=");
}

/// A structure fills its own generation in from the bytes before it walks its
/// tree, so a field of a structure it points at is measured against a number
/// already read.
#[test]
fn a_structure_fills_in_its_own_generation_and_passes_it_down() {
    let rust = rust_of(ARRIVED_IN_3, "");
    contains(
        &rust,
        "let generations = generations.with_metadata(version);",
    );
    contains(
        &rust,
        "let hw = OneromHardwareInfo::parse(view, hw_ptr, generations)?;",
    );
}

/// `onerom_info_t` points at the other two structures, and a field of one of
/// their trees is measured against the generation that structure carried - not
/// against onerom_info_t's own.
#[test]
fn a_versioned_structure_just_parsed_updates_the_generation_it_governs() {
    let rust = rust_of("", "");
    contains(
        &rust,
        "let generations = match &metadata {\n            \
         Some(v) => generations.with_metadata(v.version),\n            \
         None => generations,\n        };",
    );
}

#[test]
fn the_generations_type_names_every_versioned_structure() {
    let rust = rust_of("", "");
    for slot in ["info", "metadata", "runtime"] {
        contains(&rust, &format!("    pub {slot}: u32,"));
        contains(&rust, &format!("pub const fn with_{slot}("));
    }
    contains(&rust, "pub const UNKNOWN: Self");
}

/// The default is emitted as a variant rather than a number, so a parser
/// cannot hand back a value its own `TryFrom` refuses.  It is wrapped, because
/// the field's type is the wrapper and not the enum.
#[test]
fn an_enum_default_parses_as_the_variant_it_names() {
    let with_enum = rust_gen::generate(
        &Schema::parse(&fixture("", "", GATED_ENUM)).expect("fixture should be valid"),
    );
    contains(
        &with_enum,
        "MaybeKnown::Known(OneromMode::ModeFast)\n        };",
    );
    lacks(&rust_of("", ""), "OneromMode::ModeFast");
}

// ===========================================================================
// The plugin getter
// ===========================================================================

/// A plugin reads a gated field through the same accessor the firmware does,
/// so old metadata gives a plugin the declared default rather than whatever
/// sits at the offset.
#[test]
fn a_plugin_key_on_a_gated_field_resolves_through_the_accessor() {
    let key = "plugin_key = { name = \"BOARD_ID\", id = 1, first_release = \"0.8.0\" }";
    contains(
        &c_of(&format!("{key}\n{ARRIVED_IN_3}"), ""),
        "*(out) = (uint32_t)(onerom_hardware_info_board_id(METADATA, METADATA->hw));",
    );
    contains(
        &c_of(key, ""),
        "*(out) = (uint32_t)(METADATA->hw->board_id);",
    );
}

/// A gated field of the header itself needs no object beyond the header.
#[test]
fn a_plugin_key_on_a_gated_header_field_names_the_header_alone() {
    contains(
        &c_of(
            "",
            "plugin_key = { name = \"TURBO_BOOT\", id = 15, first_release = \"0.8.0\" }\n\
             since_metadata_version = 3\ndefault_if_absent = 1",
        ),
        "*(out) = (uint32_t)(onerom_metadata_header_turbo_boot(METADATA));",
    );
}

// ===========================================================================
// Rust: the release each generation arrived in
// ===========================================================================

/// The `[[versions]]` entries for the metadata header reach a host tool as a
/// table, because a tool composing an image has to turn the firmware version
/// it is composing for into a generation.
#[test]
fn the_generation_table_names_each_metadata_generation_and_its_release() {
    contains(
        &rust_of("", ""),
        "pub const METADATA_GENERATIONS: &[(FirmwareVersion, u32)] = &[\n    \
         (FirmwareVersion::new(0, 7, 0, 0), 2),\n    \
         (FirmwareVersion::new(0, 8, 0, 0), 3),\n];",
    );
}

/// The other two structures are written by the firmware into the binary they
/// were compiled into, so nothing outside needs their release-to-generation
/// mapping and the table does not carry it.
#[test]
fn the_generation_table_carries_the_metadata_header_alone() {
    let rust = rust_of("", "");
    let table = rust
        .split("pub const METADATA_GENERATIONS")
        .nth(1)
        .expect("the table should be emitted");
    let table = table.split("];").next().expect("the table should end");
    // onerom_info_t and onerom_runtime_info_t are generation 2 from 0.7.0 in
    // the fixture, as the metadata header also is, so a second 0.7.0 entry is
    // what their presence would look like.
    assert_eq!(table.matches("FirmwareVersion::new(0, 7, 0, 0)").count(), 1);
}

// ===========================================================================
// Rust: the writer
// ===========================================================================

/// The bytes of a field the metadata being written predates are not written
/// at all, so they keep the 0xFF the buffer was filled with - which is what a
/// device's unwritten metadata reads back.
#[test]
fn the_writer_lays_down_a_gated_field_only_at_its_generation() {
    contains(
        &serialize_of(ARRIVED_IN_3, ""),
        "if ctx.metadata_generation() >= 3u32 {\n            \
         ctx.write_u16_le(addr, self.board_id);\n        }",
    );
}

#[test]
fn the_writer_lays_down_an_ungated_field_with_no_test_at_all() {
    let serialize = serialize_of("", "");
    contains(&serialize, "ctx.write_u16_le(addr, self.board_id);");
    lacks(&serialize, "ctx.metadata_generation() >=");
}

/// The generation the context carries is the one the caller gave `new`, so
/// there is one number rather than one per field.
#[test]
fn the_context_carries_the_generation_being_written() {
    contains(
        &serialize_of("", ""),
        "pub fn new(base_addr: u32, metadata_generation: u32, buf: &'buf mut [u8]) -> Self {",
    );
}

// ===========================================================================
// Rust: refusing what the generation cannot carry
// ===========================================================================

/// Leaving a field out is right only where it was asking for nothing.  With a
/// value in it, the build is refused and told which field and which firmware.
#[test]
fn a_gated_field_away_from_its_default_is_refused_by_name() {
    contains(
        &serialize_of(ARRIVED_IN_3, ""),
        "if generation < 3u32 && self.board_id != 4660u16 {\n            \
         return Err(SerializeError::FieldTooNew {\n                \
         field: \"onerom_hardware_info_t.board_id\",\n                \
         minimum: FirmwareVersion::new(0, 8, 0, 0),\n            \
         });\n        }",
    );
}

/// The check walks the tree, so one call on the root reaches a structure the
/// header points at.
#[test]
fn the_check_walks_into_a_structure_the_header_points_at() {
    contains(
        &serialize_of(ARRIVED_IN_3, ""),
        "self.hw.check_generation(generation)?;",
    );
}

/// A branch with nothing gated under it is not walked, so a schema with no
/// markers - which is what ships - gets a check that does nothing.
#[test]
fn nothing_gated_leaves_the_check_empty() {
    let serialize = serialize_of("", "");
    contains(
        &serialize,
        "pub fn check_generation(&self, _generation: u32) -> Result<(), SerializeError> {\n        \
         Ok(())\n    }",
    );
    lacks(&serialize, "SerializeError::FieldTooNew");
    lacks(&serialize, "self.hw.check_generation(generation)?;");
}

/// An enum default is compared as the variant it names rather than a number,
/// so the comparison cannot drift from what the parser hands back.
#[test]
fn an_enum_default_is_compared_as_the_variant_it_names() {
    let with_enum = serialize_gen::generate(
        &Schema::parse(&fixture("", "", GATED_ENUM)).expect("fixture should be valid"),
    );
    contains(
        &with_enum,
        "&& self.mode != MaybeKnown::Known(OneromMode::ModeFast) {",
    );
}
