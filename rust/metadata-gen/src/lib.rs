// src/lib.rs
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The code generator behind One ROM's metadata crates.
//!
//! A metadata crate owns a TOML schema describing the structures its firmware
//! holds, and a copy of that schema as the last release shipped it.  This
//! crate turns the pair into source: a C header and a linker-script fragment
//! for the firmware, two plugin-facing C headers, the Rust types, parser,
//! serializer and host test support the crate itself is built from, and the
//! device-side Rust types a Rust firmware places in memory.
//!
//! # Calling it
//!
//! [`generate`] is the whole run, and a build script is all it takes:
//!
//! ```no_run
//! use std::env;
//! use std::path::PathBuf;
//!
//! let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
//! let outputs = onerom_metadata_gen::Outputs {
//!     source: "rust/metadata/metadata_schema.toml".into(),
//!     c_header: manifest_dir.join("firmware/generated/onerom_metadata.h"),
//!     keys_header: manifest_dir.join("firmware/ora/onerom_metadata_keys_generated.h"),
//!     constants_header: manifest_dir.join("firmware/ora/onerom_constants_generated.h"),
//!     linker_script: manifest_dir.join("firmware/generated/onerom_metadata.ld"),
//!     out_dir: PathBuf::from(env::var("OUT_DIR").unwrap()),
//! };
//! onerom_metadata_gen::generate(
//!     &manifest_dir.join("metadata_schema.toml"),
//!     &manifest_dir.join("metadata_schema_released.toml"),
//!     &outputs,
//! )
//! .unwrap();
//! ```
//!
//! Every path is the caller's.  Nothing here reads `CARGO_MANIFEST_DIR`,
//! `OUT_DIR` or any environment variable, so a calling crate keeps its schema
//! where it likes and stays self-contained in a published tarball.
//!
//! # The stages
//!
//! The modules below are the run broken open, for a caller that needs one
//! stage on its own - a test handing a generator a schema built in memory, or
//! a build script generating Rust for a second schema without the headers or
//! the comparison.  [`schema`] reads and validates the working schema,
//! [`released`] reads the copy of the last release, [`layout`] compares them,
//! and the eight `*_gen` modules each return the text of one output.
//!
//! # Hard-coded for One ROM
//!
//! - `rbcp_chip_types` values taken from `onerom_config::chip::CHIP_TYPES`:
//!   - in the C header with `onerom_chip_type_sizes` ([`c_gen`])
//!   - in the device types ([`device_gen`])
//! - The family anchor `onerom_metadata::onerom_info_t` ([`device_gen`]).
//! - One ROM's plugin API:
//!   - the key resolver macros in the C header ([`c_gen`])
//!   - the key header ([`keys_gen`])
//!   - the constants header ([`constants_gen`])
//!   - `ORA_` names for constants ([`schema`])
//!   - the `onerom_runtime_info_t` root for plugin keys ([`schema`])
//!   - the release check on plugin keys ([`layout`])
//! - In the C header ([`c_gen`]):
//!   - `#include "macros.h"`
//!   - `ONEROM_DEPRECATED`
//! - "OneROM" in three file banners:
//!   - the key header ([`keys_gen`])
//!   - the constants header ([`constants_gen`])
//!   - the linker-script fragment ([`linker_gen`])
//! - In the host test source (`host_gen`):
//!   - the root named `_metadata_start`
//!   - `#include "onerom_metadata.h"`
//!   - `rom_slots` on the root
//!   - ROM slot data in every opaque pointer
//! - onerom-metadata's names in generated docs ([`rust_gen`]):
//!   - `ALL_CONSTANTS`
//!   - `METADATA_GENERATIONS` where the schema has a metadata region
//! - In [`layout`]'s errors:
//!   - plugins
//!   - `ci/update-released-schema.sh`

mod host_gen;

// The stages are reachable so that a caller's build script and the tests in
// both crates can drive one at a time.  They are not the API.  That is
// `generate` and `Outputs`, and a stage is free to change shape.
#[doc(hidden)]
pub mod c_gen;
#[doc(hidden)]
pub mod constants_gen;
#[doc(hidden)]
pub mod device_gen;
#[doc(hidden)]
pub mod keys_gen;
#[doc(hidden)]
pub mod layout;
#[doc(hidden)]
pub mod linker_gen;
#[doc(hidden)]
pub mod released;
#[doc(hidden)]
pub mod rust_gen;
#[doc(hidden)]
pub mod schema;
#[doc(hidden)]
pub mod serialize_gen;

use std::path::{Path, PathBuf};

/// Names of the Rust sources [`generate`] writes into [`Outputs::out_dir`].
const RUST_GENERATED: &str = "metadata_generated.rs";
const RUST_SERIALIZE_GENERATED: &str = "serialize_generated.rs";
const RUST_HOST_GENERATED: &str = "host_generated.rs";
const RUST_DEVICE_GENERATED: &str = "device_generated.rs";

