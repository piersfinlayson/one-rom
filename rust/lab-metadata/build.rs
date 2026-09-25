// build.rs
//
// Build script entry point for the One ROM Lab metadata crate.
//
// It locates this crate's schema files and output paths and hands them to
// onerom-metadata-gen, which does the generating.
//
// The schema ships inside the crate, so everything resolves relative to
// CARGO_MANIFEST_DIR and a published tarball builds without reaching outside
// itself.
//
// Lab is Rust, so the three C headers and the linker-script fragment the
// generator also writes go into OUT_DIR and nothing reads them.

use std::env;
use std::path::PathBuf;

use onerom_metadata_gen::{Outputs, generate};

const METADATA_SCHEMA_FILE: &str = "metadata_schema.toml";
// The schema as generated files name it, from the repo root.
const METADATA_SCHEMA_SOURCE: &str = "rust/lab-metadata/metadata_schema.toml";
const RELEASED_SCHEMA_FILE: &str = "metadata_schema_released.toml";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let schema_path = manifest_dir.join(METADATA_SCHEMA_FILE);

    // The schema as the last release shipped it.  Lab has not released with
    // one yet, so today the file says so rather than describing a layout.
    let released_path = manifest_dir.join(RELEASED_SCHEMA_FILE);

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rerun-if-changed={}", released_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("build.rs").display()
    );

    generate(
        &schema_path,
        &released_path,
        &Outputs {
            source: METADATA_SCHEMA_SOURCE.into(),
            c_header: out_dir.join("onerom_lab_metadata.h"),
            keys_header: out_dir.join("onerom_lab_metadata_keys.h"),
            constants_header: out_dir.join("onerom_lab_constants.h"),
            linker_script: out_dir.join("onerom_lab_metadata.ld"),
            out_dir,
        },
    )?;

    Ok(())
}
