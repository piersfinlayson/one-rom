// build/schema.rs
//
// Serde-deserializable types mirroring the OneROM metadata TOML schema,
// plus shared size-computation helpers used by all code generators.

use serde::Deserialize;
use std::path::Path;

// ---------------------------------------------------------------------------
// Top-level document
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
pub struct Schema {
    pub schema: SchemaMetadata,
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

// Fields version, flash_base, and root_struct are read by c_gen.rs;
// the dead_code lint does not trace usage across all build-script modules.
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct SchemaMetadata {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub flash_base: u32,
    pub metadata_base: u32,
    pub metadata_size: u32,
    pub root_struct: String,
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
pub struct Constant {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub value: ConstantValue,
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// [[type_aliases]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
pub struct TypeAlias {
    pub name: String,
    pub underlying: String,
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// [[enums]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
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

/// A field within a [[structs]] definition or a [[tagged_fams]] common/variant
/// section.  Uses a flat layout: all optional members are None when
/// inapplicable to the field's `kind`.
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

    pub expected_const: Option<String>,

    pub none_on_parse_error: Option<bool>,}

// ---------------------------------------------------------------------------
// [[structs]]
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
pub struct Struct {
    pub name: String,
    pub comment: Option<String>,
    pub generate: Generate,
    /// Expected total byte size for STATIC_ASSERT (absent if no assertion in original C).
    pub size: Option<u32>,
    /// true = this struct is placed at metadata_base (the root of the generated region).
    #[allow(dead_code)]
    pub root: Option<bool>,
    /// false = fields are non-const (runtime-written structs such as onerom_runtime_info_t).
    /// Defaults to true.
    pub const_fields: Option<bool>,
    #[serde(default)]
    pub fields: Vec<Field>,
}

impl Struct {
    pub fn has_const_fields(&self) -> bool {
        self.const_fields.unwrap_or(true)
    }
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
pub struct SimpleFam {
    pub name: String,
    pub comment: Option<String>,
    pub generate: Generate,
    pub param_len_field: String,
}

// ---------------------------------------------------------------------------
// Schema loading
// ---------------------------------------------------------------------------

impl Schema {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let schema: Schema = toml::from_str(&content)?;
        Ok(schema)
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

/// Byte size of a struct field.  Used for layout offset tracking in
/// generated C comments and for Rust struct layout verification.
pub fn field_size(field: &Field, schema: &Schema) -> usize {
    match field.kind.as_str() {
        "scalar" => prim_size(field.type_.as_deref().unwrap_or("u8")),
        "enum" => schema
            .enums
            .iter()
            .find(|e| field.type_.as_deref() == Some(e.name.as_str()))
            .map(|e| e.size as usize)
            .unwrap_or(1),
        "type_alias" => schema
            .type_aliases
            .iter()
            .find(|a| field.type_.as_deref() == Some(a.name.as_str()))
            .map(|a| prim_size(&a.underlying))
            .unwrap_or(2),
        "inline_array" => {
            prim_size(field.element.as_deref().unwrap_or("u8")) * field.count.unwrap_or(0) as usize
        }
        "inline_array2d" => {
            prim_size(field.element.as_deref().unwrap_or("u8"))
                * field.rows.unwrap_or(0) as usize
                * field.cols.unwrap_or(0) as usize
        }
        "cstr_ptr"
        | "struct_ptr"
        | "struct_array_ptr"
        | "struct_ptr_array_ptr"
        | "tagged_fam_ptr"
        | "simple_fam_ptr"
        | "opaque_ptr"
        | "fn_ptr" => 4,
        "padding" => field.size.unwrap_or(0) as usize,
        _ => 0,
    }
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
