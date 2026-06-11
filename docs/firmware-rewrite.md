# v0.7.0 Firmware Rewrite Plans

This is a temporary document intended to capture plans and open issues for rewriting the core firmware, likely released as v0.7.0.

## Overview

The catalyst for the rewrite is to collapse the current complexity around how different image types (with incompatible pin-outs) are served.  This complexity is currently spread between both the firmware and the pre-processor (as well as the two different regression test mechanisms).

The rewrite is expected to capture that complexity in the pre-processor only, with the firmware having a small number of different PIO serving algorithms,where there are multiple algorithms for each of CS handling, byte serving, and address reading.  One of each of these is selected by the pre-processor for a particular ROM, via the metadata, based on the ROM type.  Each serving algorithm has its own set of arguments that it takes (like base address pin and number of address pins), which the pre-processor populates.

The ROM type itself is not be "known" to the firmware, although it is stored in the metadata as a human readable string (based on the actual alias used to program One ROM), so that the firmware can be usefully queried.

Metadata for a particular ROM type is only included once, even if there are multiple ROM images of that type on the firmware (i.e. the metadata for each points to a single instance) to save flash space, as metadata needs to continue to fit into 16KB.

Ideally, the complexity - that is the different algorithms and their parameters required for each ROM type on each board - would be inferred from the chip type and board (JSON) metadata.  This is possible to some extend, much as the existing pre-processor and firmware infers behaviour like contiguous or non-contiguous CS pins.  However, it isn't entirely possible (hence handle_snowflake_chip_types() in the pre-processor), so there is expected to be a match, switch or table driven approach taken, using the information documented below in [ROM Configuration](#rom-configuration).

## Build Variants

It is expected to collapse down to a single firmware binary for all hardware revisions.  This can be achieved by building for the RP2350B (assuming the larger pin counts).  The only issue this gives is that ADC pins are different GPIOs, but that is not an issue as the firmware doesn't use ADC.  Any plugin that wishes to use ADC can query the RP2350A/B variant using the standard mechanism.

## Deprecated One ROM Hardware Revisions

Ice support and non-PIO Fire support are expected to be dropped in this rewrite.  Ice continues to be supported via the 0.6.* firmware but isn't expected to be enhanced in future.

The only requirement for non-PIO Fire support is for boards fire-24-a/b when serving multi-ROM sets, as PIO + multi-ROM set serving are incompatible on these revisions.  This continues to be supported by 0.6.* firmware, but won't be enhanced in future.

## Language Choice

Consideration has been given to moving from C to Rust for the core firmware.  However, C was chosen for the rewrite for a number of reasons:
- Make the firmware more approachable by a wider group of people.
- Avoid needing to rewrite the extensive plugin API and supporting framework in Rust, as well as requiring laying the existing C plugin API on top of Rust (as C is much more likely to be used for plugins).

Rust remains the choice for the pre-processor.  It continues to be available as Rust crates and WASM libraries for integration into different tools. 

## Testing

One ROM's automated testing consists of:
- `make test` - checks with independent code that the ROM image as included in firmware is mangled as expected
- `make test-pio` - emulates the One ROM firmware and included metadata + ROM images, to ensure every byte is served as expected, and data lines are not driven when they shouldn't be
- `rust/lab-new` - runs on One ROM hardware and reads attached One ROMs (and real ROMs, EPROMs and EEPROMs) to ensure they read correctly 

As part of the firmware rewrite all these tests are retained and used to validate the new firmware, for all supported ROM types, including CS configurations.

## Open issues

- RBCP currently exposes the ROM type to the host.  If the firmware doesn't know the ROM type, we either need to remove this from RBCP, or, perhaps more usefully, include an RBCP ROM type identifier in the metadata, which the firmware just dumbly serves via RBCP.

- It is expected that we'll collapse down to a single metadata/image generator with this firmware, removing the support in `sdrr-gen` to generate a firmware with it all included in one shot.  This means that
  - We have to find a way to include the metadata/ROM images in the `make test-pio` type testing.
  - We still need a way to generate the ROM image on its own and perform the `make test` type testing, to ensure the ROM image is mangled as appropriate.
  It is currently unknown how these will be achieved.

## Detailed To-Dos

- Add 23C1001 support to firmware and test it (fire-32-b only).
- fire-28-c Add multi-ROM sets and dynamic bank switching.
- Add method to get RP2350A/B from config::hw::Board, and use in generator.rs instead of hard-coding.
- fire-28-c optimise image sizes
- fire-32-b optimise image sizes
- In general we are missing any ability in the core firmware (and also elsewhere) to support specific ROM types only on certain revisions of different pin boards.  This likely needs somworking through.
- Make 27C301 use less flash on fire-32-b (currently using 512KB, 256KB should be possible)
- config/hw generator is subtracting 8 from 28 pin board address lines, but should be 10 for fire-28-a - should be avoiding the X pins as well, better than just hardcoding the fire-28-c type
- Add concept of don't care for CS line (23C1001, HN62402, HN62302)
- Change HN62402 to be 16 bit only and HN62302 to be 8 bit only.

