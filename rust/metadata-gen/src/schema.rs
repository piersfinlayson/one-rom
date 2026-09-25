// src/schema.rs
//
// Serde-deserializable types mirroring the OneROM metadata TOML schema,
// plus shared size-computation helpers used by all code generators.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Top-level document
// ---------------------------------------------------------------------------

// Every type here denies unknown fields.  serde's default is to drop a key it
// does not recognise, so a misspelled `since_metadata_version` would silently
// do nothing at all.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    /// The schema file, as the `Source:` line of each generated file names
    /// it. Not read from the file. [`crate::generate`] sets it.
    #[serde(skip)]
    pub source: String,
    pub schema: SchemaMetadata,
    #[serde(default)]
    pub versions: Vec<StructVersion>,
    #[serde(default)]
    pub constants: Vec<Constant>,
    #[serde(default)]
    pub type_aliases: Vec<TypeAlias>,
    #[serde(default)]
    pub enums: Vec<Enum>,
    #[serde(default)]
    pub structs: Vec<Struct>,
    #[serde(default)]
    pub tagged_fams: Vec<TaggedFam>,
    #[serde(default)]
    pub simple_fams: Vec<SimpleFam>,
}

// ---------------------------------------------------------------------------
// [schema]
// ---------------------------------------------------------------------------

// The dead_code lint does not trace usage across build-script modules, so
// fields the generators read look unused here.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SchemaMetadata {
    /// Version of this file's own set of tables and keys.  Nothing to do
    /// with the generation numbers the described structures carry.
    pub format_version: u32,
    /// Firmware release this schema describes, e.g. "0.8.0".  Matches
    /// VERSION_MAJOR/MINOR/PATCH in the repo-root Makefile, and is what a
    /// `first_release` elsewhere in the file names.
    pub firmware_release: String,
    pub name: String,
    pub description: String,
    pub flash_base: u32,
    /// Flash address of the metadata region, where something outside the
    /// firmware composes it.  Absent where the structures are consts the
    /// build places and the anchor's pointer finds.
    pub metadata_base: Option<u32>,
    /// Bytes reserved for that region, and absent for the same reason.
    pub metadata_size: Option<u32>,
    pub root_struct: String,
    /// The name of this schema's device-side type for the family's
    /// `onerom_info_t`, with its `metadata` and `runtime` pointers at this
    /// schema's structures. For a schema whose `info` slot another crate
    /// fills.
    pub header_name: Option<String>,
}

// ---------------------------------------------------------------------------
// [[versions]]
// ---------------------------------------------------------------------------

// The top-level structures carrying a generation number.  The middle word of
// `since_metadata_version` and its siblings is fixed by the field names on
// `Field`, and these constants are the one place it is tied to the structure
// it stands for.  `validate_versions` refuses a schema where one has no
// matching struct, or where that struct declares no generation field.
/// The roles a versioned structure fills. A schema says which of its own
/// structures fills each, with `generation_slot`, and the marker keys and the
/// generated Rust name a generation by the same word.
///
/// A schema need not fill all three. One ROM's fills every one, and a schema
/// whose anchor structure belongs to another crate leaves `info` empty.
pub const SLOT_INFO: &str = "info";
pub const SLOT_METADATA: &str = "metadata";
pub const SLOT_RUNTIME: &str = "runtime";

/// Every slot a schema may declare, in the order a generator emits them.
pub const SLOT_ORDER: [&str; 3] = [SLOT_INFO, SLOT_METADATA, SLOT_RUNTIME];

/// The same three in the order ownership resolves, which is a different
/// question and a different order.
///
/// A structure two trees reach belongs to the one that writes its bytes, so
/// metadata comes first. Info comes last because it points at both of the
/// others, and taken first it would own the lot.
const OWNERSHIP_SLOT_ORDER: [&str; 3] = [SLOT_METADATA, SLOT_RUNTIME, SLOT_INFO];

/// Whether `slot` is one of the three a schema may declare.
pub fn is_slot(slot: &str) -> bool {
    SLOT_ORDER.contains(&slot)
}

/// The field kinds a generation marker may sit on - those with something a
/// reader of an older structure can be handed in place of bytes nobody wrote.
/// What that is per kind is [`Schema::validate_defaults`].
const MARKABLE_KINDS: [&str; 13] = [
    "scalar",
    "enum",
    "type_alias",
    "inline_array",
    "inline_array2d",
    "cstr_ptr",
    "struct_ptr",
    "struct_array_ptr",
    "struct_ptr_array_ptr",
    "tagged_fam_ptr",
    "simple_fam_ptr",
    "opaque_ptr",
    "fn_ptr",
];

/// The kinds a marker may not sit on, each with why not.
const UNMARKABLE_KINDS: [(&str, &str); 1] = [("padding", "nothing reads a padding field")];

/// The pointer kinds that must be nullable to carry a marker.  A gated one
/// reads as `None` below its generation, and a non-nullable field has no such
/// state - its Rust type is the target itself, and its parser raises
/// `NullPointer` on the unwritten bytes.  The rest need no flag: an array
/// reads as no elements and an undereferenced pointer as null.
const NULLABLE_REQUIRED_KINDS: [&str; 4] =
    ["cstr_ptr", "struct_ptr", "tagged_fam_ptr", "simple_fam_ptr"];

/// The kinds whose bytes are a pointer the parser follows or stores, and
/// whose only `default_if_absent` is `"null"`.
pub const POINTER_KINDS: [&str; 8] = [
    "cstr_ptr",
    "struct_ptr",
    "struct_array_ptr",
    "struct_ptr_array_ptr",
    "tagged_fam_ptr",
    "simple_fam_ptr",
    "opaque_ptr",
    "fn_ptr",
];

/// The kinds whose bytes sit in the structure itself, whose
/// `default_if_absent` is a fill or a list.
pub const ARRAY_KINDS: [&str; 2] = ["inline_array", "inline_array2d"];

/// The `default_if_absent` every pointer kind takes, and the only one.
pub const NULL_DEFAULT: &str = "null";

/// The elements an array field's `default_if_absent` states, expanded and
/// flattened into the array's own order - a whole number fills every element,
/// a list states them one by one.  Both generators write this out rather than
/// each re-reading the TOML.  `check_array_default` has already refused
/// anything it cannot expand.
pub fn array_default_elements(field: &Field) -> Vec<u8> {
    let value = field
        .default_if_absent
        .as_ref()
        .expect("a gated field has a default");
    let total = array_element_count(field);
    match value.as_integer() {
        Some(fill) => vec![fill as u8; total],
        None => flatten_array_default(value)
            .expect("validate_defaults has already refused a default of another shape")
            .into_iter()
            .map(|n| n as u8)
            .collect(),
    }
}

/// How many elements an array field holds, both dimensions counted.
fn array_element_count(field: &Field) -> usize {
    match field.kind.as_str() {
        "inline_array2d" => field.rows.unwrap_or(0) as usize * field.cols.unwrap_or(0) as usize,
        _ => field.count.unwrap_or(0) as usize,
    }
}

/// A TOML list of whole numbers, or of lists of them, as one flat list.
/// `None` where anything in it is neither.
fn flatten_array_default(value: &toml::Value) -> Option<Vec<i64>> {
    let mut out = Vec::new();
    for item in value.as_array()? {
        match item.as_integer() {
            Some(n) => out.push(n),
            None => out.extend(flatten_array_default(item)?),
        }
    }
    Some(out)
}

/// Whether a listed default is nested the way the field is dimensioned - a
/// row per entry for `inline_array2d`, and a flat list otherwise.
fn array_default_shape_fits(field: &Field, value: &toml::Value) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    if field.kind != "inline_array2d" {
        return items.iter().all(|i| i.is_integer());
    }
    let cols = field.cols.unwrap_or(0) as usize;
    items.len() == field.rows.unwrap_or(0) as usize
        && items.iter().all(|row| {
            row.as_array()
                .is_some_and(|r| r.len() == cols && r.iter().all(|i| i.is_integer()))
        })
}

/// Check an array field's `default_if_absent` - see [`Schema::validate_defaults`].
fn check_array_default(
    container: &str,
    field: &Field,
    value: &toml::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = &field.name;
    let total = array_element_count(field);

    let elements = match value.as_integer() {
        Some(fill) => vec![fill],
        None => {
            let Some(flat) = flatten_array_default(value) else {
                return Err(format!(
                    "{container}.{name} has default_if_absent {value}, and an array's default is \
                     a whole number filling it or a list stating it"
                )
                .into());
            };
            // The list states the array, so a short one leaves elements
            // nobody said anything about.
            if flat.len() != total {
                return Err(format!(
                    "{container}.{name} has a default_if_absent of {} elements, and the field \
                     holds {total}",
                    flat.len(),
                )
                .into());
            }
            // Rows of uneven length would flatten to the right total while
            // saying something else.
            if !array_default_shape_fits(field, value) {
                let shape = match field.kind.as_str() {
                    "inline_array2d" => format!(
                        "{} rows of {}",
                        field.rows.unwrap_or(0),
                        field.cols.unwrap_or(0)
                    ),
                    _ => format!("a flat list of {total}"),
                };
                return Err(format!(
                    "{container}.{name} has a default_if_absent that is not the field's shape, \
                     which is {shape}"
                )
                .into());
            }
            flat
        }
    };

    for n in elements {
        if !(0..=u8::MAX as i64).contains(&n) {
            return Err(format!(
                "{container}.{name} has {n} in its default_if_absent, which a u8 cannot hold"
            )
            .into());
        }
    }
    Ok(())
}

