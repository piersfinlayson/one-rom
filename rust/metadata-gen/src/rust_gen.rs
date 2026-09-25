// src/rust_gen.rs
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
    push_file_header(&mut out, schema);
    push_generations(&mut out, schema);
    push_constants(&mut out, schema);
    push_metadata_generation_table(&mut out, schema);
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
// Section: structure generations
// ---------------------------------------------------------------------------

/// Emit `Generations`, the generation each top-level structure was found to
/// carry, which every generated `parse` takes and passes down its tree.
fn push_generations(out: &mut String, schema: &Schema) {
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Structure generations\n\
         // ---------------------------------------------------------------------------\n\n",
    );

    out.push_str(
        "/// The generation each top-level structure was found to carry.\n\
         ///\n\
         /// One is threaded through every `parse`, because a field added after its\n\
         /// structure's first generation is not present in a structure written before\n\
         /// it, and nothing in the bytes says so - unwritten metadata is 0xFF, which\n\
         /// the schema also uses as a real value.  Such a field yields its declared\n\
         /// default instead of whatever sits at its offset.\n\
         ///\n\
         /// A structure fills in its own slot from the bytes as it parses, and passes\n\
         /// the result down.  A slot nothing has filled in reads 0, older than any\n\
         /// generation a device carries, so a field gated on one yields its default\n\
         /// until the structure that governs it has been read.\n",
    );
    out.push_str(
        "///\n\
         /// A slot is added here whenever a structure starts carrying a generation,\n\
         /// so the struct is non_exhaustive and reached through [`Generations::UNKNOWN`]\n\
         /// and the `with_` methods rather than built as a literal.\n",
    );
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]\n");
    out.push_str("#[non_exhaustive]\n");
    out.push_str("pub struct Generations {\n");
    for (name, slot) in schema.generation_slots() {
        out.push_str(&format!("    /// Generation `{name}` carries.\n"));
        out.push_str(&format!("    pub {slot}: u32,\n"));
    }
    out.push_str("}\n\n");

    out.push_str("impl Generations {\n");
    out.push_str("    /// No generation read yet.  Every slot is older than anything a\n");
    out.push_str("    /// device carries, so every gated field yields its default.\n");
    out.push_str("    pub const UNKNOWN: Self = Self {\n");
    for (_, slot) in schema.generation_slots() {
        out.push_str(&format!("        {slot}: 0,\n"));
    }
    out.push_str("    };\n");

    for (name, slot) in schema.generation_slots() {
        out.push('\n');
        out.push_str(&format!(
            "    /// This, with the generation `{name}` was found to carry.\n"
        ));
        out.push_str(&format!(
            "    pub const fn with_{slot}(mut self, generation: u32) -> Self {{\n"
        ));
        out.push_str(&format!("        self.{slot} = generation;\n"));
        out.push_str("        self\n");
        out.push_str("    }\n");
    }
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Field Rust type string
// ---------------------------------------------------------------------------

