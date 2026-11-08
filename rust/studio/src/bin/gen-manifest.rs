// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates application manifest for first time use.
//!
//! Generates a manifest based on default values.  Only really useful the
//! first time the manifest is generated - after that the existing manifest
//! should be edited as required.

use onerom_studio::app::manifest::Manifest;
use std::path::Path;

// Filename for sample "studio.json" manifest
const MANIFEST_PATH: &str = "manifest/sample-studio.json";

fn main() {
    // Generate default manifest
    let default_manifest = Manifest::default();

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST_PATH);

    // Write it
    let manifest_json = serde_json::to_string_pretty(&default_manifest).unwrap();
    std::fs::write(manifest_path, manifest_json).unwrap();
}
