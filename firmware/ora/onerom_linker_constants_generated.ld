/* OneROM Plugin Linker Constants
 *
 * Values a plugin's linker script must agree with the firmware on, taken from
 * the same schema the firmware's own definitions come from.
 *
 * A constant's `@since firmware X.Y.Z` line names the release it first
 * reached this file in.  A plugin using it sets its min_fw_version to that
 * release or a later one.
 *
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 *
 * GENERATED FILE - DO NOT EDIT
 * Source: rust/metadata/metadata_schema.toml
 */

/* Address of the flash chip on QSPI chip select 0 (built-in on an RP2354). */
/* @since firmware 0.8.0 */
ORA_FLASH_CS0_BASE_ADDR = 0x10000000;

/* Address of the flash chip on QSPI chip select 1. */
/* @since firmware 0.8.0 */
ORA_FLASH_CS1_BASE_ADDR = 0x11000000;

/* Offset of the system plugin region from the start of flash. */
/* @since firmware 0.8.0 */
ORA_SYSTEM_PLUGIN_OFFSET = 0x10000;

/* Size of the system plugin region in bytes. */
/* @since firmware 0.8.0 */
ORA_SYSTEM_PLUGIN_SIZE = 0x10000;

/* Offset of the user plugin region from the start of flash. */
/* @since firmware 0.8.0 */
ORA_USER_PLUGIN_OFFSET = 0x20000;

/* Size of the user plugin region in bytes. */
/* @since firmware 0.8.0 */
ORA_USER_PLUGIN_SIZE = 0x10000;

