// src/serialize_gen.rs
//
// Generates $OUT_DIR/serialize_generated.rs from the OneROM metadata schema.
//
// Output file layout:
//   1. File header
//   2. Schema constants   (METADATA_BASE, METADATA_SIZE)
//   3. SerializeContext   (struct + impl)
//   4. Struct impls       (layout, layout_sub_objects,
//                          check_generation, write)             generate == Both
//   5. Tagged FAM impls   (layout, write)                       generate == Both
//   6. Simple FAM impls   (layout, write)                       generate == Both
//
// Two-phase design
// ----------------
// Phase 1 — layout():
//   Check the per-type intern table (HashMap<Type, u32>).  If found, return
//   the existing address (idempotent).  Otherwise allocate space via the
//   bump allocator, intern the value, then recurse into sub-objects via
//   layout_sub_objects().
//
//   layout_sub_objects() is separated so that struct_array_ptr inline
//   elements can have their sub-objects laid out without allocating or
//   interning the element itself (its address is fixed by the array block).
//
// Phase 2 — write():
//   Guard with `written: HashSet<u32>` to prevent writing the same flash
//   address twice (dedup).  Write each field at its statically-computed
//   byte offset.  For pointer fields, call layout() on the sub-object
//   (idempotent by this point) to recover its address, write the u32
//   pointer, then recurse into write().
//
// Generations
// -----------
// The metadata is written at the generation the target firmware reads, which
// the caller states once and `new` takes onto the context.  A field introduced
// after that generation is not written.  check_generation() runs the whole
// tree first and refuses such a field where it holds anything but its declared
// default, because dropping a value that was asked for would compose an image
// serving something else.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

#![allow(clippy::collapsible_if)]

use std::collections::HashMap;

use crate::rust_gen::{rust_type_name, variant_ident};
use crate::schema::*;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn generate(schema: &Schema) -> String {
    validate_schema_sizes(schema);

    let mut out = String::with_capacity(64 * 1024);
    push_file_header(&mut out, schema);
    push_schema_constants(&mut out, schema);
    push_serialize_context(&mut out, schema);
    push_struct_serialize_impls(&mut out, schema);
    push_tagged_fam_serialize_impls(&mut out, schema);
    push_simple_fam_serialize_impls(&mut out, schema);
    out
}

// ---------------------------------------------------------------------------
// Codegen-time schema size validation
// ---------------------------------------------------------------------------

fn validate_schema_sizes(schema: &Schema) {
    // Every generate=both struct with an explicit size: field sum must match.
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        if let Some(schema_size) = s.size {
            let computed: usize = s.fields.iter().map(|f| field_size(f, schema)).sum();
            if computed != schema_size as usize {
                panic!(
                    "Schema size mismatch for {}: declared size={} but \
                     field sum computed={} (check field definitions match C struct layout)",
                    s.name, schema_size, computed
                );
            }
        }
    }

    // Every generate=both tagged FAM: 1(disc)+1(param_len)+common_fields == base_size.
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        let disc_size = schema
            .enums
            .iter()
            .find(|e| e.name == tf.discriminant_type)
            .map_or(1u32, |e| e.size) as usize;
        let common_size: usize = tf.common_fields.iter().map(|f| field_size(f, schema)).sum();
        let computed_base = disc_size + 1 + common_size;
        if computed_base != tf.base_size as usize {
            panic!(
                "base_size mismatch for {}: declared={} but \
                 discriminant({})+param_len(1)+common_fields({})={}",
                tf.name, tf.base_size, disc_size, common_size, computed_base
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------------

/// Strip the `_t` suffix; the remainder is already lower_snake_case.
fn snake_name(c_name: &str) -> &str {
    c_name.strip_suffix("_t").unwrap_or(c_name)
}

/// Look up a schema constant's integer value; panic if absent or non-integer.
fn const_value(schema: &Schema, name: &str) -> usize {
    schema
        .constants
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| match &c.value {
            ConstantValue::Integer(i) => Some(*i as usize),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Constant `{name}` not found (or not integer) in schema"))
}

/// (write-method name, byte width) for a scalar Rust type string.
fn scalar_write(ty: &str) -> (&'static str, usize) {
    match ty {
        "u8" | "char" => ("write_u8", 1),
        "u16" => ("write_u16_le", 2),
        "u32" => ("write_u32_le", 4),
        _ => ("write_u8", 1),
    }
}

/// (write-method name, byte width) for an enum field.
fn enum_write(field: &Field, schema: &Schema) -> (&'static str, usize) {
    let sz = schema
        .enums
        .iter()
        .find(|e| field.type_.as_deref() == Some(e.name.as_str()))
        .map_or(1usize, |e| e.size as usize);
    if sz == 2 {
        ("write_u16_le", 2)
    } else {
        ("write_u8", 1)
    }
}

/// Rust variant identifier for a tagged FAM discriminant enum variant.
/// Must produce the same names as `variant_ident` in rust_gen.rs.
fn fam_variant_ident(c_name: &str, strip_prefix: &str) -> String {
    variant_ident(c_name, strip_prefix)
}

// ---------------------------------------------------------------------------
// File header
// ---------------------------------------------------------------------------

fn push_file_header(out: &mut String, schema: &Schema) {
    out.push_str(&format!(
        "// @generated — do not edit by hand.\n\
         // Source:    {}\n\
         // Generator: onerom-metadata-gen serialize_gen.rs\n\
         //\n\
         // Regenerated by every build of the crate that owns the schema.\n\n",
        schema.source
    ));
}

// ---------------------------------------------------------------------------
// Schema constants
// ---------------------------------------------------------------------------

fn push_schema_constants(out: &mut String, schema: &Schema) {
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Schema constants\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    // A schema whose structures are consts its own build places has no region
    // for anything to compose, so it declares neither and gets neither.
    if let Some(base) = schema.schema.metadata_base {
        out.push_str(&format!(
            "/// Flash base address of the metadata region.\n\
             pub const METADATA_BASE: u32 = {base:#010x};\n\n"
        ));
    }
    if let Some(size) = schema.schema.metadata_size {
        out.push_str(&format!(
            "/// Byte size of the metadata region.\n\
             pub const METADATA_SIZE: usize = {size};\n\n"
        ));
    }
}

// ---------------------------------------------------------------------------
// SerializeContext — struct definition
// ---------------------------------------------------------------------------

fn push_serialize_context(out: &mut String, schema: &Schema) {
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // SerializeContext\n\
         // ---------------------------------------------------------------------------\n\n",
    );

    out.push_str("/// State threaded through both phases of the serializer.\n");
    out.push_str("pub struct SerializeContext<'buf> {\n");
    out.push_str("    base_addr: u32,\n");
    out.push_str("    next_addr: u32,\n");
    out.push_str(
        "    /// Metadata generation being written.  A field introduced after it is\n\
         /// left out - see [`SerializeContext::new`].\n",
    );
    out.push_str("    metadata_generation: u32,\n");
    out.push_str("    pub buf: &'buf mut [u8],\n");
    out.push_str("    /// Addresses written in Phase 2; prevents writing the same object twice.\n");
    out.push_str("    written: hashbrown::HashSet<u32>,\n");
    out.push_str("    /// String content -> flash address (dedup across cstr_ptr fields).\n");
    out.push_str("    string_addrs: hashbrown::HashMap<alloc::string::String, u32>,\n");

    // Per named-type intern tables.
    out.push_str("    // Per-type intern tables: Rust value -> assigned flash address.\n");
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let tn = rust_type_name(&s.name);
        let sn = snake_name(&s.name);
        out.push_str(&format!("    {sn}_addrs: hashbrown::HashMap<{tn}, u32>,\n"));
    }
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        let tn = rust_type_name(&tf.name);
        let sn = snake_name(&tf.name);
        out.push_str(&format!("    {sn}_addrs: hashbrown::HashMap<{tn}, u32>,\n"));
    }
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        let tn = rust_type_name(&sf.name);
        let sn = snake_name(&sf.name);
        out.push_str(&format!("    {sn}_addrs: hashbrown::HashMap<{tn}, u32>,\n"));
    }

    // Anonymous array allocation tables keyed by parent-struct flash address.
    out.push_str(
        "    // parent flash address -> array base flash address,\n\
         // one table per struct_array_ptr / struct_ptr_array_ptr field.\n",
    );
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let sn = snake_name(&s.name);
        for f in &s.fields {
            match f.kind.as_str() {
                "struct_array_ptr" => out.push_str(&format!(
                    "    {sn}_{}_arr_addrs: hashbrown::HashMap<u32, u32>,\n",
                    f.name
                )),
                "struct_ptr_array_ptr" => out.push_str(&format!(
                    "    {sn}_{}_ptr_arr_addrs: hashbrown::HashMap<u32, u32>,\n",
                    f.name
                )),
                _ => {}
            }
        }
    }

    out.push_str("}\n\n");

    push_serialize_context_impl(out, schema);
}

