# onerom-lab-parser

Reads One ROM Lab's own structures from a device or an image.

A host reads `onerom_info_t` and branches on `firmware_type`. For a Lab this
crate follows the `metadata` and `runtime` pointers with the parsers
[onerom-lab-metadata](/rust/lab-metadata) generates. The header itself is One
ROM's structure and [onerom-fw-parser](/rust/fw-parser) reads it.

It is `no_std` with `alloc` and reads through the same `Reader` trait as
onerom-fw-parser.
