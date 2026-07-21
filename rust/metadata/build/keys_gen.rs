// build/keys_gen.rs
//
// Generates the plugin-facing metadata key header
// (firmware/ora/onerom_metadata_keys_generated.h) from the OneROM metadata
// schema.
//
// Emits the `ora_metadata_key_t` enum: the fixed NONE sentinel, one value per
// schema field tagged with a `plugin_key` (ordered by id), and the INVALID
// sentinel.  This header is included by the plugin API (firmware/ora/api.h) and
// therefore reaches user plugins; it deliberately contains only the identifier
// space, never any access into the internal metadata structures.

use crate::schema::Schema;

const GUARD: &str = "ONEROM_METADATA_KEYS_H";
const ENUM_NAME: &str = "ora_metadata_key_t";
const PREFIX: &str = "ORA_METADATA_KEY_";

struct Variant {
    name: String,
    value: u32,
    comment: String,
}

/// Generate the plugin-facing key header.
pub fn generate(schema: &Schema) -> String {
    // NONE first, the schema keys (already sorted by id), INVALID last.
    let mut variants = vec![Variant {
        name: format!("{PREFIX}NONE"),
        value: 0x0000_0000,
        comment: "Reserved. Never a live key; a zero-initialised value is invalid.".to_string(),
    }];
    for entry in schema.plugin_keys() {
        variants.push(Variant {
            name: format!("{PREFIX}{}", entry.key.name),
            value: entry.key.id,
            comment: entry
                .comment
                .and_then(|c| c.lines().next())
                .unwrap_or("")
                .to_string(),
        });
    }
    variants.push(Variant {
        name: format!("{PREFIX}INVALID"),
        value: 0xFFFF_FFFF,
        comment: "Invalid metadata key".to_string(),
    });

    let width = variants.iter().map(|v| v.name.len()).max().unwrap_or(0);

    let mut out = String::with_capacity(2048);
    out.push_str(&format!(
        "\
// OneROM Metadata Keys
//
// Plugin-facing device metadata key space.  Used with the metadata getter API
// (ora_get_metadata_str_fn_t and siblings) to retrieve device-level metadata
// without exposing the internal metadata structures to plugins.
//
// Keys are a single, unified namespace shared across all typed accessors.  A
// key value is a stable, permanent identifier: once assigned it is never
// renumbered or reused.  New metadata is exposed by tagging a schema field
// with a `plugin_key`; retired keys keep returning ORA_RESULT_NOT_SUPPORTED.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: firmware/metadata_schema.toml

#ifndef {GUARD}
#define {GUARD}

#include <stdint.h>

typedef enum {{
"
    ));

    for v in &variants {
        let comment = if v.comment.is_empty() {
            String::new()
        } else {
            format!("  // {}", v.comment)
        };
        out.push_str(&format!(
            "    {:<width$} = 0x{:08X},{}\n",
            v.name,
            v.value,
            comment,
            width = width
        ));
    }

    out.push_str(&format!("}} {ENUM_NAME};\n"));
    out.push_str(&format!(
        "STATIC_ASSERT(sizeof({ENUM_NAME}) == 4, \"{ENUM_NAME} must be 4 bytes\");\n\n"
    ));
    out.push_str(&format!("#endif // {GUARD}\n"));
    out
}
