// src/constants_gen.rs
//
// Generates the plugin-facing constants header
// (firmware/ora/onerom_constants_generated.h) from the OneROM metadata schema.
//
// A plugin builds against firmware/ora only, so it cannot include the
// firmware's own metadata header.  Where a plugin has to agree with the
// firmware on a value - the longest hold either will accept, say - writing the
// number out in both places means a change to one flags nothing in the other.
// Setting `ora_api` on the schema constant emits it here instead, so the
// firmware, the plugin and the Rust crate all take it from the schema.  The
// name is the schema's with an `ORA_` prefix, derived rather than given, so
// either name can be found from the other.
//
// This header carries values.  The identifier space plugins use with the
// metadata getters is a separate concern and lives in
// onerom_metadata_keys_generated.h.
//
// Each constant carries an `@since firmware X.Y.Z` line naming the release it
// reached the plugin API in, as api.h does for every identifier.

use crate::c_gen::format_const_value;
use crate::schema::Schema;

const GUARD: &str = "ONEROM_CONSTANTS_H";

/// Generate the plugin-facing constants header.
pub fn generate(schema: &Schema) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "// OneROM Constants\n\
         //\n\
         // Values a plugin must agree with the firmware on, taken from the same\n\
         // schema the firmware's own definitions come from.\n\
         //\n\
         // A constant's `@since firmware X.Y.Z` line names the release it first\n\
         // reached this header in.  A plugin using it asks for that release, or a\n\
         // later one, in its min_fw_version.\n\
         //\n\
         // Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>\n\
         //\n\
         // MIT License\n\
         //\n\
         // GENERATED FILE - DO NOT EDIT\n\
         // Source: {}\n\n",
        schema.source
    ));

    out.push_str(&format!("#ifndef {GUARD}\n#define {GUARD}\n\n"));
    out.push_str("#include <stdint.h>\n\n");

    for constant in schema.ora_constants() {
        if let Some(comment) = constant.plugin_documentation() {
            for line in comment.lines() {
                out.push_str(&format!("// {line}\n"));
            }
        }
        out.push_str(&format!(
            "#define {} {}\n\n",
            constant.ora_name(),
            format_const_value(&constant.value, &constant.type_)
        ));
    }

    out.push_str(&format!("#endif // {GUARD}\n"));
    out
}
