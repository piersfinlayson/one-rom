// OneROM Metadata Keys
//
// Plugin-facing device metadata key space.  Used with the metadata getter API
// (ora_get_metadata_str_fn_t and siblings) to retrieve device-level metadata
// without exposing the internal metadata structures to plugins.
//
// Keys are a single, unified namespace shared across all typed accessors.  A
// key value is a stable, permanent identifier: once assigned it is never
// renumbered or reused.  New metadata is exposed by tagging a schema field
// with a `plugin_key`; retired keys keep returning ORA_RESULT_NOT_SUPPORTED.
//
// A key's `@since firmware X.Y.Z` line names the release it first appeared in
// here.  A plugin using it asks for that release, or a later one, in its
// min_fw_version.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: rust/metadata/metadata_schema.toml

#ifndef ONEROM_METADATA_KEYS_H
#define ONEROM_METADATA_KEYS_H

#include <stdint.h>

typedef enum {
    ORA_METADATA_KEY_NONE              = 0x00000000,  // Reserved. Never a live key; a zero-initialised value is invalid.
    // Name of this One ROM unit. May be NULL. At most MAX_UNIT_NAME_LEN bytes.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_UNIT_NAME         = 0x00000001,
    // Override serial number. NULL = derive from unique MCU chip ID. At most MAX_SERIAL_NUMBER_LEN bytes.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_SERIAL_OVERRIDE   = 0x00000002,
    // GPIO for status LED. Holds the GPIO-none sentinel if not present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_STATUS       = 0x00000003,
    // GPIO for Neopixel LED. Holds the GPIO-none sentinel if not present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_NEOPIXEL     = 0x00000004,
    // Live status-LED on/off state and plugin coordination channel; seeded from the led config default, then updated via ora_set_status_led (see api.h)
    // @since firmware 0.7.1
    ORA_METADATA_KEY_STATUS_LED_STATE  = 0x00000005,
    // Hardware revision string. Must be present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_HW_REV            = 0x00000006,
    // Number of physical pins on the emulated ROM package
    // @since firmware 0.7.1
    ORA_METADATA_KEY_NUM_PHYS_PINS     = 0x00000007,
    // Whether this board has a USB port. 0=no, 1=yes.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_USB_CAPABLE       = 0x00000008,
    // GPIO pin for VBUS detection. Holds the GPIO-none sentinel if not present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_VBUS         = 0x00000009,
    // GPIO for secondary flash chip select. Holds the GPIO-none sentinel if no secondary flash.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_EXT_FLASH_CS = 0x0000000A,
    // GPIO also connected to SWDIO. Holds the GPIO-none sentinel if not present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_SWDIO        = 0x0000000B,
    // GPIO also connected to SWCLK. Holds the GPIO-none sentinel if not present.
    // @since firmware 0.7.1
    ORA_METADATA_KEY_GPIO_SWCLK        = 0x0000000C,
    // Whether boot logging is active
    // @since firmware 0.7.1
    ORA_METADATA_KEY_BOOT_LOGGING      = 0x0000000D,
    // Detected RP235x silicon variant
    // @since firmware 0.7.1
    ORA_METADATA_KEY_RP_VARIANT        = 0x0000000E,
    // Enable fast boot, disabling image select jumper reading. 0=off, 1=on.
    // @since firmware 0.7.2
    ORA_METADATA_KEY_TURBO_BOOT        = 0x0000000F,
    // Image select GPIO pins (MAX_IMG_SEL_PINS entries).
    // Filled contiguously from index 0. An unused entry holds the GPIO-none
    // sentinel, so a reader stops at the first of those rather than at the end
    // of the array - the array's length is not the number of pins fitted.
    // @since firmware 0.7.2
    ORA_METADATA_KEY_GPIO_SEL          = 0x00000010,
    // X1 expansion pin GPIO mapping (MAX_X_PIN_GPIOS entries).
    // Filled contiguously from index 0. An unused entry holds the GPIO-none
    // sentinel, so a reader stops at the first of those rather than at the end
    // of the array. Entry 0 is unused when the board has no X1 pin, entry 1
    // when X1 connects to only one GPIO.
    // @since firmware 0.7.2
    ORA_METADATA_KEY_GPIO_X1           = 0x00000011,
    // X2 expansion pin GPIO mapping (MAX_X_PIN_GPIOS entries).
    // Filled contiguously from index 0. An unused entry holds the GPIO-none
    // sentinel, so a reader stops at the first of those rather than at the end
    // of the array. Entry 0 is unused when the board has no X2 pin, entry 1
    // when X2 connects to only one GPIO.
    // @since firmware 0.7.2
    ORA_METADATA_KEY_GPIO_X2           = 0x00000012,
    // Index of currently selected ROM slot. Initialised to 0xFF.
    // @since firmware 0.7.2
    ORA_METADATA_KEY_ROM_SLOT_INDEX    = 0x00000013,
    // The board's size from the total flash OTP configures, worked out at boot.
    // Chip select 1 counts only on a board with a secondary flash chip select.
    // @since firmware 0.8.0
    ORA_METADATA_KEY_BOARD_SIZE        = 0x00000014,
    // Size of the flash chip on QSPI chip select 0, as an ORA_FLASH_SIZE_* value.
    // See ora_get_metadata_uint_fn_t in api.h.
    // @since firmware 0.8.0
    ORA_METADATA_KEY_FLASH_CS0_SIZE    = 0x00000015,
    // Size of the flash chip on QSPI chip select 1, as an ORA_FLASH_SIZE_* value.
    // See ora_get_metadata_uint_fn_t in api.h.
    // @since firmware 0.8.0
    ORA_METADATA_KEY_FLASH_CS1_SIZE    = 0x00000016,
    ORA_METADATA_KEY_INVALID           = 0xFFFFFFFF,  // Invalid metadata key
} ora_metadata_key_t;
_Static_assert(sizeof(ora_metadata_key_t) == 4, "ora_metadata_key_t must be 4 bytes");

#endif // ONEROM_METADATA_KEYS_H
