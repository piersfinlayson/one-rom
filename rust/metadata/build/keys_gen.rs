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

use crate::c_gen::format_const_value;
use crate::schema::Schema;

const GUARD: &str = "ONEROM_METADATA_KEYS_H";
const ENUM_NAME: &str = "ora_metadata_key_t";
const PREFIX: &str = "ORA_METADATA_KEY_";

/// Schema constant naming the value that marks an unused GPIO array entry.
/// Emitted into this header under `SENTINEL_NAME` so a plugin can compare
/// against it without including the firmware's own metadata header.
const SENTINEL_SOURCE: &str = "GPIO_NONE";
const SENTINEL_NAME: &str = "ORA_GPIO_NONE";

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
            // The whole comment, not just its first line: an array key needs
            // its sentinel convention stated here, where a plugin author reads
            // it, rather than only in the firmware's own metadata header.
            comment: entry.comment.unwrap_or("").to_string(),
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

"
    ));

    // The sentinel marking an unused entry in a GPIO array, taken from the same
    // schema constant the firmware's own header takes it from so the two cannot
    // drift apart.  Without it here a plugin has no value to compare against,
    // the firmware metadata header not being plugin-facing.
    let sentinel = schema
        .constants
        .iter()
        .find(|c| c.name == SENTINEL_SOURCE)
        .unwrap_or_else(|| {
            panic!(
                "schema must define the {SENTINEL_SOURCE} constant, exposed here as {SENTINEL_NAME}"
            )
        });
    out.push_str(&format!(
        "\
// Marks an entry of a GPIO array as unused.  The array keys below are filled
// contiguously from index 0, so a caller reads upwards and stops at the first
// entry equal to this rather than at the end of the array.
#define {SENTINEL_NAME} {}

typedef enum {{
",
        format_const_value(&sentinel.value, &sentinel.type_)
    ));

    for v in &variants {
        let lines: Vec<&str> = v
            .comment
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        // A one-line comment trails its enumerator, as it always has.  A longer
        // one goes above it, so the sentinel convention an array key carries
        // survives into this header instead of being truncated away.
        if lines.len() > 1 {
            for line in &lines {
                out.push_str(&format!("    // {}\n", line));
            }
            out.push_str(&format!(
                "    {:<width$} = 0x{:08X},\n",
                v.name,
                v.value,
                width = width
            ));
        } else {
            let comment = match lines.first() {
                Some(c) => format!("  // {}", c),
                None => String::new(),
            };
            out.push_str(&format!(
                "    {:<width$} = 0x{:08X},{}\n",
                v.name,
                v.value,
                comment,
                width = width
            ));
        }
    }

    out.push_str(&format!("}} {ENUM_NAME};\n"));
    out.push_str(&format!(
        "STATIC_ASSERT(sizeof({ENUM_NAME}) == 4, \"{ENUM_NAME} must be 4 bytes\");\n\n"
    ));
    out.push_str(&format!("#endif // {GUARD}\n"));
    out
}