// ---------------------------------------------------------------------------
// SerializeContext — impl
// ---------------------------------------------------------------------------

fn push_serialize_context_impl(out: &mut String, schema: &Schema) {
    out.push_str("impl<'buf> SerializeContext<'buf> {\n");

    // new() -----------------------------------------------------------------
    out.push_str(
        "    /// Construct a fresh context writing metadata of generation\n\
         /// `metadata_generation`.  Fills `buf` with `0xFF`, which is what a\n\
         /// device's unwritten metadata reads back - so a field this generation\n\
         /// leaves out is indistinguishable from one never written.\n",
    );
    out.push_str(
        "    pub fn new(base_addr: u32, metadata_generation: u32, buf: &'buf mut [u8]) -> Self {\n",
    );
    out.push_str("        buf.fill(0xFF);\n");
    out.push_str("        Self {\n");
    out.push_str("            base_addr,\n");
    out.push_str("            next_addr: base_addr,\n");
    out.push_str("            metadata_generation,\n");
    out.push_str("            buf,\n");
    out.push_str("            written:      hashbrown::HashSet::new(),\n");
    out.push_str("            string_addrs: hashbrown::HashMap::new(),\n");
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let sn = snake_name(&s.name);
        out.push_str(&format!(
            "            {sn}_addrs: hashbrown::HashMap::new(),\n"
        ));
    }
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        let sn = snake_name(&tf.name);
        out.push_str(&format!(
            "            {sn}_addrs: hashbrown::HashMap::new(),\n"
        ));
    }
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        let sn = snake_name(&sf.name);
        out.push_str(&format!(
            "            {sn}_addrs: hashbrown::HashMap::new(),\n"
        ));
    }
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        let sn = snake_name(&s.name);
        for f in &s.fields {
            match f.kind.as_str() {
                "struct_array_ptr" => out.push_str(&format!(
                    "            {sn}_{}_arr_addrs: hashbrown::HashMap::new(),\n",
                    f.name
                )),
                "struct_ptr_array_ptr" => out.push_str(&format!(
                    "            {sn}_{}_ptr_arr_addrs: hashbrown::HashMap::new(),\n",
                    f.name
                )),
                _ => {}
            }
        }
    }
    out.push_str("        }\n");
    out.push_str("    }\n\n");

    // metadata_generation() -------------------------------------------------
    out.push_str(
        "    /// The metadata generation this context is writing.  A generated `write`\n\
         /// asks before laying down a field introduced after it.\n",
    );
    out.push_str("    pub fn metadata_generation(&self) -> u32 {\n");
    out.push_str("        self.metadata_generation\n");
    out.push_str("    }\n\n");

    // alloc_aligned() -------------------------------------------------------
    out.push_str("    /// Allocate `size` bytes, aligning `next_addr` to 4 first.\n");
    out.push_str("    /// Returns the allocated flash address.\n");
    out.push_str(
        "    pub fn alloc_aligned(&mut self, size: usize) -> Result<u32, SerializeError> {\n",
    );
    out.push_str("        let aligned = (self.next_addr.saturating_add(3)) & !3u32;\n");
    out.push_str("        let end = aligned\n");
    out.push_str("            .checked_add(size as u32)\n");
    out.push_str("            .ok_or(SerializeError::Overflow)?;\n");
    out.push_str("        let limit = self.base_addr.saturating_add(self.buf.len() as u32);\n");
    out.push_str("        if end > limit {\n");
    out.push_str("            return Err(SerializeError::Overflow);\n");
    out.push_str("        }\n");
    out.push_str("        self.next_addr = end;\n");
    out.push_str("        Ok(aligned)\n");
    out.push_str("    }\n\n");

    // alloc_bytes() ---------------------------------------------------------
    out.push_str(
        "    /// Allocate `size` bytes with no alignment constraint.  Used for strings.\n",
    );
    out.push_str(
        "    pub fn alloc_bytes(&mut self, size: usize) -> Result<u32, SerializeError> {\n",
    );
    out.push_str("        let end = self.next_addr\n");
    out.push_str("            .checked_add(size as u32)\n");
    out.push_str("            .ok_or(SerializeError::Overflow)?;\n");
    out.push_str("        let limit = self.base_addr.saturating_add(self.buf.len() as u32);\n");
    out.push_str("        if end > limit {\n");
    out.push_str("            return Err(SerializeError::Overflow);\n");
    out.push_str("        }\n");
    out.push_str("        let addr = self.next_addr;\n");
    out.push_str("        self.next_addr = end;\n");
    out.push_str("        Ok(addr)\n");
    out.push_str("    }\n\n");

    // write helpers ---------------------------------------------------------
    out.push_str("    #[inline]\n");
    out.push_str("    pub fn write_u8(&mut self, addr: u32, val: u8) {\n");
    out.push_str("        self.buf[(addr - self.base_addr) as usize] = val;\n");
    out.push_str("    }\n\n");

    out.push_str("    #[inline]\n");
    out.push_str("    pub fn write_u16_le(&mut self, addr: u32, val: u16) {\n");
    out.push_str("        let off = (addr - self.base_addr) as usize;\n");
    out.push_str("        let b = val.to_le_bytes();\n");
    out.push_str("        self.buf[off]     = b[0];\n");
    out.push_str("        self.buf[off + 1] = b[1];\n");
    out.push_str("    }\n\n");

    out.push_str("    #[inline]\n");
    out.push_str("    pub fn write_u32_le(&mut self, addr: u32, val: u32) {\n");
    out.push_str("        let off = (addr - self.base_addr) as usize;\n");
    out.push_str("        self.buf[off..off + 4].copy_from_slice(&val.to_le_bytes());\n");
    out.push_str("    }\n\n");

    out.push_str("    #[inline]\n");
    out.push_str("    pub fn write_bytes(&mut self, addr: u32, data: &[u8]) {\n");
    out.push_str("        let off = (addr - self.base_addr) as usize;\n");
    out.push_str("        self.buf[off..off + data.len()].copy_from_slice(data);\n");
    out.push_str("    }\n");

    out.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Struct serialize impls
