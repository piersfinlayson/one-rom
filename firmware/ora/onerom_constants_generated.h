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

// A FLASH_DEVINFO size field for a 2MB chip. The bootrom reads a size n other
// than 0 as 4KB shifted left n times, and 0 as no chip.
// @since firmware 0.8.0
#define ORA_OTP_FLASH_DEVINFO_SIZE_2MB ((uint16_t)9)

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

// The longest hold either LED accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_LED_MAX_HOLD_MS ((uint32_t)0x0000EA60)

// The longest bounded GPIO hold a plugin accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_GPIO_MAX_HOLD_MS ((uint32_t)0x0000EA60)

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

// Not recorded. Firmware before 0.8.0 doesn't record the board size.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_UNKNOWN ((uint8_t)0)

// 2MB of built-in flash without a chip on chip select 1.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_M ((uint8_t)1)

// 2MB of built-in flash and 2MB of external flash on chip select 1.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_L ((uint8_t)2)

// An unspecified size.
// @since firmware 0.8.0
#define ORA_BOARD_SIZE_OTHER ((uint8_t)0xFF)

#endif // ONEROM_CONSTANTS_H
