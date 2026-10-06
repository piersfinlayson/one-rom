// tests/bitfields.rs
//
// Tests for `[[bitfields]]` validation, generator output and the comparison
// with the last release.
//
// Each refusal test breaks exactly one rule.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata_gen::layout;
use onerom_metadata_gen::released::Released;
use onerom_metadata_gen::schema::Schema;
use onerom_metadata_gen::{c_gen, constants_gen, device_gen, rust_gen, serialize_gen};

// ===========================================================================
// The fixture
// ===========================================================================

/// The fixture's `firmware_release`.
const NOW: &str = "0.9.0";

/// A schema with one bit field in runtime info, with:
/// - a one-bit member
/// - a multi-bit member
/// - an enum member that arrived after the bit field
const FIXTURE: &str = r#"
[schema]
format_version = 1
firmware_release = "0.9.0"
name = "Test"
description = "Bit field fixture"
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
value = 2

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
value = 3

[[bitfields]]
name = "onerom_state_t"
size = 2
comment = "Test states"
strip_prefix = "STATE_"
ora_api = true
first_release = "0.8.0"

[[bitfields.members]]
name = "STATE_READY"
bit = 0
comment = "Ready"

[[bitfields.members]]
name = "STATE_LEVEL"
bit = 4
width = 3
comment = "A level"

[[bitfields.members]]
name = "STATE_MODE"
bit = 8
width = 2
type = "onerom_mode_t"
first_release = "0.9.0"
comment = "The mode"

[[structs]]
name = "onerom_info_t"
generate = "parse"
version_field = "version"
version_constant = "ONEROM_INFO_VERSION"
generation_slot = "info"

[[structs.fields]]
name = "major_version"
kind = "scalar"
type = "u16"

[[structs.fields]]
name = "minor_version"
kind = "scalar"
type = "u16"

[[structs.fields]]
name = "patch_version"
kind = "scalar"
type = "u16"

[[structs.fields]]
name = "build_number"
kind = "scalar"
type = "u16"

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
generate = "parse"
root = true
version_field = "version"
version_constant = "CURRENT_METADATA_VERSION"
generation_slot = "metadata"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"

[[structs.fields]]
name = "spare"
kind = "scalar"
type = "u16"

[[structs]]
name = "onerom_runtime_info_t"
generate = "parse"
const_fields = false
version_field = "version"
version_constant = "RUNTIME_INFO_VERSION"
generation_slot = "runtime"

[[structs.fields]]
name = "version"
kind = "scalar"
type = "u32"

[[structs.fields]]
name = "states"
kind = "bitfield"
type = "onerom_state_t"
"#;

fn edited(from: &str, to: &str) -> String {
    edited_in(FIXTURE, from, to)
}

fn edited_in(toml: &str, from: &str, to: &str) -> String {
    assert!(toml.contains(from), "the fixture no longer contains {from}");
    toml.replacen(from, to, 1)
}

fn parsed(toml: &str) -> Schema {
    Schema::parse(toml).expect("the schema should have been accepted")
}

fn refused(toml: &str, expected: &str) {
    let err = Schema::parse(toml)
        .expect_err("the schema should have been refused")
        .to_string();
    assert!(err.contains(expected), "unexpected error: {err}");
}

fn contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected to find:\n{needle}\n\nin:\n{haystack}"
    );
}

fn lacks(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "expected not to find:\n{needle}\n\nin:\n{haystack}"
    );
}

const LEVEL: &str = "name = \"STATE_LEVEL\"\nbit = 4\nwidth = 3";

// ===========================================================================
// Validation
// ===========================================================================

#[test]
fn the_fixture_is_valid() {
    parsed(FIXTURE);
}

#[test]
fn a_bit_field_of_three_bytes_is_refused() {
    refused(
        &edited(
            "size = 2\ncomment = \"Test states\"",
            "size = 3\ncomment = \"Test states\"",
        ),
        "is 3 bytes",
    );
}