// ---------------------------------------------------------------------------

fn push_struct_serialize_impls(out: &mut String, schema: &Schema) {
    if !schema.structs.iter().any(|s| s.generate == Generate::Both) {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Struct serialize impls (layout / layout_sub_objects / write)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for s in schema
        .structs
        .iter()
        .filter(|s| s.generate == Generate::Both)
    {
        push_struct_serialize(out, s, schema);
    }
}

fn push_struct_serialize(out: &mut String, s: &Struct, schema: &Schema) {
    let tn = rust_type_name(&s.name);
    let sn = snake_name(&s.name);

    let size = s.size.map(|n| n as usize).unwrap_or_else(|| {
        s.fields
            .iter()
            .map(|f| field_size(f, schema))
            .sum::<usize>()
    });

    // Map: count_field_name -> vec_field_name.
    // Used in write() to derive count scalar values from Vec lengths.
    let derived_counts: HashMap<String, String> = s
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

    crate::rust_gen::push_deprecated_allow(out, s.fields.iter(), "");
    out.push_str(&format!("impl {tn} {{\n"));
    push_struct_layout(out, sn, size);
    push_struct_layout_sub_objects(out, s, sn, schema);
    push_struct_check_generation(out, s, schema);
    push_struct_write(out, s, sn, &derived_counts, schema);
    out.push_str("}\n\n");
}

/// Emit `check_generation`, which refuses a value the metadata generation
/// being written has nowhere to put.
///
/// The walk goes only where a gated field can be found.  The shipped schema
/// has none, so every one of these is a body that returns `Ok`.
fn push_struct_check_generation(out: &mut String, s: &Struct, schema: &Schema) {
    let gated: Vec<&Field> = s
        .fields
        .iter()
        .filter(|f| f.metadata_since().is_some())
        .collect();
    let recurse: Vec<&Field> = s
        .fields
        .iter()
        .filter(|f| {
            f.referenced_type()
                .is_some_and(|t| carries_gated_field(schema, t))
        })
        .collect();

    out.push_str(
        "    /// Refuse a field metadata of `generation` cannot carry and that holds\n\
         /// anything but the default a reader of that generation uses.\n\
         ///\n\
         /// Walks everything this object reaches, so one call on the root covers\n\
         /// the whole tree.  Called by `serialize` before anything is written.\n",
    );
    let param = if gated.is_empty() && recurse.is_empty() {
        "_generation"
    } else {
        "generation"
    };
    out.push_str(&format!(
        "    pub fn check_generation(&self, {param}: u32) -> Result<(), SerializeError> {{\n"
    ));

    for f in gated {
        let generation = f
            .metadata_since()
            .expect("the field was filtered on carrying a metadata generation");
        let (major, minor, patch) = schema
            .metadata_generation_release(generation)
            .unwrap_or_else(|| {
                panic!(
                    "{}.{} arrived in metadata generation {generation}, and no [[versions]] \
                     entry says which release that is",
                    s.name, f.name
                )
            });
        out.push_str(&format!(
            "        if {param} < {generation}u32 && {} {{\n",
            away_from_default(f, schema),
        ));
        out.push_str("            return Err(SerializeError::FieldTooNew {\n");
        out.push_str(&format!(
            "                field: \"{}.{}\",\n",
            s.name, f.name
        ));
        out.push_str(&format!(
            "                minimum: FirmwareVersion::new({major}, {minor}, {patch}, 0),\n"
        ));
        out.push_str("            });\n");
        out.push_str("        }\n");
    }

    for f in recurse {
        emit_check_generation_recursion(out, f);
    }

    out.push_str("        Ok(())\n");
    out.push_str("    }\n\n");
}

/// The test that a gated field holds something other than its declared
/// default.  A number is compared against the literal, and the kinds whose
/// default is "nothing there" are asked whether they hold anything - which
/// says the same thing and is what clippy wants of an `Option`.
fn away_from_default(f: &Field, schema: &Schema) -> String {
    let name = &f.name;
    match f.kind.as_str() {
        "cstr_ptr" | "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
            format!("self.{name}.is_some()")
        }
        "struct_array_ptr" | "struct_ptr_array_ptr" => format!("!self.{name}.is_empty()"),
        "opaque_ptr" | "fn_ptr" => format!("!self.{name}.is_null()"),
        _ => format!(
            "self.{name} != {}",
            crate::rust_gen::rust_default_expr(f, schema)
        ),
    }
}

