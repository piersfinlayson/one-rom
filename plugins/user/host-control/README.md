# Host Control

One ROM's Host Control plugin is a full implementation of the [ROM Bus Control Protocol (RBCP)](https://github.com/piersfinlayson/rom-bus-control-protocol), which enables bidirectional communication between a host computer and an RBCP-capable ROM emulator using only the ROM address and data buses — no additional hardware required.

This allows a host system to query and modify the state of the emulated ROM installed within it, allowing a wide range of applications, including:

- ROM based bootloaders (think `grub` for the C64)
- Dynamic ROM patching for games, demos and other applications
- Remote debugging of code running on real retro systems

## Building the Plugin

```bash
make
```

This creates `build/plugin_user.bin`, which can be loaded onto One ROM as a user plugin, enabling RBCP support

## Using the Plugin

The plugin is designed to be driven by the host system's CPU directly.  A [C64 kernal bootloader](https://github.com/piersfinlayson/rom-bus-control-protocol/tree/main/reference/host/6502/c64-boot) is available as part of the RBCP reference implementation.

To use, build the C64 kernal bootloader, and then install as the first non-plugin image on One ROM.  You will then need to follow it with one or more other C64 kernal images that you want to be able to switch between using the bootloader.

## Address signalling

RBCP command signalling (the knock and command bytes) travels on the address lines the device observes at the ROM socket — which are not always the host's own least-significant address lines.

This plugin omits the least-significant address line from command signalling for every ROM served on the **40-pin variant**: on that hardware the ROM's least-significant line is served through a separately-read pin the address monitor cannot sample.  A host must therefore carry command data from address bit 1 upward, advancing its read address by two per command byte.  On the 24-, 28- and 32-pin variants every address line is observed, so command data uses address bit 0 upward with stride 1.

See "Address Line Presentation" in the [RBCP specification](https://github.com/piersfinlayson/rom-bus-control-protocol) for the general model.

## Unsupported ROM types

RBCP and this host-control plugin rely on One ROM's address monitor, which watches chip-select and captures the addresses the host reads.  The monitor does not yet support ROM/board combinations that use a **qualifier-based chip-select** — where address lines factor into the select decision and the ROM is deselected over certain address ranges (the firmware's `ALG_CS_2` algorithm).  On such combinations RBCP does not function.

This currently affects:

- **23QL384**, on every board — it inherently combines address lines into its CS decision and does not serve certain address ranges.
- **23QL512**, only on board configurations where its upper address pins double as bank-select (X) pins — for example the 28-pin Fire revisions that have X pins (fire-28-c/d) with CS1 active-low — where the same qualifier-based algorithm is selected.  Where 23QL512 resolves to a plain chip-select it is fully supported.

Note that the chip-select algorithm is resolved from the chip, the board and the per-image CS configuration, so whether a given ROM type is supported can depend on the board and configuration, not the chip type alone.

Support for these ROM types is planned as a future enhancement to the address monitor.