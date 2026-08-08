# One ROM Plugins

This folder contains One ROM plugins, which are modular chunks of functionality that can be added to One ROM configuration and run alongside One ROM's core ROM serving.

Plugins are only supported on One ROM Fire boards running the PIO algorithm.  (All Fire One ROMs run the PIO algorithm by default in recent firmwares.)  The plugins run on the RP2350 MCU's two CPU cores.

## Plugin Types

There are two types of plugins:

- **System** - These are included as part of the One ROM project and provide commonly asked for extensions to One ROM's functionality, such as a USB stack than runs while One ROM is serving bytes.

- **User** - These are plugins that are developed by users and are not included as part of the One ROM project.  They can be used to add custom or niche functionality to One ROM.  If you write a user plugin and would like to share it, consider contributing it to the One ROM project.

There are not many restrictions on the differences in functionality allowed between different types of plugins, although each plugin is run from a well-defined (and different) flash location, and the amount and location of RAM available differs between types.

## Usage

In future it is expected builds of common plugins will be made available on a download server and configured using the One ROM Web and Studio tools.

For now, you first need to build it, write a config file that uses it and then build and flash the firmware.

### Building Plugins

See the individual plugin README files for instructions on how to build them.  Typically, you `cd` into the plugin directory and run `make` to build the plugin binary.  This creates a `.bin` file in the plugin's `build` directory, which can then be used in the One ROM config file.

A full One ROM build environment is required to build the plugins.  It can be convenient to use the One ROM build container for this, as described in the [One ROM Build Container README](../ci/docker/README.md).

### Sample Config File

The sample config file below is for a One ROM 24, and configures both system and user plugins.

- The system plugin provides a USB stack with picobootx (extended picoboot) protocol support.  In particular the logical ROM being served by One ROM is exposed over picoboot for live reads and writes from logical address 0x90000000.  This can be accessed by [pico⚡flash](https://picoflash.org), and by the future One ROM CLI tool.

- The user plugin is a simple blinky example that links the status LED at around 1Hz.  It is completely independent from the system plugin, and does not interact with the logical ROM being served by One ROM.

Constraints:

- Only a maximum of one system plugin and one user plugin can be used in a config (as they are built to execute from fixed flash locations and use fixed RAM locations).

- If a user plugin is used, a system plugin must also be used (due to constraints around where the user plugin can be executed from on flash).

- If they are provided, the system plugin must be the first ROM set, and the user plugin must be the second ROM set.

- The plugins must be certain sizes (128KB for a system plugin and 64KB for a user plugin).  Use the `size_handling` field to pad a smaller binary to the correct size.

```json
{
    "$schema": "https://images.onerom.org/configs/schema.json",
    "version": 1,
    "name": "24-plugin",
    "description": "24 pin plugin example",
    "categories": [
        "atari",
        "16-bit"
    ],
    "rom_sets": [
        {
            "type": "single",
            "roms": [
                {
                    "description": "System plugin - USB",
                    "file": "plugins/system/usb/build/plugin_system_usb.bin",
                    "type": "Plugin",
                    "size_handling": "pad"
                }
            ]
        },
        {
            "type": "single",
            "roms": [
                {
                    "description": "User plugin - blinky",
                    "file": "sdrr/ora/examples/blink/build/plugin_user.bin",
                    "type": "Plugin",
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

You should save this off the in the `onerom-config/user/` directory, and then reference it when building the firmware as described below.

### Building and Flashing Firmware

The recommended build scripts are documented in the [Scripts README](../scripts/README.md).  In particular, you can use `scripts/onerom.sh` to build a firmware image that includes the plugins, and optionally flash it to a connected One ROM device using a connected SWD probe:

```bash
scripts/onerom.sh -d -l -f fire-24-e onerom-config/user/24-plugin.json
```

Alternatively, omit the `-f` flag to build it and upload the created `.bin` or `.elf` file to One ROM Web or Studio to flash to the device.

`-l` and `-d` enable logging and debug logging respectively, which can be helpful when developing and testing plugins.

### Testing

A simple way of testing the USB plugin is to run Raspberry Pi's `picotool` on the host One ROM is connected to.  Running `picotool info -a` should return information about the underlying RP2350 MCU even while One ROM is running and serving bytes.

## Writing Plugins

Start with the [One ROM Plugin API](../firmware/ora/api.h) documentation.

Then view the [examples](../firmware/ora/examples/) and the released plugins:
- [The system USB plugin](system/usb/).
- [The user blink plugin](user/blink/).
