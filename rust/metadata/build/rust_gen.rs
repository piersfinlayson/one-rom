// build/rust_gen.rs
//
// Generates $OUT_DIR/metadata_generated.rs from the OneROM metadata schema.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License
//
// Output file layout:
//   1. Header / use declarations
//   2. Constants            (pub const)
//   3. Type aliases         (pub type)
//   4. Enums                (pub enum + TryFrom impls)
//   5. Structs              (pub struct + parse impls)       generate != Skip
//   6. Tagged FAMs          (pub enum  + parse impls)       generate != Skip
//   7. Simple FAMs          (pub struct + parse impls)      generate != Skip

#![allow(clippy::collapsible_if)]

use crate::schema::*;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn generate(schema: &Schema) -> String {
    let mut out = String::with_capacity(128 * 1024);
    push_file_header(&mut out);
    push_constants(&mut out, schema);
    push_type_aliases(&mut out, schema);
    push_enums(&mut out, schema);
    push_structs(&mut out, schema);
    push_tagged_fams(&mut out, schema);
    push_simple_fams(&mut out, schema);
    out
}

// ---------------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------------

/// Convert a UPPER_SNAKE or lower_snake identifier to PascalCase.
///
/// Each `_`-separated chunk: first character uppercased, rest lowercased.
/// Empty chunks (from consecutive underscores) are dropped.
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|chunk| {
            let mut chars = chunk.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

/// C type name → Rust type name.
///
/// Strips a trailing `_t` suffix if present, then applies `to_pascal_case`.
pub fn rust_type_name(c_name: &str) -> String {
    to_pascal_case(c_name.strip_suffix("_t").unwrap_or(c_name))
}

/// C enum variant name → Rust variant identifier.
pub(crate) fn variant_ident(c_name: &str, _strip_prefix: &str) -> String {
    let pascal = to_pascal_case(c_name);
    if pascal.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{pascal}")
    } else {
        pascal
    }
}

// ---------------------------------------------------------------------------
// Field Rust type string
// ---------------------------------------------------------------------------

fn field_rust_type(field: &Field) -> String {
    match field.kind.as_str() {
        "scalar" => field.type_.as_deref().unwrap_or("u8").to_string(),

        "enum" => rust_type_name(field.type_.as_deref().unwrap_or("")),

        "type_alias" => rust_type_name(field.type_.as_deref().unwrap_or("")),

        "inline_array" => {
            let elem = field.element.as_deref().unwrap_or("u8");
            let n = field.count.unwrap_or(0);
            format!("[{elem}; {n}]")
        }

        "inline_array2d" => {
            let elem = field.element.as_deref().unwrap_or("u8");
            let cols = field.cols.unwrap_or(0);
            let rows = field.rows.unwrap_or(0);
            format!("[[{elem}; {cols}]; {rows}]")
        }

        "cstr_ptr" => {
            if field.nullable.unwrap_or(false) {
                "Option<String>".into()
            } else {
                "String".into()
            }
        }

        "struct_ptr" => {
            let tn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            if field.nullable.unwrap_or(false) {
                format!("Option<{tn}>")
            } else {
                tn
            }
        }

        // Both kinds collapse to Vec<ElemType> in Rust.
        "struct_array_ptr" | "struct_ptr_array_ptr" => {
            format!(
                "Vec<{}>",
                rust_type_name(field.element.as_deref().unwrap_or(""))
            )
        }

        "tagged_fam_ptr" | "simple_fam_ptr" => {
            let tn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            if field.nullable.unwrap_or(false) {
                format!("Option<{tn}>")
            } else {
                tn
            }
        }

        // Pointer fields: stored as the Pointer enum, never dereferenced by
        // the generated parser.
        "opaque_ptr" | "fn_ptr" => "Pointer".into(),

        // Padding is omitted from Rust struct definitions.
        _ => "u8".into(),
    }
}

// ---------------------------------------------------------------------------
// Read-method selection helpers
// ---------------------------------------------------------------------------

/// (`DeviceMemoryView` method name, byte size) for a scalar Rust type string.
fn scalar_rw(ty: &str) -> (&'static str, usize) {
    match ty {
        "u8" => ("read_u8", 1),
        "u16" => ("read_u16_le", 2),
        "u32" => ("read_u32_le", 4),
        _ => ("read_u8", 1),
    }
}

