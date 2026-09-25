// src/linker_gen.rs
//
// Generates the linker-script fragment (firmware/generated/onerom_metadata.ld)
// from the OneROM metadata schema.
//
// A linker script cannot include a C header.  A constant with `linker_script`
// set is written here as well, under its schema name, so a linker script uses
// the same value as the C and Rust code.

use crate::schema::{ConstantValue, Schema};

/// Generate the linker-script fragment.
pub fn generate(schema: &Schema) -> String {
    let mut out = format!(
        "/* OneROM Linker Constants
 *
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 *
 * GENERATED FILE - DO NOT EDIT
 * Source: {}
 */

",
        schema.source
    );

    for constant in schema.linker_constants() {
        if let Some(doc) = constant.documentation() {
            for line in doc.lines() {
                out.push_str(&format!("/* {line} */\n"));
            }
        }
        // Schema validation refuses a linker_script constant holding text.
        if let ConstantValue::Integer(value) = constant.value {
            out.push_str(&format!("{} = {value:#X};\n\n", constant.name));
        }
    }

    out
}
