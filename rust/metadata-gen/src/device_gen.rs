// src/device_gen.rs
//
// Generates $OUT_DIR/device_generated.rs: a Rust type for every type the C
// header defines, for a Rust firmware to place in flash or RAM. Structs are
// `#[repr(C)]`. Enums and type aliases keep their C storage type.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License
//
// Output file layout:
//   1. File header
//   2. Type aliases
//   3. Enums                (newtype over the storage integer)
//   4. Structs              (pub struct, MAGIC, layout asserts)
//   4. Tagged FAMs          (base struct, one param struct per variant)
//   5. Simple FAMs          (pub struct + layout asserts)
//   6. Family anchor        (the alias `header_name` names, if any)
//
// Every type takes its C name, and is generated whatever its `generate` mode,
// as the C header does.
//
// `Ptr` and `RuntimeCell` are hand-written in onerom-metadata. The generated
// file names them by path - `crate::` in the schema filling the info slot,
// which is onerom-metadata's, and `::onerom_metadata::` in any other.
//
// Layout asserts compile on a 32-bit target only. The C header's layout is the
// device's, where a pointer is four bytes.

use crate::c_gen::{derive_param_struct_name, num_chip_types, rbcp_chip_types};
use crate::rust_gen::{rust_type_name, variant_ident};
use crate::schema::*;

/// The crate holding the family anchor, `Ptr` and `RuntimeCell`.
const FAMILY_CRATE: &str = "onerom_metadata";

/// The family anchor, the type a `header_name` aliases.
const FAMILY_ANCHOR: &str = "onerom_info_t";

pub fn generate(schema: &Schema) -> String {
    let g = Gen {
        schema,
        support: match schema.struct_in_slot(SLOT_INFO) {
            Some(_) => "crate".into(),
            None => format!("::{FAMILY_CRATE}"),
        },
    };
    let mut out = String::with_capacity(64 * 1024);
    push_file_header(&mut out, schema);
    g.push_type_aliases(&mut out);
    g.push_enums(&mut out);
    g.push_structs(&mut out);
    g.push_tagged_fams(&mut out);
    g.push_simple_fams(&mut out);
    g.push_anchor(&mut out);
    out
}

/// One field of a generated type, as the layout asserts need it.
struct Member {
    name: String,
    ty: String,
    offset: usize,
    doc: Option<String>,
}

struct Gen<'a> {
    schema: &'a Schema,
    /// The path `Ptr` and `RuntimeCell` are reached by.
    support: String,
}

fn push_file_header(out: &mut String, schema: &Schema) {
    out.push_str(&format!(
        "// @generated — do not edit by hand.\n\
         // Source:    {}\n\
         // Generator: onerom-metadata-gen device_gen.rs\n\n",
        schema.source
    ));
}