fn field_rust_type(field: &Field) -> String {
    match field.kind.as_str() {
        "scalar" => field.type_.as_deref().unwrap_or("u8").to_string(),

        // An enum field holds a value the list may have grown past, so the
        // wrapper is the field's type rather than the enum itself.  See
        // `MaybeKnown` in src/lib.rs.
        "enum" => format!(
            "MaybeKnown<{}>",
            rust_type_name(field.type_.as_deref().unwrap_or(""))
        ),

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
// Fixed-list field helpers
// ---------------------------------------------------------------------------

/// Emit the binding that turns `{name}_raw` into the field's `MaybeKnown`.
///
/// A value the list has grown past is kept rather than refused, so a structure
/// carrying one still parses and its other fields still hold what the device
/// holds.
fn push_maybe_known_bind(out: &mut String, name: &str, rust_type: &str, indent: &str) {
    out.push_str(&format!(
        "{indent}let {name} = match {rust_type}::try_from({name}_raw) {{\n\
         {indent}    Ok(value) => MaybeKnown::Known(value),\n\
         {indent}    Err(_) => MaybeKnown::Unknown({name}_raw as u32),\n\
         {indent}}};\n"
    ));
}

/// The expression giving an enum field's stored value back as `repr`, so a
/// value this build has no name for goes back out as it came in.
///
/// Emitted on one line, because the caller decides the indentation of the
/// statement this sits inside and a wrapped arm would not follow it.
pub fn maybe_known_raw_expr(field_expr: &str, repr: &str) -> String {
    format!(
        "match {field_expr} {{ \
         MaybeKnown::Known(value) => value as {repr}, \
         MaybeKnown::Unknown(raw) => raw as {repr} }}"
    )
}

// ---------------------------------------------------------------------------
// Parse code emission — mutable-offset style (structs)
// ---------------------------------------------------------------------------

/// Emit the expected-offset assertion for an ABI-stable field.
///
/// Unconditional, not a debug_assert: it compares the parser's own walk
/// against the offset the schema declares, so only a generator bug can make it
/// fire, and a release build without it would read the wrong bytes in silence.
///
/// It sits ahead of the read so a field the generation gates is checked for
/// position whether its bytes are read or skipped.
fn emit_expected_offset_assert(out: &mut String, field: &Field, indent: &str) {
    let name = &field.name;
    if let Some(expected) = field.expected_offset {
        out.push_str(&format!(
            "{indent}assert_eq!(\n\
             {indent}    (offset - addr) as usize,\n\
             {indent}    {expected}usize,\n\
             {indent}    \"field `{name}` not at expected offset {expected}\",\n\
             {indent});\n"
        ));
    }
}

/// Emit the DeviceMemoryView read + `offset +=` code for one struct field.
///
/// For `struct_array_ptr` and `struct_ptr_array_ptr` this emits both the
/// pointer read and the loop body in one shot (used when the count field
/// precedes the array field).  When the count field comes *after* the array
/// field, `push_struct_parse` calls `emit_array_ptr_read` and
/// `emit_array_loop_body` separately instead of this function.
fn emit_field_parse_offset(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let name = &field.name;

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
            push_maybe_known_bind(out, name, &rtn, indent);
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
            // Optional magic validation: compare the leading bytes against
            // the generated constants named. More than one name means the
            // value changed between firmware generations, and every value
            // named is still read, so the check takes any of them.
            if let Some(expected) = field.expected_const.as_ref() {
                let rejects = expected
                    .names()
                    .iter()
                    .map(|konst| format!("!{name}.starts_with({konst}.as_bytes())"))
                    .collect::<Vec<_>>()
                    .join(" && ");
                out.push_str(&format!(
                    "{indent}if {rejects} {{\n\
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
                    format!("{tn}::parse(view, {name}_ptr, generations).ok()")
                } else {
                    format!("Some({tn}::parse(view, {name}_ptr, generations)?)")
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
                    "{indent}if {name}_ptr == 0 || {name}_ptr == 0xFFFF_FFFF {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n\
                     {indent}let {name} = {tn}::parse(view, {name}_ptr, generations)?;\n"
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
                    format!("{tn}::parse(view, {name}_ptr, generations).ok()")
                } else {
                    format!("Some({tn}::parse(view, {name}_ptr, generations)?)")
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
                    "{indent}if {name}_ptr == 0 || {name}_ptr == 0xFFFF_FFFF {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n\
                     {indent}let {name} = {tn}::parse(view, {name}_ptr, generations)?;\n"
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
                    "{indent}if {name}_ptr == 0 || {name}_ptr == 0xFFFF_FFFF {{\n\
                     {indent}    return Err(ParseError::NullPointer {{ field: \"{name}\" }});\n\
                     {indent}}}\n"
                ));
            }
            out.push_str(&format!("{indent}let mut {name} = Vec::new();\n"));

            let li = if nullable {
                out.push_str(&format!(
                    "{indent}if {name}_ptr != 0 && {name}_ptr != 0xFFFF_FFFF {{\n"
                ));
                format!("{indent}    ")
            } else {
                indent.to_string()
            };
            out.push_str(&format!("{li}for i in 0usize..({count_f} as usize) {{\n"));
            out.push_str(&format!(
                "{li}    let ea = {name}_ptr + (i as u32 * {stride}u32);\n"
            ));
            out.push_str(&format!(
                "{li}    {name}.push({elem_rn}::parse(view, ea, generations)?);\n"
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
                "{li}    if inner_ptr == 0 || inner_ptr == 0xFFFF_FFFF {{\n\
                 {li}        return Err(ParseError::NullPointer {{ field: \"{name}[]\" }});\n\
                 {li}    }}\n"
            ));
            out.push_str(&format!(
                "{li}    {name}.push({elem_rn}::parse(view, inner_ptr, generations)?);\n"
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
            push_maybe_known_bind(out, name, &rtn, indent);
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

fn push_file_header(out: &mut String, schema: &Schema) {
    out.push_str(&format!(
        "// @generated — do not edit by hand.\n\
         // Source:    {}\n\
         // Generator: onerom-metadata-gen rust_gen.rs\n\
         //\n\
         // Regenerated by every build of the crate that owns the schema.\n\n",
        schema.source
    ));
}

// ---------------------------------------------------------------------------
// Section: constants
// ---------------------------------------------------------------------------

/// Emit a doc comment as one `/// ` line per source line, prefixed with
/// `indent`.  Correctly handles multi-line (triple-quoted) schema comments;
/// all doc-comment emission routes through here so a single-line variant that
/// silently drops later lines cannot creep back in.
fn push_doc_comment(out: &mut String, indent: &str, comment: &str) {
    for line in comment.lines() {
        out.push_str(&format!("{indent}/// {}\n", line.trim()));
    }
}

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
        if let Some(cmt) = c.documentation() {
            push_doc_comment(out, "", &cmt);
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

    push_constant_table(out, schema);
}

/// Emit the `[[versions]]` entries for the metadata header, as the release
/// each generation arrived in.  A tool composing an image turns the target
/// firmware's version into a generation, and the schema is the only answer.
fn push_metadata_generation_table(out: &mut String, schema: &Schema) {
    let generations = schema.metadata_generations();
    if generations.is_empty() {
        return;
    }

    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Metadata generations\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    out.push_str(
        "/// Every metadata generation, oldest first, with the firmware release that\n\
         /// introduced it.\n",
    );
    // A host writes the metadata only where the schema has a metadata region.
    // metadata_generation_for is onerom-metadata's, for that host.
    if schema.schema.metadata_base.is_some() {
        out.push_str(
            "///\n\
             /// A host writes the metadata and the firmware reads it, and the two are of\n\
             /// different ages.  This is what says which generation a given firmware\n\
             /// reads - see [`metadata_generation_for`], which is how a caller asks.\n",
        );
    }
    out.push_str("pub const METADATA_GENERATIONS: &[(FirmwareVersion, u32)] = &[\n");
    for (generation, (major, minor, patch)) in generations {
        out.push_str(&format!(
            "    (FirmwareVersion::new({major}, {minor}, {patch}, 0), {generation}),\n"
        ));
    }
    out.push_str("];\n\n");
}

/// Emit every constant a second time, as name and value in plain text.
///
/// A `pub const` cannot be used where Rust demands a literal - a doc comment
/// among them, which is where the CLI states the periods and holds a One ROM
/// takes when a command names none. A consumer's build script walks this table
/// and writes each value to a file it can then `include_str!`, so the number a
/// user reads in `--help` is the number the firmware was built with.
///
/// Every constant is here rather than a tagged subset: a name nobody includes
/// costs nothing, and gating it would be one more thing to remember when adding
/// a constant. Integers are rendered in decimal, because this is what a person
/// reads, not what a compiler takes.
fn push_constant_table(out: &mut String, schema: &Schema) {
    out.push_str(
        r####"// ---------------------------------------------------------------------------
// Constants as text
// ---------------------------------------------------------------------------

/// Every schema constant, as `(name, value)` with the value rendered as the
/// plain text a reader sees - integers in decimal, strings as themselves.
///
/// # Why this exists
///
/// Rust demands a *literal* in some positions, and a `pub const` is not one.
/// A doc comment is the position that matters here: clap takes an option's
/// help text from it, so a tool wanting to state a value the firmware chose -
/// the period a mode runs at when a command names none, the longest hold a
/// device accepts - cannot do it by naming the constant. Written out by hand
/// instead, the number silently stops matching the firmware the day the schema
/// changes.
///
/// `include_str!` is accepted there, and so is `concat!`. So a consumer writes
/// each value to its own file at build time and includes the one it wants.
///
/// # How to use it
///
/// Take this crate as a build dependency:
///
/// ```toml
/// [build-dependencies]
/// onerom-metadata = "0.2"
/// ```
///
/// Write every constant to `OUT_DIR` from `build.rs`. Name no constant here,
/// and one added to the schema later needs no change:
///
/// ```no_run
/// use std::{env, fs, path::PathBuf};
///
/// let dir = PathBuf::from(env::var("OUT_DIR").unwrap()).join("const");
/// fs::create_dir_all(&dir).unwrap();
/// for (name, value) in onerom_metadata::ALL_CONSTANTS {
///     fs::write(dir.join(format!("{name}.txt")), value).unwrap();
/// }
/// ```
///
/// Then read one where a literal is required. A macro keeps the call sites
/// short:
///
/// ```ignore
/// macro_rules! const_str {
///     ($name:literal) => {
///         include_str!(concat!(env!("OUT_DIR"), "/const/", $name, ".txt"))
///     };
/// }
///
/// const HELP_PERIOD: &str = concat!(
///     "Milliseconds for one blink. Defaults to ",
///     const_str!("LED_BEACON_DEFAULT_PERIOD_MS"),
///     "."
/// );
/// ```
///
/// Note the `const`. A `#[doc = concat!(...)]` on the field compiles, but a
/// consumer using clap gets an option with no help at all, because clap reads
/// a doc comment as a literal and sees an unexpanded macro.
///
/// A name with no such constant fails the build and says which file it looked
/// for, so a typo or a retired constant is caught rather than leaving a stale
/// number in place.
///
/// Every constant is listed, not a tagged subset: a name nobody includes costs
/// a file nobody opens, and gating it would be one more thing to remember when
/// adding a constant.
pub const ALL_CONSTANTS: &[(&str, &str)] = &[
"####,
    );
    for c in &schema.constants {
        let name = &c.name;
        let text = match &c.value {
            ConstantValue::Integer(v) => v.to_string(),
            ConstantValue::Text(s) => s.clone(),
        };
        out.push_str(&format!("    ({name:?}, {text:?}),\n"));
    }
    out.push_str("];\n\n");
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
            push_doc_comment(out, "", cmt);
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
        push_doc_comment(out, "", cmt);
    }

    // Enum definition — only non-sentinel variants.
    out.push_str(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n",
    );
    out.push_str(&format!("#[repr({repr})]\n"));
    out.push_str(&format!("pub enum {tn} {{\n"));
    for v in e.variants.iter().filter(|v| !v.is_sentinel()) {
        if let Some(cmt) = &v.comment {
            push_doc_comment(out, "    ", cmt);
        }
        let vn = variant_ident(&v.name, strip);
        out.push_str(&format!("    {vn} = {},\n", v.value));
    }
    out.push_str("}\n\n");

    // Sentinel variants become free-standing pub constants.
    let has_sentinels = e.variants.iter().any(|v| v.is_sentinel());
    for v in e.variants.iter().filter(|v| v.is_sentinel()) {
        if let Some(cmt) = &v.comment {
            push_doc_comment(out, "", cmt);
        }
        out.push_str(&format!("pub const {}: {repr} = {};\n", v.name, v.value));
    }

    // Alias variants become pub constants pointing at the target variant.
    for a in &e.aliases {
        if let Some(cmt) = &a.comment {
            push_doc_comment(out, "", cmt);
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
        push_struct_def(out, s, schema);
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

fn push_struct_def(out: &mut String, s: &Struct, schema: &Schema) {
    let tn = rust_type_name(&s.name);

    if let Some(cmt) = &s.comment {
        push_doc_comment(out, "", cmt);
    }

    let derives = if s.generate == Generate::Both {
        "#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]"
    } else {
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]"
    };
    out.push_str(derives);
    out.push('\n');
    // The allow covers this item alone, so code outside still hears the note.
    push_deprecated_allow(out, s.fields.iter(), "");
    out.push_str(&format!("pub struct {tn} {{\n"));

    for f in s.fields.iter().filter(|f| f.kind != "padding") {
        if let Some(cmt) = &f.comment {
            // Only the first line of multi-line comments, for compactness.
            if let Some(first) = cmt.lines().next() {
                out.push_str(&format!("    /// {}\n", first.trim()));
            }
        }
        push_since_doc(out, f, "    ", schema);
        push_deprecated_attribute(out, f, "    ", schema);
        if field_needs_big_array(f) {
            out.push_str("    #[serde(with = \"serde_big_array::BigArray\")]\n");
        }
        let ftype = field_rust_type(f);
        out.push_str(&format!("    pub {}: {ftype},\n", f.name));
    }
    out.push_str("}\n\n");

    let stem = s.name.strip_suffix("_t").unwrap_or(&s.name).to_uppercase();

    if let Some(sz) = s.size {
        out.push_str(&format!(
            "/// Binary size of [`{tn}`] in bytes.\npub const {stem}_SIZE: usize = {sz};\n\n"
        ));
    }

    push_field_offsets(out, s, schema, &stem, &tn);
}

/// Emit a byte-offset constant for each field carrying `expected_offset`.
///
/// A host bootstrapping its parse of `onerom_info_t` reads pointers out of the
/// header before it knows enough to run the generated parser, and these
/// constants are what it reads them at, in place of a literal that would go
/// stale in silence.
///
/// The value emitted is the generator's own layout walk.  `Schema::load` has
/// already refused a schema where that disagrees with `expected_offset`.
fn push_field_offsets(out: &mut String, s: &Struct, schema: &Schema, stem: &str, tn: &str) {
    let offsets = field_offsets(s, schema);
    for (f, offset) in s.fields.iter().zip(offsets) {
        if f.expected_offset.is_none() {
            continue;
        }
        let fname = f.name.to_uppercase();
        out.push_str(&format!(
            "/// Byte offset of `{}` within [`{tn}`].\npub const {stem}_{fname}_OFFSET: usize = {offset};\n\n",
            f.name
        ));
    }
}

// ---------------------------------------------------------------------------
// Generation markers on the generated types
// ---------------------------------------------------------------------------

/// The structure filling `slot`, for a comment naming it.  A marker names a
/// slot, and a reader of the generated source wants the structure.
fn governing_struct<'a>(schema: &'a Schema, slot: &'a str) -> &'a str {
    schema
        .struct_in_slot(slot)
        .map_or(slot, |s| s.name.as_str())
}

/// Say, on the field, which generation brought it in and what a reader of an
/// older structure gets instead.
fn push_since_doc(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let Some((governor, generation)) = field.since_marker() else {
        return;
    };
    // `"null"` is how the schema says a pointer is not there, which is not
    // how a doc comment says it.
    let default = match field.default_if_absent.as_ref() {
        Some(v) if v.as_str() == Some(NULL_DEFAULT) => "nothing".to_string(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    let governor = governing_struct(schema, governor);
    out.push_str(&format!(
        "{indent}/// Arrived in `{governor}` generation {generation}.  An older \
         structure reads {default} here.\n"
    ));
}

/// Mark a deprecated field, so anything reaching for it is told.  Its bytes
/// are still written and still parsed - what changes is that nothing should be
/// decided from them.
fn push_deprecated_attribute(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let Some((governor, generation)) = field.deprecated_marker() else {
        return;
    };
    let governor = governing_struct(schema, governor);
    out.push_str(&format!(
        "{indent}#[deprecated(note = \"deprecated from {governor} generation {generation}\")]\n"
    ));
}

/// Emit `#[allow(deprecated)]` where any of `fields` is deprecated.
///
/// Generated code reads every field it was given, so it would raise the note
/// on the reader's behalf and say nothing about the reader's own code.
pub(crate) fn push_deprecated_allow<'a>(
    out: &mut String,
    mut fields: impl Iterator<Item = &'a Field>,
    indent: &str,
) {
    if fields.any(|f| f.deprecated_marker().is_some()) {
        out.push_str(&format!("{indent}#[allow(deprecated)]\n"));
    }
}

/// Emit the signature every generated `parse` shares.
///
/// A parse that gates nothing and points at nothing still takes `generations`,
/// so a structure that later gains a gated field does not change shape.
fn push_parse_signature(out: &mut String) {
    out.push_str("    pub fn parse(\n");
    out.push_str("        view: &DeviceMemoryView,\n");
    out.push_str("        addr: u32,\n");
    out.push_str("        #[allow(unused_variables)] generations: Generations,\n");
    out.push_str("    ) -> Result<Self, ParseError> {\n");
}

/// Emit the read for one struct field, behind its generation where it has one.
///
/// Below the generation the bytes are not read, but the offset still moves on
/// so the walk stays in step, and the field takes the schema's declared
/// default.
fn emit_gated_field_parse(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    emit_expected_offset_assert(out, field, indent);

    let Some((governor, generation)) = field.since_marker() else {
        emit_field_parse_offset(out, field, indent, schema);
        return;
    };
    let slot = governor;
    let name = &field.name;
    let size = field_size(field, schema);
    let default = rust_default_expr(field, schema);

    out.push_str(&format!(
        "{indent}// {name} arrived in {} generation {generation}.\n",
        governing_struct(schema, governor)
    ));
    out.push_str(&format!(
        "{indent}let {name} = if generations.{slot} >= {generation}u32 {{\n"
    ));
    emit_field_parse_offset(out, field, &format!("{indent}    "), schema);
    out.push_str(&format!("{indent}    {name}\n"));
    out.push_str(&format!("{indent}}} else {{\n"));
    out.push_str(&format!("{indent}    offset += {size};\n"));
    out.push_str(&format!("{indent}    {default}\n"));
    out.push_str(&format!("{indent}}};\n"));
}

/// Emit the pointer read for an array field whose count comes later, behind
/// its generation where it has one.
///
/// The two halves of such a field are emitted apart, so each is gated on its
/// own.  Below the generation the walk carries a null the loop half never
/// reaches.
fn emit_deferred_array_ptr_read(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let Some((governor, generation)) = field.since_marker() else {
        emit_array_ptr_read(out, field, indent);
        return;
    };
    let slot = governor;
    let var = deferred_ptr_var(field);

    out.push_str(&format!(
        "{indent}// {} arrived in {} generation {generation}.\n",
        field.name,
        governing_struct(schema, governor)
    ));
    out.push_str(&format!(
        "{indent}let {var} = if generations.{slot} >= {generation}u32 {{\n"
    ));
    emit_array_ptr_read(out, field, &format!("{indent}    "));
    out.push_str(&format!("{indent}    {var}\n"));
    out.push_str(&format!("{indent}}} else {{\n"));
    out.push_str(&format!("{indent}    offset += 4;\n"));
    out.push_str(&format!("{indent}    0u32\n"));
    out.push_str(&format!("{indent}}};\n"));
}

/// Emit the iteration for an array field whose count comes later, behind its
/// generation where it has one.
fn emit_deferred_array_loop_body(out: &mut String, field: &Field, indent: &str, schema: &Schema) {
    let Some((governor, generation)) = field.since_marker() else {
        emit_array_loop_body(out, field, indent, schema);
        return;
    };
    let slot = governor;
    let name = &field.name;

    out.push_str(&format!(
        "{indent}let {name} = if generations.{slot} >= {generation}u32 {{\n"
    ));
    emit_array_loop_body(out, field, &format!("{indent}    "), schema);
    out.push_str(&format!("{indent}    {name}\n"));
    out.push_str(&format!("{indent}}} else {{\n"));
    out.push_str(&format!("{indent}    Vec::new()\n"));
    out.push_str(&format!("{indent}}};\n"));
}

/// The variable [`emit_array_ptr_read`] binds the pointer to.
fn deferred_ptr_var(field: &Field) -> String {
    match field.kind.as_str() {
        "struct_ptr_array_ptr" => format!("{}_outer", field.name),
        _ => format!("{}_ptr", field.name),
    }
}

/// Emit the `generations` update a just-parsed field calls for.
///
/// Two fields do: the one holding this structure's own generation, and one
/// pointing at a versioned structure a later field will be measured against.
/// `onerom_info_t` is where both happen.
fn emit_generation_update(
    out: &mut String,
    s: &Struct,
    field: &Field,
    indent: &str,
    schema: &Schema,
) {
    let name = &field.name;

    if s.version_field.as_deref() == Some(name.as_str())
        && let Some(slot) = schema.slot_of(&s.name)
    {
        let widen = widen_to_u32(field);
        out.push_str(&format!(
            "{indent}let generations = generations.with_{slot}({name}{widen});\n"
        ));
        return;
    }

    if field.kind != "struct_ptr" {
        return;
    }
    let Some(target) = field.type_.as_deref() else {
        return;
    };
    let Some(slot) = schema.slot_of(target) else {
        return;
    };
    let Some(t) = schema.structs.iter().find(|t| t.name == target) else {
        return;
    };
    let Some(version_field) = t.version_field.as_deref() else {
        return;
    };
    let Some(vf) = t.fields.iter().find(|f| f.name == version_field) else {
        return;
    };
    let widen = widen_to_u32(vf);

    if field.nullable.unwrap_or(false) {
        out.push_str(&format!("{indent}let generations = match &{name} {{\n"));
        out.push_str(&format!(
            "{indent}    Some(v) => generations.with_{slot}(v.{version_field}{widen}),\n"
        ));
        out.push_str(&format!("{indent}    None => generations,\n"));
        out.push_str(&format!("{indent}}};\n"));
    } else {
        out.push_str(&format!(
            "{indent}let generations = generations.with_{slot}({name}.{version_field}{widen});\n"
        ));
    }
}

/// The cast a generation-number field needs to become the `u32` a
/// `Generations` slot holds, or nothing where it is one already.
fn widen_to_u32(version_field: &Field) -> &'static str {
    match version_field.type_.as_deref() {
        Some("u32") => "",
        _ => " as u32",
    }
}

/// The literal a gated field takes where the structure predates it.
///
/// It is the C accessor's default said in Rust: `None` and an empty `Vec` and
/// a null `Pointer` are each what that accessor's `NULL` means for the kind
/// in front of it, and an array's elements are the same bytes.
pub fn rust_default_expr(field: &Field, schema: &Schema) -> String {
    match field.kind.as_str() {
        "cstr_ptr" | "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            return "None".to_string();
        }
        "struct_array_ptr" | "struct_ptr_array_ptr" => return "Vec::new()".to_string(),
        "opaque_ptr" | "fn_ptr" => return "Pointer::Null".to_string(),
        "inline_array" => {
            return format!(
                "[{}]",
                array_default_elements(field)
                    .iter()
                    .map(|b| format!("{b}u8"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        "inline_array2d" => {
            let cols = field.cols.unwrap_or(0) as usize;
            let rows: Vec<String> = array_default_elements(field)
                .chunks(cols.max(1))
                .map(|row| {
                    format!(
                        "[{}]",
                        row.iter()
                            .map(|b| format!("{b}u8"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect();
            return format!("[{}]", rows.join(", "));
        }
        _ => {}
    }

    let value = field
        .default_if_absent
        .as_ref()
        .and_then(|v| v.as_integer())
        .expect("a gated field of this kind has a whole-number default");

    match field.kind.as_str() {
        "enum" => {
            let named = field.type_.as_deref().unwrap_or("");
            let e = schema
                .enums
                .iter()
                .find(|e| e.name == named)
                .expect("a gated enum field names a declared enum");
            let strip = e.strip_prefix.as_deref().unwrap_or("");
            let v = e
                .variants
                .iter()
                .find(|v| v.value == value)
                .expect("a gated enum field defaults to a declared variant");
            format!(
                "MaybeKnown::Known({}::{})",
                rust_type_name(named),
                variant_ident(&v.name, strip)
            )
        }
        "type_alias" => {
            let named = field.type_.as_deref().unwrap_or("");
            let underlying = schema
                .type_aliases
                .iter()
                .find(|a| a.name == named)
                .map(|a| a.underlying.as_str())
                .expect("a gated type_alias field names a declared alias");
            format!("{value}{underlying}")
        }
        _ => format!("{value}{}", field.type_.as_deref().unwrap_or("u8")),
    }
}

fn push_struct_parse(out: &mut String, s: &Struct, schema: &Schema) {
    let tn = rust_type_name(&s.name);

    // The struct literal at the end names every field, so a deprecated one
    // would raise its note against the generator rather than a reader.
    push_deprecated_allow(out, s.fields.iter(), "");
    out.push_str(&format!("impl {tn} {{\n"));
    // A gated read sits in a block yielding the field, and for the kinds whose
    // read is a single `let` that block ends on the binding it just made.
    if s.fields.iter().any(|f| f.since_marker().is_some()) {
        out.push_str("    #[allow(clippy::let_and_return)]\n");
    }
    push_parse_signature(out);
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
            emit_deferred_array_ptr_read(out, f, "        ", schema);
            pending.push(idx);
        } else {
            emit_gated_field_parse(out, f, "        ", schema);
        }

        emit_generation_update(out, s, f, "        ", schema);

        // After emitting this field, emit any loops that are now unblocked.
        let fname = f.name.as_str();
        let unblocked: Vec<usize> = pending
            .iter()
            .copied()
            .filter(|&pi| s.fields[pi].count_field.as_deref() == Some(fname))
            .collect();
        for pi in unblocked {
            emit_deferred_array_loop_body(out, &s.fields[pi], "        ", schema);
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
    // Each generation update shadows the last, so only the final binding can
    // go unused.
    out.push_str("        let _ = generations;\n");
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
        push_doc_comment(out, "", cmt);
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
            push_doc_comment(out, "    ", cmt);
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

    push_tagged_fam_unknown_variant(out, tf);

    out.push_str("}\n\n");

    // ---- parse impl --------------------------------------------------
    push_tagged_fam_parse(out, tf, schema, &tn, strip, disc_enum);
}

/// Emit the arm for a discriminant this build has no name for.
///
/// A serving algorithm added after a host was built reaches it as a
/// discriminant with no name, and refusing it would cost the whole metadata
/// tree over one structure.  Everything at an offset the layout fixes is still
/// readable, and the parameter length bounds the bytes whose layout is not, so
/// the structure round-trips.
///
/// Both field names come from the schema, so neither can collide with a
/// common field - in C the discriminant, `params[]` and the common fields are
/// members of one struct.
fn push_tagged_fam_unknown_variant(out: &mut String, tf: &TaggedFam) {
    out.push_str("    /// A discriminant this build has no name for.\n");
    out.push_str("    ///\n");
    out.push_str(
        "    /// The common fields are read as they stand.  Only the parameter\n\
         \x20   /// layout is unknown, so the parameter bytes are carried whole.\n",
    );
    out.push_str("    Unknown {\n");
    out.push_str("        /// The discriminant value, as the device stored it.\n");
    out.push_str(&format!("        {}: u32,\n", tf.discriminant_field));
    for f in tf.common_fields.iter().filter(|f| f.kind != "padding") {
        if let Some(cmt) = &f.comment {
            if let Some(first) = cmt.lines().next() {
                out.push_str(&format!("        /// {}\n", first.trim()));
            }
        }
        out.push_str(&format!("        {}: {},\n", f.name, field_rust_type(f)));
    }
    out.push_str("        /// The parameter bytes, exactly as the device stored them.\n");
    out.push_str("        params: Vec<u8>,\n");
    out.push_str("    },\n");
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
    push_parse_signature(out);

    // Discriminant and param_len.  param_len is what bounds the parameter
    // bytes of a discriminant this build has no name for.
    out.push_str(&format!(
        "        let discriminant = view.{disc_reader}(addr)?;\n"
    ));
    out.push_str(&format!(
        "        let param_len = view.read_u8(addr + {param_len_off}u32)?;\n"
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

    // A discriminant this build has no name for.  The parameter bytes run
    // from the end of the common fields for the length the header gave.
    out.push_str("            _ => {\n");
    out.push_str(&format!(
        "                let params = view.slice_at(addr + {byte_off}u32, param_len as usize)?.to_vec();\n"
    ));
    out.push_str("                Ok(Self::Unknown {\n");
    out.push_str(&format!(
        "                    {}: discriminant as u32,\n",
        tf.discriminant_field
    ));
    for f in tf.common_fields.iter().filter(|f| f.kind != "padding") {
        out.push_str(&format!("                    {},\n", f.name));
    }
    out.push_str("                    params,\n");
    out.push_str("                })\n");
    out.push_str("            }\n");
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
        push_doc_comment(out, "", cmt);
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
    push_parse_signature(out);
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
