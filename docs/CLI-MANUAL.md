# One ROM CLI Manual

`onerom` (`onerom.exe` on Windows) is the command-line tool for managing One ROM
ROM emulators: discovering connected devices, building and flashing firmware,
inspecting device state, and manipulating ROM image files.

This manual is in two parts. The **Guide** walks through installation and the
common workflows. The **Reference** documents every command, subcommand and
option.

> This manual documents the `onerom` CLI as of release v0.1.10. Board,
> chip and plugin lists shown in examples are illustrative — the set your build
> supports may differ. Run `onerom --version` to check your version, and
> `onerom boards` / `onerom chips` for the definitive lists your build knows
> about. Commands marked **(not yet supported)** are present in the CLI surface
> but not yet functional.

---

# Part 1 — Guide

## Installation

Download the CLI from **<https://onerom.org/cli>**. Builds are provided for:

- Windows — x86 64-bit and ARM 64-bit
- macOS
- Ubuntu/Debian — x86 64-bit, and ARM 64-bit (also for Raspberry Pi)

The Windows and macOS builds are digitally signed. A sha256 checksum is published
alongside every download so you can verify what you fetched.

**Windows / macOS** — unzip the archive and place the `onerom` (`onerom.exe`)
executable in a folder on your `PATH`.

**Linux** — install the `.deb` as usual, which places `onerom` on your `PATH`:

```
sudo dpkg -i onerom-cli-x.y.z-1_amd64.deb
```

(replace `x.y.z` with the version, e.g. `0.1.10`, and `amd64` with `arm64` for
the ARM/Pi build).

**Windows SmartScreen** — as a relatively new publisher, the first run may raise
a *"Windows protected your PC"* dialog. Click **More info**, confirm the
publisher reads *"Open Source Developer, Piers Finlayson"*, then **Run anyway**.

Verify it runs:

```
onerom --version
```

## How One ROM talks to the CLI

