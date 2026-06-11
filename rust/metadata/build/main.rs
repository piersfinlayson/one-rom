// build/main.rs
//
// Build script entry point for the onerom metadata crate.
//
// Loads firmware/metadata_schema.toml from the project root and runs
// all code generators.  Currently generates:
//   - A single C header  (firmware/generated/onerom_metadata.h by default)
//
// The C header output path can be overridden by setting the environment
// variable ONEROM_C_HEADER_OUT to an absolute path before building.
//
// Rust source generation (parse + serialize) is added in subsequent steps.

mod c_gen;
mod rust_gen;
mod schema;
mod serialize_gen;

use std::env;
use std::path::PathBuf;

const ENV_C_HEADER_OUT: &str = "ONEROM_C_HEADER_OUT";
const METADATA_SCHEMA_FILE: &str = "firmware/metadata_schema.toml";
const C_HEADER_FILE: &str = "firmware/generated/onerom_metadata.h";
const RUST_GENERATED: &str = "metadata_generated.rs";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -------------------------------------------------------------------------
    // Locate the project root and key paths
    // -------------------------------------------------------------------------

    // CARGO_MANIFEST_DIR points to rust/metadata/.
    // The project root is two levels above that (rust/metadata -> rust -> root).
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let project_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be two levels below the project root")
        .to_path_buf();

    let schema_path = project_root.join(METADATA_SCHEMA_FILE);

    // C header output path.  Configurable so CI or the C build system can
    // redirect it without touching the build script.
    let c_header_path = env::var(ENV_C_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| project_root.join(C_HEADER_FILE));

    // -------------------------------------------------------------------------
    // Cargo rerun-if-changed directives
    // -------------------------------------------------------------------------

    let build_dir = manifest_dir.join("build");
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("c_gen.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("main.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("rust_gen.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("schema.rs").display()
    );
    println!(
       "cargo:rerun-if-changed={}",
       build_dir.join("serialize_gen.rs").display()
   );
    let schema_path = build_dir.join(format!("../../../{METADATA_SCHEMA_FILE}")).canonicalize()?;
    println!(
        "cargo:rerun-if-changed={}",
        schema_path.display()
    );

    // -------------------------------------------------------------------------
    // Load and validate the schema
    // -------------------------------------------------------------------------

    let schema = schema::Schema::load(&schema_path).map_err(|e| {
        format!(
            "Failed to load schema from {}: {}",
            schema_path.display(),
            e
        )
    })?;

    // -------------------------------------------------------------------------
    // C header generation
    // -------------------------------------------------------------------------

    let c_header = c_gen::generate(&schema);

    if let Some(parent) = c_header_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&c_header_path, &c_header).map_err(|e| {
        format!(
            "Failed to write C header to {}: {}",
            c_header_path.display(),
            e
        )
    })?;

    eprintln!(
        "onerom build: wrote C header    -> {}",
        c_header_path.display()
    );

    // -------------------------------------------------------------------------
    // Rust source generation
    // -------------------------------------------------------------------------

    let rust_src = rust_gen::generate(&schema);

    let out_dir = env::var("OUT_DIR")?;
    let rust_out_path = PathBuf::from(&out_dir).join(RUST_GENERATED);
    std::fs::write(&rust_out_path, &rust_src).map_err(|e| {
        format!(
            "Failed to write generated Rust source to {}: {}",
            rust_out_path.display(),
            e
        )
    })?;
    eprintln!(
        "onerom build: wrote Rust source -> {}",
        rust_out_path.display()
    );
    println!("cargo:rerun-if-changed={}", rust_out_path.display());

    // -------------------------------------------------------------------------
    // Serialize source generation
    // -------------------------------------------------------------------------
 
    let serialize_src = serialize_gen::generate(&schema);
 
    let serialize_out_path = PathBuf::from(&out_dir).join("serialize_generated.rs");
    std::fs::write(&serialize_out_path, &serialize_src).map_err(|e| {
        format!(
            "Failed to write generated serialize source to {}: {}",
            serialize_out_path.display(),
            e
        )
    })?;
    eprintln!(
        "onerom build: wrote serialize source -> {}",
        serialize_out_path.display()
    );
    println!("cargo:rerun-if-changed={}", serialize_out_path.display());
 
    Ok(())
}
