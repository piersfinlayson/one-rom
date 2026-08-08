# One ROM Lab

Alternate firmware for a One ROM **Fire** (RP2350) board that turns its ROM
socket into a parallel bus reader and tester.  Flash it to a spare board, connect
over USB, and drive it from an interactive shell.

Two things it is used for:

- **Reading real chips** — mask ROMs, EPROMs and EEPROMs sitting in the socket,
  output as a checksum, a hex dump or Intel HEX.
- **Testing a One ROM under test** — checking it serves the right bytes at speed
  and tristates its data lines when deselected.  This is how Fire 40 boards are
  tested before shipping.

Fire only.  Lab refuses any board whose MCU is not an RP2350.

## Build and flash

The board is put into BOOTSEL, then `picotool` loads over USB:

```bash
scripts/flash.sh                # board set at runtime, with B:<board>
scripts/flash.sh fire-40-a      # bake in a default board
```

The argument is a **board** name, not a chip type.  A single binary reads every
chip type the board supports, so the only reason to bake in a board is
convenience.

The `z` command reboots back into BOOTSEL, so reflashing never needs the button.

## Connect

Lab presents a USB CDC serial port.  Connect at 115200:

```bash
scripts/serial.sh /dev/cu.usbmodem1103
```

Nothing is printed until you press Enter — input is discarded until then, so a
terminal opening mid-boot cannot leave you looking at a blank screen wondering.
You then get a `> ` prompt, with line editing, arrow keys and a 16 entry command
history.

## Commands

Each command is a single letter, optionally followed by colon-separated
arguments.  Give the command alone and it prompts for what it needs, remembering
your last answer.

```text
  B   Set One ROM Lab board type            B:<board>
  r   Read ROM                              r[:<chip>[:<start>[:<len>[:<fmt>[:<cs1>[:<cs2>[:<cs3>]]]]]]]
  b   Batch ROM read                        b[:<chip>[:<start>[:<len>[:<fmt>[:<secs>[:<cs1>[:<cs2>[:<cs3>]]]]]]]]
  i   Chip type information                 i[:<chip>]
  c   Set or change chip type               c:<chip>[:<cs1>[:<cs2>[:<cs3>]]]
  f   Set or change output format
  t   Toggle tri-state testing during checksum mode on and off
  q   Quick read (uses default chip, range and format)
  l   List chips supported by this board type
  v   Display One ROM Lab version and hardware information
  s   Display settings
  p   Show board pin map (socket pin -> GPIO)   p[:<chip>]
  T   List supported board types
  z   Reset to bootloader
  ?/h This help
```

Formats are `cs` (checksum and SHA1, the default), `hex` (hex dump) and `ihex`
(Intel HEX).  Addresses are decimal unless prefixed `0x`, `0X` or `$`, and
`len=0` means to the end of the ROM.

Commands are case sensitive — `B` sets the board, `b` starts a batch read.

## Chip select polarity

Mask ROM chip selects are mask-programmed, so an unmarked chip may need any
combination.  Pass `0` for active low, `1` for active high, or **`?` to
auto-detect**:

```text
r:2364:0:0:cs:?
```

Lab then reads the chip once per combination and flags the ones that produced
something other than all `0x00` or all `0xFF` as `*** candidate ***`.

## Tri-state testing

In checksum mode, with `t` enabled, Lab drives each of `/OE` and `/CE` high
independently and checks the data lines float — detected via the reader board's
internal pull-downs.  The timing here is deliberately relaxed, to cope with weak
pulls and the capacitance of the test setup.  Failures should be zero.

For a 16-bit capable chip, Lab reads in both 8-bit and 16-bit modes and both the
SHA1 and the 32-bit summing checksum should match.

## Pin map

`p` prints the board's socket pin to signal to GPIO mapping, including the X
header, which is the fastest way to work out what is wired where when debugging
hardware.  On Fire 32 and 40 boards, where socket pins are twinned across two
GPIOs, `p` deliberately shows both even though the reader drives only the first.
