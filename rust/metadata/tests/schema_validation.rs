// tests/schema_validation.rs
//
// Tests for the schema validation that runs before any generator sees the
// schema.  Most drive a fixture built here, and one drives this crate's own
// metadata_schema.toml, which is why the tests live with it rather than with
// the generator that carries the rules.
//
// Each test breaks exactly one rule, so a rule that stops working takes its
// own test down with it.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata_gen::schema::Schema;

// ===========================================================================
// Schema construction
// ===========================================================================

/// The fixture, with extra lines on `onerom_hardware_info_t.board_id` - an
/// ordinary field governed by the metadata header's generation.
///
/// `extra` is further TOML appended to the document, so it lands inside
/// `onerom_runtime_info_t` unless it opens a table of its own.
fn schema_toml(field_attrs: &str, extra: &str) -> String {
    schema_toml_full(field_attrs, "", extra)
}

/// The fixture, with extra lines on `onerom_rom_slot_t.slot_id` - a field of
/// the structure both the metadata header and the runtime structure reach.
fn shared_slot_toml(slot_attrs: &str) -> String {
    schema_toml_full("", slot_attrs, "")
}

/// A valid schema carrying all three versioned structures.
///
/// It mirrors the shape the ownership rule exists for: `onerom_rom_slot_t` is
/// reached by the metadata header through `rom_slots` and by the runtime
/// structure through `current_rom_slot`, which on a device is a pointer into
/// the metadata region rather than a copy.
fn schema_toml_full(field_attrs: &str, slot_attrs: &str, extra: &str) -> String {
    format!(
        r#"
[schema]
format_version = 1
firmware_release = "0.8.0"
name = "Test"
description = "Schema validation fixture"
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
nullable = false

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
name = "rom_slot_count"
kind = "scalar"
type = "u8"

[[structs.fields]]
name = "rom_slots"
kind = "struct_array_ptr"
element = "onerom_rom_slot_t"
count_field = "rom_slot_count"
nullable = false

[[structs]]
name = "onerom_hardware_info_t"
generate = "both"

[[structs.fields]]
name = "board_id"
kind = "scalar"
type = "u16"
{field_attrs}

[[structs]]
name = "onerom_rom_slot_t"
generate = "both"

[[structs.fields]]
name = "slot_id"
kind = "scalar"
type = "u8"
{slot_attrs}

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
name = "current_rom_slot"
kind = "struct_ptr"
type = "onerom_rom_slot_t"
nullable = true

[[structs.fields]]
name = "image_sel"
kind = "scalar"
type = "u8"
{extra}
"#
    )
}

/// The fixture with `attrs` on an enum-typed field of
/// `onerom_hardware_info_t`, plus the enum it names.
fn enum_field_toml(attrs: &str) -> String {
    schema_toml_full(
        &format!(
            "\n[[structs.fields]]\nname = \"mode\"\nkind = \"enum\"\n\
                  type = \"onerom_mode_t\"\n{attrs}"
        ),
        "",
        "\n[[enums]]\nname = \"onerom_mode_t\"\nsize = 1\n\n\
         [[enums.variants]]\nname = \"MODE_SLOW\"\nvalue = 0\n\n\
         [[enums.variants]]\nname = \"MODE_FAST\"\nvalue = 7",
    )
}

/// The fixture with `attrs` on an array field of `onerom_hardware_info_t`.
fn array_field_toml(attrs: &str) -> String {
    schema_toml(
        &format!(
            "\n[[structs.fields]]\nname = \"pins\"\nkind = \"inline_array\"\n\
             element = \"u8\"\ncount = 4\n{attrs}"
        ),
        "",
    )
}

/// The fixture with `attrs` on a `struct_ptr` field of the runtime structure.
fn pointer_field_toml(attrs: &str) -> String {
    schema_toml(
        "",
        &format!(
            "\n[[structs.fields]]\nname = \"later\"\nkind = \"struct_ptr\"\n\
             type = \"onerom_rom_slot_t\"\n{attrs}"
        ),
    )
}

/// The fixture with an array and the field counting it on the runtime
/// structure, each carrying its own attributes.
fn counted_array_toml(array_attrs: &str, count_attrs: &str) -> String {
    schema_toml(
        "",
        &format!(
            "\n[[structs.fields]]\nname = \"tags\"\nkind = \"struct_ptr_array_ptr\"\n\
             element = \"onerom_rom_slot_t\"\ncount_field = \"tag_count\"\n\
             nullable = true\n{array_attrs}\n\n\
             [[structs.fields]]\nname = \"tag_count\"\nkind = \"scalar\"\n\
             type = \"u8\"\n{count_attrs}"
        ),
    )
}

