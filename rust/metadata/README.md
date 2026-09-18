# onerom-metadata

A crate to automatically generate code to handle One ROM firmware metadata structures and types including:
- the C header files for the core firmware
- Rust metadata parsing code
- Rust metadata generator code

The metadata schema for this crate lives in [metadata_schema.toml](/rust/metadata/metadata_schema.toml).

Beside it, [metadata_schema_released.toml](/rust/metadata/metadata_schema_released.toml) is that file as the last release shipped it. The build lays both out and compares them, so a structure whose bytes moved without its generation number rising fails the build. Refresh the copy once per development cycle with `ci/update-released-schema.sh`, in the same commit as the bump to `[schema] firmware_release`.
