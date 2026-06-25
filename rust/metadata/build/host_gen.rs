// build/host_gen.rs
//
// Generates $OUT_DIR/host_metadata_generated.rs from the OneROM metadata
// schema.
//
// What the GENERATED code does
// -----------------------------
// For each `generate = "both"` struct / tagged FAM / simple FAM type, this
// generates a `host_name` / `host_define_fields` pair of methods on the
// corresponding abstract Rust type.  Given a value of that type, they emit
// C source text defining the equivalent object as an externally-linked
// `const` global, with real `&other_object` pointers to its sub-objects —
// for use in host test builds, where the host linker resolves addresses
// naturally and there is no flash-region / u32-address scheme to maintain.
//
// Single-pass design
// -------------------
// Unlike serialize_gen.rs's two-phase layout/write split (needed because a
// flat byte buffer requires addresses to be assigned before any pointer
// field can be written), C source generation needs no such split: every
// generated object gets an `extern const T name;` forward declaration up
// front, so definitions can reference each other (`&other_name`) in any
// order. `host_name()` therefore does layout-naming AND body-emission in
// one recursive pass:
//
//   1. Check the per-type intern table (HashMap<Type, String>).  If found,
//      return the existing name (idempotent — this is the dedup point: two
//      identical sub-objects referenced from different parents get ONE
//      definition and a shared name).
//   2. Otherwise: compute the field-by-field body (recursing into
//      sub-objects via their own `host_name()`/`host_define_fields()` —
//      this is where children's declarations/definitions get pushed),
//      allocate a fresh name, intern it, push a forward declaration and a
//      definition, and return the name.
//
// The schema's object graph is a DAG (no cycles), so interning *after*
// computing the body (rather than before, as a cycle-breaker) is safe and
// simpler.
//
// Root object
// -----------
// `onerom_metadata_header_t` is special-cased to the name `_metadata_start`,
// matching the existing `extern char _metadata_start;` linker-symbol
// convention in globals.c.  No forward declaration is emitted for it (the
// existing `extern char _metadata_start;` declaration in globals.c already
// covers it — see the file-level doc comment in the generated output).
//
// ROM image data
// ---------------
// `onerom_rom_slot_t::data` (opaque_ptr) is handled specially: the caller
// passes `rom_data: &[Vec<u8>]`, one entry per ROM slot, in the same order
// as `header.rom_slots`.  Each slot's `data` field becomes a generated
// `rom_data_slot_N[]` byte array (brace-initializer — see the FAM test
// confirming this is fine for arrays of this size on your toolchains).
//
// Tagged FAM structs (`onerom_alg_*_config_t`)
// ----------------------------------------------
// These have a trailing flexible array member `params[]`.  Designated-
// initializer support for FAMs is a GNU/Clang extension flagged under
// `-Wpedantic`; each such definition is wrapped in
// `#pragma GCC diagnostic push / ignored "-Wpedantic" / pop` (verified to
// compile cleanly under `-std=c99 -Wall -Wextra -Wpedantic` with both
// arm-none-eabi-gcc and host gcc/clang).
//
// Hand-written support required
// -------------------------------
// The generated code calls one hand-written helper, expected to live in
// onerom_metadata/src/lib.rs:
//
//     /// Escape a string for use inside a C string literal (adds \" and \\
//     /// escaping; NUL bytes are rejected since cstr_ptr fields are
//     /// null-terminated C strings).
//     pub fn escape_c_string(s: &str) -> String { ... }
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

#![allow(clippy::collapsible_if)]

use crate::rust_gen::{rust_type_name, variant_ident};
use crate::schema::*;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn generate(schema: &Schema) -> String {
    let mut out = String::with_capacity(64 * 1024);
    push_file_header(&mut out);
    push_host_gen_context(&mut out, schema);
    push_enum_c_name_impls(&mut out, schema);
    push_struct_host_impls(&mut out, schema);
    push_tagged_fam_host_impls(&mut out, schema);
    push_simple_fam_host_impls(&mut out, schema);
    push_top_level_entry_point(&mut out, schema);
    out
}

// ---------------------------------------------------------------------------
// Naming helpers (shared with serialize_gen.rs's conventions)
// ---------------------------------------------------------------------------

/// Strip the `_t` suffix; the remainder is already lower_snake_case.
fn snake_name(c_name: &str) -> &str {
    c_name.strip_suffix("_t").unwrap_or(c_name)
}

// ---------------------------------------------------------------------------
// File header
// ---------------------------------------------------------------------------

fn push_file_header(out: &mut String) {
    out.push_str(
        "// @generated — do not edit by hand.\n\
         // Source:    firmware/metadata_schema.toml\n\
         // Generator: build/host_gen.rs\n\
         //\n\
         // Regenerate by running `cargo build` in rust/metadata/.\n\n",
    );
}

// ---------------------------------------------------------------------------
// HostGenContext
// ---------------------------------------------------------------------------