#[test]
fn a_bit_field_without_members_is_refused() {
    let toml = format!(
        "{FIXTURE}\n[[bitfields]]\nname = \"empty_t\"\nsize = 1\nfirst_release = \"0.9.0\"\n"
    );
    refused(&toml, "bit field empty_t doesn't list any members");
}

#[test]
fn overlapping_members_are_refused() {
    refused(
        &edited(LEVEL, "name = \"STATE_LEVEL\"\nbit = 0\nwidth = 3"),
        "onerom_state_t::STATE_LEVEL overlaps another member",
    );
}

#[test]
fn a_member_past_the_size_is_refused() {
    refused(
        &edited(LEVEL, "name = \"STATE_LEVEL\"\nbit = 14\nwidth = 3"),
        "holds bits 14 to 16",
    );
}

#[test]
fn a_member_no_bits_wide_is_refused() {
    refused(
        &edited(LEVEL, "name = \"STATE_LEVEL\"\nbit = 4\nwidth = 0"),
        "STATE_LEVEL is 0 bits wide",
    );
}

#[test]
fn a_member_declared_twice_is_refused() {
    refused(
        &edited("name = \"STATE_LEVEL\"", "name = \"STATE_READY\""),
        "onerom_state_t::STATE_READY is declared twice",
    );
}

#[test]
fn a_member_named_as_the_unknown_bits_is_refused() {
    refused(
        &edited("name = \"STATE_LEVEL\"", "name = \"STATE_UNKNOWN_BITS\""),
        "becomes the Rust field unknown_bits",
    );
}

#[test]
fn a_member_too_narrow_for_its_enum_is_refused() {
    refused(
        &edited("bit = 8\nwidth = 2", "bit = 8\nwidth = 1"),
        "onerom_mode_t::MODE_FAST is 3, which doesn't fit",
    );
}

#[test]
fn a_member_holding_no_declared_enum_is_refused() {
    refused(
        &edited(
            "type = \"onerom_mode_t\"\nfirst",
            "type = \"onerom_gone_t\"\nfirst",
        ),
        "holds a onerom_gone_t, and no enum of that name is declared",
    );
}

#[test]
fn a_bit_field_without_a_first_release_is_refused() {
    refused(
        &edited(
            "ora_api = true\nfirst_release = \"0.8.0\"\n",
            "ora_api = true\n",
        ),
        "first_release",
    );
}

#[test]
fn a_member_arriving_before_its_bit_field_is_refused() {
    refused(
        &edited(
            "first_release = \"0.9.0\"\ncomment = \"The mode\"",
            "first_release = \"0.7.0\"\ncomment = \"The mode\"",
        ),
        "onerom_state_t::STATE_MODE arrived in 0.7.0, before onerom_state_t did in 0.8.0",
    );
}

#[test]
fn a_member_retired_as_it_arrives_is_refused() {
    refused(
        &edited(LEVEL, &format!("{LEVEL}\ndeprecated_release = \"0.8.0\"")),
        "is deprecated from 0.8.0 and arrived in 0.8.0",
    );
}

/// `onerom_metadata_header_t.spare` as a bit field.
fn in_the_metadata() -> String {
    edited(
        "name = \"spare\"\nkind = \"scalar\"\ntype = \"u16\"",
        "name = \"spare\"\nkind = \"bitfield\"\ntype = \"onerom_state_t\"",
    )
}

#[test]
fn a_bit_field_in_the_metadata_is_accepted() {
    parsed(&in_the_metadata());
}

#[test]
fn a_bit_field_onerom_info_t_doesnt_reach_is_refused() {
    let toml = format!(
        "{FIXTURE}\n[[structs]]\nname = \"onerom_loose_t\"\ngenerate = \"parse\"\n\n\
         [[structs.fields]]\nname = \"states\"\nkind = \"bitfield\"\ntype = \"onerom_state_t\"\n"
    );
    refused(
        &toml,
        "onerom_loose_t.states is a bit field, and onerom_info_t doesn't reach onerom_loose_t",
    );
}

