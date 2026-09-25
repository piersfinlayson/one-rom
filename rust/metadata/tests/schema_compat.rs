// tests/schema_compat.rs
//
// Tests for the comparison between this crate's schema and the one the last
// release shipped.  Most drive a fixture pair built here, and the rest drive
// the shipped files, which is why the tests live with them rather than with
// the generator that carries the rules.
//
// Each test breaks exactly one rule, so a rule that stops working takes its
// own test down with it.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata_gen::layout;
use onerom_metadata_gen::released::Released;
use onerom_metadata_gen::schema::Schema;

// ===========================================================================
// The fixture pair
// ===========================================================================

/// `[schema]` as a file of this generation writes it.
const HEAD: &str = r#"
[schema]
format_version = 1
firmware_release = "0.8.0"
name = "Test"
description = "Layout comparison fixture"
flash_base = 0x10000000
metadata_base = 0x1000C000
metadata_size = 16384
root_struct = "onerom_metadata_header_t"
"#;

/// `[schema]` as a pre-0.8.0 file wrote it: the key naming the file's own
/// layout was called `version`, and nothing named the release.
const OLD_HEAD: &str = r#"
[schema]
version = 1
name = "Test"
description = "Layout comparison fixture"
flash_base = 0x10000000
metadata_base = 0x1000C000
metadata_size = 16384
root_struct = "onerom_metadata_header_t"
"#;

/// The generations, and the release each shipped in.  A pre-0.8.0 file has no
/// table like this, which is why the constants below are what a generation is
/// read from on both sides.
const VERSIONS: &str = r#"
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
"#;

/// Everything both files describe: the same structures, constants, enum and
/// tagged FAM, laid out the same way.
const BODY: &str = r#"
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

[[constants]]
name = "MAX_PINS"
type = "u8"
value = 4

[[constants]]
name = "ALG_CS_0_PARAMS_LEN"
type = "usize"
value = 2

[[type_aliases]]
name = "pin_map_t"
underlying = "u16"

[[enums]]
name = "onerom_alg_cs_t"
size = 1

[[enums.variants]]
name = "ALG_CS_0"
value = 0

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

[[structs.fields]]
name = "alg"
kind = "tagged_fam_ptr"
type = "onerom_alg_cs_config_t"
nullable = true

[[structs]]
name = "onerom_hardware_info_t"
generate = "both"

[[structs.fields]]
name = "board_id"
kind = "scalar"
type = "u16"

[[structs.fields]]
name = "pins"
kind = "inline_array"
element = "u8"
count = 4
count_ref = "MAX_PINS"

[[structs.fields]]
name = "reserved"
kind = "padding"
size = 6

[[structs]]
name = "onerom_rom_slot_t"
generate = "both"

[[structs.fields]]
name = "slot_id"
kind = "scalar"
type = "u8"

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

[[tagged_fams]]
name = "onerom_alg_cs_config_t"
generate = "both"
discriminant_field = "alg"
discriminant_type = "onerom_alg_cs_t"
param_len_field = "param_len"
base_size = 4

[[tagged_fams.common_fields]]
name = "clkdiv"
kind = "type_alias"
type = "pin_map_t"

[[tagged_fams.variants]]
discriminant = "ALG_CS_0"
params_len_constant = "ALG_CS_0_PARAMS_LEN"

[[tagged_fams.variants.fields]]
name = "pin_a"
kind = "scalar"
type = "u8"

[[tagged_fams.variants.fields]]
name = "pin_b"
kind = "scalar"
type = "u8"
"#;

/// The working schema, describing the same layout as [`released`].
fn current() -> String {
    format!("{HEAD}{VERSIONS}{BODY}")
}

/// The last released schema, written the way a pre-0.8.0 file was.
///
/// `version_constant` and `generation_slot` both arrived after it, so neither
/// is in it, and a structure's generation is readable only from the constant
/// it names.
fn released() -> String {
    let body: String = BODY
        .lines()
        .filter(|line| {
            !line.starts_with("version_constant = ") && !line.starts_with("generation_slot = ")
        })
        .map(|line| format!("{line}\n"))
        .collect();
    format!("{OLD_HEAD}{body}")
}

