# onerom-metadata-gen

Generates the source a One ROM metadata crate is built from.

A metadata crate defines a TOML schema describing the structures used by
its firmware. This crate reads that schema and writes:

- the C header the firmware is built against
- the linker-script fragment the firmware's linker scripts include
- the two plugin-facing C headers under `firmware/ora`
- the Rust types, parser, serializer and host test support
- the device-side Rust types, one per C type, which a Rust firmware places in
  memory

A build script calls `generate()` and gives it every path:

- the schema, and the copy of it from the last release
- the firmware's C header
- the linker-script fragment
- the two plugin-facing C headers
- the directory for the generated Rust

Every path comes from the caller. A metadata crate keeps its schema inside
itself, and its published tarball builds without reaching outside.
[rust/metadata/build/main.rs](/rust/metadata/build/main.rs) is a working call.

Three parts know One ROM's own schema rather than schemas in general.
`schema.rs` names the three structures that carry a generation number.
`c_gen.rs` and `device_gen.rs` build the `source = "rbcp_chip_types"` enum from
`onerom_config::chip::CHIP_TYPES`. `device_gen.rs` names the family anchor,
`onerom_metadata`'s `onerom_info_t`, which a schema's `header_name` aliases.
