# Scripts

Helper scripts for testing and debugging.

## onerom.sh

Builds a blank firmware image, adds the chip metadata, adds the ROM/RAM images, and optionally flashes the firmware to a connected OneROM device.

Example, to build a C64 set of ROMs for Fire 24 D:

```bash
scripts/onerom.sh fire-24-d onerom-config/c64.json
```

Example, to build a C64 set of ROMs for Ice 24 J, MCU STM32F411RE, with logging and debug logging, and flash it:

```bash
scripts/onerom.sh -d -l -f ice-24-j f411re onerom-config/c64.json
```

## build-empty-fw.sh

Creates a blank firmware image for the OneROM device, with no metadata and no ROM (or other chip) images or types.

Example:

```bash
scripts/build-empty-fw.sh -d -l sdrr/build/sdrr-rp2350.bin /tmp/
```

## add.sh

Takes a blank firmware image, adds the chip metadata to it.

Example:

```bash
scripts/add.sh sdrr/build/sdrr-rp2350.bin /tmp/onerom-fw rom-config/c64.json
```

Creates:

- `/tmp/onerom-fw.bin` - the firmware image with the ROM metadata added
- `/tmp/onerom-fw.elf` - the ELF file with symbols for debugging
