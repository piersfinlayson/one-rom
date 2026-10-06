// src/linker_constants_gen.rs
//
// Generates the plugin-facing linker-script fragment
// (firmware/ora/onerom_linker_constants_generated.ld) from the OneROM metadata
// schema.
//
// A plugin builds against firmware/ora alone, so its linker script cannot
// include the firmware's fragment in firmware/generated.  A constant with both
// `ora_api` and `linker_script` set is written here as well, under its `ORA_`
// name, as the constants header does for C.

use crate::linker_gen::push_constant;
use crate::schema::Schema;

/// Generate the plugin-facing linker-script fragment.
pub fn generate(schema: &Schema) -> String {
    let mut out = format!(
        "/* OneROM Plugin Linker Constants
 *
 * Values a plugin's linker script must agree with the firmware on, taken from
 * the same schema the firmware's own definitions come from.
 *
 * A constant's `@since firmware X.Y.Z` line names the release it first
 * reached this file in.  A plugin using it sets its min_fw_version to that
 * release or a later one.
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

    for constant in schema.ora_linker_constants() {
        push_constant(
            &mut out,
            &constant.ora_name(),
            constant.plugin_documentation(),
            &constant.value,
        );
    }

    out
}
