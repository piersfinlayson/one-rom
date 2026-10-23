# Lab Changelog

## v0.4.1 - unreleased

- `R` queries One ROM information using RBCP.
- `a` is a batch read that reads the ROM and queries One ROM information.
- `x` reads a chip continuously at MHz rates.
- `d` drives a chip's data lines as a square wave, with the chip deselected.
- `H` emulates a host's bus timing, including chip select glitches.
- 8-bit and 16-bit result lines now line up.
- `z` no longer mounts the RP2350's mass storage device when it reboots Lab into the bootloader.

## v0.4.0 - 2026-10-06

- Lab is now released as ready-built images:
  - One for each Fire hardware revision.
  - One without a hardware revision baked in.
- CLI 0.5.0 and later can manage Lab.
- Windows Device Manager no longer shows an exclamation mark against Lab.
- The batch read interval defaults to 1 second, down from 5.
- Fix: Lines printed before a blocking operation are now output to USB before that operation.
