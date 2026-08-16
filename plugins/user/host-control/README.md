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

## Sending bytes out through One ROM

RBCP's Pipes group lets the host write bytes to a pipe on the device, and this plugin's pipe is One ROM's log channel — so a retro system can get its output to a PC over One ROM's USB, with no serial port or display of its own.  Read it with `onerom monitor log`, or any terminal on the CDC serial port.  See [Logging](/docs/LOGGING.md) for what else arrives there.

Four bytes per command, transferred whole or not at all.  A refusal means the channel is full because nothing has drained it, and `GET_PIPE_INFO` reports the room left so a host can decide whether to retry or drop the bytes.  The device never waits for space — a host blocked on a stalled USB link would be a stalled retro system.

Three things follow from the pipe being the log channel rather than a channel of its own, and a host cannot detect any of them:

- **One ROM's own logging is interleaved with the host's bytes.**  Errors are always logged, and boot and plugin logging can be switched on, so a host writing text should expect One ROM's output mixed into it.  A released build carries none of the optional kinds, so in practice only errors arrive uninvited.  The plugin's own messages about the Pipes group are a step quieter again — they are debug output, so they stay out even of a build made with `PLUGIN_LOGGING=1`, where the rest of its RBCP messages would appear.
- **Bytes can be corrupted, not merely interleaved.**  This plugin runs on core 0 and the USB plugin on core 1, and interrupt masking does not cross cores, so a write from each at the same moment can interleave *within* a record.
- **A debug probe reading the log will take the host's bytes too**, and both readers advance the same position, so attach one or the other.

Pipes need **firmware v0.7.2 or later**, where the plugin logging API arrived.  On older firmware the plugin runs exactly as before and `GET_PIPE_CAPABILITY` reports no pipes, which the specification provides for — a host should query it before writing, as it should on any device.

## Driving One ROM's pins

RBCP's Auxiliary I/O group lets the host drive and read device pins over the ROM bus, so a wire from a One ROM pad can reach a reset line, a drive, a relay or an indicator and the host can operate it from software.  RBCP describes mechanism only — a pin number, a level, a duration — because the device has no idea what is on the far end of the wire.

Three pin groups are exposed, each with a type byte a host reads from `GET_AUX_GROUP_INFO`:

| Type | Group | Pins |
|------|-------|------|
| `0x01` | GPIO | Every GPIO on the running RP2350 variant, numbered as the datasheet numbers them — 0 to 29 on an A, 0 to 47 on a B |
| `0x80` | Image select | The image select pads, in the order the board's metadata lists them: pin 0 is SEL0 |
| `0x81` | X | The X expansion pads, X1 then X2 |

`0x80` and `0x81` are this implementation's own values, from the range RBCP reserves for exactly that (`0x80`–`0xFE`).  They are not portable to another RBCP device, and another device may use the same two values for something else entirely.  What they buy a host is that they *are* portable across One ROM boards: SEL1 means SEL1 on every board that has one, while the GPIO behind it changes from revision to revision.

**Groups are numbered densely, so read the type rather than assuming an index.**  A board with no X pads — every 32- and 40-pin board, and the earlier 28-pin revisions — exposes two groups, not three, and the group a host would find at index 2 elsewhere is simply not there.

A pin is reported drivable only where One ROM is using none of it.  That means the whole address, chip select and data set of the *active* slot is off limits, and so are the board's status LED, Neopixel, VBUS and external flash chip select pins.  Switching slots can change the answer, since a GPIO that is an address line for one ROM type is free for another.

An X pad can reach two GPIOs on some boards.  Both are the same electrical net, so the pin is drivable only if both are free, and setting it drives both.

`SET_AUX` with a non-zero hold does not complete until the hold has elapsed and the `after` state has been applied, so a host seeing the command complete knows the pin reached its final state.  This plugin accepts holds up to the protocol's maximum of 255 units, 2.55 seconds.  **RBCP is unresponsive for the whole of a hold** — the plugin has no task loop, so it waits in the command handler.

A pin keeps whatever state it was left in when the session ends, and across `RBCP_RESET`.  Only a One ROM reset restores it.

Auxiliary I/O is built on the GPIO API added in **firmware v0.7.1**, which is this plugin's minimum.  Timed holds and the image select and X groups need **v0.7.2**: on v0.7.1 the device reports a `max_hold` of zero — the specification's way of saying it offers no timed holds, and a host wanting a pulse must time it itself with two commands — and exposes the GPIO group alone.  A device that can offer nothing at all reports no groups, and every other command in the group then fails.

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
