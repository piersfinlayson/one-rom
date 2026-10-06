# Second Flash Chip

Describes how One ROM's firmware and tools use a second flash chip.

A board's first flash chip is on QSPI chip select 0 at `0x10000000`. A second
chip is on chip select 1 at `0x11000000` irrespective of the first chip's size.

The bootloader reaches the second chip over USB once programmed in OTP. Firmware
0.8.0 and later leaves chip select 1's pin as the bootloader set it (from OTP)
and sets the second chip's clock divisor to the first chip's.

## Layout

Firmware, metadata and plugins are on the first chip. Plugins run from fixed
addresses there.

Each ROM slot is on one chip. A slot never spans both chips because the
firmware copies a slot with a single DMA transfer or `memcpy`. (This could be
enhanced in future.)

The generator places slots in config order. Each goes on the first chip if it
fits in the space left there, otherwise on the second chip. A slot's position
in the config still sets its image select index.

An image uses the second chip only when its slots don't all fit on the first.
An image on the first chip alone runs on any board.

Metadata holds each slot's data address, so a slot on the second chip has an
address of `0x11000000` or above. Metadata doesn't otherwise record which chips
an image uses.

## Board Size

An image build is for a specific board size:
- M (medium) means 2MB of flash in total
- L (large) means 4MB of flash in total

The tools currently only support L implemented using 2MB flash chips.
See [Board Sizes](OTP.md#board-sizes).

Firmware before 0.8.0 supports only M, so the generator refuses a build for
any other size with that firmware, even when every slot fits on the first chip.
That firmware resets chip select 1's pin at boot and doesn't set the second
chip's clock divisor.

The second chip's address is declared in `rust/metadata/metadata_schema.toml`
and the tools' layout for each board size in `FLASH_LAYOUTS` in
`rust/metadata/src/otp.rs`.

`onerom program` builds for the connected One ROM's board size, or for M when
the firmware it programs is before 0.8.0. The CLI reads the size from runtime
info on a running One ROM and from OTP on a stopped one. A One ROM running
firmware before 0.8.0 doesn't record it in runtime info, so the CLI reads it
from OTP through the running USB plugin.

`onerom firmware build` takes the board size with `--size`. It defaults to M.

Studio builds for the Board Size selected in Create, or for M when the firmware
is pre-v0.8.0. Detect sets Board Size from the One ROM. Loading an image file
in Analyse sets Board Size to the smallest size the image requires.

## Image Files

An image file is a `.bin`. For an image on the first chip alone it holds that
chip's contents, laid out as before. For an image that uses the second chip the
first chip's contents are padded with `0xFF` to the first chip's size, and the
second chip's contents follow.

One ROM's tools refuse an image file when:
- it is longer than the first chip without a slot on the second chip
- a slot's data runs past the end of the file

A host tool reads an image file as two regions. The first starts at
`0x10000000`. The second starts at `0x11000000` and holds everything past the
first chip's size.

Other tools write a `.bin` in one piece from `0x10000000`, so they can't
program an image that uses the second chip. picotool refuses a file larger than
the first chip unless the first chip is blank.

## Programming

An image that uses the second chip is programmed in three steps:
1. Erase the first chip.
2. Erase and write the second chip.
3. Write the first chip.

A run interrupted after step 1 leaves the first chip without firmware, so the
One ROM stays in the bootloader until it is programmed again. It never boots
with its metadata pointing at another image's data on the second chip.

`onerom program`, Studio and the web programmer refuse an image that uses the
second chip for a board without a second chip.

`onerom program --verify` reads back each chip it wrote.

On a board with a second chip:
- `onerom control erase --all` erases every chip.
- `onerom control erase --address` and `onerom inspect peek memory` accept the
  second chip's addresses.

## Firmware

Firmware checks a slot lies within the flash OTP configures before reading it.
On a board without a second chip, a slot on the second chip:
- puts One ROM into limp mode when it is the ROM image to serve
- isn't started when it is a plugin
- makes `ora_copy_flash_slot_to_ram_slot` return `ORA_RESULT_INVALID_SLOT`

## Host Tools

The shared Rust crates build an image in this layout and split it into the
programming steps above. The CLI, Studio and one-rom-wasm use them.

Studio reads the board size as the CLI does, over USB or through a debug probe.
It programs through a debug probe using the same steps as over USB.

The web programmer reads the board size from OTP when the One ROM is stopped
and from runtime info when it runs. It follows the CLI for firmware before
0.8.0.

one-rom-wasm provides each section's position for the web programmer's capacity
bar. The bar shows the flash as one block of the first chip's size plus the
second chip's. Its sections are in flash order. For an image that uses the
second chip, the space left at the end of the first chip is an "Unused"
section. Its tooltip says only a slot up to that size fits there.