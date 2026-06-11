# One ROM Lens

This directory contains the source code for the One ROM Lens, a tool which runs the One ROM PIO algorithms in a browser and visualizes the results.

## Pre-requisites

To [build](#building) and run One ROM Lens, you need to build the One ROM firmware.  This requires either a Linux or macOS host, and the dependencies listed in the [INSTALL.md](/INSTALL.md) instructions, including C build tools, the Rust toolchain and python3.

## Building

To build, first build the firmware, including the ROM image for the emulated One ROM to serve.  This must be done using the original `make` approach, as opposed to the newer `scripts/onerom.sh` approach.

For example:

```bash 
HW_REV=fire-24-e MCU=rp2350 ROM_CONFIGS=file=images/test/0_63_8192.rom,type=2364,cs1=0 make
```

Then, build One ROM Lens:

```bash
make -C sdrr -f lens.mk
```

## Running

```bash
make -C sdrr -f lens.mk run
```

Point a browser at `http://localhost:8000` to access the One ROM Lens interface.