/// Where a run of the generator writes, and the schema file its output names.
///
/// Missing parent directories of the three headers and the linker-script
/// fragment are created.  `out_dir` must exist, which for a build script's
/// `OUT_DIR` it does.
pub struct Outputs {
    /// The schema file as every generated file's `Source:` line names it,
    /// such as `rust/metadata/metadata_schema.toml`.
    pub source: String,
    /// The firmware's C header, holding every type the schema describes.
    pub c_header: PathBuf,
    /// The plugin-facing metadata key header.
    pub keys_header: PathBuf,
    /// The plugin-facing constants header.
    pub constants_header: PathBuf,
    /// The linker-script fragment, holding every `linker_script` constant.
    pub linker_script: PathBuf,
    /// Directory for `metadata_generated.rs`, `serialize_generated.rs`,
    /// `host_generated.rs` and `device_generated.rs` - a build script's
    /// `OUT_DIR`.
    pub out_dir: PathBuf,
}

/// Generate everything `schema_path` describes.
///
/// `released_path` is that schema as the last release shipped it.  The two are
/// laid out and compared before any generator emits a byte, so a structure
/// whose bytes moved without its generation number rising fails here.
pub fn generate(
    schema_path: &Path,
    released_path: &Path,
    outputs: &Outputs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut schema = schema::Schema::load(schema_path).map_err(|e| {
        format!(
            "Failed to load schema from {}: {}",
            schema_path.display(),
            e
        )
    })?;
    schema.source = outputs.source.clone();

    let released = released::Released::load(released_path).map_err(|e| {
        format!(
            "Failed to load the last released schema from {}: {}",
            released_path.display(),
            e
        )
    })?;

    layout::compare(&schema, &released)?;

    // -------------------------------------------------------------------------
    // C header generation
    // -------------------------------------------------------------------------

    write_with_parent(&outputs.c_header, &c_gen::generate(&schema), "C header")?;

    eprintln!(
        "onerom build: wrote C header    -> {}",
        outputs.c_header.display()
    );

    // -------------------------------------------------------------------------
    // Plugin-facing key header generation
    // -------------------------------------------------------------------------

    write_with_parent(
        &outputs.keys_header,
        &keys_gen::generate(&schema),
        "keys header",
    )?;

    // -------------------------------------------------------------------------
    // Plugin-facing constants header generation
    // -------------------------------------------------------------------------

    write_with_parent(
        &outputs.constants_header,
        &constants_gen::generate(&schema),
        "constants header",
    )?;

    eprintln!(
        "onerom build: wrote keys header -> {}",
        outputs.keys_header.display()
    );

    // -------------------------------------------------------------------------
    // Linker-script fragment generation
    // -------------------------------------------------------------------------

    write_with_parent(
        &outputs.linker_script,
        &linker_gen::generate(&schema),
        "linker script",
    )?;

    // -------------------------------------------------------------------------
    // Rust source generation
    // -------------------------------------------------------------------------

    let rust_out_path = outputs.out_dir.join(RUST_GENERATED);
    write(
        &rust_out_path,
        &rust_gen::generate(&schema),
        "generated Rust source",
    )?;
    eprintln!(
        "onerom build: wrote Rust source -> {}",
        rust_out_path.display()
    );

    // -------------------------------------------------------------------------
    // Serialize source generation
    // -------------------------------------------------------------------------

    let serialize_out_path = outputs.out_dir.join(RUST_SERIALIZE_GENERATED);
    write(
        &serialize_out_path,
        &serialize_gen::generate(&schema),
        "generated serialize source",
    )?;
    eprintln!(
        "onerom build: wrote serialize source -> {}",
        serialize_out_path.display()
    );

    // -------------------------------------------------------------------------
    // Host source generation
    // -------------------------------------------------------------------------

    let host_out_path = outputs.out_dir.join(RUST_HOST_GENERATED);
    write(
        &host_out_path,
        &host_gen::generate(&schema),
        "generated host source",
    )?;
    eprintln!(
        "onerom build: wrote host source  -> {}",
        host_out_path.display()
    );

    // -------------------------------------------------------------------------
    // Device-side source generation
    // -------------------------------------------------------------------------

    let device_out_path = outputs.out_dir.join(RUST_DEVICE_GENERATED);
    write(
        &device_out_path,
        &device_gen::generate(&schema),
        "generated device source",
    )?;
    eprintln!(
        "onerom build: wrote device source -> {}",
        device_out_path.display()
    );

    Ok(())
}

/// Write `contents` to `path`, creating its parent directory.
///
/// `what` names the file in the error, so a build script failure says which
/// output could not be written rather than only where.
fn write_with_parent(
    path: &Path,
    contents: &str,
    what: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write(path, contents, what)
}

/// Write `contents` to `path`, whose directory already exists.
fn write(path: &Path, contents: &str, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(path, contents)
        .map_err(|e| format!("Failed to write {what} to {}: {e}", path.display()))?;
    Ok(())
}