/// Emit the walk into one field that reaches a gated field.
fn emit_check_generation_recursion(out: &mut String, f: &Field) {
    let name = &f.name;
    let ind = "        ";

    match f.kind.as_str() {
        "struct_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}if let Some(sub) = &self.{name} {{\n\
                     {ind}    sub.check_generation(generation)?;\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!(
                    "{ind}self.{name}.check_generation(generation)?;\n"
                ));
            }
        }

        "struct_array_ptr" | "struct_ptr_array_ptr" => {
            out.push_str(&format!(
                "{ind}for elem in &self.{name} {{\n\
                 {ind}    elem.check_generation(generation)?;\n\
                 {ind}}}\n"
            ));
        }

        // A FAM's fields are scalars and enums, and a generation marker is
        // refused on one, so nothing gated can sit under a FAM pointer.
        kind => panic!("{name} is a {kind}, which reaches no gated field"),
    }
}

/// Whether `type_name`, or anything it reaches, has a field the metadata
/// generation gates.
///
/// This is what prunes the `check_generation` walk: a branch with nothing
/// gated under it is not entered at all.
fn carries_gated_field(schema: &Schema, type_name: &str) -> bool {
    // A type reached twice was settled the first time: had it carried a gated
    // field the walk would have stopped there, so the second answer is no.
    fn walk<'a>(schema: &'a Schema, type_name: &'a str, seen: &mut Vec<&'a str>) -> bool {
        if seen.contains(&type_name) {
            return false;
        }
        seen.push(type_name);

        // A FAM's fields are scalars and enums and a marker is refused on one,
        // so a FAM ends the walk.  So does a name no struct answers to, which
        // the schema's own reference checks have already ruled out.
        let Some(s) = schema.structs.iter().find(|s| s.name == type_name) else {
            return false;
        };
        for f in &s.fields {
            if f.metadata_since().is_some() {
                return true;
            }
            if let Some(referenced) = f.referenced_type()
                && walk(schema, referenced, seen)
            {
                return true;
            }
        }
        false
    }

    walk(schema, type_name, &mut Vec::new())
}

fn push_struct_layout(out: &mut String, sn: &str, size: usize) {
    out.push_str(
        "    /// Phase 1: assign a flash address, interning for dedup.\n\
         /// Idempotent: returns the existing address if already interned.\n",
    );
    out.push_str(
        "    pub fn layout(&self, ctx: &mut SerializeContext<'_>) -> Result<u32, SerializeError> {\n",
    );
    out.push_str(&format!(
        "        if let Some(&addr) = ctx.{sn}_addrs.get(self) {{\n"
    ));
    out.push_str("            return Ok(addr);\n");
    out.push_str("        }\n");
    out.push_str(&format!(
        "        let addr = ctx.alloc_aligned({size}usize)?;\n"
    ));
    out.push_str(&format!(
        "        ctx.{sn}_addrs.insert(self.clone(), addr);\n"
    ));
    out.push_str("        self.layout_sub_objects(ctx, addr)?;\n");
    out.push_str("        Ok(addr)\n");
    out.push_str("    }\n\n");
}

fn push_struct_layout_sub_objects(out: &mut String, s: &Struct, sn: &str, schema: &Schema) {
    out.push_str(
        "    /// Phase 1 (sub-objects only): lay out everything pointed to by this\n\
         /// struct.  Separated from layout() so inline array elements can have\n\
         /// sub-objects laid out without allocating the element itself.\n",
    );
    out.push_str("    pub fn layout_sub_objects(\n");
    out.push_str("        &self,\n");

    let needs_ctx = s.fields.iter().any(|f| {
        matches!(
            f.kind.as_str(),
            "cstr_ptr"
                | "struct_ptr"
                | "struct_array_ptr"
                | "struct_ptr_array_ptr"
                | "tagged_fam_ptr"
                | "simple_fam_ptr"
        )
    });
    let needs_self_addr = s
        .fields
        .iter()
        .any(|f| matches!(f.kind.as_str(), "struct_array_ptr" | "struct_ptr_array_ptr"));

    if needs_ctx {
        out.push_str("        ctx: &mut SerializeContext<'_>,\n");
    } else {
        out.push_str("        _ctx: &mut SerializeContext<'_>,\n");
    }
    out.push_str("        self_addr: u32,\n");
    out.push_str("    ) -> Result<(), SerializeError> {\n");

    if !needs_self_addr {
        out.push_str("        #[allow(unused_variables)]\n");
        out.push_str("        let _ = self_addr;\n");
    }

    for f in &s.fields {
        emit_layout_sub_objects_field(out, f, sn, schema);
    }

    out.push_str("        Ok(())\n");
    out.push_str("    }\n\n");
}

