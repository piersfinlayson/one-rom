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

## Deselected address ranges

RBCP and this host-control plugin rely on One ROM's address monitor, which watches chip-select and captures the addresses the host reads.  Every ROM type is supported, including those with a **qualifier-based chip-select** — where address lines factor into the select decision, so the ROM is deselected over part of its address space (the firmware's `ALG_CS_2` algorithm).

One ROM type works that way: the **23QL384**, on every board and in every CS configuration.  It combines its top two address lines into the chip-select decision and serves nothing while both are high.  The monitor captures only where the chip is genuinely selected, so a host must keep its command signalling — the knock and the command bytes after it — inside an address range the ROM actually serves.  For the 23QL384 that means below the top quarter of its address space; reads there are invisible to the plugin, exactly as they are to the ROM.

No other ROM type has a deselected range, so on all of them any address the ROM answers can carry command signalling.
## Deviations from the RBCP specification

This plugin aims to implement the [RBCP specification](https://github.com/piersfinlayson/rom-bus-control-protocol) exactly, and its conformance is tested against the specification rather than against itself.  Where it knowingly differs, the difference is listed here.

### GET_FLASH_SLOT_INFO accepts a smaller back-channel than the specification requires

The specification says `GET_FLASH_SLOT_INFO` "only succeeds if there is sufficient space, which means a back channel size of at least 64 bytes".  This plugin requires a 32-byte response data section — a 40-byte back-channel region.

Forty is what the response actually needs: an 8-byte response header plus one 32-byte record.  The specification's 64 is a round number above that.  The deviation is therefore more permissive than the specification, and no specification-conformant host can be affected by it: a host that allocates the 64 bytes the specification asks for is served exactly as it expects.  A host written against this plugin, however, may allocate as little as 40 and will not be portable to a device that enforces the 64.

### NV_POKE_BEGIN may not overwrite the RAM slot the host names

The specification says a host must lend the device a slot for staging: "A RAM slot must be provided by the host for the device to use as a staging area.  This means that any RAM slot specified will be overwritten by the device and should not be used for any other purpose while a write transaction is in progress."  It also has `NV_POKE_BEGIN` fail "if ... the RAM slot specified is invalid, active or **too small**".

Where this plugin has RAM slots of its own — see [RAM slots above 170 are not offered to the host](#ram-slots-above-170-are-not-offered-to-the-host) — it stages the transaction in those and leaves the host's slot untouched, and it does not then require that slot to be large enough.  Two consequences, both more permissive than the specification:

- The named slot survives the transaction, where the specification says it will be overwritten.  A conformant host cannot notice, because it has been told not to rely on that slot's contents; a host written against this plugin might come to rely on them surviving and would not be portable.
- A transaction succeeds where the specification allows it to fail.  A slot is exactly one ROM region, and a small ROM makes every slot far smaller than the 4KB of NV storage plus the erase routine that staging needs — so on those devices a strictly conformant implementation could never perform a write at all.  Staging in the plugin's own slots is what makes NV storage writable there.

The host still names a slot, and it is still rejected if it is 0xAA, out of range, or the slot being served.  Those checks are what a host can act on, and they cost nothing to keep.

### RAM slots above 170 are not offered to the host

Every RBCP command that names a RAM slot rejects an argument of 0xAA, so that a reset started mid-command stays detectable.  A slot whose index is 170 or above therefore cannot be named by any host, so this plugin reports at most 170 slots from `GET_RAM_SLOT_INFO_ALL` and rejects any higher index, even where the firmware has more.  The slots above that are used for the plugin's own purposes, as described above.

This is not a deviation from anything the specification requires — `total_count` is "Total number of RAM slots available on the device", and these are not available to a host — but it is worth stating, because the firmware's own slot count and the number a host sees are not the same number.
