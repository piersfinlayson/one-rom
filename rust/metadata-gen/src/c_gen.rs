// src/c_gen.rs
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
//   accessors             (static inline readers for generation-gated fields)
//   header guard close

use onerom_config::chip::{CHIP_TYPES, ChipType};

use crate::schema::{
    ARRAY_KINDS, ConstantValue, Field, POINTER_KINDS, Schema, SimpleFam, Struct, TaggedFam,
    array_default_elements, field_size, strip_type_suffix,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate the C header.
pub fn generate(schema: &Schema) -> String {
    let mut out = String::with_capacity(65536);
    emit_file_header(schema, &mut out);
    emit_deprecation_macro(schema, &mut out);
    emit_forward_declarations(schema, &mut out);
    emit_type_aliases(schema, &mut out);
    emit_constants(schema, &mut out);
    emit_enums(schema, &mut out);
    emit_struct_defs(schema, &mut out);
    emit_tagged_fam_defs(schema, &mut out);
    emit_simple_fam_defs(schema, &mut out);
    emit_accessors(schema, &mut out);
    emit_metadata_str_cases(schema, &mut out);
    emit_metadata_uint_cases(schema, &mut out);
    emit_metadata_uint_at_cases(schema, &mut out);
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
{region}// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: {source}

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
        region = match (schema.schema.metadata_base, schema.schema.metadata_size) {
            (Some(base), Some(size)) => {
                format!("// Metadata base address: 0x{base:08X}  ({size} bytes reserved)\n//\n")
            }
            _ => String::new(),
        },
        guard = guard,
        source = schema.source,
    ));
}

// ---------------------------------------------------------------------------
// Deprecation
// ---------------------------------------------------------------------------

/// Emit the attribute macro a deprecated field's declaration carries.
///
/// Nothing is emitted where the schema deprecates nothing, so the macro
/// reaches the header only in the release something first needs it.
fn emit_deprecation_macro(schema: &Schema, out: &mut String) {
    if !schema
        .all_fields()
        .iter()
        .any(|(_, f)| f.deprecated_marker().is_some())
    {
        return;
    }

    emit_major_section_header("Deprecation", out);
    let comment = wrap_comment_text(
        "ONEROM_DEPRECATED(reason) marks what a reader of a deprecated field would \
         reach for. A field is deprecated where its bytes are still written and \
         still parsed, and nothing should be decided from what they hold - so the \
         declaration says so where somebody would otherwise use it, and the \
         firmware's -Werror build turns that into a stop.\n\n\
         It lands on the accessor where the field has one and on the member \
         otherwise, which in both cases is what a reader names. A generated \
         writer names the member of a gated field directly and is unaffected.",
        76,
    );
    emit_comment(&comment, "", out);
    out.push_str(
        "#if defined(__GNUC__) || defined(__clang__)\n\
         #define ONEROM_DEPRECATED(reason) __attribute__((deprecated(reason)))\n\
         #else\n\
         #define ONEROM_DEPRECATED(reason)\n\
         #endif\n\n",
    );
}

