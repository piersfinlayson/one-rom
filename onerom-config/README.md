# ROM Configs

A config is a JSON file naming the ROM images a One ROM serves and, per set, any firmware settings that differ from the defaults.  Studio and the CLI build a complete flashable image from it, and the CLI and Web programmer use it internally to create firmware from per-slot configuration.

| Reader | Use |
| --- | --- |
| [One ROM Studio](https://onerom.org/studio) | Pick a published config or local config file, build, flash |
| `onerom` CLI | `onerom program --config <file>` and `onerom firmware build --config <file>`, see [CLI-MANUAL.md](/docs/CLI-MANUAL.md) |

The format is defined by [schema.json](schema.json), also published at https://images.onerom.org/configs/schema.json.  [json-schema.app](https://json-schema.app/view/%23?url=https%3A%2F%2Fimages.onerom.org%2Fconfigs%2Fschema.json) renders it readably.

## What is here

| Path | Contents |
| --- | --- |
| `*.json` | Published configs.  `set-` holds several jumper-selected sets, `bank-` bank-switched sets, `28-` targets 28-pin boards. |
| `blank.json` | No ROMs.  Flashes a One ROM that ships empty. |
| `test/` | Configs CI builds and tests on hardware (`ci/test-emu.sh`). |
| `user/` | Your own configs.  Git-ignored. |

## A config

```json
{
    "$schema": "https://images.onerom.org/configs/schema.json",
    "version": 1,
    "name": "Simple Config",
    "description": "Two 2364 ROMs, selected by jumper",
    "chip_sets": [
        {
            "type": "single",
            "chips": [
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
            "chips": [
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

- Each set is one image-select jumper position, the first with all jumpers open: [IMAGE-SELECTION.md](/docs/IMAGE-SELECTION.md).
- `multi` and `banked` sets serve several chips at once: [MULTI-ROM-SETS.md](/docs/MULTI-ROM-SETS.md).  [set-vic20-pal.json](set-vic20-pal.json) and [bank-c64-char.json](bank-c64-char.json) are real examples.
- Chip types, 24 to 40 pins and RAM, and the select lines each takes: [CHIP-TYPES.md](/docs/CHIP-TYPES.md).  27-series EPROMs need no select lines.
- The older `rom_sets` and `roms` keys still parse.

### Top-level keys

| Key | Meaning |
| --- | --- |
| `version` | Format version, `1`. |
| `description` | Required.  Shown by the builder after `name`. |
| `name`, `detail`, `notes` | Optional text shown by the builder, `notes` after the set list. |
| `categories` | Tags Studio groups and searches on. |
| `instance_name` | A name for this One ROM. |
| `serial_override` | Replaces the USB serial number. |
| `turbo_boot` | Skip reading the jumpers and serve the first image. |
| `boot_logging` | Log boot over USB (with the USB plugin) or RTT. |
| `swd_enabled` | `false` shuts SWD off as ROM serving starts, so debug-port reads cannot steal cycles from the serving DMAs.  BOOTSEL and PICOBOOT are unaffected. |

### Per-set keys

| Key | Meaning |
| --- | --- |
| `type` | `single`, `multi` or `banked`. |
| `description` | Shown by the builder. |
| `chips` | The chips in the set.  Order sets the X-pin assignment in a `multi` set. |
| `firmware_overrides` | Firmware settings for this set, below. |

### Per-chip keys

| Key | Meaning |
| --- | --- |
| `file` | Path or URL. |
| `type` | Chip type, e.g. `2364`, `27256`, `6116`. |
| `cs1`..`cs4`, `ce`, `oe` | `active_low`, `active_high` or `ignore`, for the lines the chip type has.  `allow_cs_ignore: true` permits `ignore` where the chip type does not explicitly allow it. |
| `description`, `label` | `label` replaces the filename in the device metadata. |
| `license` | URL the builder asks the user to accept first. |
| `extract` | Path inside the archive when `file` is a zip or tar. |
| `location` | `start` and `length` of the image inside a larger file. |
| `format`, `load_address` | `ihex` decodes Intel HEX, `load_address` mapping to byte 0. |
| `transform` | Byte transforms applied in order, see [Image transforms](/docs/CLI-MANUAL.md#image-transforms). |
| `size_handling` | `duplicate`, `pad` or `truncate` when the image is not the chip's size. |

Windows paths take doubled backslashes (`"C:\\roms\\kernal.bin"`) or forward slashes.

## Firmware overrides

Fire (RP2350) boards only.  Each set carries its own, and a set without one uses the firmware defaults.

| Key | Meaning |
| --- | --- |
| `fire.cpu_freq` | Default `150MHz`.  Above that, set `overclock: true`. |
| `fire.overclock` | Permits a frequency above the rated maximum. |
| `fire.vreg` | Core voltage, e.g. `1.20V`.  Left out, the firmware picks a conservative value for the frequency. |
| `fire.force_16_bit` | Combined 8/16 bit ROM types.  Ignores `/BYTE` and serves 16 bits always, which reads the address lines a third more often. |
| `led.enabled` | `false` turns the status LED off while serving.  Limp mode still blinks it. |
| `swd.swd_enabled` | As the top-level key, for this set. |

```json
"firmware_overrides": {
    "fire": {
        "cpu_freq": "200MHz",
        "overclock": true,
        "vreg": "1.15V"
    },
    "led": {
        "enabled": false
    }
}
```

A frequency the PLL cannot hit is rounded to the nearest it can.  A setting the firmware cannot recover from puts the device in limp mode, blinking the status LED.  Change the setting and reflash, or read the boot log: [LOGGING.md](/docs/LOGGING.md).

## Checking a config

Build the image without a device attached:

```bash
onerom firmware build --config user/mine.json --board fire-24-e --out /tmp/onerom.bin
```

## Publishing a config

Studio lists every config in `configs.json` at [one-rom-images](https://github.com/piersfinlayson/one-rom-images), sorted by name with Blank first.

1. Add the file here and commit.
2. Copy it to `one-rom-images/configs/`.
3. Add `"configs/<name>.json"` to the `configs` list in `one-rom-images/configs.json`.
4. Push `one-rom-images` main.  GitHub Pages deploys it.

When `schema.json` changes, the copy in `one-rom-images/configs/` goes with the release.