/// The fixture with `attrs` on a field of a tagged FAM the metadata header
/// reaches.
fn tagged_fam_toml(attrs: &str) -> String {
    schema_toml(
        "\n[[structs.fields]]\nname = \"alg\"\nkind = \"tagged_fam_ptr\"\n\
         type = \"onerom_alg_cs_config_t\"\nnullable = true",
        &format!(
            "\n[[tagged_fams]]\nname = \"onerom_alg_cs_config_t\"\ngenerate = \"both\"\n\
             discriminant_field = \"alg\"\ndiscriminant_type = \"onerom_alg_cs_t\"\n\
             param_len_field = \"param_len\"\nbase_size = 4\n\n\
             [[tagged_fams.common_fields]]\nname = \"clkdiv\"\nkind = \"scalar\"\n\
             type = \"u8\"\n{attrs}\n\n\
             [[tagged_fams.variants]]\ndiscriminant = \"ALG_CS_0\"\n\
             params_len_constant = \"ALG_CS_0_PARAMS_LEN\"\n\n\
             [[tagged_fams.variants.fields]]\nname = \"pin_a\"\nkind = \"scalar\"\n\
             type = \"u8\"\n\n\
             [[enums]]\nname = \"onerom_alg_cs_t\"\nsize = 1\n\n\
             [[enums.variants]]\nname = \"ALG_CS_0\"\nvalue = 0"
        ),
    )
}

/// The fixture with `attrs` on a field declared ahead of the metadata header's
/// own generation number.
fn ahead_of_the_metadata_version(attrs: &str) -> String {
    let anchor = "version_constant = \"CURRENT_METADATA_VERSION\"\ngeneration_slot = \"metadata\"";
    let toml = schema_toml("", "");
    assert!(toml.contains(anchor), "no metadata header in the fixture");
    toml.replace(
        anchor,
        &format!(
            "{anchor}\n\n[[structs.fields]]\nname = \"early\"\nkind = \"scalar\"\n\
             type = \"u8\"\n{attrs}"
        ),
    )
}

/// The fixture with one generation constant raised.
///
/// A test adding a `[[versions]]` entry for a further generation raises the
/// constant with it, because the two are one statement and the schema refuses
/// them apart.
fn raise(toml: &str, constant: &str, version: u32) -> String {
    let was = format!("name = \"{constant}\"\ntype = \"u32\"\nvalue = 2");
    let now = format!("name = \"{constant}\"\ntype = \"u32\"\nvalue = {version}");
    assert!(toml.contains(&was), "no constant {constant} in the fixture");
    toml.replace(&was, &now)
}

/// Parse a schema that should be accepted.
fn accepted(toml: &str) {
    if let Err(e) = Schema::parse(toml) {
        panic!("schema should have been accepted: {e}");
    }
}

/// Parse a schema that should be refused, and check the message says why.
fn refused(toml: &str, expected: &str) {
    let err = Schema::parse(toml).expect_err("schema should have been refused");
    let msg = err.to_string();
    assert!(msg.contains(expected), "unexpected error: {msg}");
}

// ===========================================================================
// The fixture, and the real schema
// ===========================================================================

#[test]
fn fixture_is_valid() {
    accepted(&schema_toml("", ""));
}

#[test]
fn shipped_schema_is_valid() {
    accepted(include_str!("../metadata_schema.toml"));
}

/// Every fixture a refusal below is built on, with nothing marked.
///
/// Each refusal has to add something to the fixture to carry the marker - an
/// enum, an array, a tagged FAM, a field ahead of a version number - and this
/// says the refusal comes from the marker rather than from that.
#[test]
fn the_marker_fixtures_are_valid_unmarked() {
    accepted(&enum_field_toml(""));
    accepted(&array_field_toml(""));
    accepted(&tagged_fam_toml(""));
    accepted(&ahead_of_the_metadata_version(""));
}

// ===========================================================================
// Unknown keys
// ===========================================================================

/// A misspelled marker must not be swallowed.  Silently ignoring one would
/// leave the field looking original, which is the failure the whole mechanism
/// exists to prevent.
#[test]
fn a_misspelled_marker_is_refused() {
    refused(
        &schema_toml("since_metdata_version = 3\ndefault_if_absent = 0", ""),
        "since_metdata_version",
    );
}

// ===========================================================================
// Generation markers
// ===========================================================================

