# External flash

## Hardware

Some Fire boards have a footprint for a second flash chip, including 32 and 40 pins boards. See `external_flash` in `rust/config/json/`, which includes the chip select GPIO.

There are two build options, chosen at fab time by which 0R resistor is populated:
1. External flash CS connects to a GPIO, allowing its use as CS1.
2. External flash CS connects to the main chip select, making the chip the boot flash for an RP2350B (with no flash).

The RP2350 CS1 is available on GPIO 0, 8, 19 and 47. All current 32/40 pin boards use GPIO 47.

This document covers using the external flash as additional storage, allowing One ROM to store more ROM images.

## Tested

Tested on a fire-40-a - RP2354B with a W25Q16JVSS on CS1, at 150MHz using `plugins/user/ext-flash`.

The external flash can be configured, read, erased, written and can be used to execute code stored.

Supported erase and program methods:

| Operation | Method | Address |
| --- | --- | --- |
| Erase | bootrom `flash_op` | `0x11000000` |
| Erase | bootrom `flash_range_erase` | `0x1000000` |
| Program | QSPI, driven by the plugin | device offset |
| Program | bootrom `flash_op` | `0x11000000` |
| Program | bootrom `flash_range_program` | `0x1000000` |

Notes:

- QMI addresses are 24 bits capping external flash at 16MB.
- The external flash is always located at 0x11000000.
- The low-level calls take an offset from flash start, so 0x1000000 to access external flash.
- Reads work at every clock divisor from 6 to 1 - 25MHz to 150MHz.
- Both chips have QE set so quad reads work untouched.
- The bootrom muxes the CS1 pin before launching the firmware which currently then unsets it in setup_initial_gpios.
- Bootrom flash calls require `FLASH_DEVINFO`:
  - `flash_op` requires external flash's size
  - both require the CS1 pin via `connect_internal_flash`.
  The plugin writes the bootrom's RAM copy so no OTP update is needed.

## Changes required

| Area | Change |
| --- | --- |
| Firmware | Set M1's clock divisor alongside M0's. |
| Firmware | If an image spans both chips, chain a second DMA transfer when loading ROM image to RAM. |
| Firmware | Handle image spanning both chips when loaded via ORA APIs. |
| Firmware | Skip the CS1 pin in setup_initial_gpios if using OTP as the bootrom has already muxed it. |
| Config | Record external flash size in JSON board data, move flash size from MCU to Board, and add external flash as separate item. |
| Metadata | Add external flash configuration. |
| Generator | Allocate images across both flash chips, using correct external flash offset. |
| Programmer | Split the built image at the flash boundary and program appropriately. |
| CLI/Studio/Web | Support programming and reading both flash chips. |

## Host programming without OTP

The bootloader reads external flash's size and CS pin from a 16-bit `flash_devinfo` word in boot RAM on every operation. The bootrom sets that word at boot a default (`varm_boot_path.c:721-728`) or OTP if present.

Picotool and the boot ROM do not allow the bootrom's RAM to be modified via picoboot directly.  However, SWD can be used to set it.

1. Enter BOOTSEL, then attach the probe.
2. Find the word via the bootrom's `flash_devinfo16_ptr` entry, which gives a ROM address holding the pointer. On the A4 bootrom it is entry `0x48` word `0x400E033C`.
3. Use a halfword write.
4. Value is `CS1_SIZE<<12 | CS0_SIZE<<8 | D8H<<7 | CS1_GPIO`. Internal flash of 2MB, with 2MB external on GPIO 47, with D8h erase on both devices, is `0x99AF`. The default is `0x0C00`.
5. You must manually configure the CS1 pin over SWD - the bootrom only does this at boot, if OTP is set. Set its GPIO control to function 9 (QMI CS1n), and clear the isolation and pull-down bits in its pad register. For GPIO 47 that is `0x4002817C = 9` and `0x400380C0 = 0x52`.
6. Read, erase and program at `0x11000000`.

Do not halt the device during this procedure.

There are two picotool problems:
1. `load` pads to the 4K erase boundary with zeros, wiping the rest of the sector.
2. `load` rejects the external flash region due to [issue #360](https://github.com/raspberrypi/picotool/issues/360).

## OTP

To configure external flash support within OTP, the following fields must be set:
- `FLASH_DEVINFO` is row `0x054`, fields `CS1_SIZE`, `CS1_GPIO`, `D8H_ERASE_SUPPORTED`, `CS0_SIZE`.  All must be correctly set. 
- `FLASH_DEVINFO_ENABLE` is bit 5 of `BOOT_FLAGS0` row `0x048`.

See the RP2350 datasheet, section 13.10.

## Programming OTP

Instructions for programming:
- fire-40-a
- RP2354B
- 2MB W25Q16JVSS external flash on GPIO 47

`FLASH_DEVINFO` is `0x99AF`: 
- `CS1_SIZE` bits 15:12 = 9
- `CS0_SIZE` bits 11:8 = 9
- `D8H_ERASE_SUPPORTED` bit 7 = 1
- `CS1_GPIO` bits 5:0 = 47

`BOOT_FLAGS0`, `_R1` and `_R2` are `0x048`, `0x049`, `0x04a`
- `FLASH_DEVINFO_ENABLE` bit 5

`plugins/user/ext-flash/scripts/program-otp.sh` describes the required sequence.

Notes:

- A non-zero existing `0x054` row value means the ECC row has been written, and picotool refuses it.
- `0x054` and `0x055` form an ECC pair. Once `FLASH_DEVINFO_ENABLE` is set the boot path does a guarded read of `0x054`, which fails unless `0x055` also holds a valid ECC codeword. The chip then halts on every boot, and OTP cannot be rewritten (RP2350-E17). An unwritten row reads `0x000000` and is valid. If `0x055` reads anything else, run `picotool otp get 0x055`, which decodes the row and warns on a bad codeword. Do not set `FLASH_DEVINFO_ENABLE` if it warns.
- The write to `0x048` sets just bit 5.
- `0x054` must read back correctly before `FLASH_DEVINFO_ENABLE` is set. The boot path guarded-reads `0x054`, and an invalid row halts the chip.
