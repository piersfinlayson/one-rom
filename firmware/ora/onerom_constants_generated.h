// OneROM Constants
//
// Values a plugin must agree with the firmware on, taken from the same
// schema the firmware's own definitions come from.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// GENERATED FILE - DO NOT EDIT
// Source: firmware/metadata_schema.toml

#ifndef ONEROM_CONSTANTS_H
#define ONEROM_CONSTANTS_H

#include <stdint.h>

// Sentinel: no GPIO is connected to this pin position
#define ORA_GPIO_NONE ((uint8_t)0xFF)

// The longest hold either LED accepts, in milliseconds.
#define ORA_LED_MAX_HOLD_MS ((uint32_t)0x0000EA60)

// The longest bounded GPIO hold a plugin accepts, in milliseconds.
#define ORA_GPIO_MAX_HOLD_MS ((uint32_t)0x0000EA60)

#endif // ONEROM_CONSTANTS_H