#[test]
fn a_since_marker_naming_the_governing_structure_is_accepted() {
    accepted(&schema_toml(
        "since_metadata_version = 3\ndefault_if_absent = 0",
        "",
    ));
}

/// board_id sits under the metadata header, so the runtime structure's
/// generation says nothing about it - even though onerom_info_t points at
/// both.
#[test]
fn a_since_marker_naming_another_structure_is_refused() {
    refused(
        &schema_toml("since_runtime_version = 3\ndefault_if_absent = 0", ""),
        "is governed by onerom_metadata_header_t",
    );
}

#[test]
fn a_deprecated_marker_naming_another_structure_is_refused() {
    refused(
        &schema_toml("deprecated_info_version = 3", ""),
        "is governed by onerom_metadata_header_t",
    );
}

/// A field of a versioned structure is governed by that structure itself.
#[test]
fn a_runtime_field_takes_the_runtime_marker() {
    accepted(&schema_toml(
        "",
        "since_runtime_version = 3\ndefault_if_absent = 255",
    ));
}

// ===========================================================================
// Which structure owns a shared sub-structure
// ===========================================================================

/// The metadata header writes the slot's bytes and the runtime structure only
/// points at them, so the slot moves with the metadata header's generation.
#[test]
fn a_shared_structure_takes_the_metadata_marker() {
    accepted(&shared_slot_toml(
        "since_metadata_version = 3\ndefault_if_absent = 0",
    ));
}

#[test]
fn a_shared_structure_does_not_take_the_runtime_marker() {
    refused(
        &shared_slot_toml("since_runtime_version = 3\ndefault_if_absent = 0"),
        "onerom_rom_slot_t is governed by onerom_metadata_header_t",
    );
}

/// Precedence must not hand the runtime structure's own tree to the metadata
/// header: a structure the metadata root cannot reach stays runtime-owned.
#[test]
fn a_runtime_only_structure_takes_the_runtime_marker() {
    accepted(&schema_toml(
        "",
        "\n[[structs.fields]]\nname = \"live\"\nkind = \"struct_ptr\"\n\
         type = \"live_state_t\"\nnullable = true\n\n[[structs]]\n\
         name = \"live_state_t\"\ngenerate = \"parse\"\n\n[[structs.fields]]\n\
         name = \"served\"\nkind = \"scalar\"\ntype = \"u32\"\n\
         since_runtime_version = 3\ndefault_if_absent = 0",
    ));
}

// ===========================================================================
// One governing structure per field
// ===========================================================================

#[test]
fn two_since_markers_are_refused() {
    refused(
        &schema_toml(
            "since_info_version = 3\nsince_metadata_version = 3\ndefault_if_absent = 0",
            "",
        ),
        "names both onerom_info_t and onerom_metadata_header_t in since markers",
    );
}

#[test]
fn two_deprecated_markers_are_refused() {
    refused(
        &schema_toml(
            "deprecated_metadata_version = 3\ndeprecated_runtime_version = 3",
            "",
        ),
        "names both onerom_metadata_header_t and onerom_runtime_info_t in deprecated markers",
    );
}

#[test]
fn a_since_and_a_deprecated_marker_naming_different_structures_are_refused() {
    refused(
        &schema_toml(
            "since_metadata_version = 3\ndefault_if_absent = 0\ndeprecated_info_version = 4",
            "",
        ),
        "appeared in a generation of onerom_metadata_header_t but is deprecated from a \
         generation of onerom_info_t",
    );
}

// ===========================================================================
// Release strings
// ===========================================================================

#[test]
fn a_schema_release_that_is_not_a_version_is_refused() {
    let toml = schema_toml("", "").replace(
        "firmware_release = \"0.8.0\"",
        "firmware_release = \"next\"",
    );
    refused(&toml, "[schema] firmware_release is 'next'");
}

#[test]
fn a_versions_release_that_is_not_a_version_is_refused() {
    let toml = schema_toml(
        "",
        "\n[[versions]]\nstruct_name = \"onerom_info_t\"\nversion = 3\n\
         first_release = \"0.8\"",
    );
    refused(
        &raise(&toml, "ONEROM_INFO_VERSION", 3),
        "first_release on the [[versions]] entry for onerom_info_t version 3 is '0.8'",
    );
}

#[test]
fn a_release_written_with_a_leading_zero_is_refused() {
    let toml = schema_toml("", "").replace(
        "firmware_release = \"0.8.0\"",
        "firmware_release = \"0.08.0\"",
    );
    refused(&toml, "[schema] firmware_release is '0.08.0'");
}

