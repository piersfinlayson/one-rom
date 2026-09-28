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

// The longest hold either LED accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_LED_MAX_HOLD_MS ((uint32_t)0x0000EA60)

// The longest bounded GPIO hold a plugin accepts, in milliseconds.
// @since firmware 0.7.2
#define ORA_GPIO_MAX_HOLD_MS ((uint32_t)0x0000EA60)

#endif // ONEROM_CONSTANTS_H
