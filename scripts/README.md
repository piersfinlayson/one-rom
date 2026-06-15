# Scripts

Helper scripts for testing and debugging.

## build-empty-fw.sh

Creates a blank firmware image for the OneROM device, with no metadata and no ROM (or other chip) images or types.

Example:

```bash
scripts/build-empty-fw.sh -d -l sdrr/build/sdrr-rp2350.bin /tmp/
```

## run-single-test-emu.sh

Runs a single One ROM Emulator test.

For example:

```bash
scripts/run-single-test-emu.sh fire-24-a images/test/rand_8KB.rom 2364 --cs1 active_low
```