#[test]
fn a_constant_release_that_is_not_a_version_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
             ora_api = true\nfirst_release = \"v0.8.0\"",
        ),
        "first_release on constant SPARE is 'v0.8.0'",
    );
}

// ===========================================================================
// Markers and their defaults
// ===========================================================================

#[test]
fn a_since_marker_without_a_default_is_refused() {
    refused(
        &schema_toml("since_metadata_version = 3", ""),
        "no default_if_absent",
    );
}

#[test]
fn a_default_without_a_since_marker_is_refused() {
    refused(
        &schema_toml("default_if_absent = 0", ""),
        "no generation it appeared in",
    );
}

/// The default is emitted as a literal into C and into Rust, so one the field
/// cannot hold would surface as a compile error inside generated code.
#[test]
fn a_default_too_large_for_the_field_is_refused() {
    refused(
        &schema_toml("since_metadata_version = 3\ndefault_if_absent = 65536", ""),
        "default_if_absent 65536, which a u16 cannot hold",
    );
}

#[test]
fn a_negative_default_is_refused() {
    refused(
        &schema_toml("since_metadata_version = 3\ndefault_if_absent = -1", ""),
        "default_if_absent -1, which a u16 cannot hold",
    );
}

#[test]
fn a_default_that_is_not_a_number_is_refused() {
    refused(
        &schema_toml(
            "since_metadata_version = 3\ndefault_if_absent = \"none\"",
            "",
        ),
        "and a default is a whole number",
    );
}

/// An enum's default names a state the enum has.  Any other number would leave
/// the parser handing back a value its own `TryFrom` refuses.
#[test]
fn an_enum_default_that_is_not_a_variant_is_refused() {
    refused(
        &enum_field_toml("since_metadata_version = 3\ndefault_if_absent = 3"),
        "default_if_absent 3, which is not a variant of onerom_mode_t",
    );
}

#[test]
fn an_enum_default_naming_a_variant_is_accepted() {
    accepted(&enum_field_toml(
        "since_metadata_version = 3\ndefault_if_absent = 7",
    ));
}

// ===========================================================================
// Where a marker may sit
// ===========================================================================

/// An array's bytes are in the structure whatever the generation, so what a
/// default says is what a reader sees in place of bytes nobody wrote.  Either
/// form of it is accepted.
#[test]
fn a_marker_on_an_array_is_accepted() {
    accepted(&array_field_toml(
        "since_metadata_version = 3\ndefault_if_absent = 255",
    ));
    accepted(&array_field_toml(
        "since_metadata_version = 3\ndefault_if_absent = [1, 2, 3, 4]",
    ));
}

/// Only a u8 array, which is the only element type anything writes down.
#[test]
fn a_marker_on_an_array_of_anything_else_is_refused() {
    refused(
        &schema_toml(
            "\n[[structs.fields]]\nname = \"words\"\nkind = \"inline_array\"\n\
             element = \"u16\"\ncount = 2\n\
             since_metadata_version = 3\ndefault_if_absent = 0",
            "",
        ),
        "a generation marker on an array of u16",
    );
}

/// A pointer that is not there is null, and its type has to be able to say so.
#[test]
fn a_marker_on_a_nullable_pointer_is_accepted() {
    accepted(&pointer_field_toml(
        "nullable = true\nsince_runtime_version = 3\ndefault_if_absent = \"null\"",
    ));
}

#[test]
fn a_marker_on_a_pointer_that_is_not_nullable_is_refused() {
    refused(
        &pointer_field_toml(
            "nullable = false\nsince_runtime_version = 3\ndefault_if_absent = \"null\"",
        ),
        "a generation marker on a struct_ptr that is not nullable",
    );
}

/// Nobody reads a padding field, so there is nothing to hand one back.
#[test]
fn a_marker_on_padding_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[structs.fields]]\nname = \"spare\"\nkind = \"padding\"\nsize = 2\n\
             since_runtime_version = 3\ndefault_if_absent = 0",
        ),
        "a generation marker on a padding field, and nothing reads a padding field",
    );
}

/// A FAM says how long it is in its own bytes, so a reader already knows what
/// is there without consulting a generation.  This is about a field inside
/// one - the pointer to it, held in an ordinary structure, is accepted above.
#[test]
fn a_marker_on_a_flexible_array_member_is_refused() {
    refused(
        &tagged_fam_toml("since_metadata_version = 3\ndefault_if_absent = 0"),
        "states its own length in its bytes rather than taking one from a generation",
    );
}

// ===========================================================================
// What a default says, per kind
// ===========================================================================

