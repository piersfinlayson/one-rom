# onerom-fw-parser

Crate to handle parsing an SDRR firmware image.

This can can be a `.bin` or `.elf` file - or could be the contents of an STM32's flash memory.  It supports `no_std` environments, for adding to embedded applications, or `std` for use in PC-based tools.

A PC-based reference implementation is provided in the [`sdrr-info`](https://piers.rocks/u/sdrr-info) tool, which is used to parse and output the contents of a firmware image when you run `make info` or `make info-detail`.

A reference embedded implementation using this crate is [Airfrog](https://piers.rocks/u/airfrog), which is a tiny $3 probe for ARM devices, that can be used to inspect the firmware and runtime state of the SDRR device, and change its configuration and ROM data - **while it is serving ROMs**.