/// The `ONEROM_DEPRECATED(...)` text a field carries, with a trailing space,
/// or an empty string where the field is not deprecated.
fn deprecation_attribute(field: &Field, schema: &Schema) -> String {
    match field.deprecated_marker() {
        Some((governor, generation)) => format!(
            "ONEROM_DEPRECATED(\"{} is deprecated from {} generation {}\") ",
            field.name,
            schema
                .struct_in_slot(governor)
                .map_or(governor, |s| s.name.as_str()),
            generation
        ),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Generation-gated field accessors
// ---------------------------------------------------------------------------

/// Emit a reader for every field the metadata generation gates.
///
/// The metadata is the one structure a device does not compile for itself, so
/// it is the one read through an accessor - see [`Field::metadata_since`].
///
/// Each reader takes the header the generation lives in and the object the
/// bytes live in.  Both are pointers the caller already holds, and asking for
/// the header rather than a bare number leaves no way to check against the
/// wrong structure's generation.
fn emit_accessors(schema: &Schema, out: &mut String) {
    let gated = schema.metadata_gated_fields();
    if gated.is_empty() {
        return;
    }
    let Some(header) = schema.metadata_header() else {
        return;
    };
    let version_field = header
        .version_field
        .as_deref()
        .expect("the metadata header declares a version_field");

    emit_major_section_header("Generation-gated field accessors", out);

    for (s, field) in gated {
        emit_accessor(s, field, header, version_field, schema, out);
    }
}

fn emit_accessor(
    s: &Struct,
    field: &Field,
    header: &Struct,
    version_field: &str,
    schema: &Schema,
    out: &mut String,
) {
    let generation = field
        .metadata_since()
        .expect("only gated fields reach here");
    let name = accessor_name(s, field);
    let default = c_default_expr(field, schema, &name);
    let on_header = s.name == header.name;
    let object = if on_header { "header" } else { "obj" };

    out.push_str(
        "// ---------------------------------------------------------------------------\n",
    );
    let mut doc = String::new();
    if let Some(c) = &field.comment {
        doc.push_str(c);
        doc.push_str("\n\n");
    }
    doc.push_str(&format!(
        "{}.{} arrived in {} generation {}. Metadata written before that \
         generation has no such field, and this reads {} in its place.",
        s.name, field.name, header.name, generation, default,
    ));
    if let Some((_, retired)) = field.deprecated_marker() {
        doc.push_str(&format!(
            "\n\nDeprecated from generation {retired}: the bytes are still there and \
             nothing should be decided from them."
        ));
    }
    emit_comment(&wrap_comment_text(&doc, 76), "", out);

    emit_accessor_default_object(field, &name, out);

    let deprecated = deprecation_attribute(field, schema);
    let params = if on_header {
        format!("const {} *header", header.name)
    } else {
        format!("const {} *header, const {} *obj", header.name, s.name)
    };
    out.push_str(&format!(
        "static inline {deprecated}{} {{\n",
        accessor_declarator(field, &name, &params),
    ));
    out.push_str(&format!(
        "    return (header->{version_field} >= {generation}u) ? {stored} : {default};\n",
        stored = c_stored_expr(field, object),
    ));
    out.push_str("}\n\n");
}

/// The accessor's name: the struct's own name without its `_t`, then the
/// field's.  It is the name the member would have had, so the compiler's
/// "did you mean" points at it when hand-written code reaches for the member.
fn accessor_name(s: &Struct, field: &Field) -> String {
    format!(
        "{}_{}",
        crate::schema::strip_type_suffix(&s.name),
        field.name
    )
}

/// The C type a gated field's accessor returns.
///
/// A field whose value is a number is returned by value.  Everything else is
/// returned as a pointer to it - to the member on a device new enough, and to
/// the static default otherwise - because the firmware has nowhere to copy an
/// array or a string to.
fn c_value_type(field: &Field) -> String {
    match field.kind.as_str() {
        "scalar" => c_primitive(field.type_.as_deref().unwrap_or("u8")).to_string(),
        "inline_array" => format!(
            "const {} *",
            c_primitive(field.element.as_deref().unwrap_or("u8"))
        ),
        "cstr_ptr" => "const char *".to_string(),
        "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            format!("const {} *", field.type_.as_deref().unwrap_or("void"))
        }
        "struct_array_ptr" => format!("const {} *", field.element.as_deref().unwrap_or("void")),
        "struct_ptr_array_ptr" => format!(
            "const {} * const *",
            field.element.as_deref().unwrap_or("void")
        ),
        "opaque_ptr" => format!(
            "const {} *",
            c_primitive(field.pointed_type.as_deref().unwrap_or("void"))
        ),
        _ => field.type_.as_deref().unwrap_or("uint8_t").to_string(),
    }
}

/// The accessor's return type, name and parameter list, less `static inline`.
///
/// Two kinds wrap the name rather than standing in front of it - a function
/// returning a pointer to a row, and one returning a function pointer - so
/// the whole declarator is built here rather than from a type and a name.
fn accessor_declarator(field: &Field, name: &str, params: &str) -> String {
    match field.kind.as_str() {
        "inline_array2d" => format!(
            "const {} (*{name}({params}))[{}]",
            c_primitive(field.element.as_deref().unwrap_or("u8")),
            c_array_dim(field.cols_ref.as_deref(), field.cols),
        ),
        "fn_ptr" => format!("void (*{name}({params}))(void)"),
        _ => {
            let ctype = c_value_type(field);
            let gap = if ctype.ends_with('*') { "" } else { " " };
            format!("{ctype}{gap}{name}({params})")
        }
    }
}

/// The member read, as the accessor's return type.
///
/// A 2-D array is the one kind needing a cast: a member of a structure whose
/// fields are not const decays to a pointer to a non-const row, and C does
/// not convert that to a pointer to a const one.
fn c_stored_expr(field: &Field, object: &str) -> String {
    let member = field.c_member();
    match field.kind.as_str() {
        "inline_array2d" => format!(
            "(const {} (*)[{}]){object}->{member}",
            c_primitive(field.element.as_deref().unwrap_or("u8")),
            c_array_dim(field.cols_ref.as_deref(), field.cols),
        ),
        _ => format!("{object}->{member}"),
    }
}

/// The name of the static object holding an array field's default.
fn c_default_object_name(accessor: &str) -> String {
    format!("{accessor}_default")
}

/// Emit the static object an array field's accessor hands back, where the
/// field is one that needs one.
///
/// The array's bytes are in the structure whatever the generation, so what is
/// absent is anything having been written there.  It costs the array's own
/// size in flash, once, for a field that carries a marker.
fn emit_accessor_default_object(field: &Field, accessor: &str, out: &mut String) {
    if !ARRAY_KINDS.contains(&field.kind.as_str()) {
        return;
    }
    let elements = array_default_elements(field);
    let bytes = |row: &[u8]| -> String {
        row.iter()
            .map(|b| format!("0x{b:02X}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let name = c_default_object_name(accessor);
    let etype = c_primitive(field.element.as_deref().unwrap_or("u8"));

    if field.kind == "inline_array2d" {
        let cols = field.cols.unwrap_or(0) as usize;
        let rows: Vec<String> = elements
            .chunks(cols.max(1))
            .map(|row| format!("{{ {} }}", bytes(row)))
            .collect();
        out.push_str(&format!(
            "static const {etype} {name}[{}][{}] = {{ {} }};\n",
            c_array_dim(field.rows_ref.as_deref(), field.rows),
            c_array_dim(field.cols_ref.as_deref(), field.cols),
            rows.join(", "),
        ));
    } else {
        out.push_str(&format!(
            "static const {etype} {name}[{}] = {{ {} }};\n",
            c_array_dim(field.count_ref.as_deref(), field.count),
            bytes(&elements),
        ));
    }
}

/// The C expression for `default_if_absent`.
///
/// An enum's default is written as the variant's own name rather than its
/// number, so the header says which state a reader of older metadata sees.
/// A pointer's is `NULL`, and an array's is the static object above.
fn c_default_expr(field: &Field, schema: &Schema, accessor: &str) -> String {
    if POINTER_KINDS.contains(&field.kind.as_str()) {
        return "NULL".to_string();
    }
    if ARRAY_KINDS.contains(&field.kind.as_str()) {
        return c_default_object_name(accessor);
    }

    let value = field
        .default_if_absent
        .as_ref()
        .and_then(|v| v.as_integer())
        .expect("a gated field of this kind has a whole-number default");

    if field.kind == "enum" {
        let named = field.type_.as_deref().unwrap_or("");
        if let Some(e) = schema.enums.iter().find(|e| e.name == named)
            && let Some(v) = e.variants.iter().find(|v| v.value == value)
        {
            return v.name.clone();
        }
    }
    format!("({}){}", c_value_type(field), value)
}

// ---------------------------------------------------------------------------
// Plugin metadata key resolvers
// ---------------------------------------------------------------------------

/// Emit ONEROM_METADATA_STR_CASES: the generated switch arms for the string
/// metadata getter (ora_get_metadata_str in firmware/src/plugin.c).
///
/// One arm per schema field tagged `plugin_key`: string-typed fields resolve
/// their stored value; any other type returns ORA_RESULT_TYPE_MISMATCH.  The
/// expanding function owns the switch, the argument guard, and the default arm.
fn emit_metadata_str_cases(schema: &Schema, out: &mut String) {
    let keys = schema.plugin_keys();
    if keys.is_empty() {
        return;
    }

    emit_major_section_header("Plugin metadata key resolvers", out);

    let comment = wrap_comment_text(
        "ONEROM_METADATA_STR_CASES(out) expands to the case arms of the switch in \
         ora_get_metadata_str() (firmware/src/plugin.c).\n\n\
         The arms are generated from the schema fields tagged `plugin_key`: each \
         string-typed field resolves its stored value here; any non-string key \
         returns ORA_RESULT_TYPE_MISMATCH. The expanding function supplies the \
         surrounding switch, the `out == NULL` guard, and the `default:` arm that \
         returns ORA_RESULT_NOT_SUPPORTED for keys unknown to this firmware.\n\n\
         The macro references ora_metadata_key_t / ora_result_t from the plugin \
         API (api.h). It is inert text, so those symbols need only be in scope \
         where the macro is expanded (plugin.c, which includes api.h): this header \
         therefore does not - and must not - include api.h, keeping the metadata \
         type header free of any dependency on the plugin API surface.",
        76,
    );
    emit_comment(&comment, "", out);

    // Build the physical lines of the #define, then align the line-continuation
    // backslashes into a single column.
    let mut lines: Vec<String> = vec!["#define ONEROM_METADATA_STR_CASES(out)".to_string()];
    for entry in &keys {
        let key_name = format!("ORA_METADATA_KEY_{}", entry.key.name);
        lines.push(format!("    case {}:", key_name));
        if entry.kind == "cstr_ptr" {
            let access = schema
                .plugin_key_access(entry.struct_name, entry.field_name)
                .expect("plugin_key access paths are validated at schema load");
            lines.push(format!("        *(out) = {};", access));
            lines.push("        return ORA_RESULT_OK;".to_string());
        } else {
            lines.push("        return ORA_RESULT_TYPE_MISMATCH;".to_string());
        }
    }

    let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    for (i, line) in lines.iter().enumerate() {
        if i + 1 < lines.len() {
            out.push_str(&format!("{:<width$} \\\n", line, width = width));
        } else {
            out.push_str(&format!("{}\n", line));
        }
    }
    out.push('\n');
}

/// Emit ONEROM_METADATA_UINT_CASES: the generated switch arms for the unsigned
/// metadata getter (ora_get_metadata_uint in firmware/src/plugin.c).
///
/// One arm per schema field tagged `plugin_key`: unsigned scalar / enum fields
/// resolve their value zero-extended to uint32_t; any other type returns
/// ORA_RESULT_TYPE_MISMATCH.  The expanding function owns the switch, the
/// argument guard, and the default arm.
fn emit_metadata_uint_cases(schema: &Schema, out: &mut String) {
    let keys = schema.plugin_keys();
    if keys.is_empty() {
        return;
    }

    let comment = wrap_comment_text(
        "ONEROM_METADATA_UINT_CASES(out) expands to the case arms of the switch in \
         ora_get_metadata_uint() (firmware/src/plugin.c).\n\n\
         The arms are generated from the schema fields tagged `plugin_key`: each \
         unsigned scalar or enum field resolves its value here, zero-extended to \
         uint32_t; any non-numeric key returns ORA_RESULT_TYPE_MISMATCH. The \
         expanding function supplies the surrounding switch, the `out == NULL` \
         guard, and the `default:` arm that returns ORA_RESULT_NOT_SUPPORTED for \
         keys unknown to this firmware.",
        76,
    );
    emit_comment(&comment, "", out);

    let mut lines: Vec<String> = vec!["#define ONEROM_METADATA_UINT_CASES(out)".to_string()];
    for entry in &keys {
        let key_name = format!("ORA_METADATA_KEY_{}", entry.key.name);
        lines.push(format!("    case {}:", key_name));
        if entry.kind == "scalar" || entry.kind == "enum" {
            let access = schema
                .plugin_key_access(entry.struct_name, entry.field_name)
                .expect("plugin_key access paths are validated at schema load");
            lines.push(format!("        *(out) = (uint32_t)({});", access));
            lines.push("        return ORA_RESULT_OK;".to_string());
        } else {
            lines.push("        return ORA_RESULT_TYPE_MISMATCH;".to_string());
        }
    }

    let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    for (i, line) in lines.iter().enumerate() {
        if i + 1 < lines.len() {
            out.push_str(&format!("{:<width$} \\\n", line, width = width));
        } else {
            out.push_str(&format!("{}\n", line));
        }
    }
    out.push('\n');
}

/// Emit ONEROM_METADATA_UINT_AT_CASES: the generated switch arms for the
/// indexed metadata getter (ora_get_metadata_uint_at in firmware/src/plugin.c).
///
/// One arm per schema field tagged `plugin_key`: array-of-unsigned fields
/// bounds-check the index and resolve the element.  Any other type returns
/// ORA_RESULT_TYPE_MISMATCH.  The expanding function owns the switch, the
/// argument guard, and the default arm.
fn emit_metadata_uint_at_cases(schema: &Schema, out: &mut String) {
    let keys = schema.plugin_keys();
    if keys.is_empty() {
        return;
    }

    let comment = wrap_comment_text(
        "ONEROM_METADATA_UINT_AT_CASES(index, out) expands to the case arms of the \
         switch in ora_get_metadata_uint_at() (firmware/src/plugin.c).\n\n\
         The arms are generated from the schema fields tagged `plugin_key`: each \
         field that is an array of unsigned elements resolves element `index` \
         here, zero-extended to uint32_t. Any key that is not such an array \
         returns ORA_RESULT_TYPE_MISMATCH. An `index` past the last element is \
         rejected with ORA_RESULT_INVALID_ARG - the arrays these keys name are \
         terminated by GPIO_NONE, so a caller scans rather than asking for a \
         length, and reading past the end is a caller error. The expanding \
         function supplies the surrounding switch, the `out == NULL` guard, and \
         the `default:` arm that returns ORA_RESULT_NOT_SUPPORTED for keys \
         unknown to this firmware.",
        76,
    );
    emit_comment(&comment, "", out);

    let mut lines: Vec<String> =
        vec!["#define ONEROM_METADATA_UINT_AT_CASES(index, out)".to_string()];
    for entry in &keys {
        let key_name = format!("ORA_METADATA_KEY_{}", entry.key.name);
        lines.push(format!("    case {}:", key_name));
        if entry.kind == "inline_array" {
            // Prefer the named bound, so the generated arm says what the limit
            // means rather than repeating a number the schema owns.
            let bound = entry
                .count_ref
                .map(str::to_string)
                .or_else(|| entry.count.map(|c| c.to_string()))
                .expect("array plugin_key counts are validated at schema load");
            let access = schema
                .plugin_key_access(entry.struct_name, entry.field_name)
                .expect("plugin_key access paths are validated at schema load");
            lines.push(format!("        if ((index) >= {}) {{", bound));
            lines.push("            return ORA_RESULT_INVALID_ARG;".to_string());
            lines.push("        }".to_string());
            lines.push(format!("        *(out) = (uint32_t)({}[(index)]);", access));
            lines.push("        return ORA_RESULT_OK;".to_string());
        } else {
            lines.push("        return ORA_RESULT_TYPE_MISMATCH;".to_string());
        }
    }

    let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    for (i, line) in lines.iter().enumerate() {
        if i + 1 < lines.len() {
            out.push_str(&format!("{:<width$} \\\n", line, width = width));
        } else {
            out.push_str(&format!("{}\n", line));
        }
    }
    out.push('\n');
}

fn emit_file_footer(schema: &Schema, out: &mut String) {
    out.push_str(&format!("#endif // {}\n", header_guard(schema)));
}

/// Word-wrap comment text to `width` columns (before the `// ` prefix that
/// emit_comment adds), preserving blank-line paragraph breaks.  Lets generated
/// comment bodies be authored as flowing paragraphs yet match the hand-wrapped
/// house style in the rest of the header.
fn wrap_comment_text(text: &str, width: usize) -> String {
    let mut paragraphs = Vec::new();
    for para in text.split("\n\n") {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in para.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
            } else if line.len() + 1 + word.len() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        paragraphs.push(lines.join("\n"));
    }
    paragraphs.join("\n\n")
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
        if let Some(comment) = c.documentation() {
            emit_comment(&comment, "", out);
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
        // The first line of the comment, and the deprecation note where the
        // value carries one, which would otherwise be lost below that line.
        let first = v.comment.as_deref().and_then(|c| c.lines().next());
        let note = v
            .deprecated_release
            .as_deref()
            .map(|release| format!("Deprecated from firmware {release}."));
        let comment_str = match (first, note) {
            (Some(first), Some(note)) => format!("  // {first} {note}"),
            (Some(first), None) => format!("  // {first}"),
            (None, Some(note)) => format!("  // {note}"),
            (None, None) => String::new(),
        };
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

    let (canonical, aliases) = rbcp_chip_types();
    let num_chip_types = num_chip_types(&canonical);

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

/// The chip types an `rbcp_chip_types` enum holds, as (canonical, aliases).
///
/// Canonical chips are sorted by RBCP value, and `try_from_rbcp_u8` returns
/// each of them for its own value. It returns a different, canonical chip for
/// an alias's value.
pub(crate) fn rbcp_chip_types() -> (Vec<ChipType>, Vec<ChipType>) {
    let mut canonical: Vec<ChipType> = CHIP_TYPES
        .iter()
        .copied()
        .filter(|ct| ChipType::try_from_rbcp_u8(ct.rbcp_chip_type()) == Some(*ct))
        .collect();
    canonical.sort_by_key(|ct| ct.rbcp_chip_type());

    let aliases: Vec<ChipType> = CHIP_TYPES
        .iter()
        .copied()
        .filter(|ct| ChipType::try_from_rbcp_u8(ct.rbcp_chip_type()) != Some(*ct))
        .collect();

    (canonical, aliases)
}

/// `NUM_CHIP_TYPES`, one past the highest canonical RBCP value.
pub(crate) fn num_chip_types(canonical: &[ChipType]) -> u32 {
    canonical
        .iter()
        .map(|ct| ct.rbcp_chip_type() as u32)
        .max()
        .unwrap_or(0)
        + 1
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
                fname = field.c_member(),
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
    emit_major_section_header("Variable-length tagged structs", out);
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
    out.push_str(&format!(
        "    {} {};\n",
        c_primitive(fam.param_len_type()),
        fam.param_len_field
    ));
    offset += fam.param_len_size();

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

    // Per-variant param structs.  A variant holding only a string has none,
    // since C cannot declare a struct whose sole member is a flexible array.
    for variant in &fam.variants {
        if variant.fixed_fields().next().is_none() && variant.string_field().is_some() {
            continue;
        }
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
        if let Some(lconst) = &variant.params_len_constant {
            out.push_str(&format!(
                "STATIC_ASSERT(sizeof({pname}) == {lconst}, \"{pname} mis-sized\");\n",
                pname = param_name,
            ));
        }
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
    out.push_str(&field_c_decl(field, const_fields, schema));
    *offset += field_size(field, schema);
}

/// Produces the C declaration line (with leading indent and trailing newline)
/// for a single field.
///
/// A generation-gated field is declared under [`Field::c_member`] rather than
/// its own name, which belongs to the accessor.
fn field_c_decl(field: &Field, const_fields: bool, schema: &Schema) -> String {
    let ck = if const_fields { "const " } else { "" };
    let member = field.c_member();

    // A deprecated field with an accessor carries the note there, since that
    // is what a reader names.  Without one, the member is.
    let dep = if field.metadata_since().is_some() {
        String::new()
    } else {
        let attribute = deprecation_attribute(field, schema);
        match attribute.is_empty() {
            true => attribute,
            false => format!(" {}", attribute.trim_end()),
        }
    };

    match field.kind.as_str() {
        "scalar" => {
            let prim = c_primitive(field.type_.as_deref().unwrap_or("u8"));
            format!("    {}{} {}{};\n", ck, prim, member, dep)
        }
        "enum" | "type_alias" => {
            let tname = field.type_.as_deref().unwrap_or("uint8_t");
            format!("    {}{} {}{};\n", ck, tname, member, dep)
        }
        "inline_array" => {
            let etype = c_primitive(field.element.as_deref().unwrap_or("u8"));
            let dim = c_array_dim(field.count_ref.as_deref(), field.count);
            format!("    {}{} {}[{}];\n", ck, etype, member, dim)
        }
        // A string runs to the end of its entry, so it is a flexible array.
        "string" => format!("    {}char {}[];\n", ck, member),
        "inline_array2d" => {
            let etype = c_primitive(field.element.as_deref().unwrap_or("u8"));
            let rdim = c_array_dim(field.rows_ref.as_deref(), field.rows);
            let cdim = c_array_dim(field.cols_ref.as_deref(), field.cols);
            format!("    {}{} {}[{}][{}];\n", ck, etype, member, rdim, cdim)
        }
        "cstr_ptr" => {
            // const char * regardless of the struct's const_fields setting
            format!("    const char *{};\n", member)
        }
        "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            let tname = field.type_.as_deref().unwrap_or("void");
            if field.const_ptr.unwrap_or(false) {
                format!("    const {} * const {};\n", tname, member)
            } else {
                format!("    const {} *{};\n", tname, member)
            }
        }
        "struct_array_ptr" => {
            let etype = field.element.as_deref().unwrap_or("void");
            format!("    const {} *{};\n", etype, member)
        }
        "struct_ptr_array_ptr" => {
            // Pointer to an array of const pointers: const T * const *name
            let etype = field.element.as_deref().unwrap_or("void");
            format!("    const {} * const *{};\n", etype, member)
        }
        "opaque_ptr" => {
            let base = c_primitive(field.pointed_type.as_deref().unwrap_or("void"));
            if const_fields {
                format!("    const {} *{};\n", base, member)
            } else {
                format!("    {} *{};\n", base, member)
            }
        }
        "fn_ptr" => {
            format!("    void (*{})(void);\n", member)
        }
        "padding" => {
            let sz = field.size.unwrap_or(0);
            format!("    {}uint8_t {}[{}];\n", ck, member, sz)
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
pub fn format_const_value(value: &ConstantValue, type_: &str) -> String {
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
///   strip `_config_t` from the FAM name, or `_t` where it has none, append
///   the discriminant value, append `_param_t`.
///
/// Example: `onerom_alg_cs_config_t` + `ALG_CS_0` (value 0)
///          → `onerom_alg_cs0_param_t`
///          `onerom_otp_entry_t` + `OTP_KEY_COMMISSIONING_SIG` (value 2)
///          → `onerom_otp_entry2_param_t`
pub(crate) fn derive_param_struct_name(
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
    let base = fam_name
        .strip_suffix("_config_t")
        .unwrap_or_else(|| strip_type_suffix(fam_name));
    format!("{}{}_param_t", base, value)
}