/// What a gated field's bytes are declared under in C, in place of the
/// field's own name.  See [`Field::c_member`].
pub const GATED_MEMBER_SUFFIX: &str = "_stored";

/// A C type name without its `_t`, which is the stem every generated name
/// derived from that type is built on.
pub fn strip_type_suffix(name: &str) -> &str {
    name.strip_suffix("_t").unwrap_or(name)
}

// The order the ownership walk assigns in, each root taking everything it
// reaches that no earlier root has taken.  Deliberately not the order above.
//
// A structure belongs to the tree whose root writes its bytes.
/// Pair the generations a field declares with the slots they belong to,
/// dropping the ones it leaves out.
///
/// A field names a slot rather than a structure, so nothing below the schema
/// has to know which structure fills it.
fn markers(
    info: Option<u32>,
    metadata: Option<u32>,
    runtime: Option<u32>,
) -> Vec<(&'static str, u32)> {
    [
        (SLOT_INFO, info),
        (SLOT_METADATA, metadata),
        (SLOT_RUNTIME, runtime),
    ]
    .into_iter()
    .filter_map(|(slot, generation)| generation.map(|g| (slot, g)))
    .collect()
}

/// One generation of a top-level structure, and the firmware release it first
/// shipped in.  A structure's generation number rises by hand whenever its
/// layout changes, and an entry here says which release brought that
/// generation in.
// No generator emits first_release - the table is the statement of which
// release each generation arrived in.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct StructVersion {
    /// The structure this generation belongs to, e.g. "onerom_info_t".
    pub struct_name: String,
    /// The generation number itself, as the structure's own version field
    /// holds it on a device.
    pub version: u32,
    /// Firmware release this generation first shipped in, e.g. "0.7.0".
    pub first_release: String,
}

// ---------------------------------------------------------------------------
// [[constants]]
// ---------------------------------------------------------------------------

/// TOML constant values are either integers or strings (e.g. magic byte strings).
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ConstantValue {
    Integer(i64),
    Text(String),
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Constant {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub value: ConstantValue,
    pub comment: Option<String>,

    /// Whether this constant is part of the ORA plugin API.
    ///
    /// A plugin builds against firmware/ora alone and cannot include the
    /// firmware's own metadata header, so a value it must agree with the
    /// firmware on is emitted into firmware/ora/onerom_constants_generated.h
    /// as well. False for a constant no plugin needs, which is most of them.
    #[serde(default)]
    pub ora_api: bool,

    /// Whether this constant is also written into the linker-script fragment,
    /// which a linker script can include where it cannot include a C header.
    /// False for every constant no linker script uses.
    #[serde(default)]
    pub linker_script: bool,

    /// Firmware release in which this constant became visible to a plugin,
    /// which is what a plugin author sets `min_fw_version` from.  Required of
    /// every `ora_api` constant and allowed on no other.  It says when the
    /// constant reached the plugin API rather than when it was declared, and
    /// those differ wherever a constant predates its `ora_api` tag.
    pub first_release: Option<String>,

    /// Firmware release from which nothing should use this constant.  A
    /// shipped constant is never removed - the firmware, a host and every
    /// plugin already built against it name it still - so retiring one is
    /// saying so here and leaving it where it is.  Deprecation is permanent.
    pub deprecated_release: Option<String>,
}

impl Constant {
    /// The name this constant takes in the ORA plugin API.
    ///
    /// Derived rather than given, so the two names always correspond and
    /// either can be found from the other.
    pub fn ora_name(&self) -> String {
        format!("ORA_{}", self.name)
    }

    /// What every generator writes above this constant: its comment, plus the
    /// deprecation note where it carries a `deprecated_release`.  The note
    /// rides the comment rather than being a second thing each generator
    /// emits, so all three outputs say the same thing.
    pub fn documentation(&self) -> Option<String> {
        let note = self.deprecated_release.as_ref().map(|release| {
            format!("Deprecated from firmware {release} - nothing should use it from there on.")
        });
        match (&self.comment, note) {
            (Some(comment), Some(note)) => Some(format!("{comment}\n{note}")),
            (Some(comment), None) => Some(comment.clone()),
            (None, note) => note,
        }
    }

    /// What the plugin-facing header writes above this constant: everything
    /// [`Constant::documentation`] gives, plus the `@since` line naming the
    /// release the constant reached the plugin API in.  That line is for the
    /// plugin author alone, who sets `min_fw_version` from the oldest release
    /// carrying everything the plugin uses - `api.h` answers the same question
    /// for every identifier, in the same words.
    pub fn plugin_documentation(&self) -> Option<String> {
        let since = self
            .first_release
            .as_ref()
            .map(|release| format!("@since firmware {release}"));
        match (self.documentation(), since) {
            (Some(doc), Some(since)) => Some(format!("{doc}\n{since}")),
            (Some(doc), None) => Some(doc),
            (None, since) => since,
        }
    }
}

impl Schema {
    /// The constants the ORA plugin API carries, in schema order.
    pub fn ora_constants(&self) -> impl Iterator<Item = &Constant> {
        self.constants.iter().filter(|c| c.ora_api)
    }

    /// The constants the linker-script fragment carries, in schema order.
    pub fn linker_constants(&self) -> impl Iterator<Item = &Constant> {
        self.constants.iter().filter(|c| c.linker_script)
    }
}

// ---------------------------------------------------------------------------
// [[type_aliases]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct TypeAlias {
    pub name: String,
    pub underlying: String,
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// [[enums]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Enum {
    pub name: String,
    /// Byte size, verified by STATIC_ASSERT in the original C source.
    pub size: u32,
    /// Emit __attribute__((packed)) in C.
    pub packed: Option<bool>,
    pub comment: Option<String>,
    /// Common C name prefix to strip when deriving Rust variant names.
    pub strip_prefix: Option<String>,
    /// When set to "rbcp_chip_types", enum variants are generated from
    /// `onerom_config::chip::CHIP_TYPES` rather than being listed here.
    /// c_gen.rs handles the generation using `ChipType` methods directly
    /// (`rbcp_chip_type()`, `c_enum_name()`, `size_bytes()`, `try_from_rbcp_u8()`);
    /// rust_gen.rs and host_gen.rs skip this enum entirely (no Rust type is
    /// emitted).
    pub source: Option<String>,
    #[serde(default)]
    pub variants: Vec<EnumVariant>,
    /// Same-value name aliases (C: #define; Rust: const).
    #[serde(default)]
    pub aliases: Vec<EnumAlias>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct EnumVariant {
    pub name: String,
    pub value: i64,
    /// true = emit as a constant, not a Rust enum variant.
    /// In generated C the variant still appears in the enum body.
    pub sentinel: Option<bool>,
    pub comment: Option<String>,
    pub display: Option<String>,
}

impl EnumVariant {
    pub fn is_sentinel(&self) -> bool {
        self.sentinel.unwrap_or(false)
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct EnumAlias {
    pub name: String,
    pub target: String,
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared: generate flag
// ---------------------------------------------------------------------------

/// Controls what Rust code is generated for a type.
/// The C definition is always emitted regardless of this value.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Generate {
    /// C definition + Rust parse + Rust serialize.
    Both,
    /// C definition + Rust parse only.
    Parse,
    /// C definition only; no Rust codegen.
    #[serde(rename = "none")]
    Skip,
}

// ---------------------------------------------------------------------------
// Shared: Field
// ---------------------------------------------------------------------------

/// A plugin-facing metadata key.
///
/// When attached to a struct field via `plugin_key`, that field is exposed to
/// plugins through the metadata getter API under this key.  `id` is the stable,
/// permanent enum value: once assigned it must never be renumbered or reused.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PluginKey {
    pub name: String,
    pub id: u32,

    /// Firmware release in which this key reached the plugin API, e.g.
    /// "0.7.1".  Every key has one: it becomes the `@since` line in the
    /// generated key header, which is what a plugin author sets
    /// `min_fw_version` from.
    pub first_release: String,
}

/// A plugin key paired with the field it is attached to.
pub struct PluginKeyEntry<'a> {
    pub key: &'a PluginKey,
    pub comment: Option<&'a str>,
    /// Name of the struct that contains the field (for access-path derivation).
    pub struct_name: &'a str,
    /// Name of the field itself (the last hop of the access path).
    pub field_name: &'a str,
    /// Field kind, e.g. "cstr_ptr" - selects which typed accessor resolves it.
    pub kind: &'a str,
    /// Element count, for the array kinds.  None for a scalar field.
    pub count: Option<u32>,
    /// C constant naming that count, e.g. "MAX_IMG_SEL_PINS".  Preferred over
    /// `count` when emitting a bound, so the generated code reads as the
    /// constant rather than a bare number.
    pub count_ref: Option<&'a str>,
}

/// The constant, or constants, an `expected_const` names.
///
/// One name is the ordinary case. A list is for a magic whose value changed
/// between firmware generations, where devices carrying the older value are
/// still read, so a parse takes any of them.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ExpectedConst {
    One(String),
    Any(Vec<String>),
}

