# One ROM Lab

Alternate firmware for a One ROM Fire board that reads and tests a chip
connected to it. It is driven from a serial console over USB.

It reads:

- mask ROMs, EPROMs and EEPROMs, outputting
  - a checksum
  - a hex dump
  - Intel HEX
  - Motorola S-record
- a One ROM, to check it serves the right bytes and tristates its data lines
  when deselected.

## Build and flash

`scripts/flash.sh` builds Lab and loads it with `picotool`. A board running Lab
or One ROM with the USB plugin is rebooted into its bootloader by the script.
Hold BOOTSEL while connecting any other board.

```bash
scripts/flash.sh                # board set at runtime with B:<board>
scripts/flash.sh fire-40-a      # board built in
```

The argument is the Lab board's type.

## picotool

Lab is compatible with `picotool` while running.

To print Lab's version and the RP2350's chip ID, package and revision:

```bash
picotool info -a --vid 0x1209 --pid 0xf542
```

```text
Program Information
 name:          One ROM Lab
 version:       0.4.0
 description:   Use One ROM as a ROM reader
```

To reboot Lab into the bootloader and flash it, including when the serial
console stops working:

```bash
picotool reboot -u --vid 0x1209 --pid 0xf542
picotool load -t elf ../target/thumbv8m.main-none-eabihf/release/onerom-lab-fire
```

A commissioned board's bootloader uses One ROM's VID and PID so add
`--vid 0x1209 --pid 0xf540` to `load`.

With more than one One ROM connected, add `--ser <serial>` to select one.

## Connect

Lab provides a USB serial port. Connect at 115200 baud:

```bash
scripts/serial.sh /dev/cu.usbmodem1103
```

Printed when connected:

```text
----- One ROM Lab -----
One ROM Lab fire-40-a v0.4.0
Serial: 62CD9AE3C0771A7E
-----------------------
Type ? for help.
```

The board type is printed once a board is set. The `> ` prompt supports
line editing and command history with the arrow keys.

Settings are kept across reconnects.

If your terminal doesn't set DTR, press Enter to display the greeting.

## Commands

A command is a single letter, optionally followed by colon-separated arguments.
Any argument left out is prompted for, with the last value used as the default.
Commands are case sensitive.

| Command | Arguments | Description |
|---|---|---|
| `B` | `B:<board>` | Set the board type |
| `T` | | List board types |
| `c` | `c:<chip>:<cs1>:<cs2>:<cs3>` | Set the chip type and chip select polarities |
| `l` | | List chip types for the board |
| `i` | `i:<chip>` | Display chip type details |
| `f` | `f:<fmt>` | Set the output format |
| `t` | | Turn tri-state testing on or off |
| `r` | `r:<chip>:<start>:<len>:<fmt>:<cs1>:<cs2>:<cs3>` | Read the chip |
| `q` | | Read the chip with the current settings |
| `b` | `b:<chip>:<start>:<len>:<fmt>:<secs>:<cs1>:<cs2>:<cs3>` | Read the chip every `<secs>` seconds |
| `x` | `x:<chip>:<cycles>:<cs1>:<cs2>:<cs3>` | Read the chip repeatedly and compare the passes |
| `d` | `d:<chip>:<lines>:<cycles>` | Drive the data lines as a square wave, with the chip deselected |
| `R` | | Query a One ROM over RBCP |
| `a` | `a:<chip>:<start>:<len>:<fmt>:<secs>:<cs1>:<cs2>:<cs3>` | Read the chip and query the One ROM every `<secs>` seconds |
| `p` | `p:<chip>` | Display the GPIO wired to each socket pin and X header pin |
| `s` | | Display settings |
| `v` | | Display the version and serial number |
| `z` | | Reboot into the bootloader |
| `?` or `h` | | Display help |

A key or a break stops `b`, `x`, `d` and `a`.

## Reading a chip

```text
> r:27256:0:0:cs
Reading 27256, range 0x0..end, output checksum

  SHA1: 4a8a2c6ee8829e8ae3cee645f0875e9edc197e83  checksum: 0x003FEF19
  tristate failures: 0
```

The formats are:

- `cs` - checksum and SHA1, the default
- `hex` - hex dump
- `ihex` - Intel HEX
- `srec` - Motorola S-record

