# onerom-lab-metadata

One ROM Lab's metadata structures, generated from
[metadata_schema.toml](metadata_schema.toml) by
[onerom-metadata-gen](/rust/metadata-gen).

Lab fills `onerom_info_t` at the flash anchor, the same 64 bytes One ROM does,
and [onerom-metadata](/rust/metadata) owns that structure. This crate describes
what Lab's own `metadata` and `runtime` pointers reach:

- the board a Lab image was built for
- the board it is running as

`firmware_type` in `onerom_info_t` is `FIRMWARE_TYPE_LAB`, which is what says
those pointers lead here. A host reads it before following either.

The same structures are also generated as `#[repr(C)]` types under their C
names, for Lab's firmware to place in memory. `onerom_lab_info_t` is
`onerom_metadata`'s `onerom_info_t` with Lab's header and runtime structure as
its two type parameters.

[metadata_schema_released.toml](metadata_schema_released.toml) is the schema as
the last release shipped it. Lab has not released with one, so today the file
says so. The file is still required, because a missing one cannot be told from
one that was lost.
