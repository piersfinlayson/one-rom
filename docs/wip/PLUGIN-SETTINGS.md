# Plugin Settings

## Model

The plugin header points at a table describing its config struct, so a host
reading only the binary gets from `system.boot.host_reset=true` to a byte at an
offset. All-ones is unset at every granularity, erased region and single field
alike. No present-bitmap, no ID field.

## Header and descriptor

`firmware/ora/api.h:3366`'s reserved block gains one field. The block starts at
offset 30, so two bytes are declared ahead of it rather than left to the
compiler - a `uint32_t` placed at 30 takes padding and pushes the header past its
256-byte assert, confirmed under `arm-none-eabi-gcc -mcpu=cortex-m33`.

```c
    /** @brief Reserved.  Set to zero. */
    uint8_t  reserved0[2];

    /** @brief Offset from plugin base to ora_settings_info_t.  0 = none.
     *  @since firmware 0.7.2 */
    uint32_t settings_info;

    /** @brief Reserved for future use and must be set to 0. */
    uint8_t  reserved[220];
```

```c
typedef struct {
    uint8_t  size;         /* sizeof as the plugin built it */
    uint8_t  version;      /* structural change only */
    uint8_t  entry_size;   /* stride of ora_setting_entry_t */
    uint8_t  reserved;
    uint16_t count;
    uint16_t region_size;  /* bytes in each region */
    uint32_t table;        /* offsets from the plugin's base address */
    uint32_t live;
    uint32_t staging;      /* 0 = writes refused */
} ora_settings_info_t;
STATIC_ASSERT(sizeof(ora_settings_info_t) == 20, "ora_settings_info_t must be 20 bytes");
```

`size` caps what a reader takes, as `ora_gpio_info_t`
(`firmware/ora/api.h:2570`) and `ora_led_state_t` (`:2807`) do, so growth is
additive.

```c
typedef struct {
    uint32_t name;      /* offset from plugin base, NUL terminated */
    uint16_t offset;    /* within the config payload */
    uint8_t  size;
    uint8_t  type;      /* ora_setting_type_t */
} ora_setting_entry_t;
STATIC_ASSERT(sizeof(ora_setting_entry_t) == 8, "ora_setting_entry_t must be 8 bytes");

typedef enum {
    ORA_SETTING_TYPE_BOOL = 0,  /* 1 byte, 0 or 1, 0xFF unset */
    ORA_SETTING_TYPE_U8   = 1,
    ORA_SETTING_TYPE_U16  = 2,  /* 0xFFFF unset */
    ORA_SETTING_TYPE_U32  = 3,
    ORA_SETTING_TYPE_PIN  = 4,  /* ora_setting_pin_t */
    ORA_SETTING_TYPE_STR  = 5,  /* NUL padded to the entry's size */
} ora_setting_type_t;

typedef struct {
    uint8_t kind;   /* ora_setting_pin_kind_t: X1, X2, SEL, GPIO */
    uint8_t index;  /* element, from 0 */
} ora_setting_pin_t;
```

A pin resolves on the device via `ORA_ID_GET_METADATA_UINT_AT`
(`firmware/ora/api.h:741`) against `gpio_x1`, `gpio_x2`, `gpio_sel`
(`rust/metadata/metadata_schema.toml:1007`), never host-side.

## Regions

Both are `region_size` bytes, sector-aligned, starting with:

```c
#define ORA_SETTINGS_MAGIC 0x5343524Fu  /* "ORCS" in a little-endian dump */

typedef struct {
    uint32_t magic;
    uint32_t payload_crc;  /* over the payload that follows */
    uint32_t layout_crc;   /* over the table these bytes match */
    uint16_t payload_len;
    uint8_t  header_size;
    uint8_t  reserved;
} ora_settings_region_t;
STATIC_ASSERT(sizeof(ora_settings_region_t) == 16, "ora_settings_region_t must be 16 bytes");
```

`layout_crc` covers, per entry in order, name with NUL, `offset` little-endian,
`size`, `type`, binding values to one build. Both use a reflected CRC-32 (poly
`0xEDB88320`, init and final xor `0xFFFFFFFF`) in the flash library, as
`firmware/ora/` has none.

## Declaration

```c
/*  member            setting name            type  default */
#define SYSTEM_SETTINGS(X)                                     \
    X(host_reset_pin,  "host_reset_pin",       PIN,  ORA_PIN(X1, 0)) \
    X(boot_host_reset, "boot.host_reset",      BOOL, false)    \
    X(boot_hold_ms,    "boot.host_reset_hold", U16,  250)

ONEROM_DECLARE_SETTINGS(onerom, SYSTEM_SETTINGS)
```

The first argument is the emitted prefix, so a third-party plugin passes its own.
Out come `<prefix>_settings_t` (unpacked, the table carries `offsetof`),
`<prefix>_settings_table[]`, the names, `<prefix>_settings_defaults` and:

```c
static inline uint16_t onerom_setting_boot_hold_ms(void) {
    const onerom_settings_t *c = onerom_settings_live();
    return (c != NULL && c->boot_hold_ms != 0xFFFFu)
        ? c->boot_hold_ms
        : onerom_settings_defaults.boot_hold_ms;
}
```