fn emit_layout_sub_objects_field(out: &mut String, f: &Field, parent_sn: &str, schema: &Schema) {
    let name = &f.name;
    let ind = "        ";

    match f.kind.as_str() {
        "cstr_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}#[allow(clippy::collapsible_if)]\n\
                     {ind}if let Some(s) = &self.{name} {{\n\
                     {ind}    if !ctx.string_addrs.contains_key(s.as_str()) {{\n\
                     {ind}        let saddr = ctx.alloc_bytes(s.len() + 1)?;\n\
                     {ind}        ctx.string_addrs.insert(s.clone(), saddr);\n\
                     {ind}    }}\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!(
                    "{ind}if !ctx.string_addrs.contains_key(self.{name}.as_str()) {{\n\
                     {ind}    let saddr = ctx.alloc_bytes(self.{name}.len() + 1)?;\n\
                     {ind}    ctx.string_addrs.insert(self.{name}.clone(), saddr);\n\
                     {ind}}}\n"
                ));
            }
        }

        "struct_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}if let Some(sub) = &self.{name} {{\n\
                     {ind}    sub.layout(ctx)?;\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!("{ind}self.{name}.layout(ctx)?;\n"));
            }
        }

        "struct_array_ptr" => {
            let count_f = f.count_field.as_deref().unwrap_or("?");
            let elem_c = f.element.as_deref().unwrap_or("");
            let stride = struct_stride(elem_c, schema);
            out.push_str(&format!(
                "{ind}{{\n\
                 {ind}    let n = self.{name}.len();\n\
                 {ind}    if n > 255usize {{\n\
                 {ind}        return Err(SerializeError::CountOverflow {{ field: \"{count_f}\" }});\n\
                 {ind}    }}\n\
                 {ind}    let arr_addr = ctx.alloc_aligned(n * {stride}usize)?;\n\
                 {ind}    ctx.{parent_sn}_{name}_arr_addrs.insert(self_addr, arr_addr);\n\
                 {ind}    for (i, elem) in self.{name}.iter().enumerate() {{\n\
                 {ind}        let elem_addr = arr_addr + (i as u32 * {stride}u32);\n\
                 {ind}        elem.layout_sub_objects(ctx, elem_addr)?;\n\
                 {ind}    }}\n\
                 {ind}}}\n"
            ));
        }

        "struct_ptr_array_ptr" => {
            let count_f = f.count_field.as_deref().unwrap_or("?");
            out.push_str(&format!(
                "{ind}{{\n\
                 {ind}    let n = self.{name}.len();\n\
                 {ind}    if n > 255usize {{\n\
                 {ind}        return Err(SerializeError::CountOverflow {{ field: \"{count_f}\" }});\n\
                 {ind}    }}\n\
                 {ind}    for elem in &self.{name} {{\n\
                 {ind}        elem.layout(ctx)?;\n\
                 {ind}    }}\n\
                 {ind}    let ptr_arr_addr = ctx.alloc_aligned(n * 4usize)?;\n\
                 {ind}    ctx.{parent_sn}_{name}_ptr_arr_addrs.insert(self_addr, ptr_arr_addr);\n\
                 {ind}}}\n"
            ));
        }

        "tagged_fam_ptr" | "simple_fam_ptr" => {
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}if let Some(sub) = &self.{name} {{\n\
                     {ind}    sub.layout(ctx)?;\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!("{ind}self.{name}.layout(ctx)?;\n"));
            }
        }

        // scalar, enum, type_alias, inline_array*, opaque_ptr, fn_ptr,
        // padding: no pointer sub-objects to lay out.
        _ => {}
    }
}

fn push_struct_write(
    out: &mut String,
    s: &Struct,
    sn: &str,
    derived_counts: &HashMap<String, String>,
    schema: &Schema,
) {
    out.push_str(
        "    /// Phase 2: write `self`'s bytes into `ctx.buf` at `addr`.\n\
         /// Returns immediately if `addr` was already written (dedup guard).\n",
    );
    out.push_str("    pub fn write(&self, ctx: &mut SerializeContext<'_>, addr: u32) {\n");
    out.push_str("        if ctx.written.contains(&addr) { return; }\n");
    out.push_str("        ctx.written.insert(addr);\n");

    let mut byte_off: usize = 0;
    for f in &s.fields {
        emit_struct_write_field(out, f, byte_off, sn, derived_counts, schema);
        byte_off += field_size(f, schema);
    }

    out.push_str("    }\n");
}

fn addr_expr(off: usize) -> String {
    if off == 0 {
        "addr".to_string()
    } else {
        format!("addr + {off}u32")
    }
}

/// Emit the write for one struct field, behind the generation that brought it
/// in where it has one.  The offset of everything after it is unaffected,
/// because a field's place in the layout is fixed whether or not this
/// generation writes it.
fn emit_struct_write_field(
    out: &mut String,
    f: &Field,
    byte_off: usize,
    parent_sn: &str,
    derived_counts: &HashMap<String, String>,
    schema: &Schema,
) {
    let Some(generation) = f.metadata_since() else {
        emit_struct_write_field_body(out, f, byte_off, parent_sn, derived_counts, schema);
        return;
    };

    let mut body = String::new();
    emit_struct_write_field_body(&mut body, f, byte_off, parent_sn, derived_counts, schema);

    let name = &f.name;
    out.push_str(&format!(
        "        // {name} arrived in metadata generation {generation}, so older\n"
    ));
    out.push_str("        // metadata leaves its bytes unwritten.\n");
    out.push_str(&format!(
        "        if ctx.metadata_generation() >= {generation}u32 {{\n"
    ));
    for line in body.lines() {
        out.push_str(&format!("    {line}\n"));
    }
    out.push_str("        }\n");
}