/// (`DeviceMemoryView` method name, byte size) for an enum field.
fn enum_rw(field: &Field, schema: &Schema) -> (&'static str, usize) {
    let sz = schema
        .enums
        .iter()
        .find(|e| field.type_.as_deref() == Some(e.name.as_str()))
        .map_or(1, |e| e.size);
    match sz {
        2 => ("read_u16_le", 2),
        _ => ("read_u8", 1),
    }
}

/// (`DeviceMemoryView` method name, byte size) for a type_alias field.
fn alias_rw(field: &Field, schema: &Schema) -> (&'static str, usize) {
    let underlying = schema
        .type_aliases
        .iter()
        .find(|a| field.type_.as_deref() == Some(a.name.as_str()))
        .map_or("u16", |a| a.underlying.as_str());
    scalar_rw(underlying)
}

// ---------------------------------------------------------------------------
// Parse code emission — mutable-offset style (structs)
// ---------------------------------------------------------------------------

/// Emit the DeviceMemoryView read + `offset +=` code for one struct field.
///
/// For `struct_array_ptr` and `struct_ptr_array_ptr` this emits both the
/// pointer read and the loop body in one shot (used when the count field
/// precedes the array field).  When the count field comes *after* the array
/// field, `push_struct_parse` calls `emit_array_ptr_read` and
/// `emit_array_loop_body` separately instead of this function.
fn emit_field_parse_offset(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let name = &field.name;

    // Expected-offset assertion for ABI-stable fields.
    if let Some(expected) = field.expected_offset {
        out.push_str(&format!(
            "{indent}debug_assert_eq!(\n\
             {indent}    (offset - addr) as usize,\n\
             {indent}    {expected}usize,\n\
             {indent}    \"field `{name}` not at expected offset {expected}\",\n\
             {indent});\n"
        ));
    }

    match field.kind.as_str() {
        "scalar" => {
            let (method, sz) = scalar_rw(field.type_.as_deref().unwrap_or("u8"));
            out.push_str(&format!(
                "{indent}let {name} = view.{method}(offset)?; offset += {sz};\n"
            ));
        }

        "enum" => {
            let (method, sz) = enum_rw(field, schema);
            let rtn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "{indent}let {name}_raw = view.{method}(offset)?; offset += {sz};\n"
            ));
            out.push_str(&format!(
                "{indent}let {name} = {rtn}::try_from({name}_raw).map_err(|_| {{\n\
                 {indent}    ParseError::UnknownDiscriminant {{\n\
                 {indent}        type_name: \"{rtn}\",\n\
                 {indent}        value: {name}_raw as u32,\n\
                 {indent}    }}\n\
                 {indent}}})?;\n"
            ));
        }

        "type_alias" => {
            let (method, sz) = alias_rw(field, schema);
            out.push_str(&format!(
                "{indent}let {name} = view.{method}(offset)?; offset += {sz};\n"
            ));
        }

        "inline_array" => {
            let n = field.count.unwrap_or(0);
            let total = n as usize * prim_size(field.element.as_deref().unwrap_or("u8"));
            out.push_str(&format!(
                "{indent}let {name} = view.read_bytes::<{n}>(offset)?; offset += {total};\n"
            ));
            // Optional magic validation: compare the leading bytes against a
            // generated constant (e.g. ONEROM_INFO_MAGIC = "SDRR").
            if let Some(konst) = field.expected_const.as_deref() {
                out.push_str(&format!(
                    "{indent}if !{name}.starts_with({konst}.as_bytes()) {{\n\
                     {indent}    return Err(ParseError::BadMagic {{ field: \"{name}\" }});\n\
                     {indent}}}\n"
                ));
            }
        }

        "inline_array2d" => {
            let rows = field.rows.unwrap_or(0);
            let cols = field.cols.unwrap_or(0);
            let total = rows as usize * cols as usize;
            // Read flat, then reinterpret as [[u8; cols]; rows].
            out.push_str(&format!(
                "{indent}let {name}_flat = view.read_bytes::<{total}>(offset)?; offset += {total};\n"
            ));
            out.push_str(&format!(
                "{indent}let {name}: [[u8; {cols}]; {rows}] = core::array::from_fn(|r| {{\n\
                 {indent}    core::array::from_fn(|c| {name}_flat[r * {cols} + c])\n\
                 {indent}}});\n"
            ));
        }

        "cstr_ptr" => {
            // DeviceMemoryView::read_cstr reads the pointer at addr and follows it.
            // read_cstr_opt does the same but returns None for a null pointer.
            if field.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{indent}let {name} = view.read_cstr_opt(offset)?; offset += 4;\n"
                ));
            } else {
                out.push_str(&format!(
                    "{indent}let {name} = view.read_cstr(offset)?; offset += 4;\n"
                ));
            }
        }

        "struct_ptr" => {
            let tn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "{indent}let {name}_ptr = view.read_ptr(offset)?; offset += 4;\n"
            ));
            if field.nullable.unwrap_or(false) {
                // `none_on_parse_error` widens tolerance: a non-null pointer
                // whose target fails to parse yields None instead of
                // propagating (e.g. a runtime pointer into RAM that holds
                // stale/absent data on a stopped device).
                let parse_expr = if field.none_on_parse_error.unwrap_or(false) {
                    format!("{tn}::parse(view, {name}_ptr).ok()")
                } else {
                    format!("Some({tn}::parse(view, {name}_ptr)?)")
                };
                out.push_str(&format!(
                    "{indent}let {name} = if {name}_ptr == 0 || {name}_ptr == 0xFFFF_FFFF {{\n\
                     {indent}    None\n\
                     {indent}}} else {{\n\
                     {indent}    {parse_expr}\n\
                     {indent}}};\n"
                ));
            } else {
                out.push_str(&format!(
                    "{indent}if {name}_ptr == 0 {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n\
                     {indent}let {name} = {tn}::parse(view, {name}_ptr)?;\n"
                ));
            }
        }

        "struct_array_ptr" | "struct_ptr_array_ptr" => {
            emit_array_ptr_read(out, field, indent);
            emit_array_loop_body(out, field, indent, schema);
        }

        "tagged_fam_ptr" | "simple_fam_ptr" => {
            let tn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "{indent}let {name}_ptr = view.read_ptr(offset)?; offset += 4;\n"
            ));
            if field.nullable.unwrap_or(false) {
                let parse_expr = if field.none_on_parse_error.unwrap_or(false) {
                    format!("{tn}::parse(view, {name}_ptr).ok()")
                } else {
                    format!("Some({tn}::parse(view, {name}_ptr)?)")
                };
                out.push_str(&format!(
                    "{indent}let {name} = if {name}_ptr == 0 || {name}_ptr == 0xFFFF_FFFF {{\n\
                     {indent}    None\n\
                     {indent}}} else {{\n\
                     {indent}    {parse_expr}\n\
                     {indent}}};\n"
                ));
            } else {
                out.push_str(&format!(
                    "{indent}if {name}_ptr == 0 {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n\
                     {indent}let {name} = {tn}::parse(view, {name}_ptr)?;\n"
                ));
            }
        }

        // opaque_ptr and fn_ptr fields are stored as Pointer values.  The
        // address is read from the view but the pointer is never followed by
        // the generated parser.
        "opaque_ptr" | "fn_ptr" => {
            out.push_str(&format!(
                "{indent}let {name} = Pointer::new(view.read_ptr(offset)?); offset += 4;\n"
            ));
        }

        "padding" => {
            let sz = field.size.unwrap_or(0);
            if sz > 0 {
                out.push_str(&format!("{indent}offset += {sz}; // padding: {name}\n"));
            }
        }

        kind => {
            // Emit a compile-time reminder for any field kind not yet handled.
            out.push_str(&format!(
                "{indent}compile_error!(\"unhandled field kind `{kind}` for field `{name}`\");\n"
            ));
        }
    }
}