#[test]
fn a_pointer_default_that_is_not_null_is_refused() {
    refused(
        &pointer_field_toml("nullable = true\nsince_runtime_version = 3\ndefault_if_absent = 0"),
        "a struct_ptr that is not there is \"null\"",
    );
}

#[test]
fn a_listed_array_default_of_the_wrong_length_is_refused() {
    refused(
        &array_field_toml("since_metadata_version = 3\ndefault_if_absent = [1, 2]"),
        "default_if_absent of 2 elements, and the field holds 4",
    );
}

/// A 2-D default states a row per entry, so rows of uneven length are refused
/// even where they flatten to the right total.
#[test]
fn a_two_dimensional_default_of_the_wrong_shape_is_refused() {
    refused(
        &schema_toml(
            "\n[[structs.fields]]\nname = \"matrix\"\nkind = \"inline_array2d\"\n\
             element = \"u8\"\nrows = 2\ncols = 3\n\
             since_metadata_version = 3\ndefault_if_absent = [[1, 2, 3, 4], [5, 6]]",
            "",
        ),
        "default_if_absent that is not the field's shape, which is 2 rows of 3",
    );
}

#[test]
fn an_array_default_element_too_large_is_refused() {
    refused(
        &array_field_toml("since_metadata_version = 3\ndefault_if_absent = 256"),
        "has 256 in its default_if_absent, which a u8 cannot hold",
    );
}

// ===========================================================================
// A gated array and the field that counts it
// ===========================================================================

#[test]
fn a_gated_array_and_its_count_at_the_same_generation_are_accepted() {
    accepted(&counted_array_toml(
        "since_runtime_version = 3\ndefault_if_absent = \"null\"",
        "since_runtime_version = 3\ndefault_if_absent = 0",
    ));
}

/// An ungated array with a gated count would take its length from a default
/// while the array it measures is fully written.
#[test]
fn a_gated_count_whose_array_is_not_gated_is_refused() {
    refused(
        &counted_array_toml("", "since_runtime_version = 3\ndefault_if_absent = 0"),
        "counts tags and is gated on generation 3, which tags is not",
    );
}

#[test]
fn a_gated_count_at_another_generation_than_its_array_is_refused() {
    refused(
        &counted_array_toml(
            "since_runtime_version = 4\ndefault_if_absent = \"null\"",
            "since_runtime_version = 3\ndefault_if_absent = 0",
        ),
        "counts tags and is gated on generation 3, which tags is not",
    );
}

/// Below the generation the array is not there, so the count that describes
/// it is zero.
#[test]
fn a_gated_count_defaulting_to_anything_but_zero_is_refused() {
    refused(
        &counted_array_toml(
            "since_runtime_version = 3\ndefault_if_absent = \"null\"",
            "since_runtime_version = 3\ndefault_if_absent = 2",
        ),
        "counts tags, which is not there below generation 3, so its default_if_absent is 0",
    );
}

/// An ungated count beside a gated array is fine: the array reads as no
/// elements and nothing consults the count to say so.
#[test]
fn a_gated_array_with_an_ungated_count_is_accepted() {
    accepted(&counted_array_toml(
        "since_runtime_version = 3\ndefault_if_absent = \"null\"",
        "",
    ));
}

// ===========================================================================
// Where the generation is read from
// ===========================================================================

/// The parser reads a structure's generation out of its `version_field`, so a
/// field of that same structure gated on it has to come after it.
#[test]
fn a_marker_ahead_of_the_version_field_is_refused() {
    refused(
        &ahead_of_the_metadata_version("since_metadata_version = 3\ndefault_if_absent = 0"),
        "declared at or before onerom_metadata_header_t.version",
    );
}

#[test]
fn a_marker_after_the_version_field_is_accepted() {
    accepted(&schema_toml(
        "",
        "\n[[structs.fields]]\nname = \"later\"\nkind = \"scalar\"\ntype = \"u8\"\n\
         since_runtime_version = 3\ndefault_if_absent = 0",
    ));
}

/// Deprecation says nothing about whether the bytes are there, so it needs no
/// generation at parse time and sits wherever the field does.
#[test]
fn a_deprecation_ahead_of_the_version_field_is_accepted() {
    accepted(&ahead_of_the_metadata_version(
        "deprecated_metadata_version = 3",
    ));
}

// ===========================================================================
// Deprecation
// ===========================================================================

#[test]
fn deprecation_after_introduction_is_accepted() {
    accepted(&schema_toml(
        "since_metadata_version = 3\ndefault_if_absent = 0\ndeprecated_metadata_version = 4",
        "",
    ));
}

