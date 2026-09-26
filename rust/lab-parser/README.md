# onerom-lab-parser

Reads a One ROM Lab from a device or an image.

A host reads `onerom_info_t` and branches on `firmware_type`. For a Lab this
crate follows the `metadata` and `runtime` pointers with the parsers
[onerom-lab-metadata](/rust/lab-metadata) generates. It also returns the Lab's
`onerom_info_t`.

It is `no_std` with `alloc` and reads through the same `Reader` trait as
onerom-fw-parser.