/// Emit only the pointer read (and `offset += 4`) for an array field.
///
/// Used when the loop body must be deferred because the count field has not
/// yet been parsed at this point in the struct layout.
fn emit_array_ptr_read(out: &mut String, field: &Field, indent: &str) {
    let name = &field.name;
    // struct_array_ptr  → {name}_ptr  (base of a direct element array)
    // struct_ptr_array_ptr → {name}_outer (base of an array of pointers)
    match field.kind.as_str() {
        "struct_array_ptr" => {
            out.push_str(&format!(
                "{indent}let {name}_ptr = view.read_ptr(offset)?; offset += 4;\n"
            ));
        }
        "struct_ptr_array_ptr" => {
            out.push_str(&format!(
                "{indent}let {name}_outer = view.read_ptr(offset)?; offset += 4;\n"
            ));
        }
        _ => {}
    }
}

/// Emit the null-check and iteration loop for an array field.
///
/// Assumes the pointer variable (`{name}_ptr` or `{name}_outer`) and the
/// count variable (named by `count_field`) are already in scope.
fn emit_array_loop_body(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let name = &field.name;
    let count_f = field.count_field.as_deref().unwrap_or("");
    let nullable = field.nullable.unwrap_or(false);

    match field.kind.as_str() {
        "struct_array_ptr" => {
            let elem_c = field.element.as_deref().unwrap_or("");
            let elem_rn = rust_type_name(elem_c);
            let stride = crate::schema::struct_stride(elem_c, schema);

            if !nullable {
                out.push_str(&format!(
                    "{indent}if {name}_ptr == 0 {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n"
                ));
            }
            out.push_str(&format!("{indent}let mut {name} = Vec::new();\n"));

            let li = if nullable {
                out.push_str(&format!("{indent}if {name}_ptr != 0 {{\n"));
                format!("{indent}    ")
            } else {
                indent.to_string()
            };
            out.push_str(&format!("{li}for i in 0usize..({count_f} as usize) {{\n"));
            out.push_str(&format!(
                "{li}    let ea = {name}_ptr + (i as u32 * {stride}u32);\n"
            ));
            out.push_str(&format!(
                "{li}    {name}.push({elem_rn}::parse(view, ea)?);\n"
            ));
            out.push_str(&format!("{li}}}\n"));

            if nullable {
                out.push_str(&format!("{indent}}}\n"));
            }
        }

        "struct_ptr_array_ptr" => {
            // outer → array of pointers → each pointer → element struct.
            let elem_c = field.element.as_deref().unwrap_or("");
            let elem_rn = rust_type_name(elem_c);

            if !nullable {
                out.push_str(&format!(
                    "{indent}if {name}_outer == 0 {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n"
                ));
            }
            out.push_str(&format!("{indent}let mut {name} = Vec::new();\n"));

            let li = if nullable {
                out.push_str(&format!("{indent}if {name}_outer != 0 {{\n"));
                format!("{indent}    ")
            } else {
                indent.to_string()
            };
            out.push_str(&format!("{li}for i in 0usize..({count_f} as usize) {{\n"));
            out.push_str(&format!(
                "{li}    let inner_ptr = view.read_ptr({name}_outer + (i as u32 * 4u32))?;\n"
            ));
            out.push_str(&format!(
                "{li}    if inner_ptr == 0 {{\n\
                 {li}        return Err(ParseError::NullPointer {{ field: \"{name}[]\" }});\n\
                 {li}    }}\n"
            ));
            out.push_str(&format!(
                "{li}    {name}.push({elem_rn}::parse(view, inner_ptr)?);\n"
            ));
            out.push_str(&format!("{li}}}\n"));

            if nullable {
                out.push_str(&format!("{indent}}}\n"));
            }
        }

        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Parse code emission — static-address style (tagged FAM fields)
// ---------------------------------------------------------------------------

/// Emit the read code for a tagged FAM field at `addr + byte_offset`.
///
/// Tagged FAM parse functions avoid a shared mutable `offset` variable across
/// match arms by computing all addresses statically at code-generation time.
/// Only the field kinds that actually appear in tagged FAM common / variant
/// sections are handled (scalar, enum, type_alias, padding).
fn emit_field_at_addr(
    out: &mut String,
    field: &Field,
    byte_offset: usize,
    indent: &str,
    schema: &Schema,
) {
    let name = &field.name;
    match field.kind.as_str() {
        "scalar" => {
            let (method, _) = scalar_rw(field.type_.as_deref().unwrap_or("u8"));
            out.push_str(&format!(
                "{indent}let {name} = view.{method}(addr + {byte_offset}u32)?;\n"
            ));
        }

        "enum" => {
            let (method, _) = enum_rw(field, schema);
            let rtn = rust_type_name(field.type_.as_deref().unwrap_or(""));
            out.push_str(&format!(
                "{indent}let {name}_raw = view.{method}(addr + {byte_offset}u32)?;\n"
            ));
            out.push_str(&format!(
                "{indent}let {name} = {rtn}::try_from({name}_raw).map_err(|_| {{\n\
                 {indent}    ParseError::UnknownDiscriminant {{\n\
                 {indent}        type_name: \"{rtn}\",\n\
                 {indent}        value: {name}_raw as u32,\n\
                 {indent}    }}\n\
                 {indent}}})?;\n"
            ));
        }

        "type_alias" => {
            let (method, _) = alias_rw(field, schema);
            out.push_str(&format!(
                "{indent}let {name} = view.{method}(addr + {byte_offset}u32)?;\n"
            ));
        }

        "padding" => {
            // Nothing to read; the offset accounting is done by the caller.
        }

        _ => {
            // Nothing to read; the offset accounting is done by the caller.
        }
    }
}

// ---------------------------------------------------------------------------
// Section: file header
// ---------------------------------------------------------------------------

fn push_file_header(out: &mut String) {
    out.push_str(
        "// @generated — do not edit by hand.\n\
         // Source:    firmware/metadata_schema.toml\n\
         // Generator: build/rust_gen.rs\n\
         //\n\
         // Regenerate by running `cargo build` in rust/metadata/.\n\n",
    );
}

// ---------------------------------------------------------------------------
// Section: constants
// ---------------------------------------------------------------------------

fn push_constants(out: &mut String, schema: &Schema) {
    if schema.constants.is_empty() {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Constants\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for c in &schema.constants {
        if let Some(cmt) = &c.comment {
            out.push_str(&format!("/// {cmt}\n"));
        }
        let rust_ty = match c.type_.as_str() {
            "u8" => "u8",
            "u16" => "u16",
            "u32" => "u32",
            "usize" => "usize",
            "cstr" => "&str",
            _ => "u32",
        };
        let name = &c.name;
        match &c.value {
            ConstantValue::Integer(v) => {
                out.push_str(&format!("pub const {name}: {rust_ty} = {v};\n\n"));
            }
            ConstantValue::Text(s) => {
                // Use Rust's Debug formatting to produce a properly escaped
                // string literal, then emit it as a &'static str constant.
                out.push_str(&format!("pub const {name}: {rust_ty} = {s:?};\n\n"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Section: type aliases
// ---------------------------------------------------------------------------

fn push_type_aliases(out: &mut String, schema: &Schema) {
    if schema.type_aliases.is_empty() {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Type aliases\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for ta in &schema.type_aliases {
        if let Some(cmt) = &ta.comment {
            for line in cmt.lines() {
                out.push_str(&format!("/// {}\n", line.trim()));
            }
        }
        let rn = rust_type_name(&ta.name);
        let underlying = &ta.underlying;
        out.push_str(&format!("pub type {rn} = {underlying};\n\n"));
    }
}

// ---------------------------------------------------------------------------
// Section: enums
// ---------------------------------------------------------------------------

fn push_enums(out: &mut String, schema: &Schema) {
    if schema.enums.is_empty() {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Enums\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for e in &schema.enums {
        push_enum(out, e);
    }
}

fn push_enum(out: &mut String, e: &Enum) {
    // Enums sourced from external data (e.g. chip-types.json) do not generate
    // a Rust type.  The C generator handles those; consumers use the relevant
    // crate's API directly (e.g. ChipType::try_from_rbcp_u8() from onerom_config).
    if e.source.is_some() {
        return;
    }

    let tn = rust_type_name(&e.name);
    let strip = e.strip_prefix.as_deref().unwrap_or("");
    // u8 for size<=1, u16 for size==2.
    let repr = if e.size <= 1 { "u8" } else { "u16" };

    // Struct-level doc comment.
    if let Some(cmt) = &e.comment {
        for line in cmt.lines() {
            out.push_str(&format!("/// {}\n", line.trim()));
        }
    }

    // Enum definition — only non-sentinel variants.
    out.push_str(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n",
    );
    out.push_str(&format!("#[repr({repr})]\n"));
    out.push_str(&format!("pub enum {tn} {{\n"));
    for v in e.variants.iter().filter(|v| !v.is_sentinel()) {
        if let Some(cmt) = &v.comment {
            out.push_str(&format!("    /// {}\n", cmt.trim()));
        }
        let vn = variant_ident(&v.name, strip);
        out.push_str(&format!("    {vn} = {},\n", v.value));
    }
    out.push_str("}\n\n");

    // Sentinel variants become free-standing pub constants.
    let has_sentinels = e.variants.iter().any(|v| v.is_sentinel());
    for v in e.variants.iter().filter(|v| v.is_sentinel()) {
        if let Some(cmt) = &v.comment {
            out.push_str(&format!("/// {}\n", cmt.trim()));
        }
        out.push_str(&format!("pub const {}: {repr} = {};\n", v.name, v.value));
    }

    // Alias variants become pub constants pointing at the target variant.
    for a in &e.aliases {
        if let Some(cmt) = &a.comment {
            out.push_str(&format!("/// {}\n", cmt.trim()));
        }
        let target_vn = e
            .variants
            .iter()
            .find(|v| v.name == a.target)
            .map_or_else(String::new, |v| variant_ident(&v.name, strip));
        out.push_str(&format!(
            "pub const {}: {tn} = {tn}::{target_vn};\n",
            a.name
        ));
    }

    if has_sentinels || !e.aliases.is_empty() {
        out.push('\n');
    }

    // TryFrom<repr> — maps discriminant values back to variants.
    out.push_str(&format!(
        "impl core::convert::TryFrom<{repr}> for {tn} {{\n"
    ));
    out.push_str("    type Error = ();\n");
    out.push_str(&format!(
        "    fn try_from(value: {repr}) -> Result<Self, Self::Error> {{\n"
    ));
    out.push_str("        match value {\n");
    for v in e.variants.iter().filter(|v| !v.is_sentinel()) {
        let vn = variant_ident(&v.name, strip);
        out.push_str(&format!("            {} => Ok(Self::{vn}),\n", v.value));
    }
    out.push_str("            _ => Err(()),\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str(&format!("impl core::fmt::Display for {tn} {{\n"));
    out.push_str("    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n");
    out.push_str("        f.write_str(match self {\n");
    for v in e.variants.iter().filter(|v| !v.is_sentinel()) {
        let vn = variant_ident(&v.name, strip);
        let disp = v
            .display
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| display_string(&v.name, strip));
        out.push_str(&format!("            Self::{vn} => {disp:?},\n"));
    }
    out.push_str("        })\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Section: structs
// ---------------------------------------------------------------------------

fn push_structs(out: &mut String, schema: &Schema) {
    let any = schema.structs.iter().any(|s| s.generate != Generate::Skip);
    if !any {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Structs\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for s in &schema.structs {
        if s.generate == Generate::Skip {
            continue;
        }
        push_struct_def(out, s);
        push_struct_parse(out, s, schema);
    }
}

/// serde's built-in array impls stop at length 32; a fixed array whose outer
/// length exceeds that needs `serde_big_array::BigArray`. Only the outer length
/// matters — the element type serialises on its own.
fn field_needs_big_array(f: &Field) -> bool {
    match f.kind.as_str() {
        "inline_array" => f.count.unwrap_or(0) > 32,
        "inline_array2d" => f.rows.unwrap_or(0) > 32,
        _ => false,
    }
}

fn push_struct_def(out: &mut String, s: &Struct) {
    let tn = rust_type_name(&s.name);

    if let Some(cmt) = &s.comment {
        for line in cmt.lines() {
            out.push_str(&format!("/// {}\n", line.trim()));
        }
    }

    let derives = if s.generate == Generate::Both {
        "#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]"
    } else {
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]"
    };
    out.push_str(derives);
    out.push('\n');
    out.push_str(&format!("pub struct {tn} {{\n"));

    for f in s.fields.iter().filter(|f| f.kind != "padding") {
        if let Some(cmt) = &f.comment {
            // Only the first line of multi-line comments, for compactness.
            if let Some(first) = cmt.lines().next() {
                out.push_str(&format!("    /// {}\n", first.trim()));
            }
        }
        if field_needs_big_array(f) {
            out.push_str("    #[serde(with = \"serde_big_array::BigArray\")]\n");
        }
        let ftype = field_rust_type(f);
        out.push_str(&format!("    pub {}: {ftype},\n", f.name));
    }
    out.push_str("}\n\n");

    if let Some(sz) = s.size {
        out.push_str(&format!(
            "/// Binary size of [`{tn}`] in bytes.\npub const {}_SIZE: usize = {sz};\n\n",
            s.name.strip_suffix("_t").unwrap_or(&s.name).to_uppercase()
        ));
    }
}

fn push_struct_parse(out: &mut String, s: &Struct, schema: &Schema) {
    let tn = rust_type_name(&s.name);

    out.push_str(&format!("impl {tn} {{\n"));
    out.push_str(
        "    pub fn parse(view: &DeviceMemoryView, addr: u32) -> Result<Self, ParseError> {\n",
    );
    out.push_str("        #[allow(unused_variables)]\n");
    out.push_str("        let mut offset = addr;\n");

    // Detect array fields whose count_field appears AFTER them in the layout.
    // These require splitting the pointer read from the loop body.
    // We track their indices so we can emit the loop after the count is parsed.
    let mut pending: Vec<usize> = Vec::new(); // field indices with deferred loops

    for (idx, f) in s.fields.iter().enumerate() {
        // Is this an array whose count comes later?
        let defer = matches!(f.kind.as_str(), "struct_array_ptr" | "struct_ptr_array_ptr") && {
            let count_name = f.count_field.as_deref().unwrap_or("");
            // Count must appear somewhere after idx.
            s.fields[idx + 1..].iter().any(|sf| sf.name == count_name)
        };

        if defer {
            emit_array_ptr_read(out, f, "        ");
            pending.push(idx);
        } else {
            emit_field_parse_offset(out, f, "        ", schema);
        }

        // After emitting this field, emit any loops that are now unblocked.
        let fname = f.name.as_str();
        let unblocked: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|&pi| s.fields[pi].count_field.as_deref() == Some(fname))
            .collect();
        for pi in unblocked {
            emit_array_loop_body(out, &s.fields[pi], "        ", schema);
            pending.retain(|&x| x != pi);
        }
    }

    // Safety net: any still-pending loops get emitted at the end.
    // A valid schema should never reach here; it's a guard against schema bugs.
    for pi in pending {
        out.push_str(
            "        // WARNING: deferred array loop emitted out of order; \
             count_field not found\n",
        );
        emit_array_loop_body(out, &s.fields[pi], "        ", schema);
    }

    // Return Ok(Self { field, ... }) — padding fields excluded.
    out.push_str("        let _ = offset;\n"); // silence unused offset warning
    out.push_str("        Ok(Self {\n");
    for f in s.fields.iter().filter(|f| f.kind != "padding") {
        out.push_str(&format!("            {},\n", f.name));
    }
    out.push_str("        })\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Section: tagged FAMs
// ---------------------------------------------------------------------------

fn push_tagged_fams(out: &mut String, schema: &Schema) {
    let any = schema
        .tagged_fams
        .iter()
        .any(|tf| tf.generate != Generate::Skip);
    if !any {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Tagged FAMs (variable-length, discriminated)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for tf in &schema.tagged_fams {
        if tf.generate == Generate::Skip {
            continue;
        }
        push_tagged_fam(out, tf, schema);
    }
}

fn push_tagged_fam(out: &mut String, tf: &TaggedFam, schema: &Schema) {
    let tn = rust_type_name(&tf.name);

    // Resolve the discriminant enum once; use its strip_prefix for variant naming.
    let disc_enum = schema.enums.iter().find(|e| e.name == tf.discriminant_type);
    let strip = disc_enum
        .and_then(|e| e.strip_prefix.as_deref())
        .unwrap_or("");

    // ---- Rust enum definition ----------------------------------------
    if let Some(cmt) = &tf.comment {
        for line in cmt.lines() {
            out.push_str(&format!("/// {}\n", line.trim()));
        }
    }
    let derives = if tf.generate == Generate::Both {
        "#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]"
    } else {
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]"
    };
    out.push_str(derives);
    out.push('\n');
    out.push_str(&format!("pub enum {tn} {{\n"));

    for v in &tf.variants {
        if let Some(cmt) = &v.comment {
            out.push_str(&format!("    /// {}\n", cmt.trim()));
        }
        let vn = variant_ident(&v.discriminant, strip);
        out.push_str(&format!("    {vn} {{\n"));

        // Common fields (shared by every variant).
        for f in tf.common_fields.iter().filter(|f| f.kind != "padding") {
            if let Some(cmt) = &f.comment {
                if let Some(first) = cmt.lines().next() {
                    out.push_str(&format!("        /// {}\n", first.trim()));
                }
            }
            out.push_str(&format!("        {}: {},\n", f.name, field_rust_type(f)));
        }
        // Variant-specific param fields.
        for f in v.fields.iter().filter(|f| f.kind != "padding") {
            if let Some(cmt) = &f.comment {
                if let Some(first) = cmt.lines().next() {
                    out.push_str(&format!("        /// {}\n", first.trim()));
                }
            }
            out.push_str(&format!("        {}: {},\n", f.name, field_rust_type(f)));
        }
        out.push_str("    },\n");
    }
    out.push_str("}\n\n");

    // ---- parse impl --------------------------------------------------
    push_tagged_fam_parse(out, tf, schema, &tn, strip, disc_enum);
}

fn push_tagged_fam_parse(
    out: &mut String,
    tf: &TaggedFam,
    schema: &Schema,
    tn: &str,
    strip: &str,
    disc_enum: Option<&Enum>,
) {
    // Binary layout: [discriminant (1–2 B)] [param_len (1 B)] [common fields] [params…]
    let disc_size = disc_enum.map_or(1, |e| e.size) as usize;
    let disc_reader = if disc_size == 1 {
        "read_u8"
    } else {
        "read_u16_le"
    };
    let param_len_off = disc_size; // byte offset of param_len
    let common_start = disc_size + 1; // byte offset of first common field

    out.push_str(&format!("impl {tn} {{\n"));
    out.push_str(
        "    pub fn parse(view: &DeviceMemoryView, addr: u32) -> Result<Self, ParseError> {\n",
    );

    // Discriminant and param_len (param_len is read for future use / validation).
    out.push_str(&format!(
        "        let discriminant = view.{disc_reader}(addr)?;\n"
    ));
    out.push_str(&format!(
        "        let _param_len = view.read_u8(addr + {param_len_off}u32)?;\n"
    ));

    // Common fields at statically known addresses (no mutable offset variable
    // shared across match arms, which avoids spurious compiler warnings).
    let mut byte_off = common_start;
    for f in &tf.common_fields {
        emit_field_at_addr(out, f, byte_off, "        ", schema);
        byte_off += field_size(f, schema);
    }
    // byte_off == base_size at this point.

    // Match on discriminant; each arm reads its variant-specific params.
    out.push_str("        match discriminant {\n");
    for v in &tf.variants {
        let disc_val = disc_enum
            .and_then(|e| e.variants.iter().find(|ev| ev.name == v.discriminant))
            .map_or(0, |ev| ev.value);
        let vn = variant_ident(&v.discriminant, strip);

        out.push_str(&format!("            {disc_val} => {{\n"));

        // Variant param fields, continuing from byte_off (= base_size).
        let mut vbyte_off = byte_off;
        for f in &v.fields {
            emit_field_at_addr(out, f, vbyte_off, "                ", schema);
            vbyte_off += field_size(f, schema);
        }

        out.push_str(&format!("                Ok(Self::{vn} {{\n"));
        for f in tf.common_fields.iter().filter(|f| f.kind != "padding") {
            out.push_str(&format!("                    {},\n", f.name));
        }
        for f in v.fields.iter().filter(|f| f.kind != "padding") {
            out.push_str(&format!("                    {},\n", f.name));
        }
        out.push_str("                })\n");
        out.push_str("            }\n");
    }

    out.push_str(&format!(
        "            _ => Err(ParseError::UnknownDiscriminant {{\n\
         {SP}type_name: \"{tn}\",\n\
         {SP}value: discriminant as u32,\n\
         {SP}}}\n\
         {SP}),\n",
        SP = "                "
    ));
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Section: simple FAMs
// ---------------------------------------------------------------------------

fn push_simple_fams(out: &mut String, schema: &Schema) {
    let any = schema
        .simple_fams
        .iter()
        .any(|sf| sf.generate != Generate::Skip);
    if !any {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Simple FAMs (length-prefixed byte arrays)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for sf in &schema.simple_fams {
        if sf.generate == Generate::Skip {
            continue;
        }
        push_simple_fam(out, sf);
    }
}

fn push_simple_fam(out: &mut String, sf: &SimpleFam) {
    let tn = rust_type_name(&sf.name);

    if let Some(cmt) = &sf.comment {
        for line in cmt.lines() {
            out.push_str(&format!("/// {}\n", line.trim()));
        }
    }
    let derives = if sf.generate == Generate::Both {
        "#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]"
    } else {
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]"
    };
    out.push_str(derives);
    out.push('\n');
    out.push_str(&format!("pub struct {tn} {{\n"));
    out.push_str("    pub params: Vec<u8>,\n");
    out.push_str("}\n\n");

    // Parse: read param_len byte, then slice_at for the bytes.
    out.push_str(&format!("impl {tn} {{\n"));
    out.push_str(
        "    pub fn parse(view: &DeviceMemoryView, addr: u32) -> Result<Self, ParseError> {\n",
    );
    out.push_str("        let param_len = view.read_u8(addr)? as usize;\n");
    out.push_str("        let params = view.slice_at(addr + 1, param_len)?.to_vec();\n");
    out.push_str("        Ok(Self { params })\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Section: Display
// ---------------------------------------------------------------------------

fn display_string(c_name: &str, strip_prefix: &str) -> String {
    let stripped = if strip_prefix.is_empty() {
        c_name
    } else {
        c_name.strip_prefix(strip_prefix).unwrap_or(c_name)
    };
    if stripped.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        stripped.replace('_', ".") // 0_55V → 0.55V, 2316 → 2316, 0 → 0
    } else {
        stripped.to_lowercase().replace('_', " ") // ACTIVE_LOW → active low
    }
}