## Manifest fragment

Beside the plugin source, referenced from its release entry
(`rust/app/src/plugin.rs:497-513`), keeping help prose out of firmware.

```json
{ "settings_version": 1, "settings": [
  { "name": "boot.host_reset_hold", "type": "u16", "default": 250,
    "unit": "ms", "min": 1, "max": 60000,
    "title": "Boot pulse length",
    "help": "How long the boot pulse holds the pin low." } ] }
```

A host build of the header (`firmware/ora/plugin.mk:36-39`) emits name, type and
default as JSON, and the build compares the two both ways, failing on a setting in
one alone or a differing type or default.

## Read and write

`<prefix>_settings_live()` runs recovery, then validates and caches a pointer —
magic, `header_size`, `payload_len`, `payload_crc`, `layout_crc` against the
running table. Any failure yields NULL and accessors return defaults, a
`layout_crc` mismatch logged by name. Reads are XIP, so a user plugin's 512 bytes
of static RAM (`firmware/ora/plugin.ld:32-33`) suffices.

Writing is `firmware/ora/settings.c`, linked only into plugins that write,
costing plugin flash.

```c
ora_result_t onerom_settings_begin(onerom_settings_txn_t *txn);
ora_result_t onerom_settings_stage(onerom_settings_txn_t *txn, uint16_t index,
                                   const void *value, uint8_t size);
ora_result_t onerom_settings_commit(onerom_settings_txn_t *txn);
void         onerom_settings_discard(onerom_settings_txn_t *txn);
```

Plugins write their own settings through this. `begin` claims a 256-byte page
buffer and the staged flash routine from `ora_alloc` (`firmware/ora/api.h:361`),
returning `ORA_RESULT_INSUFFICIENT_FREE_MEM` when there is none. `commit` goes
page at a time, so nothing holds 4KB.

1. Erase staging. Compute the CRC in a read-only pass over live plus edits, then
   program staging in page order.
2. Erase live. Copy staging into live.
3. Erase staging.

Each page follows `host_control_main.c:1464-1571` — bootrom pointers taken while
XIP lives, exclusive mode, PRIMASK masked by the caller, one XIP exit over
erase and program. That call returns void (`flash_erase.c:36`), so pages are read
back and a mismatch aborts.

Recovery is one rule - staging validates, so redo phases 2 and 3, otherwise
nothing. A cut in phase 1 fails staging's CRC and old values stand, a cut in 2 or
3 leaves it valid for the next boot. Live only becomes valid via a completed
phase 2, never a readable mix.

The CLI writes the image at compose time, and the same phases over PICOBOOT
against a stopped device, since a torn direct write leaves some fields new and
some at default — the wrong GPIO for a reset pin. A running device is the
plugin's own protocol. `flash_erase.c` and `.h` move to `firmware/ora/`, leaving
host-control's RBCP NV untouched (`host_control_main.c:1270`).

## CLI

```
onerom program --config onerom-config/test/28-c64.json --plugin usb \
  --setting system.host_reset_pin=x1 \
  --setting system.boot.host_reset=true \
  --setting system.boot.host_reset_hold=250 \
  --base-firmware firmware/build/onerom-rp235x.bin

onerom setting list
onerom setting set --setting system.boot.host_reset_hold=400
```

Repeatable, no short flag, the same hierarchy nesting in JSON as
`"system": { "boot": { "host_reset": true } }`.

Per setting: split on the first `.`, routing `firmware` to the existing firmware
keys and `system`/`user` to a slot. Check the header magic
(`firmware/ora/api.h:3208`), follow `settings_info`, walk `count` entries at stride
`entry_size`, match the path remainder against the names exactly. Parse by type —
`true`, decimal or `0x`, `x1`/`sel0`/`gpio12`, a string shorter than `size` — the
grammar being the CLI's. Write at `live + entry.offset`, rest 0xFF, then the region
header. No plugin name or schema enters the CLI.

An updated plugin whose table changed changes `layout_crc`, and falls back to
defaults since values alone cannot be remapped. `onerom program --keep-settings`
re-applies by name from the plugin installed on the device, reporting each carried
and dropped. Off by default, since the image should be what the command line
describes — the CLI warns naming what goes.

## Failure modes

- Unknown root, empty slot, `settings_info` 0, name absent — error naming the
  path and the names available.
- Unknown `type` — that setting refused by name, the rest processed.
- Out of range or too wide — error, range only with a manifest.
- Magic or either CRC bad — all defaults, logged.
- No RAM, `staging` 0, or a read-back mismatch — write refused or aborted, reads
  unaffected.

## Open

- **Where the regions sit relative to host-control's 4KB NV**
  (`host_control_plugin.ld:77`) — it turns on whether host-control joins the
  system plugin bundle.
- **Whether a user plugin may write at all** — 512 bytes of static RAM means every
  write hangs on `ora_alloc`, so refusing trades capability for a clearer rule.