/// Compare a working schema with the released fixture.
fn against_released(current: &str) -> Result<(), String> {
    let schema = Schema::parse(current).expect("the working schema should have been accepted");
    let released = Released::parse(&released()).expect("the released schema should have parsed");
    layout::compare(&schema, &released).map_err(|e| e.to_string())
}

/// A working schema that should be accepted against the released fixture.
fn accepted(current: &str) {
    if let Err(e) = against_released(current) {
        panic!("the pair should have been accepted: {e}");
    }
}

/// A working schema that should be refused, and the message should say why.
fn refused(current: &str, expected: &str) {
    let err = against_released(current).expect_err("the pair should have been refused");
    assert!(err.contains(expected), "unexpected error: {err}");
}

/// Replace one run of text in the fixture, failing loudly if it has moved.
fn edited(from: &str, to: &str) -> String {
    edited_in(&current(), from, to)
}

/// Replace one run of text in a schema built from the fixture.
fn edited_in(toml: &str, from: &str, to: &str) -> String {
    assert!(toml.contains(from), "the fixture no longer contains {from}");
    toml.replace(from, to)
}

// ===========================================================================
// The pairs that agree
// ===========================================================================

#[test]
fn the_fixture_pair_agrees() {
    accepted(&current());
}

/// The v0.7.2 copy is a genuinely older schema - it says `version` where the
/// reader now expects `format_version`, names no release and has no
/// [[versions]] table - and the working schema still has to read against it.
#[test]
fn the_shipped_schema_agrees_with_the_last_release() {
    let schema = Schema::parse(include_str!("../metadata_schema.toml"))
        .expect("the shipped schema should have been accepted");
    let released = Released::parse(include_str!("../metadata_schema_released.toml"))
        .expect("the released schema should have parsed");
    if let Err(e) = layout::compare(&schema, &released) {
        panic!("the shipped schema should agree with the last release: {e}");
    }
}

// ===========================================================================
// The real pair
// ===========================================================================
//
// Most tests here drive the fixture, whose two sides describe one layout.
// These two drive the shipped schema against the copy of v0.7.2, so the rules
// are exercised over the shapes One ROM really has - a hundred-odd structures,
// a shared sub-structure, tagged FAMs and their variants.

/// Compare a perturbed working schema with the real v0.7.2 copy.
fn shipped_against_release(edit: impl Fn(&str) -> String) -> String {
    let toml = edit(include_str!("../metadata_schema.toml"));
    let schema = Schema::parse(&toml).expect("the working schema should have been accepted");
    let released = Released::parse(include_str!("../metadata_schema_released.toml"))
        .expect("the released schema should have parsed");
    layout::compare(&schema, &released)
        .expect_err("the pair should have been refused")
        .to_string()
}