#[test]
fn a_release_recorded_after_the_metadata_pointer_is_refused() {
    let moved = edited_in(
        &in_the_metadata(),
        "[[structs.fields]]\nname = \"major_version\"\nkind = \"scalar\"\ntype = \"u16\"\n\n",
        "",
    );
    let moved = edited_in(
        &moved,
        "nullable = true\n\n[[structs.fields]]\nname = \"runtime\"",
        "nullable = true\n\n[[structs.fields]]\nname = \"major_version\"\nkind = \"scalar\"\ntype = \"u16\"\n\n[[structs.fields]]\nname = \"runtime\"",
    );
    refused(
        &moved,
        "onerom_metadata_header_t.spare is a bit field, and onerom_info_t.major_version isn't a \
         u16 scalar ahead of the pointer onerom_info_t.metadata, which reaches \
         onerom_metadata_header_t",
    );
}

#[test]
fn a_bit_field_field_naming_no_table_is_refused() {
    refused(
        &edited("type = \"onerom_state_t\"", "type = \"onerom_gone_t\""),
        "onerom_runtime_info_t.states is a onerom_gone_t, and no bit field of that name",
    );
}

#[test]
fn a_bit_field_without_a_recorded_release_is_refused() {
    refused(
        &edited("name = \"patch_version\"", "name = \"patch\""),
        "onerom_info_t.patch_version isn't a u16 scalar ahead of the pointer",
    );
}

#[test]
fn a_release_recorded_after_the_runtime_pointer_is_refused() {
    let moved = edited(
        "[[structs.fields]]\nname = \"major_version\"\nkind = \"scalar\"\ntype = \"u16\"\n\n",
        "",
    );
    let moved = edited_in(
        &moved,
        "nullable = true\n\n[[structs]]\nname = \"onerom_metadata_header_t\"",
        "nullable = true\n\n[[structs.fields]]\nname = \"major_version\"\nkind = \"scalar\"\ntype = \"u16\"\n\n[[structs]]\nname = \"onerom_metadata_header_t\"",
    );
    refused(
        &moved,
        "onerom_info_t.major_version isn't a u16 scalar ahead of the pointer",
    );
}

#[test]
fn a_default_the_bit_field_cannot_hold_is_refused() {
    let toml = edited(
        "VERSION\"\ntype = \"u32\"\nvalue = 2\n\n[[enums]]",
        "VERSION\"\ntype = \"u32\"\nvalue = 3\n\n[[enums]]",
    );
    let toml = edited_in(
        &toml,
        "version = 2\nfirst_release = \"0.7.0\"\n\n[[constants]]",
        "version = 2\nfirst_release = \"0.7.0\"\n\n[[versions]]\nstruct_name = \"onerom_runtime_info_t\"\nversion = 3\nfirst_release = \"0.9.0\"\n\n[[constants]]",
    );
    let toml = edited_in(
        &toml,
        "type = \"onerom_state_t\"\n",
        "type = \"onerom_state_t\"\nsince_runtime_version = 3\ndefault_if_absent = 0x10000\n",
    );
    refused(
        &toml,
        "has default_if_absent 65536, which a u16 cannot hold",
    );
}

#[test]
fn a_member_taking_a_constants_plugin_name_is_refused() {
    let toml = format!(
        "{FIXTURE}\n[[constants]]\nname = \"STATE_READY\"\ntype = \"u8\"\nvalue = 1\nora_api = true\nfirst_release = \"0.9.0\"\n"
    );
    refused(&toml, "ORA_STATE_READY is the plugin API name of both");
}

// ===========================================================================
// What the generators emit
// ===========================================================================

