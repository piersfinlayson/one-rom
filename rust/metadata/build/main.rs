// build/main.rs
//
// Build script entry point for the onerom metadata crate.
//
// It locates this crate's schema files and output paths and hands them to
// onerom-metadata-gen, which does the generating.  The generator is a crate of
// its own because a second metadata crate over a different schema needs the
// same code.
//
// The schema ships inside the crate so the generated code and the schema
// version are a single, self-contained unit; published tarballs therefore
// build without reaching outside CARGO_MANIFEST_DIR.
//
// The three C header output paths can each be overridden by setting the
// matching environment variable to an absolute path before building.

use std::env;
use std::path::PathBuf;

use onerom_metadata_gen::{Outputs, device_gen, generate, rust_gen, schema, serialize_gen};

const ENV_C_HEADER_OUT: &str = "ONEROM_C_HEADER_OUT";
const ENV_KEYS_HEADER_OUT: &str = "ONEROM_KEYS_HEADER_OUT";
const ENV_CONSTANTS_HEADER_OUT: &str = "ONEROM_CONSTANTS_HEADER_OUT";
const ENV_LINKER_SCRIPT_OUT: &str = "ONEROM_LINKER_SCRIPT_OUT";
const METADATA_SCHEMA_FILE: &str = "metadata_schema.toml";
// The schema and fixture as generated files name them, from the repo root.
const METADATA_SCHEMA_SOURCE: &str = "rust/metadata/metadata_schema.toml";
const GATING_FIXTURE_SOURCE: &str = "rust/metadata/build/gating_fixture.toml";
const RELEASED_SCHEMA_FILE: &str = "metadata_schema_released.toml";
const GATING_FIXTURE_FILE: &str = "build/gating_fixture.toml";
const C_HEADER_FILE: &str = "firmware/generated/onerom_metadata.h";
const KEYS_HEADER_FILE: &str = "firmware/ora/onerom_metadata_keys_generated.h";
const CONSTANTS_HEADER_FILE: &str = "firmware/ora/onerom_constants_generated.h";
const LINKER_SCRIPT_FILE: &str = "firmware/generated/onerom_metadata.ld";
const FIXTURE_GENERATED: &str = "gating_fixture_generated.rs";
const FIXTURE_SERIALIZE_GENERATED: &str = "gating_fixture_serialize_generated.rs";
const FIXTURE_DEVICE_GENERATED: &str = "gating_fixture_device_generated.rs";

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

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // C header output path.  Configurable so CI or the C build system can
    // redirect it without touching the build script.  The fallback lands
    // inside the crate build tree; it is always produced, and for consumers
    // it simply appears under their target/ and is otherwise unused.
    let c_header = env::var(ENV_C_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(C_HEADER_FILE));

    // Plugin-facing key header.  Redirected to firmware/ora by the workspace
    // .cargo/config.toml (relative to rust/), the same mechanism as the C
    // header above; the in-crate fallback is otherwise unused.
    let keys_header = env::var(ENV_KEYS_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(KEYS_HEADER_FILE));

    // Plugin-facing constants header, redirected the same way.
    let constants_header = env::var(ENV_CONSTANTS_HEADER_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(CONSTANTS_HEADER_FILE));

    // Linker-script fragment, redirected to firmware/generated the same way as
    // the C header.
    let linker_script = env::var(ENV_LINKER_SCRIPT_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join(LINKER_SCRIPT_FILE));

    // -------------------------------------------------------------------------
    // Cargo rerun-if-changed directives
    // -------------------------------------------------------------------------
    //
    // The generator's own sources need none: it is a build dependency, so a
    // change there rebuilds this script and cargo runs it again.

    let fixture_path = manifest_dir.join(GATING_FIXTURE_FILE);
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rerun-if-changed={}", released_path.display());
    println!("cargo:rerun-if-changed={}", fixture_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("build/main.rs").display()
    );

    // -------------------------------------------------------------------------
    // Generation
    // -------------------------------------------------------------------------

    generate(
        &schema_path,
        &released_path,
        &Outputs {
            source: METADATA_SCHEMA_SOURCE.into(),
            c_header,
            keys_header,
            constants_header,
            linker_script,
            out_dir: out_dir.clone(),
        },
    )?;

    // -------------------------------------------------------------------------
    // Generation-gating fixture
    // -------------------------------------------------------------------------
    //
    // No field of metadata_schema.toml carries a generation marker, so running
    // the generated writer against the generated parser with a gated field
    // between them needs a schema that has one.  tests/generation_roundtrip.rs
    // includes what comes out.  tests/fixture_device.rs includes the device
    // types, the only place gated fields reach the device generator.
    //
    // The fixture is excluded from the published crate, so a consumer's build
    // does not generate code for a test it does not have.  Deleting it from
    // the repo is not silent: the test's include! then has no file and fails
    // to compile.

    if fixture_path.exists() {
        let mut fixture = schema::Schema::load(&fixture_path).map_err(|e| {
            format!(
                "Failed to load the gating fixture schema from {}: {}",
                fixture_path.display(),
                e
            )
        })?;
        fixture.source = GATING_FIXTURE_SOURCE.into();

        let fixture_out_path = out_dir.join(FIXTURE_GENERATED);
        std::fs::write(&fixture_out_path, rust_gen::generate(&fixture))?;

        let fixture_serialize_out_path = out_dir.join(FIXTURE_SERIALIZE_GENERATED);
        std::fs::write(
            &fixture_serialize_out_path,
            serialize_gen::generate(&fixture),
        )?;

        std::fs::write(
            out_dir.join(FIXTURE_DEVICE_GENERATED),
            device_gen::generate(&fixture),
        )?;

        eprintln!(
            "onerom build: wrote gating fixture -> {}",
            fixture_out_path.display()
        );
    }

    Ok(())
}