The CLI communicates with a One ROM over USB using picoboot (the Raspberry Pi
bootloader protocol, extended by
[picobootx](https://github.com/piersfinlayson/picobootx)). A One ROM is reachable in two
situations:

- **Running** — normal firmware is running and serving ROMs; its USB stack
  (provided by the system USB plugin) exposes the picobootx interface.
- **Stopped** — the device is in the RP2350 bootloader (BOOTSEL). A bare RP2350
  bootloader is also reachable here, which is how unprogrammed or bricked units
  are recovered.

Some commands work in either state; some require one specifically. Each
reference entry notes when a device connection is required, and the state model
is summarised under [Device states](#device-states).

## Identifying your device

With **exactly one** One ROM connected that the CLI recognises, you don't need
to identify it — commands find it automatically, and the board type is inferred
from the device.

With **multiple** devices connected, select one with `--serial` (`-s`). It
accepts `*` and `?` wildcards:

```
onerom --serial 'A1B2*' inspect info
```

`--serial` is **global**: it can appear at any level of the command line.

Discover what's attached:

```
onerom scan
onerom scan --slots        # also list each device's ROM slots
```

Two situations need extra flags:

- **Unrecognised / unprogrammed / bricked** units: add `--unrecognised` (`-u`)
  and supply `--board`, since the board type can't be inferred. The unit must
  still expose a valid picoboot USB interface.
- **Non-standard USB IDs**: add `--vid-pid <VID:PID>` (hex), repeatable. When
  supplied, only the given VID/PID pairs are matched.

`--board` (`-b`) can also be given on most commands to **override** the detected
board type.

## Common workflows

### Program a device from a config file

The primary workflow. Build firmware from a JSON config and flash it in one
step:

```
onerom program --config-file c64.json
```

`program` builds *and* flashes. To build a firmware binary **without** flashing,
use [`firmware build`](#firmware-build) instead. To build and also keep the
binary while flashing, add `--output`:

```
onerom program --config-file c64.json --out firmware.bin
```

### Program from `--slot` specifications

Instead of a config file, describe each ROM slot inline. Repeat `--slot` per
slot. The required chip-select lines depend on the chip type (e.g. a 2332 needs
`cs1` and `cs2`):

```
onerom program --board fire-24-e \
    --slot file=kernal.bin,type=2364,cs1=active_low \
    --slot file=basic.bin,type=2364,cs1=active_low
```

The full slot spec grammar is documented under [ROM slot
specification](#rom-slot-specification) — it covers CS polarity, size handling,
per-slot CPU frequency/voltage, the status LED and 16-bit forcing.

### Program with a plugin

Plugins masquerade as ROMs. At most one system plugin and one user plugin are
supported; a user plugin requires a system plugin. The system plugin lands in
slot 0, the user plugin in slot 1:

```
onerom program --board fire-24-e \
    --slot file=kernal.bin,type=2364,cs1=active_low \
    --plugin usb
```

Plugin spec forms are listed under [Plugin
specification](#plugin-specification).

### Build firmware without flashing

```
onerom firmware build --config-file c64.json --board fire-24-e --out firmware.bin
```

### Download and flash a pre-built release

```
onerom firmware download --version 0.6.5 --board fire-24-e --out firmware.bin
onerom program --firmware firmware.bin
```

Or in one step — `program` will download the base firmware, build in your ROMs,
and flash:

```
onerom program --config-file c64.json --version 0.6.5
```

### Inspect a device

```
onerom inspect info      # serial, name, board, MCU, firmware version, hw revision
onerom inspect slots     # ROM slots, with the active one marked
```

### Read the live ROM image

Read what the device would serve for a given logical ROM address (device must be
running). The top-level `peek` is an alias for `inspect peek live`:

```
onerom peek live --address 0x100 --length 64
onerom peek live --address 0 --length 8192 --output rom-image.bin
```

### Patch a running image

`poke live` writes to the ROM image currently being served, at a logical ROM
offset. Changes are transient — lost on reboot. The top-level `poke` is an alias
for `control poke live`:

```
onerom poke live --address 0x100 --byte 0xEA
onerom poke live --address 0 --input patch.bin
```

For file patches you can write only the differing bytes, and preview first:

```
onerom poke live --input patch.bin --delta --dry-run
onerom poke live --input patch.bin --delta
```

### Identify a physical unit

Make the status LED beacon so you can spot which board is which:

```
onerom control led beacon
```

### Erase / recover a device

Erase flash. This is best done while stopped; by default the command reboots the
device into the required state first. A fully erased unit falls back to the
RP2350 bootloader and is then reprogrammed with `--unrecognised` + `--board`:

```
onerom control erase --all
onerom control erase --offset 0x20000 --length 0x1000
```

Read [`control erase`](#control-erase) before using it while the device is
running — erasing the core firmware or system plugin will take down the USB
stack.

### Prepare a 16-bit ROM image

16-bit ROM types (e.g. 27C400) may need their byte pairs swapped to match the
order One ROM expects:

```
onerom image swap-bytes --input kick.bin --output kick-swapped.bin
```

## Device states

Many commands reboot the device and, by default, pause briefly afterwards to let
it re-enumerate on the USB bus.

- **Running** (default reboot target) — firmware active, serving ROMs.
- **Stopped** — RP2350 bootloader (BOOTSEL); required for some flash operations.

Common controls, where a command supports them:

- `--running` (`-r`) / `--stopped` (`-p`) — choose the post-operation state.
- `--no-reboot` — leave the device as-is.
- `--fast` — skip the re-enumeration pause.
- `--msd` (`-m`) — mount the mass-storage device when rebooting into stopped
  mode.

## Global behaviour worth knowing

- `--yes` (`-y`) auto-confirms all prompts (non-interactive use). It also
  suppresses the confirmation otherwise required for CPU frequencies above
  150 MHz and voltages above 1.10 V in slot specs. Use with care.
- `--verbose` (`-v`) prints device-selection progress and other detail.
- `--log-level <LEVEL>` sets log verbosity; defaults to `warn`. Run
  `onerom --help` for the accepted levels.

---

# Part 2 — Reference

## Synopsis

```
onerom [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

## Global options

Available on every command (they are `global` in clap terms and may appear at
any level).

| Option | Description |
|---|---|
| `--serial, -s <DEVICE>` | Select a One ROM by serial number. Required when multiple are connected; auto-selected when exactly one is present. Accepts `*` and `?` wildcards. |
| `--vid-pid, -i <VID:PID>` (alias `--id`) | USB vendor/product ID pair in hex (e.g. `1234:abcd`). Repeatable; when given, only these pairs are matched. Use with `--unrecognised`. |
| `--unrecognised, -u` (alias `--unrecognized`) | Allow management of unrecognised/unprogrammed/bricked RP2350 boards. The unit must still expose a valid picoboot USB interface. Use with caution — permits programming any attached RP2350 board. |
| `--yes, -y` | Auto-confirm all prompts. Also suppresses the over-limit CPU frequency/voltage confirmations. |
| `--verbose, -v` | Enable verbose output. |
| `--log-level <LEVEL>` | Set log level. Defaults to `warn`. |
| `--version, -V` | Print version. |
| `--help, -h` | Print help. Works on any subcommand. |

Most commands accept `--board` (`-b`) to identify or override the board type,
and rely on `--serial` (global) to pick a specific device.

## Command summary

| Command | Purpose | Device required |
|---|---|---|
| [`scan`](#scan) | Discover connected One ROMs | No |
| [`program`](#program) | Build and flash firmware to a One ROM | Yes |
| [`inspect`](#inspect) | Read-only device state and information | Yes |
| [`control`](#control) | Transient (non-persistent) device actions | Yes |
| [`update`](#update) | Persistent device modifications | Yes |
| [`image`](#image) | ROM image file manipulation | No |
| [`firmware`](#firmware) | Build, inspect and manage firmware binaries | Varies |
| [`plugin`](#plugin) | List available plugins | No |
| [`chips`](#chips) | List supported chip types | No |
| [`boards`](#boards) | List supported board types | No |
| [`peek`](#peek-top-level-alias) | Alias for `inspect peek live` | Yes |
| [`poke`](#poke-top-level-alias) | Alias for `control poke live` | Yes |
| [`reboot`](#reboot-top-level-alias) | Alias for `control reboot` | Yes |

---

## scan

Discover and list connected One ROMs — serial, USB location, name, board type,
MCU and loaded firmware version.

```
onerom scan
onerom scan --board fire-24-e
onerom scan --slots
```

| Option | Description |
|---|---|
| `--board <BOARD>` | Only show devices matching this board type. Conflicts with `--list-boards`. |
| `--list-boards` | List all known board types. |
| `--slots` (alias `--slot`) | Also show the ROM slot contents for each device found. Conflicts with `--list-boards`. |

Device required: no.

---

## program

Build a firmware image (from a config file, inline `--slot` specs, or a supplied
binary) and flash it to a connected One ROM. This is the primary workflow.
`onerom firmware program` is an alias for this command.

```
onerom program --config-file c64.json
onerom program --serial '5*' --config-file c64.json
onerom program --board fire-24-e \
    --slot file=kernal.bin,type=2364,cs1=active_low \
    --slot file=basic.bin,type=2364,cs1=active_low
onerom program --firmware firmware.bin
onerom program --config-file c64.json --out firmware.bin
```

### Source of the firmware (mutually exclusive groups)

| Option | Description |
|---|---|
| `--config-file, -j <FILE>` (aliases `--config-json`, `--config`, `--json`) | ROM configuration JSON file. Conflicts with `--slot`, `--config-name`, `--config-description`, `--save-config`, `--no-config`, `--firmware`. |
| `--slot <SPEC>` (alias `--rom`) | ROM slot specification; repeatable. See [ROM slot specification](#rom-slot-specification). Conflicts with `--config-file`, `--no-config`, `--firmware`. |
| `--firmware <FILE>` (alias `--fw`) | Flash a pre-built complete firmware binary directly. Conflicts with `--config-file`, `--slot`, `--base-firmware`, `--version`. |
| `--base-firmware <FILE>` | Use a local minimal firmware instead of downloading. With `--slot`, ROMs are built into it; alone, requires `--no-config`. Must be built with `EXCLUDE_METADATA=1` and `ROM_CONFIGS=`. Conflicts with `--firmware`, `--version`. |
| `--no-config` | Confirm flashing a base firmware with no ROM configuration. Only valid with `--config-name` and/or `--config-description`. Conflicts with `--config-file`, `--slot`, `--firmware`, and the config-override options below. |

### Configuration metadata

| Option | Description |
|---|---|
| `--plugin <SPEC>` | Plugin specification; repeatable. See [Plugin specification](#plugin-specification). Conflicts with `--config-file`. |
| `--config-name <NAME>` (alias `--name`) | Name for the generated ROM configuration. Conflicts with `--config-file`. |
| `--config-description <DESC>` (aliases `--desc`, `--description`) | Description for the generated configuration. Defaults to *"Created by the One ROM CLI"*. Conflicts with `--config-file`. |
| `--save-config <FILE>` | Save the generated configuration to JSON. Only valid with `--slot` or `--no-config`. Conflicts with `--config-file`. |

### Per-device overrides

These are rejected with `--no-config`.

| Option | Description |
|---|---|
| `--instance-name <NAME>` (aliases `--onerom`, `--one-rom`, `--onerom-name`, `--one-rom-name`, …) | Give this One ROM a name. |
| `--serial-override <NEW SERIAL>` | Override the device's reported serial number. |
| `--logging [BOOL]` (aliases `--boot-logging`) | Enable boot logging. Takes an optional boolean; bare flag means `true`. |
| `--disable-swd [BOOL]` (aliases `--swd-disable`) | Enable/disable SWD debugging. Optional boolean; bare flag means `true`. |
| `--turbo-boot [BOOL]` | Enable turbo boot — starts serving faster but supports only a single programmed slot. Optional boolean; bare flag means `true`. |

### Board, version and output

| Option | Description |
|---|---|
| `--board, -b <BOARD>` | Target board type. Inferred from the connected device if omitted. |
| `--version <VERSION>` | Firmware version to build against. Defaults to the latest release. Conflicts with `--firmware`, `--base-firmware`. |
| `--output, -o <FILE>` (alias `--out`) | Also write the built firmware to this file while flashing. |

### Reboot and flashing behaviour

| Option | Description |
|---|---|
| `--stopped, -p` | After flashing, reboot into stopped (bootloader) mode. Conflicts with `--running`. |
| `--running, -r` | After flashing, reboot into running mode (the default). Conflicts with `--stopped`. |
| `--no-reboot` | Do not reboot after flashing. Conflicts with `--stopped`. |
| `--fast` | Skip the re-enumeration pause after the final reboot. Conflicts with `--no-reboot`. |
| `--msd, -m` | Mount mass storage when rebooting into stopped mode. |
| `--verify` | Verify flash by reading back after programming. **(not yet supported)** |
| `--force, -f` | Continue even if the assembled firmware has parse errors. |
| `--batch` (aliases `--multiple`, `--multi`) | Program multiple devices, pausing for confirmation between each. Every board is programmed with the same configuration as the first. |
| `--scan-slots` | After programming, run `onerom scan --slots` to show the result. Conflicts with `--fast`. |

Device required: yes.

---

## inspect

Read-only inspection of a connected One ROM.

```
onerom inspect <COMMAND>
```

| Subcommand | Purpose | Device required |
|---|---|---|
| [`info`](#inspect-info) | Identity and configuration | Yes |
| [`telemetry`](#inspect-telemetry) | Runtime telemetry **(not yet supported)** | Yes |
| [`slots`](#inspect-slots) | List ROM slots | Yes |
| [`image`](#inspect-image) | Read a slot's ROM image **(not yet supported)** | Yes |
| [`peek`](#inspect-peek) | Read SRAM or the live ROM image | Yes |
| [`gpio`](#inspect-gpio) | Read GPIO pin state **(not yet supported)** | Yes |

### inspect info

Show the device's serial number, user-assigned name, board type, MCU, firmware
version and hardware revision. No options.

```
onerom inspect info
onerom --serial 1234abcd inspect info
```

### inspect telemetry

Access counts, timing statistics and other runtime metrics. **(not yet
supported)**

| Option | Description |
|---|---|
| `--json` | Output telemetry as JSON. |

### inspect slots

List the ROM image slots stored on the device — index, ROM type, size and
description — marking the active slot. No options.

### inspect image

Read (or save) the ROM image from a slot. **(not yet supported)**

| Option | Description |
|---|---|
| `--slot, -l <INDEX>` | Slot index to read. Reads the active slot if omitted. |
| `--output, -o <FILE>` (alias `--out`) | Save the image data to this file. |

### inspect peek

Read device memory. `peek memory` reads SRAM (and, in stopped state,
page-aligned flash); `peek live` reads the ROM image currently being served.

```
onerom inspect peek <COMMAND>
```

#### inspect peek live

Read from the live ROM image at a **logical** ROM offset (starting at 0). The
device must be running. Also reachable as the top-level [`peek`](#peek-top-level-alias).

```
onerom inspect peek live --address 0x100 --length 64
onerom inspect peek live --address 0 --length 8192 --output rom-image.bin
```

| Option | Description |
|---|---|
| `--address, -a <ADDRESS>` (alias `--addr`) | Logical ROM address to read from, starting at 0. Decimal or `0x` hex. Default `0`. |
| `--length, -l <LENGTH>` (aliases `--len`, `--size`) | Number of bytes to read. Decimal or hex. If omitted, reads to the end of the live image. |
| `--output, -o <FILE>` (alias `--out`) | Save the data to this file. |

#### inspect peek memory

Read One ROM's SRAM. Most addresses reachable via PICOBOOT can be queried. In
stopped state, SRAM holds no meaningful data, and flash reads must be aligned to
flash page boundaries.

```
onerom inspect peek memory --address 0x20000000 --length 128
onerom inspect peek memory --address 0x10000000 --length 8192 --output flash-start.bin
```

| Option | Description |
|---|---|
| `--address, -a <ADDRESS>` (alias `--addr`) | Address to read from. Decimal or `0x` hex. |
| `--length, -l <LENGTH>` (aliases `--len`, `--size`) | Number of bytes to read. Decimal or hex. |
| `--output, -o <FILE>` (alias `--out`) | Save the data to this file. |

### inspect gpio

Show the direction and logic level of each exposed GPIO pin. **(not yet
supported)**

| Option | Description |
|---|---|
| `--pin <PIN>` | Show only this pin. |

---

## control

Transient actions on a connected One ROM. These affect current state but do not
persist across power cycles.

```
onerom control <COMMAND>
```

| Subcommand | Purpose | Device required |
|---|---|---|
| [`reboot`](#control-reboot) | Reboot the device | Yes |
| [`led`](#control-led) | Control the status LED | Yes |
| [`poke`](#control-poke) | Write to SRAM or the live ROM image | Yes |
| [`reset`](#control-reset) | Assert the host reset signal **(not yet supported)** | Yes |
| [`select`](#control-select) | Select the active ROM slot **(not yet supported)** | Yes |
| [`gpio`](#control-gpio) | Set a GPIO pin state **(not yet supported)** | Yes |
| [`erase`](#control-erase) | Erase flash memory | Yes |

### control reboot

Restart the firmware; the device re-initialises and resumes serving. By default
pauses afterwards for re-enumeration. Also reachable as the top-level
[`reboot`](#reboot-top-level-alias).

```
onerom control reboot
```

| Option | Description |
|---|---|
| `--stopped, -p` | Reboot into stopped (bootloader) state. |
| `--running, -r` | Reboot into running (serving) state. Default. |
| `--fast` | Don't pause for re-enumeration. |
| `--msd, -m` | Mount mass storage when rebooting into stopped mode. Conflicts with `--running`. |

`--stopped` and `--running` are mutually exclusive.

### control led

```
onerom control led on
onerom control led off
```

| Subcommand | Description |
|---|---|
| `on` | Turn the status LED on. |
| `off` | Turn the status LED off. |
| `beacon` | Beacon the LED to identify a physical unit. |
| `flame` | Flame effect on the LED. |

None take options. Device required: yes.

### control poke

Transient writes to device memory — changes are lost on reboot. Use
[`update`](#update) for persistent flash writes.

```
onerom control poke <COMMAND>
```

#### control poke memory

Write a single byte or a binary file to SRAM at a given address. When the device
is running, virtual addresses are available (e.g. `0x90000000` is the start of
the live ROM image — though prefer `poke live` for that). Writing arbitrary SRAM
can corrupt firmware state.

```
onerom control poke memory --address 0x20000010 --byte 0xFF
onerom control poke memory --address 0x20000000 --input patch.bin
```

| Option | Description |
|---|---|
| `--address, -a <ADDRESS>` (alias `--addr`) | Address to write to. Decimal or `0x` hex. |
| `--byte, -b <BYTE>` (alias `--value`) | Single byte value to write. Decimal or hex. |
| `--input <FILE>` (alias `--in`) | Write the contents of this binary file. |

Exactly one of `--byte` / `--input` is required.

#### control poke live

Write a single byte or a binary file to the live ROM image at a **logical** ROM
offset (starting at 0). Useful for patching a running ROM without reflashing.
Also reachable as the top-level [`poke`](#poke-top-level-alias).

```
onerom control poke live --address 0x100 --byte 0xEA
onerom control poke live --address 0 --input patch.bin
```

| Option | Description |
|---|---|
| `--address, -a <ADDRESS>` (alias `--addr`) | Logical ROM address, starting at 0. Decimal or `0x` hex. Default `0`. |
| `--byte, -b <BYTE>` (alias `--value`) | Single byte value to write. Decimal or hex. |
| `--input <FILE>` (alias `--in`) | Write the contents of this binary file. |
| `--delta` (alias `--deltas`) | Only write bytes that differ from current device content. Requires `--input`. |
| `--dry-run` (alias `--dryrun`) | Show what would be written without writing. Requires `--delta`. |

Exactly one of `--byte` / `--input` is required.

### control reset

Drive the reset pin to reset the host system One ROM is installed in — useful in
scripted workflows after programming. **(not yet supported)**

```
onerom control reset --hold 500
```

| Option | Description |
|---|---|
| `--hold <MS>` | Milliseconds to hold reset asserted. Default `100`. |

### control select

Switch the device to serving the specified slot immediately (not persistent).
**(not yet supported)**

| Option | Description |
|---|---|
| `--slot, -l <INDEX>` | Slot index to activate. Required. |

### control gpio

Set a GPIO pin high, low, or high-impedance. **(not yet supported)**

```
onerom control gpio --pin 3 --state high
onerom control gpio --pin 3 --state z
```

| Option | Description |
|---|---|
| `--pin <PIN>` | GPIO pin number. Required. |
| `--state <STATE>` | `high`, `low`, or `z` (high-impedance). Required. |

### control erase

Permanently erase flash contents — firmware, metadata and ROM images. A fully
erased unit boots into the RP2350 bootloader and is reprogrammed with
`--unrecognised` + `--board`.

Best performed while stopped; by default the command reboots into the required
state first. Erasing the core firmware or the system plugin while **running**
takes down the USB stack (requiring manual BOOTSEL via the header pins), and
large erases may cause a temporary USB drop and re-enumerate — in which case the
erase likely succeeded and can be checked with `inspect peek memory`. Anything
else running from flash (e.g. a user plugin) may crash during an erase.

Offsets are relative to the flash base `0x10000000`. Ranges must be 4096-aligned.
Multiple ranges may be erased in one operation.

```
onerom control erase --all
onerom control erase --offset 0x20000 --length 0x1000
```

| Option | Description |
|---|---|
| `--all, -a` | Erase all flash contents. |
| `--offset, -o <OFFSET>` | Erase at offset(s) from the flash base. 4096-aligned; pair each with a `--length`; repeatable. Conflicts with `--address`. |
| `--address <ADDRESS>` (alias `--addr`) | Erase at absolute address(es). 4096-aligned; pair each with a `--length`; repeatable. Conflicts with `--offset`. |
| `--length <LENGTH>` (aliases `--len`, `--size`) | Length of each range. 4096-aligned; specify once per `--offset`/`--address`; repeatable. Conflicts with `--all`. |
| `--no-reboot, -n` | Don't reboot before or after erasing. Risky if One ROM is accessing the range. |
| `--reboot-stopped, -p` | Reboot into stopped mode after erasing. |
| `--reboot-running, -r` | Reboot into running mode after erasing. |
| `--msd, -m` | Mount mass storage when rebooting into stopped mode. Requires `--reboot-stopped`. |
| `--fast` | Don't pause for re-enumeration. Requires a reboot mode. |

One of `--all` / `--offset` / `--address` is required. `--reboot-stopped` and
`--reboot-running` are mutually exclusive, and both conflict with `--no-reboot`.

---

## update

Persistent modifications — these write to flash and survive power cycles.

```
onerom update <COMMAND>
```

| Subcommand | Purpose | Device required |
|---|---|---|
| [`slot`](#update-slot) | Write a ROM image to a flash slot **(not yet supported)** | Yes |
| [`commit`](#update-commit) | Commit the live image to flash **(not yet supported)** | Yes |
| [`otp`](#update-otp) | Read/write OTP memory **(not yet supported, hidden)** | Yes |

### update slot

Write a ROM image to a flash slot; persists across power cycles. The ROM type
and chip-select configuration must match the slot's existing configuration, or
the slot must be empty. **(not yet supported)**

```
onerom update slot --slot 2 --image kernal.bin
```

| Option | Description |
|---|---|
| `--slot, -l <INDEX>` | Flash slot index to write. Required. |
| `--image, -m <FILE>` | ROM image file to write. Required. |

### update commit

Persist the currently active RAM image to its corresponding flash slot. **(not
yet supported)**

```
onerom update commit
onerom update commit --slot 2
```

| Option | Description |
|---|---|
| `--slot, -l <INDEX>` | Slot to commit. Commits the active slot if omitted. |

### update otp

Read or write RP2350 OTP memory, including One ROM-specific USB configuration and
identity data. Hidden, advanced. **OTP writes are irreversible.** **(not yet
supported)**

| Option | Description |
|---|---|
| `--read` | Read and display OTP contents. Conflicts with `--write`. |
| `--write <ROW=VALUE>` | Write a value to an OTP row. Conflicts with `--read`. |

---

## image

ROM image file manipulation. No device connection required.

```
onerom image <COMMAND>
```

### image swap-bytes

Swap adjacent byte pairs — reverses byte order within each 16-bit word
throughout the image. Required for 16-bit ROM types (e.g. 27C400) when the source
has the opposite byte order to what One ROM expects. The input must have an even
number of bytes.

```
onerom image swap-bytes --input kick.bin --output kick-swapped.bin
```

| Option | Description |
|---|---|
| `--input, -i <FILE>` (alias `--in`) | Input ROM image file. |
| `--output, -o <FILE>` (alias `--out`) | Output file path. |

Device required: no.

---

## firmware

Build, inspect and manage firmware binaries. Use [`program`](#program) to flash;
`firmware build` produces a binary without flashing.

```
onerom firmware <COMMAND>
```

| Subcommand | Purpose | Device required |
|---|---|---|
| [`build`](#firmware-build) | Build a firmware binary from a config | No |
| [`inspect`](#firmware-inspect) | Inspect a firmware binary | No |
| [`releases`](#firmware-releases) | List firmware releases | No |
| [`download`](#firmware-download) | Download a release binary | No |
| [`chips`](#firmware-chips) | List supported chip types | No |
| `program` | Alias for [`onerom program`](#program) | Yes |

### firmware build

Produce a flashable firmware binary for a board and MCU from a JSON config or
inline `--slot` args, without flashing.

```
onerom firmware build --config-file c64.json --board fire-24-e --out firmware.bin
onerom firmware build --board fire-24-e \
    --slot file=kernal.bin,type=2364,cs1=active_low \
    --out firmware.bin
```

The configuration options mirror [`program`](#program): `--config-file` (`-j`),
`--slot`, `--plugin`, `--config-name`, `--config-description`, `--save-config`,
`--no-config`, and the per-device overrides `--instance-name`,
`--serial-override`, `--logging`, `--disable-swd`, `--turbo-boot` (all rejected
with `--no-config`). Build-specific options:

| Option | Description |
|---|---|
| `--board, -b <BOARD>` | Target board type. Required when not inferrable from a connected device. |
| `--version <VERSION>` | Firmware version to build against. Defaults to latest. |
| `--base-firmware <FILE>` | Use a local minimal firmware instead of downloading. Must be built with `EXCLUDE_METADATA=1` and `ROM_CONFIGS=`. Conflicts with `--version`. |
| `--output, -o <FILE>` (alias `--out`) | Output file path. Defaults to `onerom-<board>-<version>.bin`. Conflicts with `--path`. |
| `--path <DIR>` | Output directory, using the default filename. Conflicts with `--output`. |
| `--force, -f` | Continue even if the assembled firmware has parse errors. |

Device required: no.

### firmware inspect

Show a firmware binary's version, board type, MCU, and embedded ROM images and
metadata.

```
onerom firmware inspect --firmware firmware.bin
```

| Option | Description |
|---|---|
| `--firmware <FILE>` (aliases `--fw`, `--in`, `--input`) | Firmware binary to inspect. |
| `--board, -b <BOARD>` | Inspect the release firmware for this board type. Conflicts with `--firmware`. |
| `--version <VERSION>` | Firmware version to inspect. Defaults to latest. Conflicts with `--firmware`. |

### firmware releases

List available firmware releases with supported boards and MCUs.

```
onerom firmware releases
```

| Option | Description |
|---|---|
| `--board, -b <BOARD>` | Show only releases for this board type. |
| `--all, -a` | Show all releases even if a device is attached and detected. Conflicts with `--board`. |

### firmware download

Download the base (ROM-less) firmware binary for a version/board/MCU.

```
onerom firmware download --version 0.6.5 --board fire-24-e --out firmware.bin
```

| Option | Description |
|---|---|
| `--version <VERSION>` | Version to download. Defaults to latest. |
| `--board, -b <BOARD>` | Target board type. Inferred from device if omitted. |
| `--output, -o <FILE>` (alias `--out`) | Output file path. Defaults to `onerom_<board>_<version>.bin`. Conflicts with `--path`. |
| `--path <DIR>` | Output directory, using the default filename. Conflicts with `--output`. |

### firmware chips

List supported chip types for a board, or all chip types grouped by pin count.
Identical to the top-level [`chips`](#chips).

```
onerom firmware chips --board fire-24-e
onerom firmware chips --all
```

| Option | Description |
|---|---|
| `--board, -b <BOARD>` | Show supported chips for this board. Conflicts with `--all`. |
| `--all, -a` | Show all chips grouped by pin count. Conflicts with `--board`. |

---

## plugin

List available plugins from the release manifest, with versions and minimum
firmware requirements. Without a connected device or `--fw-version`, minimum
firmware requirements are shown for reference; with either, incompatible plugins
are flagged.

```
onerom plugin
onerom plugin --all-versions
onerom plugin --type system
onerom plugin --fw-version 0.6.6
```

| Option | Description |
|---|---|
| `--all-versions, -a` | Show all versions of each plugin, not just the latest. |
| `--type, -t <TYPE>` | Filter by plugin type: `system` or `user`. |
| `--fw-version <VERSION>` | Firmware version to check compatibility against. |

Device required: no.

---

## chips

List supported chip types — for a board, or all grouped by pin count. Top-level
alias for [`firmware chips`](#firmware-chips).

```
onerom chips --board fire-24-e
onerom chips --all
```

| Option | Description |
|---|---|
| `--board, -b <BOARD>` | Show supported chips for this board. Conflicts with `--all`. |
| `--all, -a` | Show all chips grouped by pin count. Conflicts with `--board`. |

Example output (illustrative — your build may differ):

```
Supported 24-pin chips:
  2016, 2316, 2332, 2364, 2704, 2708, 2716, 2732, 27C32, 28C16, 4732, 4764, ...
Supported 28-pin chips:
  231024, 23128, 23256, 23512, 23C1000, 23QL384, 23QL512, 27128, 27256, ...
Supported 32-pin chips:
  23C1001, 23C1010, 27C010, 27C020, 27C040, 29F010, 39SF010, SST39SF040, ...
Supported 40-pin chips:
  23C4100, 27C200, 27C400, 27C4100, AT27C400, HN62402, M27C400, MX23C4100, ...
```

Device required: no.

---

## boards

List supported One ROM board types. No options.

```
onerom boards
```

Example output (illustrative — your build may differ):

```
Supported One ROM board types:
  fire-24-a, fire-24-c, fire-24-d, fire-24-e, fire-24-f, fire-24-usb-b,
  fire-28-a, fire-28-b, fire-28-c, fire-32-a, fire-32-b, fire-40-a, fire-40-b,
  ice-24-d, ice-24-e, ice-24-f, ice-24-g, ice-24-i, ice-24-j, ice-24-usb-h,
  ice-28-a
```

Device required: no.

---

## Top-level aliases

Convenience aliases for frequently used nested commands. They take the same
options as their targets.

### peek (top-level alias)

Alias for [`inspect peek live`](#inspect-peek-live).

```
onerom peek live --address 0x100 --length 64
```

### poke (top-level alias)

Alias for [`control poke live`](#control-poke-live).

```
onerom poke live --address 0x100 --input patch.bin
```

### reboot (top-level alias)

Alias for [`control reboot`](#control-reboot).

```
onerom reboot
```

---

## ROM slot specification

Used by `--slot` in [`program`](#program) and [`firmware build`](#firmware-build).
Repeat `--slot` once per slot. Comma-separated `key=value` pairs:

```
file=<path_or_url>,type=<romtype>[,cs1=<logic>][,cs2=<logic>][,cs3=<logic>]
    [,size_handling=<handling>][,cpu-freq=<freq>][,cpu-vreg=<voltage>]
    [,led=<bool>][,force_16bit=<bool>]
```

| Key | Values / notes |
|---|---|
| `file` | Local path or URL to the ROM image. |
| `type` | Chip type (see [`chips`](#chips)), e.g. `2364`, `2332`, `2716`, `27C400`. |
| `cs1`, `cs2`, `cs3` | CS polarity: `active_low` (or `0`), `active_high` (or `1`). Which lines are required depends on the chip type (e.g. `2332` requires `cs1` and `cs2`). |
| `size_handling` (alias `size`) | `none`, `duplicate` (or `dup`), `truncate` (or `trunc`), `pad`. |
| `cpu-freq` | e.g. `150`, `150mhz`, `150MHz`. Values above 150 MHz require confirmation (suppressed by `--yes`) and set overclock automatically. |
| `cpu-vreg` | e.g. `1.1`, `1.10`, `1.10v`, `1.10V`. Values above 1.10 V require confirmation (suppressed by `--yes`). Must be a supported level. |
| `led` | Boolean: `on`/`off`, `true`/`false`, `1`/`0`. |
| `force_16bit` | Boolean (as above). Valid only on 40-pin boards. |

Examples:

```
--slot file=kernal.bin,type=2364,cs1=active_low
--slot file=chargen.bin,type=2332,cs1=active_low,cs2=active_high
--slot file=https://example.com/basic.bin,type=2716
--slot file=small.bin,type=2364,cs1=active_low,size_handling=duplicate
--slot file=kernal.bin,type=2364,cs1=active_low,cpu-freq=200MHz,cpu-vreg=1.2V
--slot file=char.bin,type=2332,cs1=active_low,cs2=active_high,led=off
--slot file=amiga.bin,type=27C400,force_16bit=true
--slot file=undersized.bin,type=2732,size=pad
--slot file=oversized.bin,type=2732,size=trunc
--slot file=halfsized.bin,type=2732,size=dup
```

## Plugin specification

Used by `--plugin` in [`program`](#program) and [`firmware build`](#firmware-build).
At most one system plugin and one user plugin; a user plugin requires a system
plugin. The system plugin is placed in slot 0, the user plugin in slot 1.

| Form | Meaning |
|---|---|
| `--plugin usb` | Latest compatible version, by name. |
| `--plugin system/usb` | With explicit type (`system` or `user`). |
| `--plugin usb,version=0.1.0` | Pinned version. |
| `--plugin file=path/to/plugin.bin` | Local file. |
| `--plugin file=https://example.com/plugin.bin` | Remote file. |