fn emit_struct_write_field_body(
    out: &mut String,
    f: &Field,
    byte_off: usize,
    parent_sn: &str,
    derived_counts: &HashMap<String, String>,
    schema: &Schema,
) {
    let name = &f.name;
    let ind = "        ";

    match f.kind.as_str() {
        "scalar" => {
            let ty = f.type_.as_deref().unwrap_or("u8");
            let (method, _) = scalar_write(ty);
            if let Some(vec_field) = derived_counts.get(name.as_str()) {
                let a = addr_expr(byte_off);
                out.push_str(&format!(
                    "{ind}// {name}: derived from {vec_field}.len()\n\
                     {ind}ctx.{method}({a}, self.{vec_field}.len() as {ty});\n"
                ));
            } else {
                let a = addr_expr(byte_off);
                out.push_str(&format!("{ind}ctx.{method}({a}, self.{name});\n"));
            }
        }

        "enum" => {
            let (method, sz) = enum_write(f, schema);
            let repr = if sz == 1 { "u8" } else { "u16" };
            let a = addr_expr(byte_off);
            let value = crate::rust_gen::maybe_known_raw_expr(&format!("self.{name}"), repr);
            out.push_str(&format!("{ind}ctx.{method}({a}, {value});\n"));
        }

        "type_alias" => {
            let underlying = schema
                .type_aliases
                .iter()
                .find(|a| f.type_.as_deref() == Some(a.name.as_str()))
                .map_or("u16", |a| a.underlying.as_str());
            let (method, _) = scalar_write(underlying);
            let a = addr_expr(byte_off);
            out.push_str(&format!("{ind}ctx.{method}({a}, self.{name});\n"));
        }

        "inline_array" => {
            let elem = f.element.as_deref().unwrap_or("u8");
            if elem == "u8" || elem == "char" {
                let a = addr_expr(byte_off);
                out.push_str(&format!("{ind}ctx.write_bytes({a}, &self.{name});\n"));
            } else {
                out.push_str(&format!(
                    "{ind}compile_error!(\"non-u8 inline_array write not implemented for `{name}`\");\n"
                ));
            }
        }

        "inline_array2d" => {
            let cols = f.cols.unwrap_or(0);
            let a = addr_expr(byte_off);
            out.push_str(&format!(
                "{ind}for (r, row) in self.{name}.iter().enumerate() {{\n\
                 {ind}    ctx.write_bytes({a} + (r as u32 * {cols}u32), row);\n\
                 {ind}}}\n"
            ));
        }

        "cstr_ptr" => {
            let a = addr_expr(byte_off);
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}match &self.{name} {{\n\
                     {ind}    None => ctx.write_u32_le({a}, 0u32),\n\
                     {ind}    Some(s) => {{\n\
                     {ind}        let s_addr = *ctx.string_addrs\n\
                     {ind}            .get(s.as_str())\n\
                     {ind}            .expect(\"serialize invariant: string addr missing\");\n\
                     {ind}        ctx.write_u32_le({a}, s_addr);\n\
                     {ind}        if ctx.written.insert(s_addr) {{\n\
                     {ind}            ctx.write_bytes(s_addr, s.as_bytes());\n\
                     {ind}            ctx.write_u8(s_addr + s.len() as u32, 0u8);\n\
                     {ind}        }}\n\
                     {ind}    }}\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!(
                    "{ind}{{\n\
                     {ind}    let s_addr = *ctx.string_addrs\n\
                     {ind}        .get(self.{name}.as_str())\n\
                     {ind}        .expect(\"serialize invariant: string addr missing\");\n\
                     {ind}    ctx.write_u32_le({a}, s_addr);\n\
                     {ind}    if ctx.written.insert(s_addr) {{\n\
                     {ind}        ctx.write_bytes(s_addr, self.{name}.as_bytes());\n\
                     {ind}        ctx.write_u8(s_addr + self.{name}.len() as u32, 0u8);\n\
                     {ind}    }}\n\
                     {ind}}}\n"
                ));
            }
        }

        "struct_ptr" => {
            let a = addr_expr(byte_off);
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}match &self.{name} {{\n\
                     {ind}    None => ctx.write_u32_le({a}, 0u32),\n\
                     {ind}    Some(sub) => {{\n\
                     {ind}        let sub_addr = sub\n\
                     {ind}            .layout(ctx)\n\
                     {ind}            .expect(\"serialize invariant: layout in write\");\n\
                     {ind}        ctx.write_u32_le({a}, sub_addr);\n\
                     {ind}        sub.write(ctx, sub_addr);\n\
                     {ind}    }}\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!(
                    "{ind}{{\n\
                     {ind}    let sub_addr = self.{name}\n\
                     {ind}        .layout(ctx)\n\
                     {ind}        .expect(\"serialize invariant: layout in write\");\n\
                     {ind}    ctx.write_u32_le({a}, sub_addr);\n\
                     {ind}    self.{name}.write(ctx, sub_addr);\n\
                     {ind}}}\n"
                ));
            }
        }

        "struct_array_ptr" => {
            let elem_c = f.element.as_deref().unwrap_or("");
            let stride = struct_stride(elem_c, schema);
            let a = addr_expr(byte_off);
            out.push_str(&format!(
                "{ind}{{\n\
                 {ind}    let arr_addr = *ctx.{parent_sn}_{name}_arr_addrs\n\
                 {ind}        .get(&addr)\n\
                 {ind}        .expect(\"serialize invariant: array base addr missing\");\n\
                 {ind}    ctx.write_u32_le({a}, arr_addr);\n\
                 {ind}    for (i, elem) in self.{name}.iter().enumerate() {{\n\
                 {ind}        let elem_addr = arr_addr + (i as u32 * {stride}u32);\n\
                 {ind}        elem.write(ctx, elem_addr);\n\
                 {ind}    }}\n\
                 {ind}}}\n"
            ));
        }

        "struct_ptr_array_ptr" => {
            let a = addr_expr(byte_off);
            out.push_str(&format!(
                "{ind}{{\n\
                 {ind}    let ptr_arr_addr = *ctx.{parent_sn}_{name}_ptr_arr_addrs\n\
                 {ind}        .get(&addr)\n\
                 {ind}        .expect(\"serialize invariant: ptr array addr missing\");\n\
                 {ind}    ctx.write_u32_le({a}, ptr_arr_addr);\n\
                 {ind}    for (i, elem) in self.{name}.iter().enumerate() {{\n\
                 {ind}        let elem_addr = elem\n\
                 {ind}            .layout(ctx)\n\
                 {ind}            .expect(\"serialize invariant: layout in write\");\n\
                 {ind}        ctx.write_u32_le(ptr_arr_addr + (i as u32 * 4u32), elem_addr);\n\
                 {ind}        elem.write(ctx, elem_addr);\n\
                 {ind}    }}\n\
                 {ind}}}\n"
            ));
        }

        "tagged_fam_ptr" | "simple_fam_ptr" => {
            let a = addr_expr(byte_off);
            if f.nullable.unwrap_or(false) {
                out.push_str(&format!(
                    "{ind}match &self.{name} {{\n\
                     {ind}    None => ctx.write_u32_le({a}, 0u32),\n\
                     {ind}    Some(sub) => {{\n\
                     {ind}        let sub_addr = sub\n\
                     {ind}            .layout(ctx)\n\
                     {ind}            .expect(\"serialize invariant: layout in write\");\n\
                     {ind}        ctx.write_u32_le({a}, sub_addr);\n\
                     {ind}        sub.write(ctx, sub_addr);\n\
                     {ind}    }}\n\
                     {ind}}}\n"
                ));
            } else {
                out.push_str(&format!(
                    "{ind}{{\n\
                     {ind}    let sub_addr = self.{name}\n\
                     {ind}        .layout(ctx)\n\
                     {ind}        .expect(\"serialize invariant: layout in write\");\n\
                     {ind}    ctx.write_u32_le({a}, sub_addr);\n\
                     {ind}    self.{name}.write(ctx, sub_addr);\n\
                     {ind}}}\n"
                ));
            }
        }

        "opaque_ptr" | "fn_ptr" => {
            let a = addr_expr(byte_off);
            out.push_str(&format!("{ind}ctx.write_u32_le({a}, self.{name}.raw());\n"));
        }

        "padding" => {
            // Buffer pre-filled with 0xFF; nothing to emit.
        }

        kind => {
            out.push_str(&format!(
                "{ind}compile_error!(\"unhandled field kind `{kind}` for `{name}` in write\");\n"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Tagged FAM serialize impls
// ---------------------------------------------------------------------------

fn push_tagged_fam_serialize_impls(out: &mut String, schema: &Schema) {
    if !schema
        .tagged_fams
        .iter()
        .any(|tf| tf.generate == Generate::Both)
    {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Tagged FAM serialize impls (layout / write)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for tf in schema
        .tagged_fams
        .iter()
        .filter(|tf| tf.generate == Generate::Both)
    {
        push_tagged_fam_serialize(out, tf, schema);
    }
}

fn push_tagged_fam_serialize(out: &mut String, tf: &TaggedFam, schema: &Schema) {
    let tn = rust_type_name(&tf.name);
    let sn = snake_name(&tf.name);
    let disc_enum = schema.enums.iter().find(|e| e.name == tf.discriminant_type);
    let disc_size = disc_enum.map_or(1u32, |e| e.size) as usize;
    let strip = disc_enum
        .and_then(|e| e.strip_prefix.as_deref())
        .unwrap_or("");

    out.push_str(&format!("impl {tn} {{\n"));
    push_tagged_fam_layout(out, tf, sn, strip, schema);
    push_tagged_fam_write(out, tf, sn, disc_enum, disc_size, strip, schema);
    out.push_str("}\n\n");
}

fn push_tagged_fam_layout(
    out: &mut String,
    tf: &TaggedFam,
    sn: &str,
    strip: &str,
    schema: &Schema,
) {
    out.push_str(
        "    pub fn layout(&self, ctx: &mut SerializeContext<'_>) -> Result<u32, SerializeError> {\n",
    );
    out.push_str(&format!(
        "        if let Some(&addr) = ctx.{sn}_addrs.get(self) {{\n"
    ));
    out.push_str("            return Ok(addr);\n");
    out.push_str("        }\n");
    out.push_str("        let size = match self {\n");
    for v in &tf.variants {
        let vn = fam_variant_ident(&v.discriminant, strip);
        let params_len = const_value(schema, &v.params_len_constant);
        let total = tf.base_size as usize + params_len;
        out.push_str(&format!(
            "            Self::{vn} {{ .. }} => {total}usize,\n"
        ));
    }
    // A discriminant this build has no name for keeps its parameter bytes
    // whole, so their count is what the variant costs beyond the base.
    out.push_str(&format!(
        "            Self::Unknown {{ params, .. }} => {}usize + params.len(),\n",
        tf.base_size
    ));
    out.push_str("        };\n");
    out.push_str("        let addr = ctx.alloc_aligned(size)?;\n");
    out.push_str(&format!(
        "        ctx.{sn}_addrs.insert(self.clone(), addr);\n"
    ));
    out.push_str("        Ok(addr)\n");
    out.push_str("    }\n\n");
}

fn push_tagged_fam_write(
    out: &mut String,
    tf: &TaggedFam,
    _sn: &str,
    disc_enum: Option<&Enum>,
    disc_size: usize,
    strip: &str,
    schema: &Schema,
) {
    out.push_str("    pub fn write(&self, ctx: &mut SerializeContext<'_>, addr: u32) {\n");
    out.push_str("        if ctx.written.contains(&addr) { return; }\n");
    out.push_str("        ctx.written.insert(addr);\n");
    out.push_str("        match self {\n");

    for v in &tf.variants {
        let vn = fam_variant_ident(&v.discriminant, strip);

        // All non-padding field names for the match-arm pattern binding.
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

        // Discriminant value.
        let disc_val = disc_enum
            .and_then(|e| e.variants.iter().find(|ev| ev.name == v.discriminant))
            .map_or(0i64, |ev| ev.value);
        let disc_method = if disc_size == 1 {
            "write_u8"
        } else {
            "write_u16_le"
        };
        out.push_str(&format!(
            "                ctx.{disc_method}(addr, {disc_val} as _);\n"
        ));

        // param_len byte.
        let params_len = const_value(schema, &v.params_len_constant);
        let a = addr_expr(disc_size);
        out.push_str(&format!(
            "                ctx.write_u8({a}, {params_len}u8);\n"
        ));

        // Common fields at [disc_size+1 .. base_size).
        let mut off = disc_size + 1;
        for f in &tf.common_fields {
            emit_fam_field_write(out, f, off, schema);
            off += field_size(f, schema);
        }

        // Variant param fields at [base_size .. base_size + params_len).
        off = tf.base_size as usize;
        for f in &v.fields {
            emit_fam_field_write(out, f, off, schema);
            off += field_size(f, schema);
        }

        out.push_str("            }\n");
    }

    push_tagged_fam_unknown_write(out, tf, disc_size, schema);

    out.push_str("        }\n");
    out.push_str("    }\n");
}

/// Emit the write arm for a discriminant this build has no name for.
///
/// Everything read back out goes back as it came in - the discriminant, the
/// parameter length, the common fields and the parameter bytes - so a host
/// that reads a device and composes from what it read does not silently
/// rewrite an algorithm it has never heard of.
fn push_tagged_fam_unknown_write(
    out: &mut String,
    tf: &TaggedFam,
    disc_size: usize,
    schema: &Schema,
) {
    let disc = &tf.discriminant_field;
    let fields: Vec<&str> = tf
        .common_fields
        .iter()
        .filter(|f| f.kind != "padding")
        .map(|f| f.name.as_str())
        .collect();
    let mut bindings = vec![disc.as_str()];
    bindings.extend(fields);
    bindings.push("params");
    out.push_str(&format!(
        "            Self::Unknown {{ {} }} => {{\n",
        bindings.join(", ")
    ));

    // Note: the {} below is a format specifier in the GENERATED code's
    // debug_assert!, not in our generator.
    out.push_str("                debug_assert!(\n");
    out.push_str("                    params.len() <= 255,\n");
    out.push_str("                    \"unknown variant params length {} exceeds u8 range\",\n");
    out.push_str("                    params.len(),\n");
    out.push_str("                );\n");

    let disc_method = if disc_size == 1 {
        "write_u8"
    } else {
        "write_u16_le"
    };
    out.push_str(&format!(
        "                ctx.{disc_method}(addr, *{disc} as _);\n"
    ));
    let a = addr_expr(disc_size);
    out.push_str(&format!(
        "                ctx.write_u8({a}, params.len() as u8);\n"
    ));

    let mut off = disc_size + 1;
    for f in &tf.common_fields {
        emit_fam_field_write(out, f, off, schema);
        off += field_size(f, schema);
    }

    let params_at = addr_expr(tf.base_size as usize);
    out.push_str("                if !params.is_empty() {\n");
    out.push_str(&format!(
        "                    ctx.write_bytes({params_at}, params);\n"
    ));
    out.push_str("                }\n");
    out.push_str("            }\n");
}

/// Emit one write call for a tagged FAM field (scalar / enum / type_alias only).
/// The variable is bound by reference in the match-arm pattern (`field: &T`).
fn emit_fam_field_write(out: &mut String, f: &Field, off: usize, schema: &Schema) {
    let name = &f.name;
    match f.kind.as_str() {
        "scalar" => {
            let ty = f.type_.as_deref().unwrap_or("u8");
            let (method, _) = scalar_write(ty);
            let a = addr_expr(off);
            out.push_str(&format!("                ctx.{method}({a}, *{name});\n"));
        }
        "enum" => {
            let (method, sz) = enum_write(f, schema);
            let repr = if sz == 1 { "u8" } else { "u16" };
            let a = addr_expr(off);
            let value = crate::rust_gen::maybe_known_raw_expr(&format!("*{name}"), repr);
            out.push_str(&format!("                ctx.{method}({a}, {value});\n"));
        }
        "type_alias" => {
            let underlying = schema
                .type_aliases
                .iter()
                .find(|a| f.type_.as_deref() == Some(a.name.as_str()))
                .map_or("u16", |a| a.underlying.as_str());
            let (method, _) = scalar_write(underlying);
            let a = addr_expr(off);
            out.push_str(&format!("                ctx.{method}({a}, *{name});\n"));
        }
        "padding" => {}
        other => {
            out.push_str(&format!(
                "                compile_error!(\"unexpected FAM field kind `{other}` for `{name}`\");\n"
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Simple FAM serialize impls
// ---------------------------------------------------------------------------

fn push_simple_fam_serialize_impls(out: &mut String, schema: &Schema) {
    if !schema
        .simple_fams
        .iter()
        .any(|sf| sf.generate == Generate::Both)
    {
        return;
    }
    out.push_str(
        "// ---------------------------------------------------------------------------\n\
         // Simple FAM serialize impls (layout / write)\n\
         // ---------------------------------------------------------------------------\n\n",
    );
    for sf in schema
        .simple_fams
        .iter()
        .filter(|sf| sf.generate == Generate::Both)
    {
        push_simple_fam_serialize(out, sf);
    }
}

fn push_simple_fam_serialize(out: &mut String, sf: &SimpleFam) {
    let tn = rust_type_name(&sf.name);
    let sn = snake_name(&sf.name);

    out.push_str(&format!("impl {tn} {{\n"));

    // layout()
    out.push_str(
        "    pub fn layout(&self, ctx: &mut SerializeContext<'_>) -> Result<u32, SerializeError> {\n",
    );
    out.push_str(&format!(
        "        if let Some(&addr) = ctx.{sn}_addrs.get(self) {{\n"
    ));
    out.push_str("            return Ok(addr);\n");
    out.push_str("        }\n");
    out.push_str("        let size = 1usize + self.params.len();\n");
    out.push_str("        let addr = ctx.alloc_aligned(size)?;\n");
    out.push_str(&format!(
        "        ctx.{sn}_addrs.insert(self.clone(), addr);\n"
    ));
    out.push_str("        Ok(addr)\n");
    out.push_str("    }\n\n");

    // write()
    out.push_str("    pub fn write(&self, ctx: &mut SerializeContext<'_>, addr: u32) {\n");
    out.push_str("        if ctx.written.contains(&addr) { return; }\n");
    out.push_str("        ctx.written.insert(addr);\n");
    // Note: the {} below is a format specifier in the GENERATED code's debug_assert!,
    // not in our generator.  push_str does no substitution, so it passes through as-is.
    out.push_str("        debug_assert!(\n");
    out.push_str("            self.params.len() <= 255,\n");
    out.push_str("            \"simple FAM params length {} exceeds u8 range\",\n");
    out.push_str("            self.params.len(),\n");
    out.push_str("        );\n");
    out.push_str("        ctx.write_u8(addr, self.params.len() as u8);\n");
    out.push_str("        if !self.params.is_empty() {\n");
    out.push_str("            ctx.write_bytes(addr + 1u32, &self.params);\n");
    out.push_str("        }\n");
    out.push_str("    }\n");

    out.push_str("}\n\n");
}