impl ExpectedConst {
    /// Every constant named, in declaration order.
    pub fn names(&self) -> Vec<&str> {
        match self {
            Self::One(name) => vec![name.as_str()],
            Self::Any(names) => names.iter().map(String::as_str).collect(),
        }
    }
}

/// A field within a `[[structs]]` definition or a `[[tagged_fams]]`
/// common/variant section.  Uses a flat layout: all optional members are None
/// when inapplicable to the field's `kind`.
///
/// Field kinds and their relevant members:
///
/// | kind                  | members used                                    |
/// |-----------------------|-------------------------------------------------|
/// | scalar                | type_                                           |
/// | enum                  | type_                                           |
/// | type_alias            | type_                                           |
/// | inline_array          | element, count, count_ref?                      |
/// | inline_array2d        | element, rows, cols, rows_ref?, cols_ref?       |
/// | cstr_ptr              | nullable                                        |
/// | struct_ptr            | type_, nullable                                 |
/// | struct_array_ptr      | element, count_field, nullable                  |
/// | struct_ptr_array_ptr  | element, count_field, nullable                  |
/// | tagged_fam_ptr        | type_, nullable                                 |
/// | simple_fam_ptr        | type_, nullable                                 |
/// | opaque_ptr            | (none; const-ness derived from struct setting)  |
/// | fn_ptr                | (none; generates void (*name)(void))            |
/// | padding               | size                                            |
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub kind: String,

    // Type reference
    #[serde(rename = "type")]
    pub type_: Option<String>,

    // Array element type
    pub element: Option<String>,

    // 1-D array dimensions
    pub count: Option<u32>,
    /// C constant name to use as array dimension in generated C (e.g. "MAX_ADDR_PINS").
    /// The integer `count` is still used for size tracking.
    pub count_ref: Option<String>,

    // 2-D array dimensions
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub rows_ref: Option<String>,
    pub cols_ref: Option<String>,

    // Pointer attributes
    pub nullable: Option<bool>,
    /// Name of the sibling field that holds the array length at runtime.
    pub count_field: Option<String>,

    /// C type for opaque_ptr fields where void is not appropriate,
    /// e.g. "u8" yields `const uint8_t *`. Absent → void.
    pub pointed_type: Option<String>,

    /// true = emit `const T * const name` — the pointer itself is also const.
    /// Applies to struct_ptr, tagged_fam_ptr, simple_fam_ptr.
    pub const_ptr: Option<bool>,

    // Padding byte count
    pub size: Option<u32>,

    /// Expected byte offset; drives a generated STATIC_ASSERT(offsetof(...)).
    pub expected_offset: Option<u32>,

    pub comment: Option<String>,

    pub expected_const: Option<ExpectedConst>,

    pub none_on_parse_error: Option<bool>,

    /// Plugin-facing metadata key.  When set, this field is exposed to plugins
    /// through the metadata getter API under the given key name and id.
    pub plugin_key: Option<PluginKey>,

    // Generation markers.  A field added after its governing structure's first
    // generation says which generation brought it in, and what a reader of an
    // older structure uses instead.  The key names the structure because it is
    // not always the one the field sits in - a field of
    // onerom_hardware_info_t is governed by onerom_metadata_header_t, which
    // points at it.
    /// Generation of `onerom_info_t` this field first appeared in.
    pub since_info_version: Option<u32>,
    /// Generation of `onerom_metadata_header_t` this field first appeared in.
    pub since_metadata_version: Option<u32>,
    /// Generation of `onerom_runtime_info_t` this field first appeared in.
    pub since_runtime_version: Option<u32>,

    /// Value a reader uses where the structure predates this field.  Pairs
    /// with the `since_*_version` marker - a field has both or neither.  What
    /// it holds depends on the kind, spelled out in
    /// `Schema::validate_defaults`.
    pub default_if_absent: Option<toml::Value>,

    // Deprecation markers.  A shipped field is never moved, renamed or
    // removed - its bytes stay where they are - but from the generation named
    // here a reader ignores what they hold.  Deprecation is permanent, so a
    // field once deprecated is never brought back.
    /// Generation of `onerom_info_t` from which readers ignore this field.
    pub deprecated_info_version: Option<u32>,
    /// Generation of `onerom_metadata_header_t` from which readers ignore this
    /// field.
    pub deprecated_metadata_version: Option<u32>,
    /// Generation of `onerom_runtime_info_t` from which readers ignore this
    /// field.
    pub deprecated_runtime_version: Option<u32>,
}

impl Field {
    /// Every `since_*_version` this field carries, as (structure name,
    /// generation), so callers work in structure names rather than key words.
    pub fn since_markers(&self) -> Vec<(&'static str, u32)> {
        markers(
            self.since_info_version,
            self.since_metadata_version,
            self.since_runtime_version,
        )
    }

    /// Every `deprecated_*_version` this field carries, in the same shape as
    /// [`Field::since_markers`].
    pub fn deprecated_markers(&self) -> Vec<(&'static str, u32)> {
        markers(
            self.deprecated_info_version,
            self.deprecated_metadata_version,
            self.deprecated_runtime_version,
        )
    }

    /// The one generation this field first appeared in, where it says.
    /// `Schema::parse` has already refused a field naming two structures.
    pub fn since_marker(&self) -> Option<(&'static str, u32)> {
        self.since_markers().first().copied()
    }

    /// The one generation from which readers ignore this field, where it says.
    /// Single for the same reason as [`Field::since_marker`].
    pub fn deprecated_marker(&self) -> Option<(&'static str, u32)> {
        self.deprecated_markers().first().copied()
    }

