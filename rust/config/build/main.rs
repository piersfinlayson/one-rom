// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use std::env;
use std::path::Path;

mod chip;
mod fmt;
mod hw;

fn main() {
    // Get the manifest directory (where this crate's Cargo.toml is)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = Path::new(&manifest_dir);

    // Tell Cargo to rerun if anything in build/ changes
    println!("cargo:rerun-if-changed=build");

    // Or json
    println!("cargo:rerun-if-changed=json");

    // Build the auto-generated hw module
    hw::build(manifest_path);

    // Build the auto-generated chip module
    chip::build(manifest_path);
}
