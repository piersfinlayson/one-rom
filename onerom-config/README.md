# ROM Configs

This directory contains configuration files that can be used to generate various ROM collections for use with One ROM.

There is a [schema file](schema.json) that describes the structure of these configuration files. You can use this schema to validate your own configuration files or to generate new ones.

If you would like a more human readable version of the schema use a tool like [json-schema.app](https://json-schema.app/view/%23?url=https%3A%2F%2Fimages.onerom.org%2Fconfigs%2Fschema.json), pasting in https://images.onerom.org/configs/schema.json as the URL.

To be precise, these config files are used to generate the __metadata__ that is embedded on One ROM's flash __alongside__ the core firmware, adding:

- ROM images
- (optionally) overrides to stock firmware behaviour, on a per ROM basis.

A config file following this format can be used with [One ROM Studio](https://onerom.org/studio) to generate a complete One ROM image and flash it to your One ROM.

## Local File Names

Where ROM images are on the local filesystem, it is safest to use full file paths.  On Windows, use double backslashes in the file path (e.g. "C:\\\\Users\\\\piers\\\\local-chargen-custom.bin"), or forward slashes (e.g. "c:/Users/piers/local-chargen-custom.bin").

## Minimal Config

This is a minimal config:

```json
{
    "$schema": "https://images.onerom.org/configs/schema.json",
    "version": 1,
    "description": "A minimal ROM config",
    "rom_sets": []
}
```

This produces One ROM image with no ROMs in it. This may be useful for manufacturing purposes - in order to flash and ship One ROM with no ROMs installed.

This is essentially the configuration that is used to generate the base One ROM images that One ROM Studio then adds the ROMs you select to.

## Simple Config

A slightly more advanced config with 2 ROM images:

```json
{
    "$schema": "https://images.onerom.org/configs/schema.json",
    "version": 1,
    "name": "Simple Config",
    "description": "A simple ROM config with 2 ROMs",
    "rom_sets": [
        {
            "type": "single",
            "roms": [
                {
                    "description": "ROM 1",
                    "file": "http://example.com/rom1.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }
            ]
        },
        {
            "type": "single",
            "roms": [
                {
                    "description": "ROM 2",
                    "file": "http://example.com/rom2.bin",
                    "type": "2364",
                    "cs1": "active_low"
                }
            ]
        }
    ]
}
```

As can be seen, this config contains 2 ROM sets, each with a single 2364 mask programmed ROM image, each with the CS line active low.

One ROM Studio reads this config, downloads the 2 ROM images from the specified URLs, and builds a One ROM image containing these 2 ROMs.

The two ROM images are selected, at One ROM boot (power on) time, via the on-board image select jumpers.  The first ROM set is selected when all the jumpers are open.  The second ROM set is selected when the first jumper is closed.

(If you close more jumpers than there are ROM sets, the firmware will select the image indicates by the binary value encoded by the jumpers, modulo the number of installed images.)

## 23 vs 27 Series ROMs

If specifying a 27 series (EP)ROM, instead of a 23 (mask programmed) ROM, the `type` field should be set to the appropriate 27 series type (e.g. "27256" instead of "2364").

In this case, there is no need to specify CS line behaviour, as 27 series ROMs use /CE and /OE (both active low) logic by default.

For more details on the differences between 23 and 27 series ROMs, see the [One ROM Visualizer](https://onerom.org/visualizer).

## Firmware Configuration

The `firmware_overrides` section __within a ROM set__ allows fine-tuning of One ROM hardware behavior for __that specific ROM/set of ROMs__. Each base firmware image has built-in defaults - these overrides change the behavior when serving a particular ROM set.

Any ROM set can have its own `firmware_overrides` section, allowing different ROM sets to have different hardware configurations.  It can also have no `firmware_overrides` section, in which case the default firmware behavior is used.

The `firmware_overrides` fields are optional - only include the fields you wish to override from the defaults.

### Primary Use Cases

1. __Performance requirements__ - Some ROM images (particularly character ROMs) require specific clock speeds or voltage settings to serve reliably in certain host systems
2. __Hardware experimentation__ - Testing different configurations to determine optimal settings for specific host platforms and ROM types
3. __Power/performance/preference__ - Disabling features like LEDs (power saving or user preference) or debug interfaces (performance)

### Ice/Fire Specific Configuration

#### Ice Boards (STM32F4-based)

```json
"firmware_overrides": {
    "ice": {
        "cpu_freq": "72MHz",
        "overclock": false
    }
}
```

Ice boards support frequencies from 1MHz to 450MHz.

Set `overclock: true` for frequencies above the rated maximum for a specific STM32F4 MCU.

#### Fire Boards (RP2350-based)

```json
"firmware_overrides": {
    "fire": {
        "cpu_freq": "300MHz",
        "overclock": true,
        "vreg": "1.20V",
        "serve_mode": "Pio"
    }
}
```

Fire boards support 16MHz to 800MHz in various increments (as defined in the schema). For higher frequencies than 150MHz:

- set `overclock: true`.
- you may need to tune `vreg` (internal voltage regulator) - different RP2350 silicon may need different core voltages for stability at high speeds. One ROM firmware will use its own (conservative) voltage regulator settings for higher clock speeds if this is not specified.

### Other Hardware Settings

```json
"firmware_overrides": {
    "led": {
        "enabled": false
    },
    "swd": {
        "swd_enabled": false
    }
}
```

- __LED__ - Disable to save power or if you don't want a status LED on your ROM.  Note, if present and the device enters "limp mode" (e.g. due to a fault or unrecoverable configuration error), the LED will still blink to indicate an error.
- __SWD__ - Disable the debug interface to reduce bus contention and improve serving performance.  SWD is available for the whole of boot, including boot logging, and is shut off just before serving starts, staying off until the next reset.  Not a debug lockout - BOOTSEL/PICOBOOT are unaffected.

### Example: ROM-Specific Configuration

```json
{
    "rom_sets": [
        {
            "description": "C64 Character ROM - requires high clock speed",
            "firmware_overrides": {
                "ice": {
                    "cpu_freq": "150MHz",
                    "overclock": true
                }
            },
            "roms": [
                {
                    "description": "C64 Character ROM",
                    "file": "http://example.com/c64-char.bin",
                    "type": "2332",
                    "cs1": "active_low",
                    "cs2": "active_high"
                }
            ]
        }
    ]
}
```

### Advanced: PIO Serving Algorithm Parameters

For Fire boards using the PIO serving algorithm, low-level timing can be tuned via `serve_alg_params`. This is primarily for experimentation to determine what settings are required for specific ROM/host combinations.

```json
"firmware_overrides": {
    "serve_alg_params": {
        "params": [254, 0, 2, 0, 0, 0, 254, 255]
    }
}
```

As of firmware 0.6.0, the parameter array format is 8 bytes long, as follows:

- Byte 0: `0xFE` (signature)
- Byte 1: `addr_read_irq` (0=disabled, 1=enabled) - whether to use IRQ to trigger address reads
- Byte 2: `addr_read_delay` (0-31) - PIO cycles to delay between address reads
- Byte 3: `cs_active_delay` (0-31) - PIO cycles to wait after CS active before setting data pins to outputs
- Byte 4: `cs_inactive_delay` (0-31) - PIO cycles to hold data as outputs after CS goes inactive
- Byte 5: `no_dma` (0=use DMA, 1=CPU) - whether to use DMA or CPU for byte serving
- Byte 6: `0xFE` (end signature)
- Byte 7: `0xFF` (padding)

These parameters adjust the PIO state machine timing for specific ROM types or host systems. At 150MHz, each PIO cycle is ~6.67ns. See the PIO implementation source code for detailed timing analysis and pre-defined configurations.

It is likely that in future firmware versions, more PIO settings will be exposed via the config file.

### Firmware Defaults

As of 0.6.0 the following firmware defaults are used if no overrides are specified:

- Ice Clock: F401=84MHz, F411=100MHz, F405=168MHz, F446=180MHz, no overclocking.
- Fire Clock: 150MHz, overclocking enabled, VREG 1.10V.
- LED: Enabled (if hardware support is present)
- SWD: Enabled (if hardware support is present)
- PIO Serve Algorithm Params: [254, 0, 2, 0, 0, 0, 254, 255]

### Error Handling

Where possible, One ROM attempts to gracefully recover from incorrect or invalid configuration settings. 

For example, if it cannot calculate PLL values for a specific requested clock speed, it will attempt to find a close match, and if not found fall back to the values built into the firmware (likely to be the stock maximum rated speed for the MCU).

However, there are some situations where recover is not possible, or it is deemed better to enter "limp mode" (where the device hangs and flashes its status LED, irrespective of the LED enabled setting) to indicate a fault.

If the device enters limp mode, you have two choices:
- Try changing the settings and reflashing.
- Connecting a debug probe and using a build with BOOT_LOGGING/DEBUG_LOGGING included, to diagnose the issue.

The stock builds provided as part of One ROM releases _do not_ include debug or boot logging for performance reasons.

## Complex Configs

There's some advanced ROM options including the following:

- ROM Set types "multi" and "banked" for multi-ROM sets and dynamically bank switched ROM sets
- Local file and URL file sources (most generators only support URLs)
- The ability to specify licenses which must be accepted before building a config
- The ability to configure chip selects in multiple directions
- Support for all 24 and 28 pin ROM types
- Optional categories for better organization
- Support for archived ROM files (zips)
- Retrieve sections of a larger ROM file
- Duplicate and pad ROM images, if the ROM file provided is smaller than the expected size, and truncate if larger

Use the configs in this directory and the schema to build your own complex configs.

## Future Plans

The "end-game" is to have a repository of officially supported ROM configs for popular systems that can be used with One ROM Studio to build custom One ROM images.

It is intended that eventually ROM configs will specify the original ROM chip's access times and other characteristics, with the One ROM firmware to dynamically adjust its serving behavior to match the original hardware as closely as possible.