#[test]
fn the_c_header_has_a_typedef_a_mask_per_member_and_shifts() {
    let c = c_gen::generate(&parsed(FIXTURE));
    contains(&c, "typedef uint16_t onerom_state_t;\n");
    contains(&c, "// Ready\n#define STATE_READY ((uint16_t)1)\n");
    contains(
        &c,
        "#define STATE_LEVEL ((uint16_t)0x0070)\n#define STATE_LEVEL_SHIFT ((uint16_t)4)\n",
    );
    contains(
        &c,
        "#define STATE_MODE ((uint16_t)0x0300)\n#define STATE_MODE_SHIFT ((uint16_t)8)\n",
    );
    lacks(&c, "STATE_READY_SHIFT");
}

#[test]
fn the_c_header_declares_the_field_with_the_typedef_and_no_c_bit_field() {
    let c = c_gen::generate(&parsed(FIXTURE));
    contains(&c, "    onerom_state_t states;\n");
    lacks(&c, "states :");
}

#[test]
fn the_plugin_header_names_each_member_with_the_release_it_arrived_in() {
    let header = constants_gen::generate(&parsed(FIXTURE));
    contains(
        &header,
        "// Ready\n// @since firmware 0.8.0\n#define ORA_STATE_READY ((uint16_t)1)\n",
    );
    contains(
        &header,
        "// @since firmware 0.8.0\n#define ORA_STATE_LEVEL ((uint16_t)0x0070)\n\
         #define ORA_STATE_LEVEL_SHIFT ((uint16_t)4)\n",
    );
    contains(
        &header,
        "// The mode\n// @since firmware 0.9.0\n#define ORA_STATE_MODE ((uint16_t)0x0300)\n",
    );
}

#[test]
fn a_bit_field_outside_the_plugin_api_stays_out_of_its_header() {
    let header = constants_gen::generate(&parsed(&edited("ora_api = true\n", "")));
    lacks(&header, "STATE_READY");
}

#[test]
fn the_rust_struct_has_a_default_and_is_non_exhaustive() {
    contains(
        &rust_gen::generate(&parsed(FIXTURE)),
        "#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, \
         serde::Deserialize)]\n#[non_exhaustive]\npub struct OneromState {\n",
    );
}

#[test]
fn the_rust_struct_has_an_optional_field_per_member_and_the_unknown_bits() {
    let rust = rust_gen::generate(&parsed(FIXTURE));
    contains(&rust, "pub struct OneromState {\n");
    contains(&rust, "    pub ready: Option<bool>,\n");
    contains(&rust, "    pub level: Option<u8>,\n");
    contains(&rust, "    pub mode: Option<MaybeKnown<OneromMode>>,\n");
    contains(&rust, "    pub unknown_bits: u16,\n");
}

#[test]
fn a_member_is_read_only_from_firmware_that_has_it() {
    let rust = rust_gen::generate(&parsed(FIXTURE));
    contains(
        &rust,
        "pub fn from_raw(raw: u16, release: Option<FirmwareVersion>) -> Self {",
    );
    contains(
        &rust,
        "let ready = if has(FirmwareVersion::new(0, 8, 0, 0)) {\n            known |= 0x1;\n            Some((raw & 0x1) != 0)",
    );
    contains(
        &rust,
        "let level = if has(FirmwareVersion::new(0, 8, 0, 0)) {\n            known |= 0x70;\n            Some(((raw & 0x70) >> 4) as u8)",
    );
    contains(
        &rust,
        "let mode = if has(FirmwareVersion::new(0, 9, 0, 0)) {\n            known |= 0x300;",
    );
    contains(&rust, "unknown_bits: raw & !known,");
}