impl Gen<'_> {
    // -----------------------------------------------------------------------
    // Enums
    // -----------------------------------------------------------------------

    fn push_type_aliases(&self, out: &mut String) {
        if self.schema.type_aliases.is_empty() {
            return;
        }
        push_section(out, "Type aliases");
        for a in &self.schema.type_aliases {
            if let Some(line) = a.comment.as_deref().and_then(|c| c.lines().next()) {
                out.push_str(&format!("/// {line}\n"));
            }
            out.push_str("#[allow(non_camel_case_types)]\n");
            out.push_str(&format!(
                "pub type {} = {};\n\n",
                a.name,
                prim(Some(a.underlying.as_str()))
            ));
        }
    }

    fn push_enums(&self, out: &mut String) {
        if self.schema.enums.is_empty() {
            return;
        }
        push_section(out, "Enums");
        for e in &self.schema.enums {
            self.push_enum(out, e);
        }
    }

    /// A newtype over the storage integer, so any bytes a newer firmware wrote
    /// are a valid value. Every variant, sentinel and alias is a constant.
    fn push_enum(&self, out: &mut String, e: &Enum) {
        let storage = storage_for(e.size as usize);
        let host = match e.source.as_deref() {
            Some("rbcp_chip_types") => Some("onerom_config::chip::ChipType".to_string()),
            Some(_) => None,
            None => Some(rust_type_name(&e.name)),
        };
        push_type_doc(out, e.comment.as_deref(), &e.name, host.as_deref());
        out.push_str("#[allow(non_camel_case_types)]\n");
        out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
        out.push_str("#[repr(transparent)]\n");
        out.push_str(&format!("pub struct {}(pub {storage});\n\n", e.name));

        // The constants take the C names, some of which are mixed case.
        out.push_str("#[allow(non_upper_case_globals)]\n");
        out.push_str(&format!("impl {} {{\n", e.name));
        match e.source.as_deref() {
            Some("rbcp_chip_types") => push_rbcp_constants(out, e.size),
            _ => {
                for v in &e.variants {
                    push_constant(out, v.comment.as_deref(), &v.name, v.value, e.size);
                }
                for a in &e.aliases {
                    push_field_doc(out, "    ", a.comment.as_deref());
                    out.push_str(&format!(
                        "    pub const {}: Self = Self::{};\n",
                        a.name, a.target
                    ));
                }
            }
        }
        out.push_str("}\n\n");

        push_asserts(out, &e.name, &e.name, e.size as usize, &[]);
    }

    // -----------------------------------------------------------------------
    // Structs
    // -----------------------------------------------------------------------

    fn push_structs(&self, out: &mut String) {
        if self.schema.structs.is_empty() {
            return;
        }
        push_section(out, "Structs");
        for s in &self.schema.structs {
            self.push_struct(out, s);
        }
    }

    /// The type parameters of the info structure, as (field, parameter,
    /// default): one per pointer to the structure filling the metadata or
    /// runtime slot. Every member of the family writes the info structure,
    /// and its pointers reach that member's own structures.
    fn family_params<'a>(&'a self, s: &'a Struct) -> Vec<(&'a str, String, &'a str)> {
        if s.generation_slot.as_deref() != Some(SLOT_INFO) {
            return Vec::new();
        }
        s.fields
            .iter()
            .filter(|f| f.kind == "struct_ptr")
            .filter_map(|f| {
                let target = f.type_.as_deref()?;
                let slot = self.schema.slot_of(target)?;
                [SLOT_METADATA, SLOT_RUNTIME]
                    .contains(&slot)
                    .then(|| (f.name.as_str(), rust_type_name(slot), target))
            })
            .collect()
    }

    fn push_struct(&self, out: &mut String, s: &Struct) {
        let params = self.family_params(s);
        let offsets = field_offsets(s, self.schema);

        let members: Vec<Member> = s
            .fields
            .iter()
            .zip(offsets)
            .map(|(f, offset)| {
                let generic = params
                    .iter()
                    .find(|(name, _, _)| *name == f.name)
                    .map(|(_, p, _)| p.as_str());
                Member {
                    name: f.c_member(),
                    ty: self.field_type(f, generic),
                    offset,
                    doc: f.comment.clone(),
                }
            })
            .collect();

        let host = (s.generate != Generate::Skip).then(|| rust_type_name(&s.name));
        push_type_doc(out, s.comment.as_deref(), &s.name, host.as_deref());

        let (generics, args) = if params.is_empty() {
            (String::new(), String::new())
        } else {
            let names: Vec<&str> = params.iter().map(|(_, p, _)| p.as_str()).collect();
            let fields: Vec<&str> = params.iter().map(|(f, _, _)| *f).collect();
            out.push_str(&format!(
                "/// `{}` are the firmware's own {} structures.\n",
                names.join("` and `"),
                fields.join(" and "),
            ));
            let list: Vec<String> = params
                .iter()
                .map(|(_, p, d)| format!("{p} = {d}"))
                .collect();
            (
                format!("<{}>", list.join(", ")),
                format!("<{}>", names.join(", ")),
            )
        };
        push_type(out, &format!("{}{generics}", s.name), &members);

        if let Some(magic) = self.magic(s) {
            let head = match args.is_empty() {
                true => format!("impl {}", s.name),
                false => format!("impl{args} {}{args}", s.name),
            };
            out.push_str(&format!("{head} {{\n"));
            out.push_str("    /// The magic a writer writes.\n");
            out.push_str(&format!("    pub const MAGIC: {magic};\n"));
            out.push_str("}\n\n");
        }

        let size = struct_stride(&s.name, self.schema);
        push_asserts(out, &s.name, &s.name, size, &members);
    }

    /// The type and value of `MAGIC`, from the structure's `expected_const`
    /// field: the first constant named, zero-filled to the field's length.
    fn magic(&self, s: &Struct) -> Option<String> {
        let mut fields = s.fields.iter().filter(|f| f.expected_const.is_some());
        let field = fields.next()?;
        assert!(
            fields.next().is_none(),
            "device_gen: {} has more than one expected_const field",
            s.name
        );
        let name = field.expected_const.as_ref()?.names()[0];
        let Some(ConstantValue::Text(value)) = self
            .schema
            .constants
            .iter()
            .find(|c| c.name == name)
            .map(|c| &c.value)
        else {
            panic!(
                "device_gen: {}.{} expects {name}, which is not text",
                s.name, field.name
            );
        };
        let len = field.count.unwrap_or(0) as usize;
        assert!(
            value.len() <= len,
            "device_gen: {name} does not fit {}.{}",
            s.name,
            field.name
        );
        let mut bytes = value.as_bytes().to_vec();
        bytes.resize(len, 0);
        let literal: String = bytes
            .iter()
            .flat_map(|b| core::ascii::escape_default(*b))
            .map(char::from)
            .collect();
        Some(format!("[u8; {len}] = *b\"{literal}\""))
    }

    // -----------------------------------------------------------------------
    // Tagged FAMs
    // -----------------------------------------------------------------------

    fn push_tagged_fams(&self, out: &mut String) {
        if self.schema.tagged_fams.is_empty() {
            return;
        }
        push_section(out, "Tagged FAMs");
        for fam in &self.schema.tagged_fams {
            self.push_tagged_fam(out, fam);
        }
    }

    fn push_tagged_fam(&self, out: &mut String, fam: &TaggedFam) {
        let disc_size = self.schema.enum_size(&fam.discriminant_type).unwrap_or(1);
        let mut members = vec![
            Member {
                name: fam.discriminant_field.clone(),
                ty: fam.discriminant_type.clone(),
                offset: 0,
                doc: None,
            },
            Member {
                name: fam.param_len_field.clone(),
                ty: fam.param_len_type().into(),
                offset: disc_size,
                doc: None,
            },
        ];
        let mut offset = disc_size + fam.param_len_size();
        members.extend(self.following(&fam.common_fields, &mut offset));
        members.push(Member {
            name: "params".into(),
            ty: "[u8; 0]".into(),
            offset,
            doc: None,
        });

        let host = (fam.generate != Generate::Skip).then(|| rust_type_name(&fam.name));
        push_type_doc(out, fam.comment.as_deref(), &fam.name, host.as_deref());
        push_type(out, &fam.name, &members);
        push_asserts(out, &fam.name, &fam.name, fam.base_size as usize, &members);

        let strip = self
            .schema
            .enums
            .iter()
            .find(|e| e.name == fam.discriminant_type)
            .and_then(|e| e.strip_prefix.as_deref())
            .unwrap_or("");

        for variant in &fam.variants {
            // No C struct holds a variant whose only field is a string, so
            // there is nothing here to mirror.
            if variant.fixed_fields().next().is_none() && variant.string_field().is_some() {
                continue;
            }
            let c_name = derive_param_struct_name(
                &fam.name,
                &variant.discriminant,
                &fam.discriminant_type,
                self.schema,
            );
            let mut offset = 0;
            let members = self.following(&variant.fields, &mut offset);
            let size = self
                .schema
                .constants
                .iter()
                .find(|c| Some(c.name.as_str()) == variant.params_len_constant.as_deref())
                .and_then(|c| match c.value {
                    ConstantValue::Integer(n) => Some(n as usize),
                    ConstantValue::Text(_) => None,
                })
                .unwrap_or(offset);

            let host = host
                .as_ref()
                .map(|h| format!("{h}::{}", variant_ident(&variant.discriminant, strip)));
            push_type_doc(out, variant.comment.as_deref(), &c_name, host.as_deref());
            push_type(out, &c_name, &members);
            push_asserts(out, &c_name, &c_name, size, &members);
        }
    }

    /// `fields` laid out from `offset`, which is left past the last of them.
    fn following(&self, fields: &[Field], offset: &mut usize) -> Vec<Member> {
        fields
            .iter()
            .map(|f| {
                let here = *offset;
                *offset += field_size(f, self.schema);
                Member {
                    name: f.c_member(),
                    ty: self.field_type(f, None),
                    offset: here,
                    doc: f.comment.clone(),
                }
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Simple FAMs
    // -----------------------------------------------------------------------

    fn push_simple_fams(&self, out: &mut String) {
        if self.schema.simple_fams.is_empty() {
            return;
        }
        push_section(out, "Simple FAMs");
        for fam in &self.schema.simple_fams {
            let members = [
                Member {
                    name: fam.param_len_field.clone(),
                    ty: "u8".into(),
                    offset: 0,
                    doc: None,
                },
                Member {
                    name: "params".into(),
                    ty: "[u8; 0]".into(),
                    offset: 1,
                    doc: None,
                },
            ];
            let host = (fam.generate != Generate::Skip).then(|| rust_type_name(&fam.name));
            push_type_doc(out, fam.comment.as_deref(), &fam.name, host.as_deref());
            push_type(out, &fam.name, &members);
            push_asserts(out, &fam.name, &fam.name, 1, &members);
        }
    }

    // -----------------------------------------------------------------------
    // Family anchor
    // -----------------------------------------------------------------------

    /// The family anchor with this schema's metadata and runtime structures
    /// as its type parameters, under the name `header_name` gives.
    fn push_anchor(&self, out: &mut String) {
        let Some(name) = &self.schema.schema.header_name else {
            return;
        };
        // Schema::parse has refused a schema with a header_name missing
        // either structure.
        let metadata = &self.schema.struct_in_slot(SLOT_METADATA).unwrap().name;
        let runtime = &self.schema.struct_in_slot(SLOT_RUNTIME).unwrap().name;
        let anchor = format!("{FAMILY_CRATE}::{FAMILY_ANCHOR}");

        push_section(out, "Family anchor");
        out.push_str(&format!(
            "/// [`{FAMILY_ANCHOR}`]({anchor}) with its `metadata` and `runtime` pointers at\n\
             /// [`{metadata}`] and [`{runtime}`].\n\
             ///\n\
             /// Host-side type: [`{host}`]({FAMILY_CRATE}::{host}).\n",
            host = rust_type_name(FAMILY_ANCHOR),
        ));
        out.push_str("#[allow(non_camel_case_types)]\n");
        out.push_str(&format!(
            "pub type {name} = ::{anchor}<{metadata}, {runtime}>;\n"
        ));
    }

    // -----------------------------------------------------------------------
    // Field types
    // -----------------------------------------------------------------------

    /// The Rust type a field is declared with. `generic` replaces a struct
    /// pointer's target with a type parameter.
    fn field_type(&self, field: &Field, generic: Option<&str>) -> String {
        let nullable = field.nullable.unwrap_or(false);
        let named = |t: Option<&str>| t.unwrap_or("").to_string();

        match field.kind.as_str() {
            "scalar" => prim(field.type_.as_deref()).into(),
            "enum" => named(field.type_.as_deref()),
            "type_alias" => named(field.type_.as_deref()),
            "inline_array" => format!(
                "[{}; {}]",
                prim(field.element.as_deref()),
                field.count.unwrap_or(0)
            ),
            "inline_array2d" => format!(
                "[[{}; {}]; {}]",
                prim(field.element.as_deref()),
                field.cols.unwrap_or(0),
                field.rows.unwrap_or(0)
            ),
            "cstr_ptr" => self.ptr("core::ffi::c_char".into(), nullable),
            "struct_ptr" | "tagged_fam_ptr" | "simple_fam_ptr" => {
                let target = field.type_.as_deref().unwrap_or("");
                let ty = generic.map_or_else(|| target.to_string(), str::to_string);
                self.ptr(self.target(target, ty), nullable)
            }
            "struct_array_ptr" => {
                let element = field.element.as_deref().unwrap_or("");
                self.ptr(self.target(element, element.into()), nullable)
            }
            "struct_ptr_array_ptr" => {
                let element = field.element.as_deref().unwrap_or("");
                let inner = self.ptr(self.target(element, element.into()), false);
                self.ptr(inner, nullable)
            }
            "opaque_ptr" => {
                let target = match field.pointed_type.as_deref() {
                    None | Some("void") => "core::ffi::c_void",
                    other => prim(other),
                };
                self.ptr(target.into(), nullable)
            }
            "fn_ptr" => match nullable {
                true => "Option<unsafe extern \"C\" fn()>".into(),
                false => "unsafe extern \"C\" fn()".into(),
            },
            "padding" => format!("[u8; {}]", field.size.unwrap_or(0)),
            // A string runs to the end of its entry, like C's flexible array.
            "string" => "[u8; 0]".into(),
            other => panic!("device_gen: no Rust type for field kind {other}"),
        }
    }

    fn ptr(&self, target: String, nullable: bool) -> String {
        let support = &self.support;
        match nullable {
            true => format!("Option<{support}::Ptr<{target}>>"),
            false => format!("{support}::Ptr<{target}>"),
        }
    }

    /// The type a pointer to structure `name` points at. That is `ty` itself,
    /// or `RuntimeCell<ty>` where the firmware writes the structure while it
    /// runs, since the cell is what places it in RAM.
    fn target(&self, name: &str, ty: String) -> String {
        let runtime = self
            .schema
            .structs
            .iter()
            .any(|s| s.name == name && !s.has_const_fields());
        match runtime {
            true => format!("{}::RuntimeCell<{ty}>", self.support),
            false => ty,
        }
    }
}

// ---------------------------------------------------------------------------
// Enum constants
// ---------------------------------------------------------------------------

fn push_constant(out: &mut String, doc: Option<&str>, name: &str, value: i64, size: u32) {
    push_field_doc(out, "    ", doc);
    out.push_str(&format!(
        "    pub const {name}: Self = Self({});\n",
        format_value(value, size)
    ));
}

/// The chip types, as the C header's `rbcp_chip_types` enum holds them.
fn push_rbcp_constants(out: &mut String, size: u32) {
    let (canonical, aliases) = rbcp_chip_types();
    for ct in &canonical {
        let doc = format!("{} ({} bytes)", ct.name(), ct.size_bytes());
        push_constant(
            out,
            Some(&doc),
            ct.c_enum_name(),
            i64::from(ct.rbcp_chip_type()),
            size,
        );
    }
    push_constant(
        out,
        Some("Count of defined chip types. Not a valid chip type."),
        "NUM_CHIP_TYPES",
        i64::from(num_chip_types(&canonical)),
        size,
    );
    push_constant(
        out,
        Some("Invalid or unset chip type"),
        "INVALID_CHIP_TYPE",
        0xFF,
        size,
    );
    for alias in &aliases {
        let primary = onerom_config::chip::ChipType::try_from_rbcp_u8(alias.rbcp_chip_type())
            .expect("alias must resolve to a canonical chip");
        out.push_str(&format!(
            "    /// {} is electrically equivalent to {}\n",
            alias.name(),
            primary.name()
        ));
        out.push_str(&format!(
            "    pub const {}: Self = Self::{};\n",
            alias.c_enum_name(),
            primary.c_enum_name()
        ));
    }
}

/// `value` at the enum's storage width: decimal below 10, hex otherwise, as
/// the C header writes it.
fn format_value(value: i64, size: u32) -> String {
    if (0..10).contains(&value) {
        return value.to_string();
    }
    match size {
        1 => format!("0x{:02X}", value as u8),
        2 => format!("0x{:04X}", value as u16),
        _ => format!("0x{:08X}", value as u32),
    }
}

// ---------------------------------------------------------------------------
// Emission helpers
// ---------------------------------------------------------------------------

fn push_section(out: &mut String, title: &str) {
    out.push_str(&format!(
        "// ---------------------------------------------------------------------------\n\
         // {title}\n\
         // ---------------------------------------------------------------------------\n\n"
    ));
}

/// A type's doc: the first line of its schema comment, then a line naming the
/// C type and the host-side type. The rest of a schema comment may be true of
/// the host side alone.
fn push_type_doc(out: &mut String, comment: Option<&str>, c_name: &str, host: Option<&str>) {
    if let Some(first) = comment.and_then(|c| c.lines().next()) {
        out.push_str(&format!("/// {}\n///\n", first.trim()));
    }
    match host {
        Some(host) => out.push_str(&format!(
            "/// The C `{c_name}`. Host-side type: [`{host}`].\n"
        )),
        None => out.push_str(&format!("/// The C `{c_name}`.\n")),
    }
}

/// The first line of a field's schema comment.
fn push_field_doc(out: &mut String, indent: &str, comment: Option<&str>) {
    if let Some(first) = comment.and_then(|c| c.lines().next()) {
        out.push_str(&format!("{indent}/// {}\n", first.trim()));
    }
}

fn push_type(out: &mut String, head: &str, members: &[Member]) {
    out.push_str("#[allow(non_camel_case_types)]\n");
    out.push_str("#[repr(C)]\n");
    out.push_str(&format!("pub struct {head} {{\n"));
    for m in members {
        push_field_doc(out, "    ", m.doc.as_deref());
        out.push_str(&format!("    pub {}: {},\n", m.name, m.ty));
    }
    out.push_str("}\n\n");
}

/// The size and every field offset, as the C header lays them out.
fn push_asserts(out: &mut String, tn: &str, c_name: &str, size: usize, members: &[Member]) {
    out.push_str("#[cfg(target_pointer_width = \"32\")]\n");
    out.push_str("const _: () = {\n");
    let unit = if size == 1 { "byte" } else { "bytes" };
    out.push_str(&format!(
        "    assert!(core::mem::size_of::<{tn}>() == {size}, \"{c_name} must be {size} {unit}\");\n"
    ));
    for m in members {
        out.push_str(&format!(
            "    assert!(core::mem::offset_of!({tn}, {name}) == {off}, \"{c_name}.{name} must be at offset {off}\");\n",
            name = m.name,
            off = m.offset,
        ));
    }
    out.push_str("};\n\n");
}

/// The Rust primitive for a schema primitive.
fn prim(type_: Option<&str>) -> &'static str {
    match type_.unwrap_or("u8") {
        "u16" => "u16",
        "u32" => "u32",
        _ => "u8",
    }
}

/// The unsigned integer an enum of `size` bytes is stored as.
fn storage_for(size: usize) -> &'static str {
    match size {
        2 => "u16",
        4 => "u32",
        _ => "u8",
    }
}