#[test]
fn a_changed_constant_in_the_shipped_schema_is_refused() {
    let err = shipped_against_release(|toml| {
        edited_in(
            toml,
            "name = \"BUILD_DATE_BUF_LEN\"\ntype = \"usize\"\nvalue = 128",
            "name = \"BUILD_DATE_BUF_LEN\"\ntype = \"usize\"\nvalue = 256",
        )
    });
    assert!(
        err.contains("constant BUILD_DATE_BUF_LEN was 128 in the last release and is 256 now"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_moved_field_in_the_shipped_schema_is_refused() {
    let err = shipped_against_release(|toml| {
        edited_in(
            toml,
            "[[structs.fields]]\nname = \"hw_rev\"\nkind = \"cstr_ptr\"",
            "[[structs.fields]]\nname = \"lead_pad\"\nkind = \"padding\"\nsize = 4\n\n\
             [[structs.fields]]\nname = \"hw_rev\"\nkind = \"cstr_ptr\"",
        )
    });
    assert!(
        err.contains(
            "onerom_hardware_info_t.hw_rev was at offset 0 in the last release and is at 4 now"
        ),
        "unexpected error: {err}"
    );
}

// ===========================================================================
// A shipped field stays where it is
// ===========================================================================

#[test]
fn a_moved_field_is_refused() {
    refused(
        &edited(
            "[[structs.fields]]\nname = \"board_id\"",
            "[[structs.fields]]\nname = \"lead_pad\"\nkind = \"padding\"\nsize = 2\n\n\
             [[structs.fields]]\nname = \"board_id\"",
        ),
        "onerom_hardware_info_t.board_id was at offset 0 in the last release and is at 2 now",
    );
}

#[test]
fn a_resized_field_is_refused() {
    refused(
        &edited(
            "name = \"board_id\"\nkind = \"scalar\"\ntype = \"u16\"",
            "name = \"board_id\"\nkind = \"scalar\"\ntype = \"u32\"",
        ),
        "onerom_hardware_info_t.board_id was 2 byte(s) in the last release and is 4 now",
    );
}

/// The same bytes read a different way is as wrong as different bytes, and
/// nothing about the size says so.
#[test]
fn a_retyped_field_is_refused() {
    refused(
        &edited(
            "name = \"slot_id\"\nkind = \"scalar\"\ntype = \"u8\"",
            "name = \"slot_id\"\nkind = \"enum\"\ntype = \"onerom_alg_cs_t\"",
        ),
        "onerom_rom_slot_t.slot_id was scalar u8 in the last release and is enum \
         onerom_alg_cs_t now",
    );
}

#[test]
fn a_removed_field_is_refused() {
    refused(
        &edited(
            "\n[[structs.fields]]\nname = \"image_sel\"\nkind = \"scalar\"\ntype = \"u8\"\n",
            "\n",
        ),
        "onerom_runtime_info_t.image_sel was in the last release and is gone",
    );
}

#[test]
fn a_removed_structure_is_refused() {
    refused(
        &edited(
            "[[structs]]\nname = \"onerom_rom_slot_t\"\ngenerate = \"both\"\n\n\
             [[structs.fields]]\nname = \"slot_id\"\nkind = \"scalar\"\ntype = \"u8\"\n",
            "",
        ),
        "onerom_rom_slot_t was in the last release and is no longer described",
    );
}

/// A tagged FAM variant's parameter struct has a layout of its own, and an
/// older reader misparses a field moving inside one exactly as it would a
/// field of a fixed structure.
#[test]
fn a_moved_field_in_a_fam_variant_is_refused() {
    refused(
        &edited(
            "[[tagged_fams.variants.fields]]\nname = \"pin_a\"",
            "[[tagged_fams.variants.fields]]\nname = \"lead_pad\"\nkind = \"padding\"\nsize = 1\n\n\
             [[tagged_fams.variants.fields]]\nname = \"pin_a\"",
        ),
        "onerom_alg_cs_config_t::ALG_CS_0.pin_a was at offset 0 in the last release and is at 1 now",
    );
}

// ===========================================================================
// Constants
// ===========================================================================

/// A constant is a value the firmware, its plugins and every host agree on.
/// None of them rebuilds because this file changed, so there is no exemption
/// and none should be added.
#[test]
fn a_changed_constant_is_refused() {
    refused(
        &edited(
            "name = \"MAX_PINS\"\ntype = \"u8\"\nvalue = 4",
            "name = \"MAX_PINS\"\ntype = \"u8\"\nvalue = 8",
        ),
        "constant MAX_PINS was 4 in the last release and is 8 now",
    );
}

/// Taking a constant away breaks everything that names it, so the way to
/// retire one is `deprecated_release`, which leaves it here.
#[test]
fn a_removed_constant_is_refused() {
    refused(
        &edited(
            "\n[[constants]]\nname = \"MAX_PINS\"\ntype = \"u8\"\nvalue = 4\n",
            "\n",
        ),
        "constant MAX_PINS was in the last release and is gone",
    );
}

/// Deprecating one is the whole point of the rule above, so it passes: the
/// constant is still here, and still holds what it held.
#[test]
fn a_deprecated_constant_is_accepted() {
    accepted(&edited(
        "name = \"MAX_PINS\"\ntype = \"u8\"\nvalue = 4",
        "name = \"MAX_PINS\"\ntype = \"u8\"\nvalue = 4\ndeprecated_release = \"0.8.0\"",
    ));
}

// ===========================================================================
// Generations
// ===========================================================================

/// The field a new generation adds, appended past the last field of
/// `onerom_hardware_info_t` so nothing already there moves.
fn with_new_field(marker: &str) -> String {
    edited(
        "name = \"pins\"\nkind = \"inline_array\"\nelement = \"u8\"\ncount = 4\n\
         count_ref = \"MAX_PINS\"",
        &format!(
            "name = \"pins\"\nkind = \"inline_array\"\nelement = \"u8\"\ncount = 4\n\
             count_ref = \"MAX_PINS\"\n\n[[structs.fields]]\nname = \"board_rev\"\n\
             kind = \"scalar\"\ntype = \"u16\"\n{marker}"
        ),
    )
}

/// Adding a field to anything under the metadata header changes what a reader
/// of that tree sees, so the header's generation moves with it.
#[test]
fn a_new_field_without_a_generation_bump_is_refused() {
    refused(
        &with_new_field(""),
        "onerom_metadata_header_t is still generation 2, and onerom_hardware_info_t's layout has \
         changed",
    );
}

#[test]
fn a_new_field_with_a_generation_bump_is_accepted() {
    let toml = with_new_field("since_metadata_version = 3\ndefault_if_absent = 0\n");
    accepted(&raised(&toml, 3));
}

/// The real structures are of fixed size, so the bytes a new field takes come
/// out of the reserved padding beside it.  The padding shrinks every time, and
/// that is not a shipped field moving - nothing reads one.
#[test]
fn a_field_taken_out_of_reserved_padding_is_accepted() {
    accepted(&raised(&with_field_from_padding(), 3));
}

/// The last of the padding going is the same event as some of it going.
#[test]
fn a_field_taking_all_the_reserved_padding_is_accepted() {
    let toml = edited(
        "name = \"reserved\"\nkind = \"padding\"\nsize = 6",
        "name = \"board_rev\"\nkind = \"inline_array\"\nelement = \"u8\"\ncount = 6\n\
         since_metadata_version = 3\ndefault_if_absent = 0",
    );
    accepted(&raised(&toml, 3));
}

/// Taking bytes out of the padding is still a field arriving, so the
/// generation still has to move with it.
#[test]
fn a_field_taken_out_of_reserved_padding_still_needs_the_bump() {
    refused(
        &with_field_from_padding(),
        "onerom_metadata_header_t is still generation 2, and onerom_hardware_info_t's layout has \
         changed",
    );
}

/// `onerom_hardware_info_t` with a new field taking two of its six reserved
/// bytes, which is how a field is added to a structure that cannot grow.
fn with_field_from_padding() -> String {
    edited(
        "name = \"reserved\"\nkind = \"padding\"\nsize = 6",
        "name = \"board_rev\"\nkind = \"scalar\"\ntype = \"u16\"\n\
         since_metadata_version = 3\ndefault_if_absent = 0\n\n\
         [[structs.fields]]\nname = \"reserved\"\nkind = \"padding\"\nsize = 4",
    )
}

#[test]
fn a_generation_raised_by_two_is_refused() {
    refused(
        &raised(&current(), 4),
        "CURRENT_METADATA_VERSION was 2 in the last release and is 4 now",
    );
}

/// Two development cycles on one copy read as a generation that rose by two,
/// and nothing in either file can prove that is what happened, so the message
/// names it as the likely cause, says which two releases are compared and
/// points at the script that refreshes the copy.
#[test]
fn a_generation_raised_by_two_names_the_stale_copy_as_a_likely_cause() {
    let err =
        against_released(&raised(&current(), 4)).expect_err("the pair should have been refused");
    assert!(
        err.contains(
            "The copy compared against is of a release before 0.8.0, and this schema is 0.8.0"
        ),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("ci/update-released-schema.sh"),
        "unexpected error: {err}"
    );
}

/// The same message against a copy that names its own release, which is what
/// every copy taken from 0.8.0 on does.
#[test]
fn a_generation_raised_by_two_names_the_release_the_copy_is_of() {
    let schema = Schema::parse(&raised(&current(), 4))
        .expect("the working schema should have been accepted");
    let copy = format!("{}{VERSIONS}{BODY}", HEAD.replace("0.8.0", "0.7.5"));
    let released = Released::parse(&copy).expect("the released schema should have parsed");
    let err = layout::compare(&schema, &released)
        .expect_err("the pair should have been refused")
        .to_string();
    assert!(
        err.contains("The copy compared against is of 0.7.5, and this schema is 0.8.0"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_generation_lowered_is_refused() {
    let toml = current()
        .replace(
            "name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = 2",
            "name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = 1",
        )
        .replace(
            "struct_name = \"onerom_metadata_header_t\"\nversion = 2",
            "struct_name = \"onerom_metadata_header_t\"\nversion = 1",
        );
    refused(
        &toml,
        "CURRENT_METADATA_VERSION was 2 in the last release and is 1 now",
    );
}

/// A generation with no `[[versions]]` entry names no release, so a host
/// holding a device's bytes cannot say what produced them.
///
/// `Schema::parse` refuses this earlier, by comparing the constant with the
/// newest entry in the table, so the schema here is deserialized without those
/// validators - the comparison does not assume its caller ran them.
#[test]
fn a_generation_raised_without_a_versions_entry_is_refused() {
    let toml = current().replace(
        "name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = 2",
        "name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = 3",
    );
    let schema: Schema = toml::from_str(&toml).expect("the working schema should have parsed");
    let released = Released::parse(&released()).expect("the released schema should have parsed");
    let err = layout::compare(&schema, &released)
        .expect_err("the pair should have been refused")
        .to_string();
    assert!(
        err.contains(
            "CURRENT_METADATA_VERSION was raised to 3 with no [[versions]] entry for \
             onerom_metadata_header_t generation 3"
        ),
        "unexpected error: {err}"
    );
}

/// No generation number covers a structure nothing reaches, so a change to one
/// is a change nothing can be raised for.
#[test]
fn a_change_outside_every_tree_is_refused() {
    refused(
        &format!(
            "{}\n[[structs]]\nname = \"stray_t\"\ngenerate = \"none\"\n\n\
             [[structs.fields]]\nname = \"thing\"\nkind = \"scalar\"\ntype = \"u8\"\n",
            current()
        ),
        "stray_t's layout has changed since the last release, and it sits under none of the \
         versioned structures",
    );
}

// ===========================================================================
// Which release the copy is of
// ===========================================================================

/// Refreshing the copy without raising `firmware_release` leaves the build
/// comparing the working schema with itself, which passes whatever changed.
#[test]
fn a_copy_of_the_release_in_development_is_refused() {
    let schema = Schema::parse(&current()).expect("the working schema should have been accepted");
    let same = format!("{HEAD}{VERSIONS}{BODY}");
    let released = Released::parse(&same).expect("the released schema should have parsed");
    let err = layout::compare(&schema, &released)
        .expect_err("the pair should have been refused")
        .to_string();
    assert!(
        err.contains("the released schema names 0.8.0 and this schema names 0.8.0"),
        "unexpected error: {err}"
    );
}

/// Raise the metadata header to `version`, with a `[[versions]]` entry for
/// every generation the fixture does not already carry.
fn raised(toml: &str, version: u32) -> String {
    let mut out = edited_in(
        toml,
        "name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = 2",
        &format!("name = \"CURRENT_METADATA_VERSION\"\ntype = \"u32\"\nvalue = {version}"),
    );
    for generation in 3..=version {
        out.push_str(&format!(
            "\n[[versions]]\nstruct_name = \"onerom_metadata_header_t\"\n\
             version = {generation}\nfirst_release = \"0.8.0\"\n"
        ));
    }
    out
}

// ===========================================================================
// Which release a plugin-facing item says it arrived in
// ===========================================================================
//
// The working schema alone cannot answer this.  `firmware_release` names the
// release under development, and everything a plugin author already has in
// hand names an older one, so the copy of the last release is what separates
// a new item from a shipped one.

/// The fixture with a plugin-facing constant, naming `release` where it names
/// one at all.
fn with_plugin_constant(toml: &str, release: Option<&str>) -> String {
    let first_release = match release {
        Some(release) => format!("\nfirst_release = \"{release}\""),
        None => String::new(),
    };
    format!(
        "{toml}\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n\
         ora_api = true{first_release}\n"
    )
}

/// The fixture with a plugin key on `onerom_hardware_info_t.board_id`, naming
/// `release` where it names one at all.
fn with_plugin_key(toml: &str, release: Option<&str>) -> String {
    let first_release = match release {
        Some(release) => format!(", first_release = \"{release}\""),
        None => String::new(),
    };
    edited_in(
        toml,
        "name = \"board_id\"\nkind = \"scalar\"\ntype = \"u16\"",
        &format!(
            "name = \"board_id\"\nkind = \"scalar\"\ntype = \"u16\"\n\
             plugin_key = {{ name = \"BOARD_ID\", id = 1{first_release} }}"
        ),
    )
}

/// Compare a pair built here, rather than the fixture's own two sides.
fn pair(current: &str, released: &str) -> Result<(), String> {
    let schema = Schema::parse(current).expect("the working schema should have been accepted");
    let released = Released::parse(released).expect("the released schema should have parsed");
    layout::compare(&schema, &released).map_err(|e| e.to_string())
}

/// A pair that should be refused, and the message should say why.
fn pair_refused(current: &str, released: &str, expected: &str) {
    let err = pair(current, released).expect_err("the pair should have been refused");
    assert!(err.contains(expected), "unexpected error: {err}");
}

#[test]
fn an_item_keeping_the_release_the_copy_names_is_accepted() {
    let now = with_plugin_key(
        &with_plugin_constant(&current(), Some("0.7.0")),
        Some("0.7.0"),
    );
    let then = with_plugin_key(
        &with_plugin_constant(&released(), Some("0.7.0")),
        Some("0.7.0"),
    );
    if let Err(e) = pair(&now, &then) {
        panic!("the pair should have been accepted: {e}");
    }
}

#[test]
fn a_constant_whose_first_release_has_moved_is_refused() {
    pair_refused(
        &with_plugin_constant(&current(), Some("0.7.1")),
        &with_plugin_constant(&released(), Some("0.7.0")),
        "constant SPARE named 0.7.0 in the last release and names 0.7.1 now",
    );
}

#[test]
fn a_key_whose_first_release_has_moved_is_refused() {
    pair_refused(
        &with_plugin_key(&current(), Some("0.7.1")),
        &with_plugin_key(&released(), Some("0.7.0")),
        "plugin key BOARD_ID named 0.7.0 in the last release and names 0.7.1 now",
    );
}

/// Something the copy does not have arrives in the release under development,
/// and naming an earlier one tells a plugin author it can ask for firmware
/// that has none of it.
#[test]
fn a_new_constant_naming_an_earlier_release_is_refused() {
    pair_refused(
        &with_plugin_constant(&current(), Some("0.7.0")),
        &released(),
        "constant SPARE is not in the copy of the last release, so it reaches the plugin API in \
         0.8.0 - its first_release says 0.7.0",
    );
}

#[test]
fn a_new_key_naming_an_earlier_release_is_refused() {
    pair_refused(
        &with_plugin_key(&current(), Some("0.7.0")),
        &released(),
        "plugin key BOARD_ID is not in the copy of the last release, so it reaches the plugin \
         API in 0.8.0 - its first_release says 0.7.0",
    );
}

/// The other way round: a constant the copy carries shipped before the release
/// under development, whatever else the copy does or does not say about it.
#[test]
fn a_shipped_constant_claiming_the_release_in_development_is_refused() {
    pair_refused(
        &with_plugin_constant(&current(), Some("0.8.0")),
        &with_plugin_constant(&released(), None),
        "constant SPARE was already in the plugin API of a release before 0.8.0 and names 0.8.0",
    );
}

#[test]
fn a_shipped_key_claiming_the_release_in_development_is_refused() {
    pair_refused(
        &with_plugin_key(&current(), Some("0.8.0")),
        &with_plugin_key(&released(), None),
        "plugin key BOARD_ID was already in the plugin API of a release before 0.8.0 and names \
         0.8.0",
    );
}

/// A constant the copy declares without `ora_api` was invisible to a plugin
/// then, so tagging it now is the release it arrives in.
#[test]
fn a_constant_newly_tagged_for_the_plugin_api_arrives_now() {
    let then = format!(
        "{}\n[[constants]]\nname = \"SPARE\"\ntype = \"u8\"\nvalue = 1\n",
        released()
    );
    if let Err(e) = pair(&with_plugin_constant(&current(), Some("0.8.0")), &then) {
        panic!("the pair should have been accepted: {e}");
    }
    pair_refused(
        &with_plugin_constant(&current(), Some("0.7.0")),
        &then,
        "constant SPARE is not in the copy of the last release",
    );
}
