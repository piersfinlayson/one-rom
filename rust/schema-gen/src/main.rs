// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates JSON schemas

use onerom_gen::Config as RomConfig;
use schemars::schema_for;
use serde::ser::Serialize;
use serde_json::ser::{PrettyFormatter, Serializer};
use std::fs::write;
use std::path::PathBuf;

const ROM_CONFIG_DIRNAME: &str = "onerom-config";
const ROM_CONFIG_FILENAME: &str = "schema.json";

fn main() {
    generate_rom_config_schema();
}

fn workspace_root_path() -> PathBuf {
    let mut root_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root_path.pop(); // Up to workspace
    root_path.pop(); // Up to repo root
    root_path
}

fn rom_config_filename() -> PathBuf {
    workspace_root_path()
        .join(ROM_CONFIG_DIRNAME)
        .join(ROM_CONFIG_FILENAME)
}

fn generate_rom_config_schema() {
    // Create a pretty-printed JSON serializer with 4-space indentation
    let formatter = PrettyFormatter::with_indent(b"    ");
    let mut buf = Vec::new();
    let mut ser = Serializer::with_formatter(&mut buf, formatter);

    // Generate the schema for RomConfig
    let schema = schema_for!(RomConfig);
    schema
        .serialize(&mut ser)
        .expect("Failed to serialize schema");
    let json = String::from_utf8(buf).expect("Generated schema is not valid UTF-8");

    // Save it to file
    let filename = rom_config_filename();
    println!("Writing ROM config schema to {}", filename.display());
    write(filename, json).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_metadata::{
        ADDR_MON_UNSET_MIN_CS_ACT_NS, ADDR_MON_UNSET_MIN_CS_ACT_NS_ALG_CS_2, TARGET_FREQ_MHZ,
    };

    #[test]
    fn rom_schema_is_current() {
        // Create a pretty-printed JSON serializer with 4-space indentation
        let formatter = PrettyFormatter::with_indent(b"    ");
        let mut buf = Vec::new();
        let mut ser = Serializer::with_formatter(&mut buf, formatter);

        let schema = schema_for!(RomConfig);
        schema
            .serialize(&mut ser)
            .expect("Failed to serialize schema");
        let generated = String::from_utf8(buf).expect("Generated schema is not valid UTF-8");

        let checked_in =
            std::fs::read_to_string(rom_config_filename()).expect("config-schema.json missing");

        assert_eq!(
            checked_in, generated,
            "Schema out of date - run: cargo run -p schema-gen"
        );
    }

    // The description quotes these figures, typed by hand in onerom-gen.
    #[test]
    fn min_cs_act_description_matches_the_constants() {
        let schema = serde_json::to_value(schema_for!(RomConfig)).unwrap();
        let description = schema["properties"]["addr_mon_min_cs_act_ns"]["description"]
            .as_str()
            .expect("addr_mon_min_cs_act_ns has a description");
        for figure in [
            format!(" {ADDR_MON_UNSET_MIN_CS_ACT_NS}ns "),
            format!(" {ADDR_MON_UNSET_MIN_CS_ACT_NS_ALG_CS_2}ns "),
            format!(" {TARGET_FREQ_MHZ}MHz"),
        ] {
            assert!(
                description.contains(&figure),
                "{figure:?} in {description:?}"
            );
        }
    }
}