#[test]
fn to_raw_puts_back_each_member_and_the_unknown_bits() {
    let rust = rust_gen::generate(&parsed(FIXTURE));
    contains(
        &rust,
        "pub fn to_raw(self) -> u16 {\n        let mut raw = self.unknown_bits;\n        \
         if self.ready == Some(true) {\n            raw |= 0x1;\n        }\n        \
         if let Some(value) = self.level {\n            let value = u16::from(value);\n            \
         raw |= (value << 4) & 0x70;\n        }\n",
    );
    contains(
        &rust,
        "if let Some(value) = self.mode {\n            let value = match value { \
         MaybeKnown::Known(value) => value as u16, MaybeKnown::Unknown(value) => value as u16 };\n            \
         raw |= (value << 8) & 0x300;\n        }\n        raw\n    }",
    );
}

#[test]
fn each_member_is_checked_against_the_release_it_arrived_in() {
    let rust = rust_gen::generate(&parsed(FIXTURE));
    contains(
        &rust,
        "pub fn first_member_newer_than(self, release: FirmwareVersion) -> \
         Option<(&'static str, FirmwareVersion)> {\n        let raw = self.to_raw();\n",
    );
    contains(
        &rust,
        "let first = FirmwareVersion::new(0, 8, 0, 0);\n        \
         if (raw & 0x70) != 0 && release < first {\n            \
         return Some((\"level\", first));\n        }\n",
    );
    contains(
        &rust,
        "let first = FirmwareVersion::new(0, 9, 0, 0);\n        \
         if (raw & 0x300) != 0 && release < first {\n            \
         return Some((\"mode\", first));\n        }\n        None\n    }",
    );
}

#[test]
fn a_plugin_key_on_a_bit_field_returns_the_raw_value() {
    let toml = edited(
        "type = \"onerom_state_t\"\n",
        "type = \"onerom_state_t\"\nplugin_key = { name = \"STATES\", id = 1, first_release = \"0.9.0\" }\n",
    );
    contains(
        &c_gen::generate(&parsed(&toml)),
        "*(out) = (uint32_t)(RUNTIME->states);",
    );
}

#[test]
fn the_writer_writes_a_bit_field_as_its_raw_value() {
    let toml = edited_in(
        &in_the_metadata(),
        "generate = \"parse\"\nroot = true",
        "generate = \"both\"\nroot = true",
    );
    contains(
        &serialize_gen::generate(&parsed(&toml)),
        "ctx.write_u16_le(addr + 4u32, self.spare.to_raw());",
    );
}

#[test]
fn the_parser_reads_the_bits_against_the_release_info_records() {
    let rust = rust_gen::generate(&parsed(FIXTURE));
    contains(
        &rust,
        "    pub firmware_release: Option<FirmwareVersion>,\n",
    );
    contains(
        &rust,
        "pub const fn with_firmware_release(mut self, release: FirmwareVersion) -> Self {",
    );
    contains(
        &rust,
        "let patch_version = view.read_u16_le(offset)?; offset += 2;\n        \
         let generations = generations.with_firmware_release(FirmwareVersion::new(\n            \
         major_version, minor_version, patch_version, 0,\n        ));\n",
    );
    contains(
        &rust,
        "let states = OneromState::from_raw(states_raw, generations.firmware_release);",
    );
}

#[test]
fn a_schema_whose_info_records_no_release_has_no_release_to_pass_down() {
    let toml = edited("name = \"patch_version\"", "name = \"patch\"");
    // A bit field requires the release fields, so it is removed too.
    let toml = edited_in(
        &toml,
        "[[structs.fields]]\nname = \"states\"\nkind = \"bitfield\"\ntype = \"onerom_state_t\"\n",
        "",
    );
    let rust = rust_gen::generate(&parsed(&toml));
    lacks(&rust, "firmware_release");
}

#[test]
fn the_device_type_holds_the_masks_and_shifts() {
    let device = device_gen::generate(&parsed(FIXTURE));
    contains(&device, "pub struct onerom_state_t(pub u16);\n");
    contains(&device, "    pub const STATE_READY: u16 = 1;\n");
    contains(&device, "    pub const STATE_LEVEL: u16 = 0x0070;\n");
    contains(&device, "    pub const STATE_LEVEL_SHIFT: u32 = 4;\n");
    contains(&device, "    pub states: onerom_state_t,\n");
}

