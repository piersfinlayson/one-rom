# onerom-metadata

A crate to automatically generate code to handle One ROM firmware metadata structures and types including:
- the C header files for the core firmware
- the linker-script fragment the core firmware's linker scripts include
- Rust metadata parsing code
- Rust metadata generator code
- device-side Rust types with the C header's names and layout, for a Rust firmware

The metadata schema for this crate lives in [metadata_schema.toml](/rust/metadata/metadata_schema.toml). This crate's build script passes it to [onerom-metadata-gen](/rust/metadata-gen), which generates the C headers, the linker-script fragment and the Rust source.

[metadata_schema_released.toml](/rust/metadata/metadata_schema_released.toml) is the schema as at the last release. The build compares the current and old schema and fails if a structure's bytes moved but its generation number didn't. Run `ci/update-released-schema.sh` once per development cycle, in the same commit that raises `[schema] firmware_release`.
