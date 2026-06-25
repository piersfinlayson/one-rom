// build/c_gen.rs
//
// Generates a single C header file from the OneROM metadata schema.
// Replaces enums.h, config_base.h, and alg.h.
//
// Output order:
//   file header + include guard
//   forward declarations  (all struct / tagged_fam / simple_fam types)
//   type aliases          (typedef uint16_t …)
//   constants             (#define …)
//   enums                 (typedef enum … + STATIC_ASSERT)
//   structs               (typedef struct … + STATIC_ASSERT)
//   tagged FAM structs    (main struct + per-variant param structs + STATIC_ASSERTs)
//   simple FAM structs    (length-prefixed byte-array structs)
//   header guard close

use onerom_config::chip::{CHIP_TYPES, ChipType};

use crate::schema::{ConstantValue, Field, Schema, SimpleFam, Struct, TaggedFam, field_size};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate the C header.
pub fn generate(schema: &Schema) -> String {
    let mut out = String::with_capacity(65536);
    emit_file_header(schema, &mut out);
    emit_forward_declarations(schema, &mut out);
    emit_type_aliases(schema, &mut out);
    emit_constants(schema, &mut out);
    emit_enums(schema, &mut out);
    emit_struct_defs(schema, &mut out);
    emit_tagged_fam_defs(schema, &mut out);
    emit_simple_fam_defs(schema, &mut out);
    emit_file_footer(schema, &mut out);
    out
}

// ---------------------------------------------------------------------------
// File header / footer
// ---------------------------------------------------------------------------

fn emit_file_header(schema: &Schema, out: &mut String) {
    let guard = header_guard(schema);
    out.push_str(&format!(
        "\
// {name}
//
// {desc}
//
// Metadata base address: 0x{base:08X}  ({size} bytes reserved)
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: firmware/metadata_schema.toml

#ifndef {guard}
#define {guard}

#if __STDC_VERSION__ < 199901L
#error \"C99 or later required\"
#endif

#include <stdint.h>
#include <stddef.h>
#include \"macros.h\"

",
        name = schema.schema.name,
        desc = schema.schema.description,
        base = schema.schema.metadata_base,
        size = schema.schema.metadata_size,
        guard = guard,
    ));
}

fn emit_file_footer(schema: &Schema, out: &mut String) {
    out.push_str(&format!("#endif // {}\n", header_guard(schema)));
}

fn header_guard(schema: &Schema) -> String {
    format!("{}_H", schema.schema.name.to_uppercase().replace(' ', "_"))
}

// ---------------------------------------------------------------------------
// Forward declarations
// ---------------------------------------------------------------------------