## Algorithms

All pin numbers are given with respect to the GPIO_BASE value provided.  GPIO_BASE can take either 0 or 16.  16 is used to access GPIOS 32-47, and prevents 0-15 from being accesssed.  The CS and Data algorithms MUST share the same GPIO_BASE value, as the CS (and Data) algorithms both need to access the data pins.  In theory it is possible to image the data pins high and the CS pins low, or vice versa, requiring a differet approach, but this isn't required by current or planned hardware.

GPIO_BASE is always param 1 for all algorithms.

### Chip Select Handling

| ID | Name | Description | Param 1 | Param 2 | Param 3 | Param 4 | Param 5 | Param 6 | Param 7 | Param 8 | Param 9 | Param 10 |
|----|------|-------------|---------|---------|---------|---------|---------|---------|---------|---------|---------|---------|
| CS0 | CS Standard   | The ROM image is served using a contiguous set of CS pins. | GPIO_BASE | Base CS pin | Num CS pins | Which CS pins to hardware invert | Base data pin | Number of data pins (8/16) | Serve when all CS pins are low (0) or when one or more is high (1).  The latter is used for multi-ROM sets | Cycle delay after detecting CS active before setting data lines to outputs | Cycle delay after detecting CS inactive before setting data lines to inputs | /BYTE pin number or 0xFF is not used, indexed from GPIO_BASE |
| CS1 | CS non-contig single gap | Each ROM image is served using a set of CS pins, all of which must be low for the data pins to be set to outputs, but the CS pins can have up to a 1 pin total (i.e. between only one pair of CS lines). | GPIO_BASE | Base CS pin | Num CS pins including gap | Index of CS pin to ignore (1 would be second pin) | Which CS pins to hardware invert | Base data pin | Number of data pins (8/16) | Cycle delay after detecting CS active before setting data lines to outputs | Cycle delay after detecting CS inactive before setting data lines to inputs | n/a |
| CS2 | CS Enable Address Qualified | The ROM image is served when a primary enable pin is asserted AND a set of address qualifier pins do not match the inactive pattern. Used where a single global enable pin (such as /OE or /CE) gates output, with address lines providing bank selection. | GPIO_BASE | Enable pin indexed from GPIO_BASE | Hardware invert enable pin (0=no, 1=yes) | Base address qualifier pin indexed from GPIO_BASE | Num address qualifier pins including any gap pins | Inactive pattern for address qualifier pins (Y preload value) | Base data pin | Number of data pins (8/16) | n/a | n/a |

It is possible to conceive a more general non-contiguous CS algorithm, with any number of pins and gaps, and the PIO would shift through the CS pins testig them one by one, or in contiguous groups.  This would be slow, and isn't currently required.

### Address Reading

| ID | Name | Description | Param 1 | Param 2 | Param 3 | Param 4 | Param 5 | Param 6 |
|----|------|-------------|---------|---------|---------|---------|---------|--------|
| ADDR0 | Address Standard | Reads a complete block of address pins, postpends the value to a RAM lookup prefix and pushes to a DMA chain.  Number of address pins and RAM prefix are inferred from ROM image size. | GPIO_BASE | Number of cycles to wait in between address reads | Base address pin | Total number of address pins to read from | Number of ROM RAM table bits to include | List of pins (indexed from base) to always read 0/1, and which value to read |

### Data Word Serving

| ID | Name | Description | Param 1 | Param 2 | Param 3 | Param 4 |
|----|------|-------------|---------|---------|---------|---------|
| DATA0 | Data Word Standard | Reads a data word from the TX FIFO and applies is to the data pins | GPIO_BASE | Base data pin | Word size in bits (8 or 16) | n/a |
| DATA1 | Data Word with Byte Mode | Reads a data word from the TX FIFO and applies is to the data pins, but if a /BYTE pin is active, only applies the appropriate byte (and ignores the upper byte) | GPIO_BASE | Base data pin | /BYTE pin number (indexed from GPIO_BASE) | A-1 pin |

## ROM Configuration

This section describes how every ROM type is handled by every hardware variant that supports it, including:
- which algorithms are used
- how much space the image takes on flash
- notes on implementing the pre-processor support.

In general, where there are extensive notes, this indicates custom hand-coded function required in the pre-processor.

