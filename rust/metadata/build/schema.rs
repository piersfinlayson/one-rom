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
/// A plugin-facing metadata key.
///
/// When attached to a struct field via `plugin_key`, that field is exposed to
/// plugins through the metadata getter API under this key.  `id` is the stable,
/// permanent enum value: once assigned it must never be renumbered or reused.
#[derive(Deserialize, Debug, Clone)]
pub struct PluginKey {
    pub name: String,
    pub id: u32,
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
}

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

    pub none_on_parse_error: Option<bool>,

    /// Plugin-facing metadata key.  When set, this field is exposed to plugins
    /// through the metadata getter API under the given key name and id.
    pub plugin_key: Option<PluginKey>,
}

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
        schema.validate_plugin_keys()?;
        Ok(schema)
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
                    // String keys resolve a stored value, so their access path
                    // from the metadata root must exist.  Fail the build now
                    // (with a clear message) rather than emit a broken path.
                    if f.kind == "cstr_ptr" {
                        self.plugin_key_access(&s.name, &f.name)?;
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
        for (root, macro_name) in roots {
            let mut path = Vec::new();
            if self.find_struct_path(root, struct_name, &mut Vec::new(), &mut path) {
                let mut expr = String::from(macro_name);
                for step in &path {
                    expr.push_str("->");
                    expr.push_str(step);
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
