# System USB Plugin

This directory holds the source code for One ROM's USB plugin, which provides USB management of One ROM while it is serving ROMs.  This USB support can be used to live inspect and modify the ROM currently being served, as well as the rest of the One ROM firmware.

The USB stack is implemented as one of One ROM's plugins, allowing users to customize their One ROM configuration (to add or remove USB stack support as desired), and also allowing users to replace the USB with their own, or modify One ROM's - all while using the stock core firmware.

The protocol used over USB is a variant of Raspberry Pi's picoboot protocol, which is normally supported by an RP2350 in BOOTSEL mode.  However, this plugin implements the picoboot protocol while the device is running (i.e. isn't in BOOTSEL mode), and also extends picoboot with additional custom One ROM specific commands, in a method that is compatible with the original picoboot protocol.

The picoboot protocol is specified in the RP2350 datasheet, and this plugin uses the [picobootx](https://github.com/piersfinlayson/picobootx) library to implement and extend picoboot.

The primary tool used to manage live One ROMs is the [One ROM Cli](/rust/cli).

## Pre-requisites

A One ROM [build environment](/INSTALL.md).

One ROM firmware 0.6.7 or later for your One ROM.

## Building

From this directory:

```bash
make
```

Produces:

```bash
$ ls -l build/plugin*
-rwxr-xr-x  1 pdf  staff   23932 Mar 18 08:20 build/plugin_system_usb.bin
-rw-r--r--@ 1 pdf  staff  387695 Mar 18 08:20 build/plugin_system_usb.dis
-rwxr-xr-x  1 pdf  staff   49968 Mar 18 08:20 build/plugin_system_usb.elf
-rw-r--r--@ 1 pdf  staff   64681 Mar 18 08:20 build/plugin_system_usb.map
```

## Using

Config set/slot 0 of your One ROM to use the `plugin_system_usb.bin` file as a system plugin.  Here is a sample config fragment to do that:

```json
{
    "$schema": "https://images.onerom.org/configs/schema.json",
    "version": 1,
    "name": "24-plugin",
    "description": "24 pin plugin example",
    "categories": [],
    "rom_sets": [
        {
            "type": "single",
            "roms": [
                {
                    "description": "System plugin - USB",
                    "file": "/Users/pdf/builds/one-rom-temp/plugins/system/usb/build/plugin_system_usb.bin",
                    "type": "system_plugin",
                    "size_handling": "pad"
                }
            ]
        },
        {
            "type": "single",
            "roms": [
                {
                    "description": "C64 stock kernal",
                    "file": "https://www.zimmers.net/anonftp/pub/cbm/firmware/computers/c64/kernal.901227-03.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }
            ]
        }
    ]
}
```

## How it works

As of firmware 0.6.7 One ROM supports up to 2 plugins.  It launches these after the core byte serving has been started, one plugin on each of the RP2350's cores.

The USB system plugin uses tinyusb to implement a USB device containing:
- a dummy vendor interface (required to provide compatibility with Raspberry Pi's picotool)
- the picobootx vendor interface used to manage One ROM live
- a CDC interface (currently unused).

The majority of the picoboot protocol is supported, and there are also extensions to provide additional functionality:
- Live ROM image reading/writing (using a virtual address located at 0x9000_0000)
- Control of the status LED

Additional function is expected be added in the future.

Currently, updating the firmware, ROM metadata and this USB plugin must be done with One ROM in BOOTSEL ("stopped") mode.  The picoboot REBOOT2 command can be used to reboot a running One ROM with this USB plugin into stopped mode.  ROM images stored on flash can be updated while One ROM is running.

For more information about One ROM's plugin system, see the [plugin API documentation](/firmware/ora/plugin.h).