    /// The structure whose generation governs this field, where it names one.
    pub fn governor(&self) -> Option<&'static str> {
        self.since_marker()
            .or_else(|| self.deprecated_marker())
            .map(|(name, _)| name)
    }

    /// The metadata generation this field arrived in, where that is what
    /// decides whether it is there at all.
    ///
    /// The firmware writes `onerom_info_t` and `onerom_runtime_info_t` itself
    /// and meets them in the binary they were compiled into, so it never sees
    /// an older shape of either.  The metadata is the one structure written
    /// elsewhere - by whichever CLI programmed the device, which may be far
    /// older than the running firmware - so it is the one read through an
    /// accessor.
    pub fn metadata_since(&self) -> Option<u32> {
        match self.since_marker() {
            Some((SLOT_METADATA, generation)) => Some(generation),
            _ => None,
        }
    }

    /// The C member this field's bytes are declared under.
    ///
    /// A gated field's bytes take a member name of its own, because the
    /// field's own name belongs to the generated accessor - the only thing
    /// that knows what to hand back when the metadata predates the field.  C
    /// has nothing to stop a direct read of the member, so the way to stop one
    /// is to take the name away.
    pub fn c_member(&self) -> String {
        match self.metadata_since() {
            Some(_) => format!("{}{}", self.name, GATED_MEMBER_SUFFIX),
            None => self.name.clone(),
        }
    }

    /// This field as the layout walk sees it.
    pub fn shape(&self) -> FieldShape<'_> {
        FieldShape {
            name: &self.name,
            kind: &self.kind,
            type_: self.type_.as_deref(),
            element: self.element.as_deref(),
            count: self.count,
            rows: self.rows,
            cols: self.cols,
            size: self.size,
        }
    }

    /// The struct, tagged FAM or simple FAM this field points at, where it
    /// points at one.  This is the edge the containment walk follows.
    pub fn referenced_type(&self) -> Option<&str> {
        match self.kind.as_str() {
            "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => self.type_.as_deref(),
            "struct_array_ptr" | "struct_ptr_array_ptr" => self.element.as_deref(),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// [[structs]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Struct {
    pub name: String,
    pub comment: Option<String>,
    pub generate: Generate,
    /// Expected total byte size for STATIC_ASSERT (absent if no assertion in original C).
    pub size: Option<u32>,
    /// true = this struct is placed at metadata_base (the root of the generated region).
    pub root: Option<bool>,
    /// false = fields are non-const (runtime-written structs such as onerom_runtime_info_t).
    /// Defaults to true.
    pub const_fields: Option<bool>,
    /// Name of the field holding this structure's own generation number.  Only
    /// the top-level structures a host parses from a fixed anchor carry one.
    /// Declared rather than found by name, so a struct breaking the convention
    /// is caught.
    pub version_field: Option<String>,
    /// Name of the constant holding this structure's current generation
    /// number.  `version_field` says where a device carries it and this says
    /// where the schema states it, so a structure declares both or neither.
    ///
    /// It is also what lets an older schema be read: that file has no
    /// `[[versions]]` table, so the constant is the one place a generation can
    /// be read from both the old file and the new.
    pub version_constant: Option<String>,

    /// Which of [`SLOT_ORDER`] this structure fills, where it carries a
    /// generation. Declared rather than derived from the name, so a schema
    /// naming its structures whatever it likes still says which is which.
    /// A structure declares this and `version_field` together or neither.
    pub generation_slot: Option<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
}

impl Struct {
    pub fn has_const_fields(&self) -> bool {
        self.const_fields.unwrap_or(true)
    }
}

/// Byte offset of every field of `s`, in declaration order - a running sum of
/// `field_size`, as the C generator's `// Offset:` comments and the Rust
/// parser both walk it.  The schema spells every hole out as an explicit
/// `padding` field, and the C header's `sizeof` and `offsetof` assertions
/// catch it if that stops being true.
pub fn field_offsets(s: &Struct, schema: &Schema) -> Vec<usize> {
    let mut offset = 0;
    s.fields
        .iter()
        .map(|f| {
            let here = offset;
            offset += field_size(f, schema);
            here
        })
        .collect()
}

// ---------------------------------------------------------------------------
// [[tagged_fams]]
// ---------------------------------------------------------------------------

/// A variable-length C struct discriminated by an enum field.
///
/// Binary layout: [discriminant (1–2 B)] [param_len (1 B)] [common fields] [params…]
///
/// Generates as a Rust enum where the discriminant selects the variant and
/// the common + variant fields are members.  In C it is a struct with a
/// flexible array member (params[]).
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct TaggedFam {
    pub name: String,
    pub comment: Option<String>,
    pub generate: Generate,
    pub discriminant_field: String,
    pub discriminant_type: String,
    pub param_len_field: String,
    /// sizeof the fixed C struct portion (i.e. excluding params[]).
    /// Verified by STATIC_ASSERT in generated C.
    pub base_size: u32,
    #[serde(default)]
    pub common_fields: Vec<Field>,
    #[serde(default)]
    pub variants: Vec<TaggedFamVariant>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct TaggedFamVariant {
    /// Name of the discriminant enum variant, e.g. "ALG_CS_0".
    pub discriminant: String,
    pub comment: Option<String>,
    /// Name of the schema constant holding this variant's parameter byte length.
    /// Used in the generated STATIC_ASSERT for the param struct.
    pub params_len_constant: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

// ---------------------------------------------------------------------------
// [[simple_fams]]
// ---------------------------------------------------------------------------

/// Variable-length C struct: a one-byte length prefix followed by a byte array.
///
/// Generates as a Rust struct with `params: Vec<u8>`.
/// In C it is a struct with a flexible array member (params[]).
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SimpleFam {
    pub name: String,
    pub comment: Option<String>,
    pub generate: Generate,
    pub param_len_field: String,
}

// ---------------------------------------------------------------------------
// Schema loading
// ---------------------------------------------------------------------------

/// Whether `value` has the shape of a firmware release: three dot-separated
/// decimal numbers, as in "0.8.0".  Whether that release exists is beyond
/// anything the schema can see.
pub fn is_release(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| is_decimal(p))
}

/// Whether `part` is one component of a release, written the one way there is.
/// Otherwise "0.07.2" and "0.7.2" would both name the same release, and
/// anything comparing release strings as text would see two.
fn is_decimal(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
}

/// A release as three numbers, for comparing one with another.
pub fn release_parts(value: &str) -> Option<(u32, u32, u32)> {
    if !is_release(value) {
        return None;
    }
    let mut parts = value.split('.').map(|p| p.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

impl Schema {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Parse and validate schema TOML held in memory.  Split out from
    /// [`Schema::load`] so a test can exercise the validation rules against a
    /// schema it builds, with no file to write.
    pub fn parse(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let schema: Schema = toml::from_str(content)?;
        // First, because the rest of the generation machinery is written in
        // terms of the root's name - a schema that moved it should say so
        // rather than fail as three missing structures.
        schema.validate_root()?;
        schema.validate_plugin_keys()?;
        schema.validate_expected_offsets()?;
        schema.validate_expected_consts()?;
        schema.validate_version_fields()?;
        schema.validate_versions()?;
        schema.validate_header_name()?;
        schema.validate_version_constants()?;
        schema.validate_single_governor()?;
        schema.validate_field_generations()?;
        schema.validate_marker_kinds()?;
        schema.validate_defaults()?;
        schema.validate_gated_counts()?;
        schema.validate_marker_order()?;
        schema.validate_release_strings()?;
        schema.validate_constant_releases()?;
        schema.validate_constant_deprecations()?;
        schema.validate_linker_constants()?;
        Ok(schema)
    }

    /// Check the three statements of which structure is the root against each
    /// other: `root_struct` in `[schema]`, the `root = true` flag on the
    /// structure, and the structure filling the metadata slot.  Each is read by
    /// different code, so two agreeing while the third does not puts a
    /// generator and a validator on different structures.
    fn validate_root(&self) -> Result<(), Box<dyn std::error::Error>> {
        let flagged: Vec<&str> = self
            .structs
            .iter()
            .filter(|s| s.root == Some(true))
            .map(|s| s.name.as_str())
            .collect();

        let [root] = flagged[..] else {
            return Err(format!(
                "{} structures carry root = true, and exactly one is the root of the metadata \
                 region",
                flagged.len()
            )
            .into());
        };

        if root != self.schema.root_struct {
            return Err(format!(
                "{root} carries root = true but [schema] root_struct names {}",
                self.schema.root_struct
            )
            .into());
        }
        match self.slot_of(root) {
            Some(SLOT_METADATA) => {}
            Some(slot) => {
                return Err(format!(
                    "the root structure is {root}, which fills the {slot} slot - the root of the \
                     metadata region is what a generated accessor reads a generation from, so it \
                     fills {SLOT_METADATA}"
                )
                .into());
            }
            None => {
                return Err(format!(
                    "the root structure is {root}, which declares no generation_slot - the root \
                     of the metadata region fills {SLOT_METADATA}"
                )
                .into());
            }
        }
        Ok(())
    }

    /// A `header_name` names the family's `onerom_info_t` pointing at this
    /// schema's metadata and runtime structures. So the schema must have both,
    /// and must not have an `info` structure of its own.
    fn validate_header_name(&self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(name) = &self.schema.header_name else {
            return Ok(());
        };
        if let Some(s) = self.struct_in_slot(SLOT_INFO) {
            return Err(format!(
                "[schema] header_name is {name}, and {} fills the {SLOT_INFO} slot - a schema \
                 with a header_name uses the family's onerom_info_t",
                s.name
            )
            .into());
        }
        for slot in [SLOT_METADATA, SLOT_RUNTIME] {
            if self.struct_in_slot(slot).is_none() {
                return Err(format!(
                    "[schema] header_name is {name}, and no structure fills the {slot} slot for \
                     its {slot} pointer to point at"
                )
                .into());
            }
        }
        Ok(())
    }

    /// Refuse an `expected_const` naming nothing.
    ///
    /// The generator joins the names into one condition, so an empty list
    /// emits a check with no condition in it and the failure lands in
    /// OUT_DIR naming neither the field nor this file.
    fn validate_expected_consts(&self) -> Result<(), Box<dyn std::error::Error>> {
        for (struct_name, field) in self.all_fields() {
            if let Some(expected) = &field.expected_const
                && expected.names().is_empty()
            {
                return Err(format!(
                    "{struct_name}.{} declares an expected_const naming no constant",
                    field.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check each structure's generation constant against the `[[versions]]`
    /// table.  The generation is stated twice - as the constant the firmware
    /// and every host compile against, and as the newest `[[versions]]` entry.
    /// Raising one and not the other leaves a device reporting a generation no
    /// release claims, or the reverse.
    fn validate_version_constants(&self) -> Result<(), Box<dyn std::error::Error>> {
        for s in &self.structs {
            match (&s.version_field, &s.version_constant) {
                (Some(_), Some(_)) | (None, None) => {}
                (Some(_), None) => {
                    return Err(format!(
                        "{} declares where a device carries its generation but not the constant \
                         holding that generation's value",
                        s.name
                    )
                    .into());
                }
                (None, Some(name)) => {
                    return Err(format!(
                        "{} names {name} as its generation constant but declares no version_field \
                         for a device to carry the number in",
                        s.name
                    )
                    .into());
                }
            }

            let Some(name) = &s.version_constant else {
                continue;
            };
            let Some(c) = self.constants.iter().find(|c| &c.name == name) else {
                return Err(format!(
                    "{} names {name} as its generation constant, and no such constant is declared",
                    s.name
                )
                .into());
            };
            let ConstantValue::Integer(value) = c.value else {
                return Err(format!(
                    "{name} is {}'s generation constant, so it holds a generation number, and its \
                     value is text",
                    s.name
                )
                .into());
            };

            let Some(newest) = self
                .versions
                .iter()
                .filter(|v| v.struct_name == s.name)
                .map(|v| v.version)
                .max()
            else {
                return Err(format!(
                    "{} carries a generation number and has no [[versions]] entry saying which \
                     release any generation of it shipped in",
                    s.name
                )
                .into());
            };

            if value != i64::from(newest) {
                return Err(format!(
                    "{name} is {value} but the newest [[versions]] entry for {} is generation \
                     {newest}",
                    s.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// The constant naming `name`'s generation, and the value it holds.
    ///
    /// [`Schema::validate_version_constants`] has already refused a schema
    /// where a versioned structure declares no such constant, or where the
    /// constant is not a number.
    pub fn generation_constant(&self, name: &str) -> Option<(&str, u32)> {
        let s = self.structs.iter().find(|s| s.name == name)?;
        let constant = s.version_constant.as_deref()?;
        let c = self.constants.iter().find(|c| c.name == constant)?;
        let ConstantValue::Integer(value) = c.value else {
            return None;
        };
        Some((constant, u32::try_from(value).ok()?))
    }

    /// Check every `expected_offset` against the offset the layout walk gives.
    ///
    /// A field carrying one is read by hand from outside the schema - a plugin
    /// reading a runtime pointer, or a host bootstrapping its parse of the
    /// info header.  The C header asserts the same thing with `offsetof`, but
    /// only once the firmware is compiled.  Checking here fails the build
    /// before any generator emits the wrong number.
    fn validate_expected_offsets(&self) -> Result<(), Box<dyn std::error::Error>> {
        for s in &self.structs {
            let offsets = field_offsets(s, self);
            for (f, actual) in s.fields.iter().zip(offsets) {
                if let Some(expected) = f.expected_offset
                    && actual != expected as usize
                {
                    return Err(format!(
                        "{}.{} declares expected_offset {} but the layout puts it at {}",
                        s.name, f.name, expected, actual
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    /// Check that every declared generation-number field is really there and
    /// really a scalar.  `version_field` is what tells a reader where the
    /// number lives, so a name with no field behind it, or one naming an array
    /// or a pointer, leaves the structure with no generation at all.
    fn validate_version_fields(&self) -> Result<(), Box<dyn std::error::Error>> {
        for s in &self.structs {
            let Some(vf) = &s.version_field else {
                continue;
            };
            let Some(f) = s.fields.iter().find(|f| &f.name == vf) else {
                return Err(format!(
                    "{} declares version_field '{}' but has no field of that name",
                    s.name, vf
                )
                .into());
            };
            if f.kind != "scalar" {
                return Err(format!(
                    "{}.{} is the declared version_field but is a {}, not a scalar",
                    s.name, vf, f.kind
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check the `[[versions]]` table against the structures it describes.
    /// Every structure in [`VERSIONED_STRUCTS`] must exist and declare where
    /// its generation lives, which is what stops that list and the schema
    /// drifting apart.  And every entry must name a versioned structure
    /// exactly once per generation, that pair being how a release is looked
    /// up.
    fn validate_versions(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::HashSet;

        let mut filled: HashSet<&str> = HashSet::new();
        for s in &self.structs {
            match (&s.generation_slot, &s.version_field) {
                (Some(slot), Some(_)) => {
                    if !is_slot(slot) {
                        return Err(format!(
                            "{} declares generation_slot '{slot}', which is not one of {SLOT_ORDER:?}",
                            s.name
                        )
                        .into());
                    }
                    if !filled.insert(slot.as_str()) {
                        return Err(format!(
                            "{} and another structure both fill the {slot} slot",
                            s.name
                        )
                        .into());
                    }
                }
                (Some(slot), None) => {
                    return Err(format!(
                        "{} fills the {slot} slot but declares no version_field",
                        s.name
                    )
                    .into());
                }
                (None, Some(_)) => {
                    return Err(format!(
                        "{} declares a version_field but no generation_slot, so nothing says \
                         which generation it carries",
                        s.name
                    )
                    .into());
                }
                (None, None) => {}
            }
        }

        let mut seen: HashSet<(&str, u32)> = HashSet::new();
        for v in &self.versions {
            if self.slot_of(&v.struct_name).is_none() {
                return Err(format!(
                    "[[versions]] entry names '{}', which is not a versioned structure",
                    v.struct_name
                )
                .into());
            }
            if !seen.insert((v.struct_name.as_str(), v.version)) {
                return Err(format!(
                    "[[versions]] has more than one entry for {} version {}",
                    v.struct_name, v.version
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check every field's generation and deprecation markers.  A marker must
    /// name the structure owning the bytes the field sits in.  A
    /// `since_*_version` and a `default_if_absent` are two halves of one
    /// statement, so neither stands alone.  And a deprecation cannot precede
    /// the generation that introduced the field.
    fn validate_field_generations(&self) -> Result<(), Box<dyn std::error::Error>> {
        let owners = self.ownership();

        for (container, f) in self.all_fields() {
            let since = f.since_markers();
            let deprecated = f.deprecated_markers();

            // A marker names a slot and ownership answers with a structure,
            // so the two meet at the slot that structure fills. The message
            // names structures, which is what a schema author writes.
            for (named, _) in since.iter().chain(deprecated.iter()) {
                let named_struct = self
                    .struct_in_slot(named)
                    .map_or(*named, |s| s.name.as_str());
                match owners.get(container) {
                    Some(governor) if self.slot_of(governor) == Some(*named) => {}
                    Some(governor) => {
                        return Err(format!(
                            "{container}.{} names {named_struct} in a generation marker, but \
                             {container} is governed by {governor}",
                            f.name
                        )
                        .into());
                    }
                    None => {
                        return Err(format!(
                            "{container}.{} names {named_struct} in a generation marker, but \
                             {container} sits under no versioned structure",
                            f.name
                        )
                        .into());
                    }
                }
            }

            match (since.is_empty(), f.default_if_absent.is_some()) {
                (false, false) => {
                    return Err(format!(
                        "{container}.{} declares the generation it appeared in but no \
                         default_if_absent for a reader of an older structure",
                        f.name
                    )
                    .into());
                }
                (true, true) => {
                    return Err(format!(
                        "{container}.{} declares default_if_absent but no generation it appeared \
                         in, so nothing says when the default applies",
                        f.name
                    )
                    .into());
                }
                _ => {}
            }

            for (named, dep) in &deprecated {
                if let Some((_, first)) = since.iter().find(|(n, _)| n == named)
                    && dep < first
                {
                    let shown = self
                        .struct_in_slot(named)
                        .map_or(*named, |s| s.name.as_str());
                    return Err(format!(
                        "{container}.{} is deprecated from {shown} generation {dep} but only \
                         appeared in generation {first}",
                        f.name
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    /// Check that no field names more than one structure in its markers, and
    /// that its `since` and `deprecated` markers name the same one - they are
    /// the two ends of one field's life under one generation number.
    ///
    /// [`Schema::validate_field_generations`] would reject at least one of two
    /// names too, but only as a side effect of where the field sits.  This
    /// says the thing directly.
    fn validate_single_governor(&self) -> Result<(), Box<dyn std::error::Error>> {
        // A marker names a slot, and the message names the structure filling
        // it, which is what a schema author writes.
        let named = |slot: &'static str| {
            self.struct_in_slot(slot)
                .map_or(slot, |s| s.name.as_str())
                .to_string()
        };

        for (container, f) in self.all_fields() {
            let since = f.since_markers();
            let deprecated = f.deprecated_markers();

            if let [(first, _), (second, _), ..] = since[..] {
                let (first, second) = (named(first), named(second));
                return Err(format!(
                    "{container}.{} names both {first} and {second} in since markers, but one \
                     structure governs a field",
                    f.name
                )
                .into());
            }
            if let [(first, _), (second, _), ..] = deprecated[..] {
                let (first, second) = (named(first), named(second));
                return Err(format!(
                    "{container}.{} names both {first} and {second} in deprecated markers, but \
                     one structure governs a field",
                    f.name
                )
                .into());
            }
            if let ([(introduced, _)], [(retired, _)]) = (&since[..], &deprecated[..])
                && introduced != retired
            {
                let (introduced, retired) = (named(introduced), named(retired));
                return Err(format!(
                    "{container}.{} appeared in a generation of {introduced} but is deprecated \
                     from a generation of {retired}",
                    f.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check that a generation marker sits on a field the mechanism can carry.
    /// A marked field is reached through an accessor handing back either the
    /// stored bytes or `default_if_absent`, so there has to be something to
    /// hand back.  Padding has no reader, and a pointer whose type cannot say
    /// "nothing there" has nothing to be.
    fn validate_marker_kinds(&self) -> Result<(), Box<dyn std::error::Error>> {
        for (container, f) in self.all_fields() {
            if f.governor().is_none() {
                continue;
            }
            // A FAM states its own length in its bytes, so a reader needs no
            // generation to know what is there.  This is a field *inside* a
            // FAM - a pointer to one, held in an ordinary structure, is an
            // ordinary pointer field.
            if !self.structs.iter().any(|s| s.name == container) {
                return Err(format!(
                    "{container}.{} carries a generation marker, and {container} states its own \
                     length in its bytes rather than taking one from a generation",
                    f.name
                )
                .into());
            }
            if !MARKABLE_KINDS.contains(&f.kind.as_str()) {
                let why = UNMARKABLE_KINDS
                    .iter()
                    .find(|(kind, _)| *kind == f.kind)
                    .map(|(_, why)| (*why).to_string())
                    .unwrap_or_else(|| {
                        format!(
                            "a marker is carried by {} fields",
                            MARKABLE_KINDS.join(", ")
                        )
                    });
                return Err(format!(
                    "{container}.{} carries a generation marker on a {} field, and {why}",
                    f.name, f.kind,
                )
                .into());
            }
            if NULLABLE_REQUIRED_KINDS.contains(&f.kind.as_str()) && !f.nullable.unwrap_or(false) {
                return Err(format!(
                    "{container}.{} carries a generation marker on a {} that is not nullable, and \
                     a reader of an older structure has no way to say the field is not there",
                    f.name, f.kind,
                )
                .into());
            }
            if ARRAY_KINDS.contains(&f.kind.as_str())
                && f.element.as_deref().unwrap_or("u8") != "u8"
            {
                return Err(format!(
                    "{container}.{} carries a generation marker on an array of {}, and a default \
                     is written down for a u8 array only",
                    f.name,
                    f.element.as_deref().unwrap_or(""),
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check a gated array against the field that counts it.  A count gated on
    /// its own would give a length from a default while the array it measures
    /// is fully written, so a count is gated only alongside its array, at the
    /// same generation, with a default of 0.
    fn validate_gated_counts(&self) -> Result<(), Box<dyn std::error::Error>> {
        for s in &self.structs {
            for count in s.fields.iter().filter(|f| f.since_marker().is_some()) {
                let Some(array) = s
                    .fields
                    .iter()
                    .find(|f| f.count_field.as_deref() == Some(count.name.as_str()))
                else {
                    continue;
                };
                let at = count.since_marker().map(|(_, g)| g);
                if array.since_marker().map(|(_, g)| g) != at {
                    return Err(format!(
                        "{}.{} counts {} and is gated on generation {}, which {} is not",
                        s.name,
                        count.name,
                        array.name,
                        at.unwrap_or(0),
                        array.name,
                    )
                    .into());
                }
                if count
                    .default_if_absent
                    .as_ref()
                    .and_then(|v| v.as_integer())
                    != Some(0)
                {
                    return Err(format!(
                        "{}.{} counts {}, which is not there below generation {}, so its \
                         default_if_absent is 0",
                        s.name,
                        count.name,
                        array.name,
                        at.unwrap_or(0),
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    /// Check that each `default_if_absent` is a value the field can hold.  The
    /// default is emitted as a literal into C and into Rust, so one that does
    /// not fit would surface as a compile error in generated code, naming a
    /// line nobody wrote.
    ///
    /// Three forms, one per shape of field:
    ///
    /// - A **scalar, enum or type_alias** takes a whole number its type can
    ///   hold, and an enum's names one of its own variants.
    /// - An **array** keeps its bytes whatever the generation, so what is
    ///   absent is anything having been written there.  It takes a whole
    ///   number filling every element, or a list stating them: flat for
    ///   `inline_array`, a row per entry for `inline_array2d`.
    /// - A **pointer** takes `"null"`, what its bytes hold when nobody wrote
    ///   them.
    fn validate_defaults(&self) -> Result<(), Box<dyn std::error::Error>> {
        for (container, f) in self.all_fields() {
            let Some(value) = &f.default_if_absent else {
                continue;
            };
            if POINTER_KINDS.contains(&f.kind.as_str()) {
                if value.as_str() != Some(NULL_DEFAULT) {
                    return Err(format!(
                        "{container}.{} has default_if_absent {value}, and a {} that is not there \
                         is \"{NULL_DEFAULT}\"",
                        f.name, f.kind,
                    )
                    .into());
                }
                continue;
            }
            if ARRAY_KINDS.contains(&f.kind.as_str()) {
                check_array_default(container, f, value)?;
                continue;
            }
            let Some(number) = value.as_integer() else {
                return Err(format!(
                    "{container}.{} has default_if_absent {value}, and a default is a whole \
                     number",
                    f.name
                )
                .into());
            };
            self.check_default_fits(container, f, number)?;
        }
        Ok(())
    }

    /// The half of [`Schema::validate_defaults`] that needs the field's type.
    fn check_default_fits(
        &self,
        container: &str,
        f: &Field,
        number: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let primitive = match f.kind.as_str() {
            "scalar" => f.type_.as_deref().unwrap_or("u8"),
            "type_alias" => {
                let named = f.type_.as_deref().unwrap_or("");
                let Some(alias) = self.type_aliases.iter().find(|a| a.name == named) else {
                    return Err(format!(
                        "{container}.{} is a {named}, and no type alias of that name is declared",
                        f.name
                    )
                    .into());
                };
                alias.underlying.as_str()
            }
            "enum" => {
                let named = f.type_.as_deref().unwrap_or("");
                let Some(e) = self.enums.iter().find(|e| e.name == named) else {
                    return Err(format!(
                        "{container}.{} is a {named}, and no enum of that name is declared",
                        f.name
                    )
                    .into());
                };
                // Any other number leaves the parser handing back a value its
                // own TryFrom refuses.
                if !e.variants.iter().any(|v| v.value == number) {
                    return Err(format!(
                        "{container}.{} has default_if_absent {number}, which is not a variant of \
                         {named}",
                        f.name
                    )
                    .into());
                }
                return Ok(());
            }
            _ => return Ok(()),
        };

        let limit: i64 = match primitive {
            "u8" => u8::MAX as i64,
            "u16" => u16::MAX as i64,
            "u32" => u32::MAX as i64,
            other => {
                return Err(format!(
                    "{container}.{} is a {other}, which carries no default_if_absent",
                    f.name
                )
                .into());
            }
        };
        if number < 0 || number > limit {
            return Err(format!(
                "{container}.{} has default_if_absent {number}, which a {primitive} cannot hold",
                f.name
            )
            .into());
        }
        Ok(())
    }

    /// Check that a `since` marker's generation is readable by the time the
    /// field it guards is parsed.
    ///
    /// The parser reads a structure's generation out of the field its
    /// `version_field` names.  A gated field declared ahead of that one would
    /// be gated on a generation nothing had read yet.  A field of a structure
    /// further down the tree is unaffected - its generation arrives as an
    /// argument, already read by the root.
    ///
    /// The layout puts the generation near the front and new fields at the
    /// back, so this holds by construction today - and construction is what
    /// changes.
    fn validate_marker_order(&self) -> Result<(), Box<dyn std::error::Error>> {
        for s in &self.structs {
            let Some(vf) = &s.version_field else {
                continue;
            };
            let Some(version_at) = s.fields.iter().position(|f| &f.name == vf) else {
                continue;
            };
            for (at, f) in s.fields.iter().enumerate() {
                if f.since_marker().is_none() || at > version_at {
                    continue;
                }
                return Err(format!(
                    "{}.{} is gated on a generation but is declared at or before {}.{}, which \
                     the parser reads that generation from",
                    s.name, f.name, s.name, vf
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check that every release string in the file is one.  Nothing else
    /// constrains them, so "soon" would sit there until it surfaced as a
    /// nonsensical `@since firmware ...` line.
    fn validate_release_strings(&self) -> Result<(), Box<dyn std::error::Error>> {
        let bad = |what: String, value: &str| -> Box<dyn std::error::Error> {
            format!("{what} is '{value}', which is not a firmware release like 0.8.0").into()
        };

        if !is_release(&self.schema.firmware_release) {
            return Err(bad(
                "[schema] firmware_release".to_string(),
                &self.schema.firmware_release,
            ));
        }
        for v in &self.versions {
            if !is_release(&v.first_release) {
                return Err(bad(
                    format!(
                        "first_release on the [[versions]] entry for {} version {}",
                        v.struct_name, v.version
                    ),
                    &v.first_release,
                ));
            }
        }
        for c in &self.constants {
            if let Some(release) = &c.first_release
                && !is_release(release)
            {
                return Err(bad(
                    format!("first_release on constant {}", c.name),
                    release,
                ));
            }
            if let Some(release) = &c.deprecated_release
                && !is_release(release)
            {
                return Err(bad(
                    format!("deprecated_release on constant {}", c.name),
                    release,
                ));
            }
        }
        for entry in self.plugin_keys() {
            if !is_release(&entry.key.first_release) {
                return Err(bad(
                    format!("first_release on plugin key {}", entry.key.name),
                    &entry.key.first_release,
                ));
            }
        }
        Ok(())
    }

    /// Check that a constant's `first_release` and its `ora_api` tag agree.
    /// Without `first_release` a plugin author has nothing to set
    /// `min_fw_version` from.  Without `ora_api` there is no release for it to
    /// name, and every generator would ignore it, leaving an unchecked
    /// statement in the file.
    fn validate_constant_releases(&self) -> Result<(), Box<dyn std::error::Error>> {
        for c in &self.constants {
            if c.ora_api && c.first_release.is_none() {
                return Err(format!(
                    "constant {} is in the plugin API but declares no first_release",
                    c.name
                )
                .into());
            }
            if !c.ora_api && c.first_release.is_some() {
                return Err(format!(
                    "constant {} declares first_release but is not in the plugin API, so there \
                     is no release for it to name",
                    c.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check that a constant is not retired before it arrives.  The two
    /// releases are the ends of one constant's life in the plugin API:
    /// `first_release` is the oldest firmware a build using it can ask for,
    /// `deprecated_release` the point from which a new build should not.
    /// Named the other way round there is no such firmware.
    fn validate_constant_deprecations(&self) -> Result<(), Box<dyn std::error::Error>> {
        for c in &self.constants {
            let (Some(first), Some(deprecated)) = (&c.first_release, &c.deprecated_release) else {
                continue;
            };
            // validate_release_strings has already refused a malformed
            // release, so both parse.
            if release_parts(deprecated) <= release_parts(first) {
                return Err(format!(
                    "constant {} is deprecated from {deprecated} and reached the plugin API in \
                     {first} - a constant is retired in a later release than the one it arrived \
                     in",
                    c.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// Check that every `linker_script` constant is a number, since a linker
    /// script has no strings.
    fn validate_linker_constants(&self) -> Result<(), Box<dyn std::error::Error>> {
        for c in self.linker_constants() {
            if matches!(c.value, ConstantValue::Text(_)) {
                return Err(format!(
                    "constant {} is marked linker_script but holds text, and a linker script \
                     takes only numbers",
                    c.name
                )
                .into());
            }
        }
        Ok(())
    }

    /// Every field in the schema, paired with the name of the struct or FAM
    /// it is written in.
    pub fn all_fields(&self) -> Vec<(&str, &Field)> {
        let mut out = Vec::new();
        for s in &self.structs {
            out.extend(s.fields.iter().map(|f| (s.name.as_str(), f)));
        }
        for t in &self.tagged_fams {
            out.extend(t.common_fields.iter().map(|f| (t.name.as_str(), f)));
            for v in &t.variants {
                out.extend(v.fields.iter().map(|f| (t.name.as_str(), f)));
            }
        }
        out
    }

    /// The metadata header - the structure a generated accessor reads a
    /// generation from.  [`Schema::validate_root`] has already refused a
    /// schema where it is missing.
    pub fn metadata_header(&self) -> Option<&Struct> {
        self.struct_in_slot(SLOT_METADATA)
    }

    /// The structure filling `slot`, if the schema declares one.
    pub fn struct_in_slot(&self, slot: &str) -> Option<&Struct> {
        self.structs
            .iter()
            .find(|s| s.generation_slot.as_deref() == Some(slot))
    }

    /// The slot `struct_name` fills, if it carries a generation.
    pub fn slot_of(&self, struct_name: &str) -> Option<&str> {
        self.structs
            .iter()
            .find(|s| s.name == struct_name)
            .and_then(|s| s.generation_slot.as_deref())
    }

    /// Every structure carrying a generation, with the slot it fills, in
    /// [`SLOT_ORDER`].
    pub fn generation_slots(&self) -> Vec<(&str, &str)> {
        SLOT_ORDER
            .iter()
            .filter_map(|slot| self.struct_in_slot(slot).map(|s| (s.name.as_str(), *slot)))
            .collect()
    }

    /// Every structure carrying a generation, in [`OWNERSHIP_SLOT_ORDER`].
    fn ownership_roots(&self) -> Vec<&str> {
        OWNERSHIP_SLOT_ORDER
            .iter()
            .filter_map(|slot| self.struct_in_slot(slot).map(|s| s.name.as_str()))
            .collect()
    }

    /// Every structure carrying a generation, in [`SLOT_ORDER`].
    pub fn versioned_structs(&self) -> Vec<&str> {
        self.generation_slots()
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    /// Every field the metadata generation gates, paired with the struct it
    /// sits in, in declaration order.
    pub fn metadata_gated_fields(&self) -> Vec<(&Struct, &Field)> {
        self.structs
            .iter()
            .flat_map(|s| s.fields.iter().map(move |f| (s, f)))
            .filter(|(_, f)| f.metadata_since().is_some())
            .collect()
    }

    /// Every metadata generation with the release it first shipped in, oldest
    /// first.  The metadata is the one structure a host writes, so it is the
    /// only one whose release-to-generation table anything outside this crate
    /// reads - a tool composing an image for a given firmware has to know what
    /// that firmware's metadata can hold.
    pub fn metadata_generations(&self) -> Vec<(u32, (u32, u32, u32))> {
        let mut out: Vec<(u32, (u32, u32, u32))> = self
            .versions
            .iter()
            .filter(|v| self.slot_of(&v.struct_name) == Some(SLOT_METADATA))
            .map(|v| {
                let release = release_parts(&v.first_release)
                    .expect("validate_release_strings has already refused a malformed release");
                (v.version, release)
            })
            .collect();
        out.sort_by_key(|(_, release)| *release);
        out
    }

    /// The release metadata `generation` first shipped in.
    ///
    /// `None` where no `[[versions]]` entry names that generation, which is
    /// how a marker citing a generation nobody declared is caught.
    pub fn metadata_generation_release(&self, generation: u32) -> Option<(u32, u32, u32)> {
        self.metadata_generations()
            .into_iter()
            .find(|(g, _)| *g == generation)
            .map(|(_, release)| release)
    }

    /// The versioned structure whose generation governs each type in the
    /// schema, by type name.  A structure belongs to the tree whose root
    /// writes its bytes, so ownership is assigned root by root in
    /// [`OWNERSHIP_ORDER`], each root taking everything it reaches that no
    /// earlier root has taken.  A type nothing reaches is absent from the
    /// map.
    pub fn ownership(&self) -> HashMap<&str, &str> {
        let mut owners = HashMap::new();
        for root in self.ownership_roots() {
            self.claim(root, root, &mut owners);
        }
        owners
    }

    /// Give `current` and everything hanging off it to `root`, leaving alone
    /// what an earlier root already took.  The already-taken check also stops
    /// the walk looping on a cycle.
    fn claim<'a>(
        &'a self,
        current: &'a str,
        root: &'a str,
        owners: &mut HashMap<&'a str, &'a str>,
    ) {
        if owners.contains_key(current) {
            return;
        }
        owners.insert(current, root);

        for f in self.type_fields(current) {
            let Some(referenced) = f.referenced_type() else {
                continue;
            };
            // A versioned structure is a root in its own right, so the walk
            // stops rather than claim it for whatever points at it.
            if self.slot_of(referenced).is_some() {
                continue;
            }
            self.claim(referenced, root, owners);
        }
    }

    /// The fields of a named struct or tagged FAM, in declaration order.
    /// Empty for a simple FAM, which has none, and for an unknown name.
    fn type_fields(&self, name: &str) -> Vec<&Field> {
        if let Some(s) = self.structs.iter().find(|s| s.name == name) {
            return s.fields.iter().collect();
        }
        if let Some(t) = self.tagged_fams.iter().find(|t| t.name == name) {
            return t
                .common_fields
                .iter()
                .chain(t.variants.iter().flat_map(|v| v.fields.iter()))
                .collect();
        }
        Vec::new()
    }

    /// Every plugin-exposed metadata key, paired with its field comment,
    /// sorted by id.  Drives generation of the plugin-facing key header.
    pub fn plugin_keys(&self) -> Vec<PluginKeyEntry<'_>> {
        let mut keys = Vec::new();
        for s in &self.structs {
            for f in &s.fields {
                if let Some(pk) = &f.plugin_key {
                    keys.push(PluginKeyEntry {
                        key: pk,
                        comment: f.comment.as_deref(),
                        struct_name: &s.name,
                        field_name: &f.name,
                        kind: &f.kind,
                        count: f.count,
                        count_ref: f.count_ref.as_deref(),
                    });
                }
            }
        }
        keys.sort_by_key(|e| e.key.id);
        keys
    }

    /// Enforce the key-space invariants: ids 0x00000000 and 0xFFFFFFFF are
    /// reserved sentinels, and both ids and names must be unique across the
    /// whole schema so a key value identifies exactly one datum.
    fn validate_plugin_keys(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::collections::HashMap;
        let mut ids: HashMap<u32, String> = HashMap::new();
        let mut names: HashMap<String, u32> = HashMap::new();
        for s in &self.structs {
            for f in &s.fields {
                if let Some(pk) = &f.plugin_key {
                    if pk.id == 0x0000_0000 || pk.id == 0xFFFF_FFFF {
                        return Err(format!(
                            "plugin_key '{}' uses reserved id 0x{:08X}",
                            pk.name, pk.id
                        )
                        .into());
                    }
                    if let Some(prev) = ids.insert(pk.id, pk.name.clone()) {
                        return Err(format!(
                            "plugin_key id 0x{:08X} used by both '{}' and '{}'",
                            pk.id, prev, pk.name
                        )
                        .into());
                    }
                    if names.insert(pk.name.clone(), pk.id).is_some() {
                        return Err(
                            format!("plugin_key name '{}' used more than once", pk.name).into()
                        );
                    }
                    // String and array keys resolve a stored value, so their
                    // access path from the metadata root must exist.  Fail the
                    // build now (with a clear message) rather than emit a
                    // broken path.
                    if f.kind == "cstr_ptr" || f.kind == "inline_array" {
                        self.plugin_key_access(&s.name, &f.name)?;
                    }
                    // The indexed getter bounds-checks against the array's
                    // length, so an array key without one cannot be generated.
                    if f.kind == "inline_array" && f.count.is_none() {
                        return Err(format!(
                            "plugin_key '{}' is on inline_array {}.{}, which has no count",
                            pk.name, s.name, f.name
                        )
                        .into());
                    }
                }
            }
        }
        Ok(())
    }

    /// C access expression for a plugin-key field, e.g. "METADATA->fw->name"
    /// or "RUNTIME->status_led_enabled".
    ///
    /// Walks the struct-pointer graph from each known firmware root (METADATA,
    /// then RUNTIME) down to the struct that contains the field, then appends
    /// the field itself.  Every hop is a pointer, so every step joins with "->".
    pub fn plugin_key_access(
        &self,
        struct_name: &str,
        field_name: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Plugin-key fields resolve from one of a fixed set of firmware roots,
        // each reachable in plugin.c via an access macro (see macros.h). The
        // metadata-header root is tried first so existing keys keep their exact
        // output; fields in onerom_runtime_info_t (live state such as
        // status_led_enabled) resolve via the RUNTIME root instead.
        let roots: [(&str, &str); 2] = [
            (self.schema.root_struct.as_str(), "METADATA"),
            ("onerom_runtime_info_t", "RUNTIME"),
        ];
        // A gated field is reached through its accessor, so a plugin reading
        // older metadata gets the field's default rather than whatever the
        // bytes at that offset hold.
        let gated = self
            .structs
            .iter()
            .find(|s| s.name == struct_name)
            .and_then(|s| s.fields.iter().find(|f| f.name == field_name))
            .is_some_and(|f| f.metadata_since().is_some());

        for (root, macro_name) in roots {
            let mut path = Vec::new();
            if self.find_struct_path(root, struct_name, &mut Vec::new(), &mut path) {
                let mut expr = String::from(macro_name);
                for step in &path {
                    expr.push_str("->");
                    expr.push_str(step);
                }
                if gated {
                    let accessor = format!("{}_{field_name}", strip_type_suffix(struct_name));
                    return Ok(match expr.as_str() {
                        "METADATA" => format!("{accessor}(METADATA)"),
                        object => format!("{accessor}(METADATA, {object})"),
                    });
                }
                expr.push_str("->");
                expr.push_str(field_name);
                return Ok(expr);
            }
        }
        Err(format!(
            "plugin_key on {struct_name}.{field_name}: struct '{struct_name}' is not \
             reachable from any plugin-key root (METADATA, RUNTIME) via struct pointers"
        )
        .into())
    }

    /// Depth-first search of the struct-pointer graph from `current` to
    /// `target`, recording the pointer field names taken.  Returns true and
    /// fills `path` on success; `visited` guards against cycles.
    fn find_struct_path(
        &self,
        current: &str,
        target: &str,
        visited: &mut Vec<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if current == target {
            return true;
        }
        if visited.iter().any(|v| v == current) {
            return false;
        }
        visited.push(current.to_string());

        let Some(s) = self.structs.iter().find(|s| s.name == current) else {
            return false;
        };
        for f in &s.fields {
            // Only single struct pointers form a resolvable single-value path.
            if f.kind == "struct_ptr"
                && let Some(ty) = &f.type_
            {
                path.push(f.name.clone());
                if self.find_struct_path(ty, target, visited, path) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Shared size helpers
// ---------------------------------------------------------------------------

/// Byte size of a primitive type string ("u8", "u16", "u32", "char").
pub fn prim_size(type_: &str) -> usize {
    match type_ {
        "u8" | "char" => 1,
        "u16" => 2,
        "u32" => 4,
        _ => 0,
    }
}

/// A field reduced to what a layout walk needs: its kind, the type names it
/// refers to, and its dimensions.  The released-schema view builds these too,
/// so one walk sizes both schemas and a comparison cannot report a difference
/// that is really two walks disagreeing.
pub struct FieldShape<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub type_: Option<&'a str>,
    pub element: Option<&'a str>,
    pub count: Option<u32>,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub size: Option<u32>,
}

/// Byte sizes of the named types a field can refer to.  An `enum` or
/// `type_alias` field is sized by its declaration elsewhere in the same
/// document, and the two schema views hold theirs differently.
pub trait NamedSizes {
    /// Byte size of the named `[[enums]]` entry.
    fn enum_size(&self, name: &str) -> Option<usize>;
    /// Byte size of the named `[[type_aliases]]` entry.
    fn alias_size(&self, name: &str) -> Option<usize>;
}

impl NamedSizes for Schema {
    fn enum_size(&self, name: &str) -> Option<usize> {
        self.enums
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.size as usize)
    }

    fn alias_size(&self, name: &str) -> Option<usize> {
        self.type_aliases
            .iter()
            .find(|a| a.name == name)
            .map(|a| prim_size(&a.underlying))
    }
}

/// Byte size of a field.  Used for layout offset tracking in generated C
/// comments and for Rust struct layout verification.
pub fn shape_size(shape: &FieldShape, named: &dyn NamedSizes) -> usize {
    match shape.kind {
        "scalar" => prim_size(shape.type_.unwrap_or("u8")),
        "enum" => shape.type_.and_then(|n| named.enum_size(n)).unwrap_or(1),
        "type_alias" => shape.type_.and_then(|n| named.alias_size(n)).unwrap_or(2),
        "inline_array" => {
            prim_size(shape.element.unwrap_or("u8")) * shape.count.unwrap_or(0) as usize
        }
        "inline_array2d" => {
            prim_size(shape.element.unwrap_or("u8"))
                * shape.rows.unwrap_or(0) as usize
                * shape.cols.unwrap_or(0) as usize
        }
        "cstr_ptr"
        | "struct_ptr"
        | "struct_array_ptr"
        | "struct_ptr_array_ptr"
        | "tagged_fam_ptr"
        | "simple_fam_ptr"
        | "opaque_ptr"
        | "fn_ptr" => 4,
        "padding" => shape.size.unwrap_or(0) as usize,
        _ => 0,
    }
}

/// How a field's type reads when one schema's layout is compared with
/// another's, e.g. "scalar u32" or "inline_array u8 x 4".  Everything that
/// decides how the bytes are read is in here, and nothing that does not - a
/// comment or a nullable flag moving is not the type changing.
pub fn shape_type(shape: &FieldShape) -> String {
    match shape.kind {
        "inline_array" => format!(
            "inline_array {} x {}",
            shape.element.unwrap_or("u8"),
            shape.count.unwrap_or(0)
        ),
        "inline_array2d" => format!(
            "inline_array2d {} x {} x {}",
            shape.element.unwrap_or("u8"),
            shape.rows.unwrap_or(0),
            shape.cols.unwrap_or(0)
        ),
        kind => match shape.type_.or(shape.element) {
            Some(named) => format!("{kind} {named}"),
            None => kind.to_string(),
        },
    }
}

/// Byte size of a struct field.  Used for layout offset tracking in
/// generated C comments and for Rust struct layout verification.
pub fn field_size(field: &Field, schema: &Schema) -> usize {
    shape_size(&field.shape(), schema)
}

/// Total byte stride of a named struct type.
///
/// Uses the explicit `size` field from the schema when present; otherwise
/// sums `field_size` for every field (including padding).  Returns 0 if
/// the type name is not found in the schema.
///
/// Shared between rust_gen.rs (parse codegen) and serialize_gen.rs.
pub fn struct_stride(c_type: &str, schema: &Schema) -> usize {
    schema
        .structs
        .iter()
        .find(|s| s.name == c_type)
        .map_or(0, |s| {
            s.size
                .map(|n| n as usize)
                .unwrap_or_else(|| s.fields.iter().map(|f| field_size(f, schema)).sum())
        })
}
