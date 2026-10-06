# onerom_config::hw

`generated.rs` and `mod.rs` in this directory are auto-generated as part of the crate build process, and not included in source control.

They are generated in this directory structure, instead of the standard OUT_DIR, to allow IDEs to see them, and to avoid the need to include_bytes within this directory structure, which doesn't work, as outer doc comments and attributes cannot be included from the auto-generated code that way.