// ===========================================================================
// The comparison with the last release
// ===========================================================================

/// The fixture one release earlier, without `STATE_MODE`.
fn released() -> String {
    let toml = FIXTURE.replacen(
        "firmware_release = \"0.9.0\"",
        "firmware_release = \"0.8.0\"",
        1,
    );
    let mode = "\n[[bitfields.members]]\nname = \"STATE_MODE\"\nbit = 8\nwidth = 2\ntype = \"onerom_mode_t\"\nfirst_release = \"0.9.0\"\ncomment = \"The mode\"\n";
    assert!(
        toml.contains(mode),
        "the fixture no longer contains STATE_MODE"
    );
    toml.replacen(mode, "", 1)
}

fn against_released(current: &str) -> Result<(), String> {
    let schema = parsed(current);
    let released = Released::parse(&released()).expect("the released schema should have parsed");
    layout::compare(&schema, &released).map_err(|e| e.to_string())
}

fn compare_refused(current: &str, expected: &str) {
    let err = against_released(current).expect_err("the pair should have been refused");
    assert!(err.contains(expected), "unexpected error: {err}");
}

#[test]
fn the_released_pair_agrees() {
    assert_eq!(against_released(FIXTURE), Ok(()));
}

#[test]
fn a_moved_member_is_refused() {
    compare_refused(
        &edited(LEVEL, "name = \"STATE_LEVEL\"\nbit = 5\nwidth = 3"),
        "onerom_state_t::STATE_LEVEL was at bit 4 in the last release and is at bit 5 now",
    );
}

#[test]
fn a_resized_member_is_refused() {
    compare_refused(
        &edited(LEVEL, "name = \"STATE_LEVEL\"\nbit = 4\nwidth = 2"),
        "onerom_state_t::STATE_LEVEL was 3 bit(s) wide in the last release and is 2 now",
    );
}

#[test]
fn a_removed_member_is_refused() {
    compare_refused(
        &edited(
            "\n[[bitfields.members]]\nname = \"STATE_LEVEL\"\nbit = 4\nwidth = 3\ncomment = \"A level\"\n",
            "",
        ),
        "onerom_state_t::STATE_LEVEL was in the last release and is gone",
    );
}

#[test]
fn a_renamed_member_is_refused() {
    compare_refused(
        &edited("name = \"STATE_LEVEL\"", "name = \"STATE_DEPTH\""),
        "onerom_state_t::STATE_LEVEL is called STATE_DEPTH now",
    );
}

#[test]
fn a_removed_bit_field_is_refused() {
    let toml = edited(
        "[[structs.fields]]\nname = \"states\"\nkind = \"bitfield\"\ntype = \"onerom_state_t\"\n",
        "",
    );
    let start = toml
        .find("[[bitfields]]")
        .expect("the fixture has a bit field");
    let end = toml
        .find("[[structs]]")
        .expect("the fixture has structures");
    let toml = format!("{}{}", &toml[..start], &toml[end..]);
    compare_refused(
        &toml,
        "bit field onerom_state_t was in the last release and is gone",
    );
}

#[test]
fn a_changed_release_of_a_shipped_member_is_refused() {
    compare_refused(
        &edited(LEVEL, &format!("{LEVEL}\nfirst_release = \"0.9.0\"")),
        "onerom_state_t::STATE_LEVEL arrived in 0.8.0 according to the last release, not 0.9.0",
    );
}

#[test]
fn a_new_member_claiming_an_earlier_release_is_refused() {
    let toml = edited(
        "first_release = \"0.9.0\"\ncomment = \"The mode\"",
        "comment = \"The mode\"",
    );
    compare_refused(
        &toml,
        &format!(
            "onerom_state_t::STATE_MODE is not in the copy of the last release, so it arrives in {NOW}, and its release is 0.8.0"
        ),
    );
}