fn emit_forward_declarations(schema: &Schema, out: &mut String) {
    emit_major_section_header("Forward declarations", out);
    for s in &schema.structs {
        out.push_str(&format!("typedef struct {0} {0};\n", s.name));
    }
    for t in &schema.tagged_fams {
        out.push_str(&format!("typedef struct {0} {0};\n", t.name));
    }
    for s in &schema.simple_fams {
        out.push_str(&format!("typedef struct {0} {0};\n", s.name));
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

fn emit_type_aliases(schema: &Schema, out: &mut String) {
    if schema.type_aliases.is_empty() {
        return;
    }
    emit_major_section_header("Type aliases", out);
    for a in &schema.type_aliases {
        emit_item_header(a.comment.as_deref(), out);
        out.push_str(&format!(
            "typedef {} {};\n\n",
            c_primitive(&a.underlying),
            a.name
        ));
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

fn emit_constants(schema: &Schema, out: &mut String) {
    if schema.constants.is_empty() {
        return;
    }
    emit_major_section_header("Constants", out);
    for c in &schema.constants {
        if let Some(comment) = &c.comment {
            emit_comment(comment, "", out);
        }
        out.push_str(&format!(
            "#define {} {}\n\n",
            c.name,
            format_const_value(&c.value, &c.type_)
        ));
    }
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

fn emit_enums(schema: &Schema, out: &mut String) {
    if schema.enums.is_empty() {
        return;
    }
    emit_major_section_header("Enums", out);
    for e in &schema.enums {
        emit_enum(e, out);
    }
}

fn emit_enum(e: &crate::schema::Enum, out: &mut String) {
    if e.source.as_deref() == Some("rbcp_chip_types") {
        emit_rbcp_chip_type_enum(e, out);
        return;
    }

    emit_item_header(e.comment.as_deref(), out);

    let packed = if e.packed.unwrap_or(false) {
        " __attribute__((packed))"
    } else {
        ""
    };
    out.push_str(&format!("typedef enum{} {{\n", packed));

    for v in &e.variants {
        let val_str = format_enum_value(v.value, e.size);
        let comment_str = v
            .comment
            .as_deref()
            .and_then(|c| c.lines().next())
            .map(|l| format!("  // {}", l))
            .unwrap_or_default();
        out.push_str(&format!("    {} = {},{}\n", v.name, val_str, comment_str));
    }

    out.push_str(&format!("}} {};\n", e.name));
    out.push_str(&format!(
        "STATIC_ASSERT(sizeof({name}) == {size}, \
         \"{name} must be {size} {unit}\");\n",
        name = e.name,
        size = e.size,
        unit = bytes_str(e.size),
    ));

    for alias in &e.aliases {
        if let Some(c) = &alias.comment {
            emit_comment(c, "", out);
        }
        out.push_str(&format!("#define {} {}\n", alias.name, alias.target));
    }

    out.push('\n');
}

// ---------------------------------------------------------------------------
// RBCP chip type enum (sourced from onerom_config::chip::ChipType)
// ---------------------------------------------------------------------------

/// Emit the `onerom_rom_type_t` typedef enum and the chip size array.
///
/// Canonical variants are those for which `ChipType::try_from_rbcp_u8` returns
/// the same chip — i.e. they are the primary holder of their RBCP value.
/// Alias chips (e.g. `Chip23C1010`, which shares value 15 with `Chip27C010`)
/// are detected by the same test and emitted as `#define` directives rather
/// than enum variants.
///
/// `c_enum_name()` is used directly for all C constant names; no conversion
/// is needed.
fn emit_rbcp_chip_type_enum(e: &crate::schema::Enum, out: &mut String) {
    emit_item_header(e.comment.as_deref(), out);

    // Canonical chips: try_from_rbcp_u8 round-trips back to self.
    let mut canonical: Vec<ChipType> = CHIP_TYPES
        .iter()
        .copied()
        .filter(|ct| ChipType::try_from_rbcp_u8(ct.rbcp_chip_type()) == Some(*ct))
        .collect();
    canonical.sort_by_key(|ct| ct.rbcp_chip_type());

    // Alias chips: try_from_rbcp_u8 returns a different (canonical) chip.
    let aliases: Vec<ChipType> = CHIP_TYPES
        .iter()
        .copied()
        .filter(|ct| ChipType::try_from_rbcp_u8(ct.rbcp_chip_type()) != Some(*ct))
        .collect();

    let num_chip_types: u32 = canonical
        .iter()
        .map(|ct| ct.rbcp_chip_type() as u32)
        .max()
        .unwrap_or(0)
        + 1;

    // Enum body.
    out.push_str("typedef enum {\n");
    for ct in &canonical {
        let val_str = format_enum_value(ct.rbcp_chip_type() as i64, e.size);
        out.push_str(&format!(
            "    {} = {},  // {} ({} bytes)\n",
            ct.c_enum_name(),
            val_str,
            ct.name(),
            ct.size_bytes(),
        ));
    }
    out.push_str(&format!(
        "    NUM_CHIP_TYPES = {},  // Count of defined chip types. Not a valid chip type.\n",
        num_chip_types
    ));
    out.push_str("    INVALID_CHIP_TYPE = 0xFF,  // Invalid or unset chip type\n");
    out.push_str(&format!("}} {};\n", e.name));

    // STATIC_ASSERT on enum storage size.
    out.push_str(&format!(
        "STATIC_ASSERT(sizeof({name}) == {size}, \"{name} must be {size} {unit}\");\n",
        name = e.name,
        size = e.size,
        unit = bytes_str(e.size),
    ));

    // #define for each alias chip.
    for alias in &aliases {
        let primary = ChipType::try_from_rbcp_u8(alias.rbcp_chip_type())
            .expect("alias must resolve to a canonical chip");
        out.push_str(&format!(
            "// {} is electrically equivalent to {}\n",
            alias.name(),
            primary.name()
        ));
        out.push_str(&format!(
            "#define {} {}\n",
            alias.c_enum_name(),
            primary.c_enum_name()
        ));
    }

    out.push('\n');

    emit_chip_type_sizes_array(&canonical, out);
}

/// Emit the `onerom_chip_type_sizes[]` designated-initialiser array.
///
/// Indexed by `onerom_rom_type_t` value (0 … NUM_CHIP_TYPES-1).
/// Alias chips are excluded — they share an index with their canonical
/// equivalent.  Plugin types are included with their size (65536).
fn emit_chip_type_sizes_array(canonical: &[ChipType], out: &mut String) {
    out.push_str("extern const uint32_t onerom_chip_type_sizes[];\n");
    out.push_str("#if defined(ONEROM_CONSTANTS)\n");
    out.push_str("const uint32_t onerom_chip_type_sizes[NUM_CHIP_TYPES] = {\n");
    for ct in canonical {
        out.push_str(&format!(
            "    [{}] = {},\n",
            ct.c_enum_name(),
            ct.size_bytes()
        ));
    }
    out.push_str("};\n");
    out.push_str("#endif /* ONEROM_CONSTANTS */\n");
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

fn emit_struct_defs(schema: &Schema, out: &mut String) {
    if schema.structs.is_empty() {
        return;
    }
    emit_major_section_header("Structs", out);
    for s in &schema.structs {
        emit_struct(s, schema, out);
    }
}

fn emit_struct(s: &Struct, schema: &Schema, out: &mut String) {
    emit_item_header(s.comment.as_deref(), out);

    out.push_str(&format!("typedef struct {} {{\n", s.name));

    let mut offset: usize = 0;
    for field in &s.fields {
        emit_field_with_offset(field, s.has_const_fields(), &mut offset, schema, out);
    }

    out.push_str(&format!("}} {};\n", s.name));

    if let Some(sz) = s.size {
        out.push_str(&format!(
            "STATIC_ASSERT(sizeof({name}) == {sz}, \"{name} must be {sz} {unit}\");\n",
            name = s.name,
            sz = sz,
            unit = bytes_str(sz),
        ));
    }

    // offsetof assertions for any field carrying an expected_offset
    for field in &s.fields {
        if let Some(expected) = field.expected_offset {
            out.push_str(&format!(
                "STATIC_ASSERT(offsetof({sname}, {fname}) == {off}, \
                 \"{sname}.{fname} must be at offset {off}\");\n",
                sname = s.name,
                fname = field.name,
                off = expected,
            ));
        }
    }

    out.push('\n');
}

// ---------------------------------------------------------------------------
// Tagged FAM structs
// ---------------------------------------------------------------------------

fn emit_tagged_fam_defs(schema: &Schema, out: &mut String) {
    if schema.tagged_fams.is_empty() {
        return;
    }
    emit_major_section_header("Algorithm configuration structs (variable-length)", out);
    for fam in &schema.tagged_fams {
        emit_tagged_fam(fam, schema, out);
    }
}

fn emit_tagged_fam(fam: &TaggedFam, schema: &Schema, out: &mut String) {
    emit_item_header(fam.comment.as_deref(), out);

    // Main struct: discriminant + param_len + common fields + params[]
    out.push_str(&format!("typedef struct {} {{\n", fam.name));

    let mut offset: usize = 0;

    // Discriminant field
    out.push_str(&format!("    // Offset: {}\n", offset));
    let disc_size = enum_size_by_name(&fam.discriminant_type, schema);
    out.push_str(&format!(
        "    {} {};\n",
        fam.discriminant_type, fam.discriminant_field
    ));
    offset += disc_size;

    // param_len field
    out.push_str(&format!("    // Offset: {}\n", offset));
    out.push_str(&format!("    uint8_t {};\n", fam.param_len_field));
    offset += 1;

    // Fields shared across all variants — tagged FAM struct fields are not const in C
    for field in &fam.common_fields {
        emit_field_with_offset(field, false, &mut offset, schema, out);
    }

    // Flexible array member
    out.push_str(&format!("    // Offset: {}\n", offset));
    out.push_str("    uint8_t params[];\n");

    out.push_str(&format!("}} {};\n", fam.name));
    out.push_str(&format!(
        "STATIC_ASSERT(sizeof({name}) == {sz}, \
         \"{name} base struct must be {sz} {unit}\");\n",
        name = fam.name,
        sz = fam.base_size,
        unit = bytes_str(fam.base_size),
    ));
    out.push('\n');

    // Per-variant param structs
    for variant in &fam.variants {
        let param_name = derive_param_struct_name(
            &fam.name,
            &variant.discriminant,
            &fam.discriminant_type,
            schema,
        );

        if let Some(c) = &variant.comment {
            emit_comment(c, "", out);
        }

        out.push_str(&format!("typedef struct {} {{\n", param_name));
        let mut v_offset: usize = 0;
        for field in &variant.fields {
            emit_field_with_offset(field, false, &mut v_offset, schema, out);
        }
        out.push_str(&format!("}} {};\n", param_name));
        out.push_str(&format!(
            "STATIC_ASSERT(sizeof({pname}) == {lconst}, \"{pname} mis-sized\");\n",
            pname = param_name,
            lconst = variant.params_len_constant,
        ));
        out.push('\n');
    }
}

// ---------------------------------------------------------------------------
// Simple FAM structs
// ---------------------------------------------------------------------------

fn emit_simple_fam_defs(schema: &Schema, out: &mut String) {
    if schema.simple_fams.is_empty() {
        return;
    }
    emit_major_section_header("GPIO configuration structs (variable-length)", out);
    for fam in &schema.simple_fams {
        emit_simple_fam(fam, out);
    }
}

fn emit_simple_fam(fam: &SimpleFam, out: &mut String) {
    emit_item_header(fam.comment.as_deref(), out);
    out.push_str(&format!("typedef struct {} {{\n", fam.name));
    out.push_str("    // Offset: 0\n");
    out.push_str(&format!("    uint8_t {};\n", fam.param_len_field));
    out.push_str("    // Offset: 1\n");
    out.push_str("    uint8_t params[];\n");
    out.push_str(&format!("}} {};\n\n", fam.name));
}

// ---------------------------------------------------------------------------
// Field emission
// ---------------------------------------------------------------------------

fn emit_field_with_offset(
    field: &Field,
    const_fields: bool,
    offset: &mut usize,
    schema: &Schema,
    out: &mut String,
) {
    out.push_str(&format!("    // Offset: {}\n", offset));
    if let Some(c) = &field.comment {
        emit_comment(c, "    ", out);
    }
    out.push_str(&field_c_decl(field, const_fields));
    *offset += field_size(field, schema);
}

/// Produces the C declaration line (with leading indent and trailing newline)
/// for a single field.
fn field_c_decl(field: &Field, const_fields: bool) -> String {
    let ck = if const_fields { "const " } else { "" };

    match field.kind.as_str() {
        "scalar" => {
            let prim = c_primitive(field.type_.as_deref().unwrap_or("u8"));
            format!("    {}{} {};\n", ck, prim, field.name)
        }
        "enum" | "type_alias" => {
            let tname = field.type_.as_deref().unwrap_or("uint8_t");
            format!("    {}{} {};\n", ck, tname, field.name)
        }
        "inline_array" => {
            let etype = c_primitive(field.element.as_deref().unwrap_or("u8"));
            let dim = c_array_dim(field.count_ref.as_deref(), field.count);
            format!("    {}{} {}[{}];\n", ck, etype, field.name, dim)
        }
        "inline_array2d" => {
            let etype = c_primitive(field.element.as_deref().unwrap_or("u8"));
            let rdim = c_array_dim(field.rows_ref.as_deref(), field.rows);
            let cdim = c_array_dim(field.cols_ref.as_deref(), field.cols);
            format!("    {}{} {}[{}][{}];\n", ck, etype, field.name, rdim, cdim)
        }
        "cstr_ptr" => {
            // const char * regardless of the struct's const_fields setting
            format!("    const char *{};\n", field.name)
        }
        "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            let tname = field.type_.as_deref().unwrap_or("void");
            if field.const_ptr.unwrap_or(false) {
                format!("    const {} * const {};\n", tname, field.name)
            } else {
                format!("    const {} *{};\n", tname, field.name)
            }
        }
        "struct_array_ptr" => {
            let etype = field.element.as_deref().unwrap_or("void");
            format!("    const {} *{};\n", etype, field.name)
        }
        "struct_ptr_array_ptr" => {
            // Pointer to an array of const pointers: const T * const *name
            let etype = field.element.as_deref().unwrap_or("void");
            format!("    const {} * const *{};\n", etype, field.name)
        }
        "opaque_ptr" => {
            let base = c_primitive(field.pointed_type.as_deref().unwrap_or("void"));
            if const_fields {
                format!("    const {} *{};\n", base, field.name)
            } else {
                format!("    {} *{};\n", base, field.name)
            }
        }
        "fn_ptr" => {
            format!("    void (*{})(void);\n", field.name)
        }
        "padding" => {
            let sz = field.size.unwrap_or(0);
            format!("    {}uint8_t {}[{}];\n", ck, field.name, sz)
        }
        other => format!("    /* unhandled field kind: {} */\n", other),
    }
}

// ---------------------------------------------------------------------------
// Comment and section helpers
// ---------------------------------------------------------------------------

/// Emits a major `=====` section header, used once per logical group
/// (Forward declarations, Type aliases, Constants, Enums, Structs, …).
fn emit_major_section_header(title: &str, out: &mut String) {
    out.push_str(
        "// ===========================================================================\n",
    );
    out.push_str(&format!("// {}\n", title));
    out.push_str(
        "// ===========================================================================\n\n",
    );
}

/// Emits a minor `-----` item header enclosing an optional doc comment,
/// used once per individual type (each enum, struct, FAM).
fn emit_item_header(comment: Option<&str>, out: &mut String) {
    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    if let Some(c) = comment {
        emit_comment(c, "", out);
        out.push_str(
            "// ---------------------------------------------------------------------------\n",
        );
    }
}

/// Emits a C `//` comment block, handling multi-line strings.
fn emit_comment(comment: &str, indent: &str, out: &mut String) {
    for line in comment.lines() {
        let line = line.trim();
        if line.is_empty() {
            out.push_str(&format!("{}//\n", indent));
        } else {
            out.push_str(&format!("{}// {}\n", indent, line));
        }
    }
}

// ---------------------------------------------------------------------------
// Type / value formatting helpers
// ---------------------------------------------------------------------------

/// Map a primitive type string to its C equivalent.
fn c_primitive(type_: &str) -> &'static str {
    match type_ {
        "u8" | "char" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "void" => "void",
        _ => "uint8_t",
    }
}

/// "byte" for n == 1, "bytes" otherwise.
fn bytes_str(n: u32) -> &'static str {
    if n == 1 { "byte" } else { "bytes" }
}

/// Produce the C array dimension string, preferring a named constant where one
/// is provided by the schema.
fn c_array_dim(ref_name: Option<&str>, fallback: Option<u32>) -> String {
    if let Some(r) = ref_name {
        r.to_owned()
    } else {
        fallback.unwrap_or(0).to_string()
    }
}

/// Format an integer constant value for C with a type-appropriate cast.
fn format_const_value(value: &ConstantValue, type_: &str) -> String {
    match value {
        ConstantValue::Integer(n) => {
            let n = *n;
            let hex_str = if n >= 10 {
                match type_ {
                    "u8" => format!("0x{:02X}", n as u8),
                    "u16" => format!("0x{:04X}", n as u16),
                    "u32" => format!("0x{:08X}", n as u32),
                    _ => format!("0x{:X}", n as u64),
                }
            } else {
                format!("{}", n)
            };
            match type_ {
                "u8" => format!("((uint8_t){})", hex_str),
                "u16" => format!("((uint16_t){})", hex_str),
                "u32" => format!("((uint32_t){})", hex_str),
                _ => hex_str, // usize / isize: no cast needed
            }
        }
        ConstantValue::Text(s) => format!("\"{}\"", s),
    }
}

/// Format an enum discriminant value: decimal for 0–9, hex otherwise.
/// Width is matched to the enum's declared storage size.
fn format_enum_value(v: i64, size: u32) -> String {
    if (0..10).contains(&v) {
        format!("{}", v)
    } else {
        match size {
            1 => format!("0x{:02X}", v as u8),
            2 => format!("0x{:04X}", v as u16),
            4 => format!("0x{:08X}", v as u32),
            _ => format!("0x{:X}", v as u64),
        }
    }
}

/// Look up the byte size of an enum type by name.
fn enum_size_by_name(enum_name: &str, schema: &Schema) -> usize {
    schema
        .enums
        .iter()
        .find(|e| e.name == enum_name)
        .map(|e| e.size as usize)
        .unwrap_or(1)
}

/// Derive the C param struct name for a tagged FAM variant.
///
/// Convention (matching the original alg.h):
///   strip `_config_t` from the FAM name, append the discriminant value,
///   append `_param_t`.
///
/// Example: `onerom_alg_cs_config_t` + `ALG_CS_0` (value 0)
///          → `onerom_alg_cs0_param_t`
fn derive_param_struct_name(
    fam_name: &str,
    variant_discriminant: &str,
    disc_type: &str,
    schema: &Schema,
) -> String {
    let value = schema
        .enums
        .iter()
        .find(|e| e.name == disc_type)
        .and_then(|e| e.variants.iter().find(|v| v.name == variant_discriminant))
        .map(|v| v.value)
        .unwrap_or(0);
    let base = fam_name.trim_end_matches("_config_t");
    format!("{}{}_param_t", base, value)
}
