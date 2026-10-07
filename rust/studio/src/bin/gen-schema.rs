// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates application manifest JSON schema

use onerom_studio::app::manifest::Manifest;
use schemars::schema_for;
use std::path::Path;

const SCHEMA_PATH: &str = "manifest/app-schema.json";

fn main() {
    // Generate JSON schema for application manifest
    let schema = schema_for!(Manifest);
    let json = serde_json::to_string_pretty(&schema).unwrap();

    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH);

    // Write it
    std::fs::write(schema_path, json).unwrap();
}