Some notes on how the information is represented:
- All boards referred to below are Fire boards.
- CS01 indicates CS0 or CS1 is chosen automatically based of contiguous or non-contiguous set of CS pins.  In some cases (such as where a single CS line is used, e.g. 231024) the answer is obvious (contiguous), but the decision is expected to be made by the pre-processor automatically (without it being hand-coded for that type).

THIS SECTION IS NOT YET COMPLETE.  There are ROM types using all the above algorithms.

| ROM Type | ROM pins | ROM Size | Flash Size | Board     | Algorithms | Notes |
|----------|----------|----------|------------|-----------|------------|-------|
| 2316     | 24 | 2KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and all CS pins.  CS contiguity and hence CS algorithm auto-detected |
| 2332     | 24 | 4KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | See 2316 |
| 2364     | 24 | 8KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and CS pins |
| 23128    | 28 | 16KB     | 64KB       | 28-a/b/c | CS01/ADDR0/DATA0 | Address space A0-15 only.  CS contiguity and hence CS algorithm auto-detected |
| 23256    | 28 | 32KB     | 64KB       | 28-a/b/c | CS01/ADDR0/DATA0 | See 23128 |
| 23512    | 28 | 64KB     | 64KB       | 28-a/b/c | CS01/ADDR0/DATA0 | See 23128 |
| 231024   | 28 | 128KB    | 256KB      | 28-a/b   | CS01/ADDR0/DATA0 | Address space includes A0-15, /OE(A16) and /CE |
| 231024   | 28 | 128KB    | 128KB      | 28-c     | CS01/ADDR0/DATA0 | Address space includes A0-15 and /OE(A16) but not /CE |
| 2704     | 24 | 0.5KB    | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and all CS pins.  CS contiguity and hence CS algorithm auto-detected |
| 2708     | 24 | 1KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and all CS pins.  CS contiguity and hence CS algorithm auto-detected |
| 2716     | 24 | 2KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and all CS pins.  CS contiguity and hence CS algorithm auto-detected |
| 2732     | 24 | 4KB      | 64KB       | 24-a-e | CS01/ADDR0/DATA0 | Address space includes X pins and all CS pins.  CS contiguity and hence CS algorithm auto-detected |
| 2764     | 28 | 8KB      | 64KB       | 28-a/b/c | CS01/ADDR0/DATA0 | Address space A0-15 only.  CS contiguity and hence CS algorithm auto-detected |
| 27128    | 28 | 16KB      | 64KB       | 28-a/b/c | See 2764 |
| 27256    | 28 | 32KB      | 64KB       | 28-a/b/c | See 2764 |
| 27512    | 28 | 32KB      | 64KB       | 28-a/b/c | See 2764 |
| 27C301   | 32 | 128KB     | 512KB      | 32-a | CS01/ADDR0/DATA0 | Requires swapping A16 and /CE. |
| 27C301   | 32 |128KB      | 256KB      | 32-b | CS0/ADDR0/DATA0 | Shave 1 address bit by starting at A15 and running to /OE/A16. Move away from non-contiguous CS pins by forcing A19 (between A16//OE and /CE) to read 0. Requires same A16//CE swap as on fire-32-a. |

Need to add 24 pin ROMs that are supported in 28 pin boards

## Detail Record of Changes

- Removed build options:
  - MCO/MCO2 (STM32F4 specific)
  - OSC (STM32F4 specific)
  - BOOTLOADER (not required as can pull BOOT0 low instead)
  - DISABLE_PRELOAD_TO_RAM (no longer useful)
  - SERVE_ALG (superceded)
  - DFU_SUPPORTED (STM32F4 specific)
  - COUNT_ROM_ACCESS_FLAG (deprecated, use a plugin)
  - SWD (removed)
  - OVERCLOCK (superceded by slot config)
  - EXCLUDE_METADATA and EM (no longer required)
  - HW_REV (no longer required, dynamically generated)
  - ROM_CONFIGS
  - STATUS_LED
- Removed #defines:
  - MCU_VARIANT
  - MCO2
  - CONFIG
  - MAIN_LOOP_ONE_SHOT
  - DISABLE_CCM
  - C_MAIN_LOOP
  - MCU (no longer required, dynamically generated)
  - DUMB_C_MAIN_LOOP_2_cs
  - RP_USE_CP
  - RP_PIO
  - FORCE_16_bit
  - TIMER_TEST
  - TOGGLE_PA4
  - EXECUTE_FROM_RAM
  - NO_BOOTLOADER
  - STATUS_LED
- Need a way of passing in BOOT_LOGGING, etc #defines

If no metadata (i.e. not even VBUS and status LED pins) drop into bootloader.

Board-level information required in metadata





Hardware properties