#[test]
fn deprecation_before_introduction_is_refused() {
    refused(
        &schema_toml(
            "since_metadata_version = 3\ndefault_if_absent = 0\ndeprecated_metadata_version = 2",
            "",
        ),
        "only appeared in generation 3",
    );
}

// ===========================================================================
// The [[versions]] table
// ===========================================================================

#[test]
fn a_versions_entry_for_an_unversioned_structure_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[versions]]\nstruct_name = \"onerom_hardware_info_t\"\nversion = 2\n\
             first_release = \"0.7.0\"",
        ),
        "not a versioned structure",
    );
}

#[test]
fn a_second_entry_for_one_generation_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[versions]]\nstruct_name = \"onerom_info_t\"\nversion = 2\n\
             first_release = \"0.7.1\"",
        ),
        "more than one entry for onerom_info_t version 2",
    );
}

#[test]
fn a_further_generation_of_one_structure_is_accepted() {
    let toml = schema_toml(
        "",
        "\n[[versions]]\nstruct_name = \"onerom_info_t\"\nversion = 3\n\
         first_release = \"0.8.0\"",
    );
    accepted(&raise(&toml, "ONEROM_INFO_VERSION", 3));
}

// ===========================================================================
// The constant a structure's generation is stated in
// ===========================================================================

#[test]
fn a_version_constant_without_a_version_field_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[structs]]\nname = \"spare_t\"\ngenerate = \"none\"\n\
             version_constant = \"ONEROM_INFO_VERSION\"\n\n[[structs.fields]]\n\
             name = \"thing\"\nkind = \"scalar\"\ntype = \"u8\"",
        ),
        "declares no version_field for a device to carry the number in",
    );
}

#[test]
fn a_version_field_without_a_version_constant_is_refused() {
    let toml = schema_toml("", "").replace("version_constant = \"RUNTIME_INFO_VERSION\"\n", "");
    refused(&toml, "not the constant holding that generation's value");
}

#[test]
fn a_version_constant_naming_no_constant_is_refused() {
    let toml = schema_toml("", "").replace(
        "version_constant = \"RUNTIME_INFO_VERSION\"",
        "version_constant = \"RUNTIME_GENERATION\"",
    );
    refused(&toml, "no such constant is declared");
}

#[test]
fn a_version_constant_holding_text_is_refused() {
    let toml = schema_toml("", "").replace(
        "name = \"RUNTIME_INFO_VERSION\"\ntype = \"u32\"\nvalue = 2",
        "name = \"RUNTIME_INFO_VERSION\"\ntype = \"cstr\"\nvalue = \"two\"",
    );
    refused(&toml, "its value is text");
}

/// The constant and the newest `[[versions]]` entry state one generation, so
/// raising either alone leaves a device reporting a generation no release
/// claims.
#[test]
fn a_version_constant_ahead_of_the_versions_table_is_refused() {
    refused(
        &raise(&schema_toml("", ""), "RUNTIME_INFO_VERSION", 3),
        "RUNTIME_INFO_VERSION is 3 but the newest [[versions]] entry for \
         onerom_runtime_info_t is generation 2",
    );
}

#[test]
fn a_structure_with_no_versions_entry_at_all_is_refused() {
    let toml = schema_toml("", "").replace(
        "[[versions]]\nstruct_name = \"onerom_runtime_info_t\"\nversion = 2\n\
         first_release = \"0.7.0\"\n",
        "",
    );
    refused(&toml, "no [[versions]] entry saying which release");
}

// ===========================================================================
// Which structure is the root
// ===========================================================================

#[test]
fn no_structure_carrying_root_is_refused() {
    let toml = schema_toml("", "").replace("root = true\n", "");
    refused(&toml, "0 structures carry root = true");
}

#[test]
fn a_second_structure_carrying_root_is_refused() {
    let toml = schema_toml("", "").replace(
        "name = \"onerom_hardware_info_t\"\ngenerate = \"both\"",
        "name = \"onerom_hardware_info_t\"\ngenerate = \"both\"\nroot = true",
    );
    refused(&toml, "2 structures carry root = true");
}

#[test]
fn a_root_struct_naming_another_structure_is_refused() {
    let toml = schema_toml("", "").replace(
        "root_struct = \"onerom_metadata_header_t\"",
        "root_struct = \"onerom_hardware_info_t\"",
    );
    refused(
        &toml,
        "carries root = true but [schema] root_struct names onerom_hardware_info_t",
    );
}

