// build/main.rs
//
// Build script entry point for the onerom metadata crate.
//
// Loads metadata_schema.toml from the crate root and runs all code
// generators.  Currently generates:
//   - A single C header  (firmware/generated/onerom_metadata.h by default)
//
// The schema ships inside the crate so the generated code and the schema
// version are a single, self-contained unit; published tarballs therefore
// build without reaching outside CARGO_MANIFEST_DIR.
//
// The C header output path can be overridden by setting the environment
// variable ONEROM_C_HEADER_OUT to an absolute path before building.
//
// Rust source generation (parse + serialize) is added in subsequent steps.

mod c_gen;
mod constants_gen;
mod host_gen;
mod keys_gen;
mod layout;
mod released;
mod rust_gen;
mod schema;
mod serialize_gen;

use std::env;
use std::path::PathBuf;

const ENV_C_HEADER_OUT: &str = "ONEROM_C_HEADER_OUT";
const ENV_KEYS_HEADER_OUT: &str = "ONEROM_KEYS_HEADER_OUT";
const ENV_CONSTANTS_HEADER_OUT: &str = "ONEROM_CONSTANTS_HEADER_OUT";
const METADATA_SCHEMA_FILE: &str = "metadata_schema.toml";
const RELEASED_SCHEMA_FILE: &str = "metadata_schema_released.toml";
const GATING_FIXTURE_FILE: &str = "build/gating_fixture.toml";
const C_HEADER_FILE: &str = "firmware/generated/onerom_metadata.h";
const KEYS_HEADER_FILE: &str = "firmware/ora/onerom_metadata_keys_generated.h";
const CONSTANTS_HEADER_FILE: &str = "firmware/ora/onerom_constants_generated.h";
const RUST_GENERATED: &str = "metadata_generated.rs";
const RUST_SERIALIZE_GENERATED: &str = "serialize_generated.rs";
const RUST_HOST_GENERATED: &str = "host_generated.rs";
const FIXTURE_GENERATED: &str = "gating_fixture_generated.rs";
const FIXTURE_SERIALIZE_GENERATED: &str = "gating_fixture_serialize_generated.rs";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // -------------------------------------------------------------------------
    // Locate key paths
    // -------------------------------------------------------------------------

    // CARGO_MANIFEST_DIR points to the crate root.  The schema lives directly
    // inside the crate, so everything resolves relative to this with no upward
    // walking - which is what makes the published tarball self-contained.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);

    let schema_path = manifest_dir.join(METADATA_SCHEMA_FILE);

    // The schema as the last release shipped it.  It lives beside the working
    // one so it travels with the published crate, and
    // ci/update-released-schema.sh refreshes it by hand once per development
    // cycle - a copy regenerated on every build would absorb the change it is
    // here to catch.
    let released_path = manifest_dir.join(RELEASED_SCHEMA_FILE);

    // C header output path.  Configurable so CI or the C build system can
    // redirect it without touching the build script.  The fallback lands
    // inside the crate build tree; it is always produced, and for consumers
    // it simply appears under their target/ and is otherwise unused.
    let c_header_path = env::var(ENV_C_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(C_HEADER_FILE));

    // Plugin-facing key header.  Redirected to firmware/ora by the workspace
    // .cargo/config.toml (relative to rust/), the same mechanism as the C
    // header above; the in-crate fallback is otherwise unused.
    let keys_header_path = env::var(ENV_KEYS_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(KEYS_HEADER_FILE));

    // Plugin-facing constants header, redirected the same way.
    let constants_header_path = env::var(ENV_CONSTANTS_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(CONSTANTS_HEADER_FILE));

    // -------------------------------------------------------------------------
    // Cargo rerun-if-changed directives
    // -------------------------------------------------------------------------

    let build_dir = manifest_dir.join("build");
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rerun-if-changed={}", released_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join(GATING_FIXTURE_FILE).display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("layout.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("released.rs").display()
    );
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
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("host_gen.rs").display()
    );

    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("keys_gen.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        build_dir.join("constants_gen.rs").display()
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
    // Comparison against the last release
    // -------------------------------------------------------------------------
    //
    // A layout that moved without its hand-raised generation number moving
    // with it fails here, before any generator has emitted a byte.

    let released = released::Released::load(&released_path).map_err(|e| {
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
    // Plugin-facing key header generation
    // -------------------------------------------------------------------------

    let keys_header = keys_gen::generate(&schema);

    if let Some(parent) = keys_header_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&keys_header_path, &keys_header).map_err(|e| {
        format!(
            "Failed to write keys header to {}: {}",
            keys_header_path.display(),
            e
        )
    })?;

    // -------------------------------------------------------------------------
    // Plugin-facing constants header generation
    // -------------------------------------------------------------------------

    let constants_header = constants_gen::generate(&schema);

    if let Some(parent) = constants_header_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&constants_header_path, &constants_header).map_err(|e| {
        format!(
            "Failed to write constants header to {}: {}",
            constants_header_path.display(),
            e
        )
    })?;

    eprintln!(
        "onerom build: wrote keys header -> {}",
        keys_header_path.display()
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

    // -------------------------------------------------------------------------
    // Serialize source generation
    // -------------------------------------------------------------------------

    let serialize_src = serialize_gen::generate(&schema);

    let serialize_out_path = PathBuf::from(&out_dir).join(RUST_SERIALIZE_GENERATED);
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

    // -------------------------------------------------------------------------
    // Host source generation
    // -------------------------------------------------------------------------

    let host_src = host_gen::generate(&schema);

    let host_out_path = PathBuf::from(&out_dir).join(RUST_HOST_GENERATED);
    std::fs::write(&host_out_path, &host_src).map_err(|e| {
        format!(
            "Failed to write generated host source to {}: {}",
            host_out_path.display(),
            e
        )
    })?;
    eprintln!(
        "onerom build: wrote host source  -> {}",
        host_out_path.display()
    );

    // -------------------------------------------------------------------------
    // Generation-gating fixture
    // -------------------------------------------------------------------------
    //
    // No field of metadata_schema.toml carries a generation marker, so running
    // the generated writer against the generated parser with a gated field
    // between them needs a schema that has one.  tests/generation_roundtrip.rs
    // includes what comes out, and nothing else refers to it.
    //
    // The fixture is excluded from the published crate, so a consumer's build
    // does not generate code for a test it does not have.  Deleting it from
    // the repo is not silent: the test's include! then has no file and fails
    // to compile.

    let fixture_path = manifest_dir.join(GATING_FIXTURE_FILE);
    if fixture_path.exists() {
        let fixture = schema::Schema::load(&fixture_path).map_err(|e| {
            format!(
                "Failed to load the gating fixture schema from {}: {}",
                fixture_path.display(),
                e
            )
        })?;

        let fixture_out_path = PathBuf::from(&out_dir).join(FIXTURE_GENERATED);
        std::fs::write(&fixture_out_path, rust_gen::generate(&fixture))?;

        let fixture_serialize_out_path = PathBuf::from(&out_dir).join(FIXTURE_SERIALIZE_GENERATED);
        std::fs::write(
            &fixture_serialize_out_path,
            serialize_gen::generate(&fixture),
        )?;

        eprintln!(
            "onerom build: wrote gating fixture -> {}",
            fixture_out_path.display()
        );
    }

    Ok(())
}
