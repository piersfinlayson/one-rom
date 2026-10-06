// OneROM Constants
//
// Values a plugin must agree with the firmware on, taken from the same
// schema the firmware's own definitions come from.
//
// A constant's `@since firmware X.Y.Z` line names the release it first
// reached this header in.  A plugin using it asks for that release, or a
// later one, in its min_fw_version.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: rust/metadata/metadata_schema.toml

#ifndef ONEROM_CONSTANTS_H
#define ONEROM_CONSTANTS_H

#include <stdint.h>

// Maximum byte length, excluding the NUL terminator, of the
// onerom_firmware_config_t.serial_number string. Kept to 31 so the stored string
// plus its terminator occupies 32 bytes.
// @since firmware 0.8.0
#define ORA_MAX_SERIAL_NUMBER_LEN 0x1F

// Sentinel: no GPIO is connected to this pin position
// @since firmware 0.7.2
#define ORA_GPIO_NONE ((uint8_t)0xFF)

// Position of FLASH_DEVINFO's CS0_SIZE field.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT ((uint16_t)8)

// Position of FLASH_DEVINFO's CS1_SIZE field.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT ((uint16_t)0x000C)

// The bits of a FLASH_DEVINFO size field once it's shifted down to bit 0.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_SIZE_BITS ((uint16_t)0x000F)

// FLASH_DEVINFO's D8H_ERASE_SUPPORTED bit. One ROM sets it on every L board.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_D8H_ERASE_SUPPORTED ((uint16_t)0x0080)

// FLASH_DEVINFO's CS1_GPIO field.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_CS1_GPIO ((uint16_t)0x003F)

// Address of the flash chip on QSPI chip select 0 (built-in on an RP2354).
// @since firmware 0.8.0
#define ORA_FLASH_CS0_BASE_ADDR ((uint32_t)0x10000000)

// Address of the flash chip on QSPI chip select 1.
// @since firmware 0.8.0
#define ORA_FLASH_CS1_BASE_ADDR ((uint32_t)0x11000000)

// Offset of the system plugin region from the start of flash.
// @since firmware 0.8.0
#define ORA_SYSTEM_PLUGIN_OFFSET ((uint32_t)0x00010000)

// Size of the system plugin region in bytes.
// @since firmware 0.8.0
#define ORA_SYSTEM_PLUGIN_SIZE 0x10000

// Offset of the user plugin region from the start of flash.
// @since firmware 0.8.0
#define ORA_USER_PLUGIN_OFFSET ((uint32_t)0x00020000)

// Size of the user plugin region in bytes.
// @since firmware 0.8.0
#define ORA_USER_PLUGIN_SIZE 0x10000

// The longest hold either LED accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_LED_MAX_HOLD_MS ((uint32_t)0x0000EA60)

// The longest bounded GPIO hold a plugin accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_GPIO_MAX_HOLD_MS ((uint32_t)0x0000EA60)

// Shortest period the cycle mode accepts, in milliseconds.
// @since firmware 0.8.0
#define ORA_LED_CYCLE_MIN_PERIOD_MS ((uint16_t)0x03E8)

// Shortest period the breathe mode accepts, in milliseconds.
// @since firmware 0.8.0
#define ORA_LED_BREATHE_MIN_PERIOD_MS ((uint16_t)0x03E8)

// Shortest period the blink mode accepts, in milliseconds.
// @since firmware 0.8.0
#define ORA_LED_BLINK_MIN_PERIOD_MS ((uint16_t)0x0032)

// Shortest period the beacon mode accepts, in milliseconds.
// @since firmware 0.8.0
#define ORA_LED_BEACON_MIN_PERIOD_MS ((uint16_t)0x0032)

// Shortest period the flame mode accepts, in milliseconds.
// @since firmware 0.8.0
#define ORA_LED_FLAME_MIN_PERIOD_MS ((uint16_t)0x01F4)

// The USB vendor ID a One ROM presents while the system USB plugin runs.
// @since firmware 0.8.0
#define ORA_USB_PLUGIN_VID ((uint16_t)0x1209)

// The USB product ID a One ROM presents while the system USB plugin runs.
// @since firmware 0.8.0
#define ORA_USB_PLUGIN_PID ((uint16_t)0xF542)

// The USB vendor ID a commissioned One ROM's bootloader presents.
// @since firmware 0.8.0
#define ORA_USB_BOOTLOADER_VID ((uint16_t)0x1209)

// The USB product ID a commissioned One ROM's bootloader presents.
// @since firmware 0.8.0
#define ORA_USB_BOOTLOADER_PID ((uint16_t)0xF540)

// RP235xB
// @since firmware 0.8.0
#define ORA_RP235XB ((uint8_t)0)

// RP235xA
// @since firmware 0.8.0
#define ORA_RP235XA ((uint8_t)1)

// No flash chip.
// @since firmware 0.8.0
#define ORA_FLASH_SIZE_NONE ((uint8_t)0)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_8KB ((uint8_t)1)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_16KB ((uint8_t)2)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_32KB ((uint8_t)3)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_64KB ((uint8_t)4)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_128KB ((uint8_t)5)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_256KB ((uint8_t)6)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_512KB ((uint8_t)7)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_1MB ((uint8_t)8)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_2MB ((uint8_t)9)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_4MB ((uint8_t)0x0A)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_8MB ((uint8_t)0x0B)

// @since firmware 0.8.0
#define ORA_FLASH_SIZE_16MB ((uint8_t)0x0C)

// Not recorded. Firmware before 0.8.0 doesn't record the board size.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_UNKNOWN ((uint8_t)0)

// 2MB of flash in total.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_M ((uint8_t)1)

// 4MB of flash in total.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_L ((uint8_t)2)

// An unspecified size.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_OTHER ((uint8_t)0xFF)

// The boot copy of the ROM image from flash into RAM has finished, or the
// slot has no image to copy.
// @since firmware 0.8.0
#define ORA_FIRMWARE_STATE_ROM_LOADED ((uint32_t)1)

// Plugin launch has finished, reached even if there are no plugins.
// @since firmware 0.8.0
#define ORA_FIRMWARE_STATE_PLUGINS_STARTED ((uint32_t)2)

// The ROM image is loaded and plugins have started.
// @since firmware 0.8.0
#define ORA_FIRMWARE_STATE_STARTUP_DONE ((uint32_t)4)

// One ROM is in standby and doesn't serve the ROM.
// @since firmware 0.8.0
#define ORA_FIRMWARE_FLAG_STANDBY ((uint8_t)1)

#endif // ONEROM_CONSTANTS_H