fn push_host_gen_context(out: &mut String, schema: &Schema) {
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // HostGenContext\n\
         // ---------------------------------------------------------------------------\n\n",
    );

    out.push_str("/// State threaded through the host C-source generation pass.\n");
    out.push_str("pub struct HostGenContext {\n");
    out.push_str("    /// Forward declarations: (qualifier/type text including trailing\n");
    out.push_str("    /// space, name including any array brackets).  Rendered as\n");
    out.push_str("    /// `extern {0}{1};`.\n");
    out.push_str(
        "    pub decls: alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,\n",
    );
    out.push_str("    /// Definitions, in discovery order.  Order is irrelevant given\n");
    out.push_str("    /// forward declarations cover everything.\n");
    out.push_str("    pub defs: alloc::vec::Vec<alloc::string::String>,\n");
    out.push_str("    /// Per-prefix counters for fresh_name().\n");
    out.push_str("    counters: hashbrown::HashMap<alloc::string::String, u32>,\n");
    out.push_str("    /// Per-slot ROM image bytes, consumed in `rom_slots` order.\n");
    out.push_str("    rom_data: alloc::vec::Vec<alloc::vec::Vec<u8>>,\n");
    out.push_str("    rom_data_idx: usize,\n");

    out.push_str("    // Per-type intern tables: value -> generated C identifier.\n");
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let tn = rust_type_name(&s.name);
        let sn = snake_name(&s.name);
        out.push_str(&format!(
            "    {sn}_names: hashbrown::HashMap<{tn}, alloc::string::String>,\n"
        ));
    }
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        let tn = rust_type_name(&tf.name);
        let sn = snake_name(&tf.name);
        out.push_str(&format!(
            "    {sn}_names: hashbrown::HashMap<{tn}, alloc::string::String>,\n"
        ));
    }
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        let tn = rust_type_name(&sf.name);
        let sn = snake_name(&sf.name);
        out.push_str(&format!(
            "    {sn}_names: hashbrown::HashMap<{tn}, alloc::string::String>,\n"
        ));
    }
    out.push_str("}\n\n");

    out.push_str("impl HostGenContext {\n");
    out.push_str("    /// Construct a fresh context.  `rom_data` must have one entry per\n");
    out.push_str("    /// ROM slot, in the same order as `header.rom_slots`.\n");
    out.push_str("    pub fn new(rom_data: alloc::vec::Vec<alloc::vec::Vec<u8>>) -> Self {\n");
    out.push_str("        Self {\n");
    out.push_str("            decls: alloc::vec::Vec::new(),\n");
    out.push_str("            defs: alloc::vec::Vec::new(),\n");
    out.push_str("            counters: hashbrown::HashMap::new(),\n");
    out.push_str("            rom_data,\n");
    out.push_str("            rom_data_idx: 0,\n");
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let sn = snake_name(&s.name);
        out.push_str(&format!(
            "            {sn}_names: hashbrown::HashMap::new(),\n"
        ));
    }
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        let sn = snake_name(&tf.name);
        out.push_str(&format!(
            "            {sn}_names: hashbrown::HashMap::new(),\n"
        ));
    }
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        let sn = snake_name(&sf.name);
        out.push_str(&format!(
            "            {sn}_names: hashbrown::HashMap::new(),\n"
        ));
    }
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    out.push_str("    /// Generate a fresh, unique identifier with the given prefix.\n");
    out.push_str("    pub fn fresh_name(&mut self, prefix: &str) -> alloc::string::String {\n");
    out.push_str("        let n = self.counters.entry(prefix.into()).or_insert(0);\n");
    out.push_str("        let name = alloc::format!(\"{prefix}_{n}\");\n");
    out.push_str("        *n += 1;\n");
    out.push_str("        name\n");
    out.push_str("    }\n\n");

    out.push_str("    /// Consume the next ROM slot's image bytes.  Called exactly once\n");
    out.push_str("    /// per `onerom_rom_slot_t.data` field, in `rom_slots` order.\n");
    out.push_str("    pub fn next_rom_data(&mut self) -> alloc::vec::Vec<u8> {\n");
    out.push_str("        let bytes = self.rom_data[self.rom_data_idx].clone();\n");
    out.push_str("        self.rom_data_idx += 1;\n");
    out.push_str("        bytes\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Enum c_name() impls
// ---------------------------------------------------------------------------
//
// Used for `enum`-kind struct/FAM-common fields, so generated host metadata
// is readable (`.slot_type = ROM_SLOT_TYPE_SINGLE_ROM,` rather than a raw
// integer).  Tagged FAM *discriminants* don't use this — the discriminant's
// C name is known statically from the schema (`variant.discriminant`) and
// is emitted directly.
//
// Enums with `source` set (e.g. onerom_rom_type_t, which is generated from
// chip-types.json) have no corresponding Rust enum type, so no c_name() impl
// is emitted for them.  Any struct field of that enum kind should instead be
// declared as `kind = "scalar", type = "u8"` in the schema so that
// host_define_fields emits it as a plain integer rather than calling c_name().

fn push_enum_c_name_impls(out: &mut String, schema: &Schema) {
    if schema.enums.is_empty() {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Enum c_name() impls\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for e in &schema.enums {
        // Enums sourced from external data have no corresponding Rust enum type;
        // skip c_name() generation for them.
        if e.source.is_some() {
            continue;
        }

        let tn = rust_type_name(&e.name);
        let strip = e.strip_prefix.as_deref().unwrap_or("");
        out.push_str(&format!("impl {tn} {{\n"));
        out.push_str("    /// The C enum constant name for this value (used when generating\n");
        out.push_str("    /// human-readable host test-build metadata).\n");
        out.push_str("    pub fn c_name(&self) -> &'static str {\n");
        out.push_str("        match self {\n");
        for v in e.variants.iter().filter(|v| !v.sentinel.unwrap_or(false)) {
            let vn = variant_ident(&v.name, strip);
            out.push_str(&format!("            Self::{vn} => \"{}\",\n", v.name));
        }
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("}\n\n");
    }
}

// ---------------------------------------------------------------------------
// Level-0 helpers: emit level-1 (generated Rust) source fragments
// ---------------------------------------------------------------------------
//
// These build the GENERATED Rust code that, when run, pushes lines of C
// source onto a `alloc::string::String` named `s`.  Field names are spliced
// in via plain concatenation (not format!) so that literal `{`/`}` in the
// emitted C-source format strings never need `{{`/`}}` doubling.

/// Emit:  s.push_str(&alloc::format!("    .{field} = {}, \n", VALUE_EXPR));
fn emit_field_line(out: &mut String, field: &str, value_expr: &str) {
    out.push_str("        s.push_str(&alloc::format!(\"    .");
    out.push_str(field);
    out.push_str(" = {},\\n\", ");
    out.push_str(value_expr);
    out.push_str("));\n");
}

/// Emit a u8-array literal expression `{ 0x01, 0x02, ... }` from an
/// iterator expression `ITER_EXPR` yielding `&u8`.
fn array_literal_expr(iter_expr: &str) -> String {
    alloc_format_helper(&format!(
        "alloc::format!(\"{{{{ {{}} }}}}\", {iter_expr}.map(|b| alloc::format!(\"0x{{:02X}}\", b)).collect::<alloc::vec::Vec<_>>().join(\", \"))"
    ))
}

/// Pass-through; kept as a single place to adjust string-building strategy
/// if needed (e.g. switching to `write!` for very large arrays).
fn alloc_format_helper(expr: &str) -> String {
    expr.to_string()
}

// ---------------------------------------------------------------------------
// Struct host impls
// ---------------------------------------------------------------------------

fn push_struct_host_impls(out: &mut String, schema: &Schema) {
    if !schema.structs.iter().any(|s| s.generate == Generate::Both) {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Struct host impls (host_name / host_define_fields)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        push_struct_host(out, s, schema);
    }
}

fn push_struct_host(out: &mut String, s: &Struct, schema: &Schema) {
    let tn = rust_type_name(&s.name);
    let sn = snake_name(&s.name);
    let c_type = &s.name;

    // Map: count_field_name -> vec_field_name (for derived `_count` fields).
    let derived_counts: std::collections::HashMap<String, String> = s
        .fields
        .iter()
        .filter_map(|f| {
            if matches!(f.kind.as_str(), "struct_array_ptr" | "struct_ptr_array_ptr") {
                f.count_field
                    .as_ref()
                    .map(|cf| (cf.clone(), f.name.clone()))
            } else {
                None
            }
        })
        .collect();

    out.push_str(&format!("impl {tn} {{\n"));

    // host_name() ------------------------------------------------------------
    out.push_str("    /// Idempotent: returns the existing generated name if this exact\n");
    out.push_str("    /// value has already been emitted (dedup of shared sub-objects).\n");
    out.push_str(
        "    pub fn host_name(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n",
    );
    out.push_str(&format!(
        "        if let Some(name) = ctx.{sn}_names.get(self) {{\n"
    ));
    out.push_str("            return name.clone();\n");
    out.push_str("        }\n");
    out.push_str("        let body = self.host_define_fields(ctx);\n");
    out.push_str(&format!("        let name = ctx.fresh_name(\"{sn}\");\n"));
    out.push_str(&format!(
        "        ctx.{sn}_names.insert(self.clone(), name.clone());\n"
    ));
    out.push_str(&format!(
        "        ctx.decls.push((\"const {c_type} \".into(), name.clone()));\n"
    ));
    out.push_str("        {\n");
    out.push_str("            let mut d = alloc::string::String::new();\n");
    out.push_str(&format!("            d.push_str(\"const {c_type} \");\n"));
    out.push_str("            d.push_str(&name);\n");
    out.push_str("            d.push_str(\" = {\\n\");\n");
    out.push_str("            d.push_str(&body);\n");
    out.push_str("            d.push_str(\"};\\n\");\n");
    out.push_str("            ctx.defs.push(d);\n");
    out.push_str("        }\n");
    out.push_str("        name\n");
    out.push_str("    }\n\n");

    // host_define_fields() ----------------------------------------------------
    out.push_str("    /// Field-by-field body (the part between `{` and `}` in the\n");
    out.push_str("    /// definition).  Exposed separately so the root object\n");
    out.push_str("    /// (`_metadata_start`) can be emitted without going through\n");
    out.push_str("    /// `host_name()`'s naming/interning.\n");
    out.push_str("    pub fn host_define_fields(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n");
    out.push_str("        let _ = &ctx;\n");
    out.push_str("        let mut s = alloc::string::String::new();\n");

    for f in &s.fields {
        emit_host_define_field(out, f, sn, &derived_counts, schema);
    }

    out.push_str("        s\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

#[allow(clippy::useless_format)]
fn emit_host_define_field(
    out: &mut String,
    f: &Field,
    parent_sn: &str,
    derived_counts: &std::collections::HashMap<String, String>,
    schema: &Schema,
) {
    let name = &f.name;

    match f.kind.as_str() {
        "scalar" => {
            if let Some(vec_field) = derived_counts.get(name.as_str()) {
                emit_field_line(out, name, &format!("self.{vec_field}.len()"));
            } else {
                emit_field_line(out, name, &format!("self.{name}"));
            }
        }

        "enum" => {
            emit_field_line(out, name, &format!("self.{name}.c_name()"));
        }

        "type_alias" => {
            emit_field_line(out, name, &format!("self.{name}"));
        }

        "inline_array" => {
            let elem = f.element.as_deref().unwrap_or("u8");
            if elem == "u8" || elem == "char" {
                out.push_str("        s.push_str(&alloc::format!(\"    .");
                out.push_str(name);
                out.push_str(" = {{ {} }},\\n\", self.");
                out.push_str(name);
                out.push_str(".iter().map(|b| alloc::format!(\"0x{:02X}\", b)).collect::<alloc::vec::Vec<_>>().join(\", \")));\n");
            } else {
                out.push_str(&format!(
                    "        compile_error!(\"non-u8 inline_array host emission not implemented for `{name}`\");\n"
                ));
            }
        }
        "inline_array2d" => {
            // { {row0...}, {row1...}, ... }
            out.push_str("        s.push_str(\"    .");
            out.push_str(name);
            out.push_str(" = { \");\n");
            out.push_str(&format!(
                "        for (i, row) in self.{name}.iter().enumerate() {{\n"
            ));
            out.push_str("            if i > 0 { s.push_str(\", \"); }\n");
            let row_iter = "row.iter()";
            out.push_str("            s.push_str(&");
            out.push_str(&array_literal_expr(row_iter));
            out.push_str(");\n");
            out.push_str("        }\n");
            out.push_str("        s.push_str(\" },\\n\");\n");
        }

        "cstr_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!("        match &self.{name} {{\n"));
                emit_cstr_none_arm(out, name, "            ");
                out.push_str("            Some(v) => {\n");
                emit_cstr_some_body(out, name, "v", "                ");
                out.push_str("            }\n");
                out.push_str("        }\n");
            } else {
                emit_cstr_some_body(out, name, &format!("&self.{name}"), "        ");
            }
        }

        "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!("        match &self.{name} {{\n"));
                emit_field_none_arm(out, name, "            ");
                out.push_str("            Some(sub) => {\n");
                out.push_str("                let n = sub.host_name(ctx);\n");
                out.push_str("                s.push_str(\"    .");
                out.push_str(name);
                out.push_str(" = &\");\n");
                out.push_str("                s.push_str(&n);\n");
                out.push_str("                s.push_str(\",\\n\");\n");
                out.push_str("            }\n");
                out.push_str("        }\n");
            } else {
                out.push_str(&format!("        {{\n"));
                out.push_str(&format!(
                    "            let n = self.{name}.host_name(ctx);\n"
                ));
                out.push_str("            s.push_str(\"    .");
                out.push_str(name);
                out.push_str(" = &\");\n");
                out.push_str("            s.push_str(&n);\n");
                out.push_str("            s.push_str(\",\\n\");\n");
                out.push_str("        }\n");
            }
        }

        "struct_array_ptr" => {
            let elem_c = f.element.as_deref().unwrap_or("");
            out.push_str(&format!("        {{\n"));
            out.push_str(&format!(
                "            let elems: alloc::vec::Vec<alloc::string::String> = self.{name}\n"
            ));
            out.push_str("                .iter()\n");
            out.push_str("                .map(|e| e.host_define_fields(ctx))\n");
            out.push_str("                .collect();\n");
            out.push_str(&format!(
                "            let arr_name = ctx.fresh_name(\"{parent_sn}_{name}_arr\");\n"
            ));
            out.push_str(&format!(
                "            ctx.decls.push((\"const {elem_c} \".into(), alloc::format!(\"{{}}[]\", arr_name)));\n"
            ));
            out.push_str("            let items: alloc::vec::Vec<alloc::string::String> = elems\n");
            out.push_str("                .iter()\n");
            out.push_str("                .map(|e| alloc::format!(\"{{\\n{}}}\", e))\n");
            out.push_str("                .collect();\n");
            out.push_str(&format!(
                "            ctx.defs.push(alloc::format!(\"const {elem_c} {{}}[] = {{{{ {{}} }}}};\\n\", arr_name, items.join(\", \")));\n"
            ));
            out.push_str("            s.push_str(\"    .");
            out.push_str(name);
            out.push_str(" = \");\n");
            out.push_str("            s.push_str(&arr_name);\n");
            out.push_str("            s.push_str(\",\\n\");\n");
            out.push_str("        }\n");
        }

        "struct_ptr_array_ptr" => {
            let elem_c = f.element.as_deref().unwrap_or("");
            out.push_str(&format!("        {{\n"));
            out.push_str(&format!(
                "            let names: alloc::vec::Vec<alloc::string::String> = self.{name}\n"
            ));
            out.push_str("                .iter()\n");
            out.push_str("                .map(|e| e.host_name(ctx))\n");
            out.push_str("                .collect();\n");
            out.push_str(&format!(
                "            let arr_name = ctx.fresh_name(\"{parent_sn}_{name}_ptrs\");\n"
            ));
            out.push_str(&format!(
                "            ctx.decls.push((\"const {elem_c} * const \".into(), alloc::format!(\"{{}}[]\", arr_name)));\n"
            ));
            out.push_str("            let refs: alloc::vec::Vec<alloc::string::String> = names\n");
            out.push_str("                .iter()\n");
            out.push_str("                .map(|n| alloc::format!(\"&{}\", n))\n");
            out.push_str("                .collect();\n");
            out.push_str(&format!(
                "            ctx.defs.push(alloc::format!(\"const {elem_c} * const {{}}[] = {{{{ {{}} }}}};\\n\", arr_name, refs.join(\", \")));\n"
            ));
            out.push_str("            s.push_str(\"    .");
            out.push_str(name);
            out.push_str(" = \");\n");
            out.push_str("            s.push_str(&arr_name);\n");
            out.push_str("            s.push_str(\",\\n\");\n");
            out.push_str("        }\n");
        }

        "opaque_ptr" => {
            // Only onerom_rom_slot_t::data in practice: per-slot ROM image
            // bytes, supplied via HostGenContext::next_rom_data().
            out.push_str(&format!("        {{\n"));
            out.push_str("            let bytes = ctx.next_rom_data();\n");
            out.push_str("            let arr_name = ctx.fresh_name(\"rom_data_slot\");\n");
            out.push_str(
                "            ctx.decls.push((\"const uint8_t \".into(), alloc::format!(\"{}[]\", arr_name)));\n",
            );
            out.push_str("            let bytelist: alloc::string::String = bytes\n");
            out.push_str("                .iter()\n");
            out.push_str("                .map(|b| alloc::format!(\"0x{:02X}\", b))\n");
            out.push_str("                .collect::<alloc::vec::Vec<_>>()\n");
            out.push_str("                .join(\", \");\n");
            out.push_str(
                "            ctx.defs.push(alloc::format!(\"const uint8_t {}[] = {{ {} }};\\n\", arr_name, bytelist));\n",
            );
            out.push_str("            s.push_str(\"    .");
            out.push_str(name);
            out.push_str(" = \");\n");
            out.push_str("            s.push_str(&arr_name);\n");
            out.push_str("            s.push_str(\",\\n\");\n");
            out.push_str("        }\n");
        }

        "fn_ptr" => {
            out.push_str(&format!(
                "        compile_error!(\"fn_ptr host emission not implemented for `{name}`\");\n"
            ));
        }

        "padding" => {
            // Omitted: unspecified designated-initializer members are
            // zero-initialized by C, which is fine for host test builds
            // (padding has no semantic meaning on host).
        }

        kind => {
            out.push_str(&format!(
                "        compile_error!(\"unhandled field kind `{kind}` for `{name}` in host_define_fields\");\n"
            ));
        }
    }

    let _ = schema; // not currently needed per-field beyond what's used above
}

fn emit_cstr_none_arm(out: &mut String, name: &str, ind: &str) {
    out.push_str(ind);
    out.push_str("None => s.push_str(\"    .");
    out.push_str(name);
    out.push_str(" = NULL,\\n\"),\n");
}

fn emit_field_none_arm(out: &mut String, name: &str, ind: &str) {
    out.push_str(ind);
    out.push_str("None => s.push_str(\"    .");
    out.push_str(name);
    out.push_str(" = NULL,\\n\"),\n");
}

/// Emit: s.push_str("    .{name} = \""); s.push_str(&escape_c_string(EXPR)); s.push_str("\",\n");
fn emit_cstr_some_body(out: &mut String, name: &str, expr: &str, ind: &str) {
    out.push_str(ind);
    out.push_str("s.push_str(\"    .");
    out.push_str(name);
    out.push_str(" = \\\"\");\n");
    out.push_str(ind);
    out.push_str("s.push_str(&escape_c_string(");
    out.push_str(expr);
    out.push_str("));\n");
    out.push_str(ind);
    out.push_str("s.push_str(\"\\\",\\n\");\n");
}

// ---------------------------------------------------------------------------
// Tagged FAM host impls
// ---------------------------------------------------------------------------

fn push_tagged_fam_host_impls(out: &mut String, schema: &Schema) {
    if !schema
        .tagged_fams
        .iter()
        .any(|tf| tf.generate == Generate::Both)
    {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Tagged FAM host impls (host_name / host_define_fields)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        push_tagged_fam_host(out, tf, schema);
    }
}

fn push_tagged_fam_host(out: &mut String, tf: &TaggedFam, schema: &Schema) {
    let tn = rust_type_name(&tf.name);
    let sn = snake_name(&tf.name);
    let c_type = &tf.name;
    let disc_enum = schema.enums.iter().find(|e| e.name == tf.discriminant_type);
    let strip = disc_enum
        .and_then(|e| e.strip_prefix.as_deref())
        .unwrap_or("");

    out.push_str(&format!("impl {tn} {{\n"));

    // host_name() --------------------------------------------------------
    out.push_str(
        "    pub fn host_name(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n",
    );
    out.push_str(&format!(
        "        if let Some(name) = ctx.{sn}_names.get(self) {{\n"
    ));
    out.push_str("            return name.clone();\n");
    out.push_str("        }\n");
    out.push_str("        let body = self.host_define_fields(ctx);\n");
    out.push_str(&format!("        let name = ctx.fresh_name(\"{sn}\");\n"));
    out.push_str(&format!(
        "        ctx.{sn}_names.insert(self.clone(), name.clone());\n"
    ));
    out.push_str(&format!(
        "        ctx.decls.push((\"const {c_type} \".into(), name.clone()));\n"
    ));
    // FAM definitions need the #pragma wrapper (see file header).
    out.push_str("        {\n");
    out.push_str("            let mut d = alloc::string::String::new();\n");
    out.push_str("            d.push_str(\"#pragma GCC diagnostic push\\n\");\n");
    out.push_str(
        "            d.push_str(\"#pragma GCC diagnostic ignored \\\"-Wpedantic\\\"\\n\");\n",
    );
    out.push_str(&format!("            d.push_str(\"const {c_type} \");\n"));
    out.push_str("            d.push_str(&name);\n");
    out.push_str("            d.push_str(\" = {\\n\");\n");
    out.push_str("            d.push_str(&body);\n");
    out.push_str("            d.push_str(\"};\\n\");\n");
    out.push_str("            d.push_str(\"#pragma GCC diagnostic pop\\n\");\n");
    out.push_str("            ctx.defs.push(d);\n");
    out.push_str("        }\n");
    out.push_str("        name\n");
    out.push_str("    }\n\n");

    // host_define_fields() ------------------------------------------------
    out.push_str("    pub fn host_define_fields(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n");
    out.push_str("        let mut s = alloc::string::String::new();\n");
    out.push_str(
        "        let _ = &ctx; // common fields below don't need ctx, but kept for symmetry\n",
    );
    out.push_str("        match self {\n");

    for v in &tf.variants {
        let vn = variant_ident(&v.discriminant, strip);

        let fields: Vec<&str> = tf
            .common_fields
            .iter()
            .chain(v.fields.iter())
            .filter(|f| f.kind != "padding")
            .map(|f| f.name.as_str())
            .collect();

        let pattern = if fields.is_empty() {
            format!("Self::{vn}")
        } else {
            format!("Self::{vn} {{ {} }}", fields.join(", "))
        };
        out.push_str(&format!("            {pattern} => {{\n"));

        // alg / discriminant — statically known from the schema.
        out.push_str(&format!(
            "                s.push_str(\"    .{} = {},\\n\");\n",
            tf.discriminant_field, v.discriminant
        ));
        // param_len — statically known constant name from the schema.
        out.push_str(&format!(
            "                s.push_str(\"    .{} = {},\\n\");\n",
            tf.param_len_field, v.params_len_constant
        ));

        // Common fields.
        for f in &tf.common_fields {
            emit_fam_common_field(out, f);
        }

        // params[] — pack the variant's own fields into bytes, in order.
        out.push_str("                s.push_str(\"    .params = \");\n");
        out.push_str(&emit_fam_params_expr(&v.fields));
        out.push_str("                s.push_str(\",\\n\");\n");

        out.push_str("            }\n");
    }

    out.push_str("        }\n");
    out.push_str("        s\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

/// Common (shared-across-variants) field: scalar / enum / type_alias only.
/// Bound by reference in the match-arm pattern (`field: &T`).
fn emit_fam_common_field(out: &mut String, f: &Field) {
    let name = &f.name;
    match f.kind.as_str() {
        "scalar" | "type_alias" => {
            out.push_str("                s.push_str(&alloc::format!(\"    .");
            out.push_str(name);
            out.push_str(" = {},\\n\", ");
            out.push_str(name);
            out.push_str("));\n");
        }
        "enum" => {
            out.push_str("                s.push_str(&alloc::format!(\"    .");
            out.push_str(name);
            out.push_str(" = {},\\n\", ");
            out.push_str(name);
            out.push_str(".c_name()));\n");
        }
        "padding" => {}
        other => {
            out.push_str(&format!(
                "                compile_error!(\"unexpected FAM common field kind `{other}` for `{name}`\");\n"
            ));
        }
    }
}

/// Build the level-1 expression that pushes `{ b0, b1, ... }` for a tagged
/// FAM variant's `params[]`, packing each field into little-endian bytes.
/// Variant fields are bound by reference (`field: &T`) in the match arm.
fn emit_fam_params_expr(fields: &[Field]) -> String {
    let mut out = String::new();
    out.push_str("                s.push_str(&{\n");
    out.push_str("                    #[allow(unused_mut)]\n");
    out.push_str(
        "                    let mut bytes: alloc::vec::Vec<u8> = alloc::vec::Vec::new();\n",
    );
    for f in fields {
        let name = &f.name;
        match f.kind.as_str() {
            "scalar" => {
                let ty = f.type_.as_deref().unwrap_or("u8");
                match ty {
                    "u8" => {
                        out.push_str(&format!("                    bytes.push(*{name});\n"));
                    }
                    "u16" => {
                        out.push_str(&format!(
                            "                    bytes.extend_from_slice(&{name}.to_le_bytes());\n"
                        ));
                    }
                    _ => {
                        out.push_str(&format!(
                            "                    compile_error!(\"unsupported FAM param scalar type `{ty}` for `{name}`\");\n"
                        ));
                    }
                }
            }
            "enum" => {
                out.push_str(&format!("                    bytes.push(*{name} as u8);\n"));
            }
            "padding" => {
                let sz = f.size.unwrap_or(0);
                out.push_str(&format!(
                    "                    bytes.extend(core::iter::repeat(0u8).take({sz}));\n"
                ));
            }
            other => {
                out.push_str(&format!(
                    "                    compile_error!(\"unexpected FAM param field kind `{other}` for `{name}`\");\n"
                ));
            }
        }
    }
    out.push_str("                    alloc::format!(\"{{ {} }}\", bytes.iter().map(|b| alloc::format!(\"0x{:02X}\", b)).collect::<alloc::vec::Vec<_>>().join(\", \"))\n");
    out.push_str("                });\n");
    out
}

// ---------------------------------------------------------------------------
// Simple FAM host impls
// ---------------------------------------------------------------------------

fn push_simple_fam_host_impls(out: &mut String, schema: &Schema) {
    if !schema
        .simple_fams
        .iter()
        .any(|sf| sf.generate == Generate::Both)
    {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Simple FAM host impls (host_name / host_define_fields)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        push_simple_fam_host(out, sf);
    }
}

fn push_simple_fam_host(out: &mut String, sf: &SimpleFam) {
    let tn = rust_type_name(&sf.name);
    let sn = snake_name(&sf.name);
    let c_type = &sf.name;

    out.push_str(&format!("impl {tn} {{\n"));

    out.push_str(
        "    pub fn host_name(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n",
    );
    out.push_str(&format!(
        "        if let Some(name) = ctx.{sn}_names.get(self) {{\n"
    ));
    out.push_str("            return name.clone();\n");
    out.push_str("        }\n");
    out.push_str("        let body = self.host_define_fields(ctx);\n");
    out.push_str(&format!("        let name = ctx.fresh_name(\"{sn}\");\n"));
    out.push_str(&format!(
        "        ctx.{sn}_names.insert(self.clone(), name.clone());\n"
    ));
    out.push_str(&format!(
        "        ctx.decls.push((\"const {c_type} \".into(), name.clone()));\n"
    ));
    out.push_str("        {\n");
    out.push_str("            let mut d = alloc::string::String::new();\n");
    out.push_str("            d.push_str(\"#pragma GCC diagnostic push\\n\");\n");
    out.push_str(
        "            d.push_str(\"#pragma GCC diagnostic ignored \\\"-Wpedantic\\\"\\n\");\n",
    );
    out.push_str(&format!("            d.push_str(\"const {c_type} \");\n"));
    out.push_str("            d.push_str(&name);\n");
    out.push_str("            d.push_str(\" = {\\n\");\n");
    out.push_str("            d.push_str(&body);\n");
    out.push_str("            d.push_str(\"};\\n\");\n");
    out.push_str("            d.push_str(\"#pragma GCC diagnostic pop\\n\");\n");
    out.push_str("            ctx.defs.push(d);\n");
    out.push_str("        }\n");
    out.push_str("        name\n");
    out.push_str("    }\n\n");

    out.push_str("    pub fn host_define_fields(&self, ctx: &mut HostGenContext) -> alloc::string::String {\n");
    out.push_str("        let _ = &ctx;\n");
    out.push_str(&format!(
        "        let mut s = alloc::format!(\"    .{} = {{}},\\n\", self.params.len());\n",
        sf.param_len_field
    ));
    out.push_str("        let bytelist: alloc::string::String = self.params\n");
    out.push_str("            .iter()\n");
    out.push_str("            .map(|b| alloc::format!(\"0x{:02X}\", b))\n");
    out.push_str("            .collect::<alloc::vec::Vec<_>>()\n");
    out.push_str("            .join(\", \");\n");
    out.push_str(
        "        s.push_str(&alloc::format!(\"    .params = {{ {} }},\\n\", bytelist));\n",
    );
    out.push_str("        s\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Top-level entry point
// ---------------------------------------------------------------------------

fn push_top_level_entry_point(out: &mut String, schema: &Schema) {
    let root = schema
        .structs
        .iter()
        .find(|s| s.root.unwrap_or(false))
        .expect("schema must have exactly one root struct");
    let root_tn = rust_type_name(&root.name);
    let root_c = &root.name;

    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Top-level entry point\n\
         // ---------------------------------------------------------------------------\n\n",
    );

    out.push_str("/// Generate the complete host metadata `.c` source for `root`.\n");
    out.push_str("///\n");
    out.push_str("/// `rom_data` must have one entry per ROM slot, in the same order as\n");
    out.push_str("/// `root.rom_slots` — each slot's image bytes.\n");
    out.push_str("///\n");
    out.push_str("/// The root object is emitted as `_metadata_start`, matching the\n");
    out.push_str("/// `extern char _metadata_start;` linker-symbol convention already used\n");
    out.push_str("/// in globals.c — no forward declaration is emitted for it.\n");
    out.push_str(&format!(
        "pub fn generate_host_metadata_c(root: &{root_tn}, rom_data: alloc::vec::Vec<alloc::vec::Vec<u8>>) -> alloc::string::String {{\n"
    ));
    out.push_str("    assert_eq!(\n");
    out.push_str("        root.rom_slots.len(),\n");
    out.push_str("        rom_data.len(),\n");
    out.push_str("        \"rom_data must have one entry per ROM slot, in the same order\"\n");
    out.push_str("    );\n\n");
    out.push_str("    let mut ctx = HostGenContext::new(rom_data);\n");
    out.push_str("    let body = root.host_define_fields(&mut ctx);\n");
    out.push_str("    {\n");
    out.push_str("        let mut d = alloc::string::String::new();\n");
    out.push_str(&format!(
        "        d.push_str(\"const {root_c} _metadata_start = {{\\n\");\n"
    ));
    out.push_str("        d.push_str(&body);\n");
    out.push_str("        d.push_str(\"};\\n\");\n");
    out.push_str("        ctx.defs.push(d);\n");
    out.push_str("    }\n\n");

    out.push_str("    let mut out = alloc::string::String::with_capacity(64 * 1024);\n");
    out.push_str("    out.push_str(\n");
    out.push_str("        \"// @generated — do not edit by hand.\\n\\\n");
    out.push_str("         // Source:    firmware/metadata_schema.toml\\n\\\n");
    out.push_str("         // Generator: build/host_gen.rs (via generate_host_metadata_c)\\n\\\n");
    out.push_str("         //\\n\\\n");
    out.push_str("         // Host test-build metadata: real C objects with real pointers.\\n\\\n");
    out.push_str("         // `_metadata_start` is the root onerom_metadata_header_t — see\\n\\\n");
    out.push_str("         // globals.c's `extern char _metadata_start;` convention.\\n\\\n");
    out.push_str("         #include \\\"onerom_metadata.h\\\"\\n\\n\\\n");
    out.push_str("         // The root object is deliberately NOT forward-declared here:\\n\\\n");
    out.push_str(
        "         // globals.c already declares `extern char _metadata_start;`.\\n\\n\"\n",
    );
    out.push_str("    );\n\n");

    out.push_str("    out.push_str(\"// ---------------------------------------------------------------------------\\n\");\n");
    out.push_str("    out.push_str(\"// Forward declarations\\n\");\n");
    out.push_str("    out.push_str(\"// ---------------------------------------------------------------------------\\n\\n\");\n");
    out.push_str("    for (decl, name) in &ctx.decls {\n");
    out.push_str("        out.push_str(&alloc::format!(\"extern {}{};\\n\", decl, name));\n");
    out.push_str("    }\n");
    out.push_str("    out.push('\\n');\n\n");

    out.push_str("    out.push_str(\"// ---------------------------------------------------------------------------\\n\");\n");
    out.push_str("    out.push_str(\"// Definitions\\n\");\n");
    out.push_str("    out.push_str(\"// ---------------------------------------------------------------------------\\n\\n\");\n");
    out.push_str("    for def in &ctx.defs {\n");
    out.push_str("        out.push_str(def);\n");
    out.push_str("        out.push('\\n');\n");
    out.push_str("    }\n\n");

    out.push_str("    out\n");
    out.push_str("}\n");

    let _ = schema;
}
