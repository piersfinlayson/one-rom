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

Some of it is hard-coded for One ROM.
[src/lib.rs](/rust/metadata-gen/src/lib.rs) lists it.