/// Renaming the root everywhere in the file still leaves the generation
/// machinery written in terms of the old name, which is the copy nothing else
/// compares.
#[test]
fn a_root_named_anything_is_accepted_where_it_fills_the_metadata_slot() {
    let toml = schema_toml("", "").replace("onerom_metadata_header_t", "onerom_meta_root_t");
    Schema::parse(&toml).expect("a schema may name its own structures");
}

/// What a Lab schema looks like: its anchor structure belongs to another
/// crate, so it declares the metadata and runtime slots and leaves info empty.
#[test]
fn a_schema_leaving_the_info_slot_empty_is_accepted() {
    let toml = schema_toml("", "")
        .replace(
            "version_field = \"version\"\nversion_constant = \"ONEROM_INFO_VERSION\"\ngeneration_slot = \"info\"\n",
            "",
        )
        .replace(
            "[[versions]]\nstruct_name = \"onerom_info_t\"\nversion = 2\nfirst_release = \"0.7.0\"\n",
            "",
        );
    let schema = Schema::parse(&toml).expect("a schema need not fill every slot");
    assert_eq!(schema.versioned_structs().len(), 2);
}

#[test]
fn a_root_declaring_no_slot_is_refused() {
    let toml = schema_toml("", "").replace("generation_slot = \"metadata\"\n", "");
    refused(&toml, "declares no generation_slot");
}

#[test]
fn a_root_filling_another_slot_is_refused() {
    let toml = schema_toml("", "")
        .replace("generation_slot = \"info\"", "generation_slot = \"unused\"")
        .replace(
            "generation_slot = \"metadata\"",
            "generation_slot = \"info\"",
        );
    refused(&toml, "which fills the info slot");
}

// ===========================================================================
// Where a structure keeps its generation number
// ===========================================================================

#[test]
fn a_version_field_naming_no_field_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[structs]]\nname = \"spare_t\"\ngenerate = \"none\"\n\
             version_field = \"absent\"\n\n[[structs.fields]]\nname = \"thing\"\n\
             kind = \"scalar\"\ntype = \"u8\"",
        ),
        "has no field of that name",
    );
}

#[test]
fn a_version_field_that_is_not_a_scalar_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[structs]]\nname = \"spare_t\"\ngenerate = \"none\"\n\
             version_field = \"thing\"\n\n[[structs.fields]]\nname = \"thing\"\n\
             kind = \"inline_array\"\nelement = \"u8\"\ncount = 4",
        ),
        "not a scalar",
    );
}

#[test]
fn a_versioned_structure_without_a_version_field_is_refused() {
    let toml = schema_toml("", "").replace("version_field = \"version\"\n", "");
    refused(&toml, "declares no version_field");
}

// ===========================================================================
// Plugin-facing constants
// ===========================================================================

#[test]
fn a_plugin_constant_with_a_first_release_is_accepted() {
    accepted(&schema_toml(
        "",
        "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
         ora_api = true\nfirst_release = \"0.8.0\"",
    ));
}

#[test]
fn a_plugin_constant_without_a_first_release_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\nora_api = true",
        ),
        "declares no first_release",
    );
}

/// first_release says when a constant became visible to a plugin, so it says
/// nothing at all about one no plugin can see.
#[test]
fn a_first_release_on_a_firmware_only_constant_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
             first_release = \"0.8.0\"",
        ),
        "is not in the plugin API",
    );
}

// ===========================================================================
// Plugin-facing metadata keys
// ===========================================================================

/// The fixture with a plugin key on `onerom_hardware_info_t.board_id`.
fn keyed(attrs: &str) -> String {
    schema_toml(&format!("plugin_key = {{ {attrs} }}"), "")
}

#[test]
fn a_plugin_key_with_a_first_release_is_accepted() {
    accepted(&keyed(
        "name = \"BOARD_ID\", id = 1, first_release = \"0.8.0\"",
    ));
}

/// Tagging a field is what puts it in front of a plugin author, so there is no
/// such thing as a key with no release to name.
#[test]
fn a_plugin_key_without_a_first_release_is_refused() {
    refused(&keyed("name = \"BOARD_ID\", id = 1"), "first_release");
}

#[test]
fn a_plugin_key_release_that_is_not_a_version_is_refused() {
    refused(
        &keyed("name = \"BOARD_ID\", id = 1, first_release = \"v0.8.0\""),
        "first_release on plugin key BOARD_ID is 'v0.8.0'",
    );
}

// ===========================================================================
// Retiring a constant
// ===========================================================================