Addresses are decimal unless prefixed `0x`, `0X` or `$`. A length of 0 means
the rest of the chip.

A 16-bit chip is read in 8-bit and 16-bit modes. `Match:` is `true` when the
two reads are the same.

### Chip select polarity

For a mask ROM's chip selects, enter `0` for active low, `1` for active high or
`?` to try every combination:

```text
r:2364:0:0:cs:?
```

Lab reads the chip with each combination. A combination whose data isn't all
`0x00` or all `0xFF` is printed with `*** candidate ***`.

## Testing a One ROM

### Tri-state

During a checksum read Lab also checks that the chip stops driving its data
lines when deselected. `tristate failures` is 0 for a chip that passes.

### Fast read

`x` reads the chip repeatedly and compares every pass with the first.

```text
Fast reading 27256, read delay 16 cycles. Press any key to stop.
  Reference SHA1: 4a8a2c6ee8829e8ae3cee645f0875e9edc197e83
  1.0s  passes 80  reads 2.6M  2.53MHz  mismatches 0
```

Lab prints a status line each second, ending with the mismatches on each data
line that has any, for example `D1 3  D2 1`. Lab lists the first mismatches
when `x` stops.

The read delay is the number of 150MHz CPU cycles between setting an address
and reading the data. If the reference SHA1 differs from an `r` read of the same
chip, the delay is too short.

`x` reads chips up to 64KB.

### Data line drive

`d` drives data lines as a square wave with every control line deasserted, so
the chip keeps them as inputs. `<lines>` is a mask with D0 in bit 0, or `all`
for all data lines supported by the chip. A hex mask takes a `0x` or `$` prefix.

```text
Driving D0 D1 D2 D3 D4 D5 D6 D7, delay 0 cycles. Press any key to stop.
  1.0s  toggles 11.5M  5.65MHz
```

The delay is the number of 150MHz CPU cycles between toggles. Lab drives the
lines at 8mA with fast slew.

### Querying

`R` queries a One ROM running the [host-control](/plugins/user/host-control)
plugin over [RBCP](https://github.com/piersfinlayson/rom-bus-control-protocol).
Set the chip type and chip select polarities with `c` first.

```text
Device:       One ROM
Board type:   fire-24-usb-b
Board size:   M
Firmware:     v0.8.1
RBCP:         0.1.4
Boot:         flash slot 0, RAM slot 0
Chip ID:      E126C9F97C10ADAC
Commissioned: yes
  Board type:   fire-24-usb-b
  Revision:     A1
  Manufacturer: piers.rocks
  Date:         2026-09-29
  Batch:        Y1-2345
  Signing key:  piers.rocks Production (1)
  Signature:    verified
```

Revision and batch are displayed where the One ROM was commissioned with them.
Where a One ROM's size differs from the size it was commissioned as,
`Board size` displays both, for example `L (commissioned as M)`.

## Releasing

Lab is released separately from One ROM, with tags `lab-vX.Y.Z`.

1. Update the version in [Cargo.toml](Cargo.toml) and the
    [CHANGELOG](CHANGELOG.md), then commit.

2. Tag the commit and push both:

    ```bash
    git tag -s -a lab-vX.Y.Z -m "One ROM Lab vX.Y.Z"
    ```

3. Pushing the tag runs [lab-release.yml](/.github/workflows/lab-release.yml).
    It builds and checks every image and creates the GitHub release, with the
    version's CHANGELOG section as the release notes.

4. Add the release to the images repo:

    ```bash
    scripts/release.py --version X.Y.Z --output-dir ../../../one-rom-images
    ```

    The script downloads the release to `dist` and adds it to
    `lab/releases.json`.

5. Flash an image from `dist` onto a board. Check that `onerom scan` lists it as
    One ROM Lab with the new version and board.

6. Commit and push `one-rom-images`.

7. In one commit, copy `rust/lab-metadata/metadata_schema.toml` over
    `rust/lab-metadata/metadata_schema_released.toml` and raise `[schema]
    firmware_release` in `metadata_schema.toml` to the next version.

`scripts/build-release.sh --version X.Y.Z` builds and checks the same images
locally.