#[test]
fn a_deprecated_plugin_constant_is_accepted() {
    accepted(&schema_toml(
        "",
        "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
         ora_api = true\nfirst_release = \"0.7.2\"\ndeprecated_release = \"0.8.0\"",
    ));
}

/// A constant no plugin can see is retired the same way.  Nothing outside the
/// plugin API rebuilds because this file changed either.
#[test]
fn a_deprecated_firmware_only_constant_is_accepted() {
    accepted(&schema_toml(
        "",
        "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
         deprecated_release = \"0.8.0\"",
    ));
}

/// Every generator writes a constant's documentation rather than its raw
/// comment, so this is the one place the note has to appear for a reader of
/// the C header, the plugin header and the Rust `pub const` to see it.
#[test]
fn a_deprecated_constant_says_so_in_its_documentation() {
    let toml = schema_toml(
        "",
        "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
         comment = \"A spare value.\"\ndeprecated_release = \"0.8.0\"",
    );
    let schema = Schema::parse(&toml).expect("schema should have been accepted");
    let spare = schema
        .constants
        .iter()
        .find(|c| c.name == "SPARE")
        .expect("the fixture should carry SPARE");
    let documentation = spare.documentation().expect("SPARE has a comment");
    assert!(documentation.contains("A spare value."), "{documentation}");
    assert!(
        documentation.contains("Deprecated from firmware 0.8.0"),
        "{documentation}"
    );
}

#[test]
fn a_deprecated_release_that_is_not_a_version_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
             deprecated_release = \"soon\"",
        ),
        "deprecated_release on constant SPARE is 'soon'",
    );
}

/// The two releases are the ends of one constant's life, so naming them the
/// other way round describes no firmware at all.
#[test]
fn a_deprecation_before_the_first_release_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
             ora_api = true\nfirst_release = \"0.8.0\"\ndeprecated_release = \"0.7.2\"",
        ),
        "constant SPARE is deprecated from 0.7.2 and reached the plugin API in 0.8.0",
    );
}

#[test]
fn a_deprecation_in_the_release_it_arrived_in_is_refused() {
    refused(
        &schema_toml(
            "",
            "\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
             ora_api = true\nfirst_release = \"0.8.0\"\ndeprecated_release = \"0.8.0\"",
        ),
        "constant SPARE is deprecated from 0.8.0 and reached the plugin API in 0.8.0",
    );
}

// ===========================================================================
// A family member's header name
// ===========================================================================

/// `toml` with `from` replaced by `to`, where `from` is there.
fn replaced(toml: &str, from: &str, to: &str) -> String {
    assert!(toml.contains(from), "no '{from}' in the fixture");
    toml.replace(from, to)
}

/// The fixture as a family member's schema: no info structure, and a
/// header_name for the family's.
fn member_toml() -> String {
    let toml = schema_toml("", "");
    let toml = replaced(
        &toml,
        "[[versions]]\nstruct_name = \"onerom_info_t\"\nversion = 2\nfirst_release = \"0.7.0\"\n",
        "",
    );
    let info = toml.find("[[structs]]\nname = \"onerom_info_t\"").unwrap();
    let header = toml
        .find("[[structs]]\nname = \"onerom_metadata_header_t\"")
        .unwrap();
    let toml = format!("{}{}", &toml[..info], &toml[header..]);
    replaced(
        &toml,
        "root_struct = \"onerom_metadata_header_t\"\n",
        "root_struct = \"onerom_metadata_header_t\"\nheader_name = \"test_info_t\"\n",
    )
}

#[test]
fn a_header_name_is_accepted() {
    accepted(&member_toml());
}

#[test]
fn a_header_name_beside_an_info_structure_is_refused() {
    let toml = replaced(
        &schema_toml("", ""),
        "root_struct = \"onerom_metadata_header_t\"\n",
        "root_struct = \"onerom_metadata_header_t\"\nheader_name = \"test_info_t\"\n",
    );
    refused(&toml, "onerom_info_t fills the info slot");
}

#[test]
fn a_header_name_without_a_runtime_structure_is_refused() {
    let toml = replaced(
        &member_toml(),
        "[[versions]]\nstruct_name = \"onerom_runtime_info_t\"\nversion = 2\nfirst_release = \"0.7.0\"\n",
        "",
    );
    let toml = replaced(
        &toml,
        "version_field = \"version\"\nversion_constant = \"RUNTIME_INFO_VERSION\"\n\
         generation_slot = \"runtime\"\n",
        "",
    );
    refused(&toml, "no structure fills the runtime slot");
}
