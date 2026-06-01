# Chip Type Specifications

This document provides detailed specifications for the different Chip types One ROM supports, and aims to support in future, including pinouts, control lines, and programming requirements.

The document is auto-generated from the [json/rom-types.json](/rust/config/json/rom-types.json) configuration file.  That file was created by researching datasheets for the various Chip types.

Some of the pin names have been modified from the datasheet values for consistency beween Chip types:

- /OE on 2704/2408 is called Program, but serves as /OE when in read mode.  Other 27xx ROMs use /OE for that pin, hence the /OE name is used here. 
- Similarly /CE on 2704/2708 ROMs is called /CS, but is called /CE for consistency with other ROM types.
- 23256/23512 chip select lines are often called CE/OE on datasheets, but are mask programmable to be active high or low, hence these are referred to within this doc as CS lines, like the other 23xx ROMs.

There are also some other inconsistencies between types:

- 2332's CS2 is pin 21 and pin 18 is A11.  On the 2316, CS2 is pin 18, and CS3 pin 21.
- The 2332's A11 is pin18, but the 2732's A11 is pin 21.

## Contents

- [24-pin Mask ROM Family (23xx)](#24-pin-mask-rom-family-23xx)
- [28-pin Mask ROM Family (23xxx)](#28-pin-mask-rom-family-23xx)
- [32-pin Mask ROM Family (23xxx)](#32-pin-mask-rom-family-23xx)
- [24-pin EPROM Family (27xx)](#24-pin-eprom-family-27xx)
- [28-pin EPROM Family (2764 and 27xxx)](#28-pin-eprom-family-27xx)
- [32-pin EPROM Family (27xxx)](#32-pin-eprom-family-27xx)
- [40-pin EPROM Family (27xxx)](#40-pin-eprom-family-27xx)
- [24-pin EEPROM Family (28Cxx)](#24-pin-eeprom-family-28cxx)
- [28-pin EEPROM Family (28Cxx)](#28-pin-eeprom-family-28cxx)
- [32-pin EEPROM Family (28Cxx)](#32-pin-eeprom-family-28cxx)
- [RAM Chips](#ram-chips)
- [Pin Function Comparison](#pin-function-comparison)
- [Detailed Pinouts](#detailed-pinouts)

## 24-pin Mask ROM Family (23xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 2316 | 9316 | 2KB | 11 (A0-A10) | CS1 (pin 20), CS2 (pin 18), CS3 (pin 21) | None | ✓ |
| 2332 | 9332, 4732 | 4KB | 12 (A0-A11) | CS1 (pin 20), CS2 (pin 21) | None | ✓ |
| 2364 | 4764, MCM68764, MCM68A764, MCM68364, MCM68A364 | 8KB | 13 (A0-A12) | CS1 (pin 20) | None | ✓ |

## 28-pin Mask ROM Family (23xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 23128 |  | 16KB | 14 (A0-A13) | CS1 (pin 20), CS2 (pin 22), CS3 (pin 27) | None | ✓ |
| 23256 |  | 32KB | 15 (A0-A14) | CS1 (pin 20), CS2 (pin 22) | None | ✓ |
| 23QL384 |  | 48KB | 16 (A0-A15) | CS1 (pin 22) | None | ✓ |
| 23512 |  | 64KB | 16 (A0-A15) | CS1 (pin 20), CS2 (pin 22) | None | ✓ |
| 23QL512 |  | 64KB | 16 (A0-A15) | CS1 (pin 22) | None | ✓ |
| 231024 | TC531000, 23C1000, 23C1000A, MX23C1000 | 128KB | 17 (A0-A16) | CS1 (pin 20) | None | ✓ |

## 32-pin Mask ROM Family (23xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 23C1001 | D23C1001 | 128KB | 17 (A0-A16) | /CE (pin 22), CS1 (pin 31), CS2 (pin 30), /OE (pin 24) | None | ✓ |
| 23C1010 |  | 128KB | 17 (A0-A16) | /CE (pin 22), /OE (pin 24) | None | ✓ |

## 24-pin EPROM Family (27xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 2704 |  | 512B | 9 (A0-A8) | /CE (pin 18), /OE (pin 20) | VPP: pin 18 (Low during read) | ✓ |
| 2708 |  | 1KB | 10 (A0-A9) | /CE (pin 18), /OE (pin 20) | VPP: pin 18 (Low during read) | ✓ |
| 2716 |  | 2KB | 11 (A0-A10) | /CE (pin 18), /OE (pin 20) | VPP: pin 21 (VCC during read) | ✓ |
| 2732 | 27C32 | 4KB | 12 (A0-A11) | /CE (pin 18), /OE (pin 20) | VPP: pin 20 (Acts as /OE) | ✓ |

## 28-pin EPROM Family (27xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 2764 | 27C64, 27LC64 | 8KB | 13 (A0-A12) | /CE (pin 20), /OE (pin 22) | VPP: pin 1 (VCC during read); /PGM: pin 27 (High during read) | ✓ |
| 27128 | 27C128, 27LC128 | 16KB | 14 (A0-A13) | /CE (pin 20), /OE (pin 22) | VPP: pin 1 (VCC during read); /PGM: pin 27 (High during read) | ✓ |
| 27256 | 27C256, 27LC256, 27SF256 | 32KB | 15 (A0-A14) | /CE (pin 20), /OE (pin 22) | VPP: pin 1 (VCC during read) | ✓ |
| 27512 | 27C512, 27LC512, 27SF512 | 64KB | 16 (A0-A15) | /CE (pin 20), /OE (pin 22) | VPP: pin 22 (VCC during read) | ✓ |

## 32-pin EPROM Family (27xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 27C010 | 27C1001, 27C1000A, 29F010, 39SF010, SST39SF010 | 128KB | 17 (A0-A16) | /CE (pin 22), /OE (pin 24) | VPP: pin 1 (x); /PGM: pin 31 (x) | ✓ |
| 27C301 | 27C1000, 27C100 | 128KB | 17 (A0-A16) | /CE (pin 22), /OE (pin 2) | VPP: pin 1 (x); /PGM: pin 31 (x) | ✓ |
| 27C020 | 27C2001, 39SF020, 29F020, SST39SF020 | 256KB | 18 (A0-A17) | /CE (pin 22), /OE (pin 24) | VPP: pin 1 (x); /PGM: pin 31 (x) | ✓ |
| 27C040 | 27C4001 | 512KB | 19 (A0-A18) | /CE (pin 22), /OE (pin 24) | VPP: pin 1 (x); /PGM: pin 22 (Acts as /OE) | ✓ |
| 27C080 | 27C801 | 1024KB | 20 (A0-A19) | /CE (pin 22), /OE (pin 24) | VPP: pin 24 (Acts as /OE); /PGM: pin 22 (Acts as /OE) | ✓ |

## 40-pin EPROM Family (27xx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 27C200 | HN62402 | 256KB | 18 (A0-A17) | /BYTE (pin 31), /CE (pin 10), /OE (pin 12) | None | ✓ |
| 27C400 | AT27C400, M27C400, 23C4100, MX23C4100, TCS534200, 27C4100, MX27C4100, HN62404, HN62424, MB834200 | 512KB | 19 (A0-A18) | /BYTE (pin 31), /CE (pin 10), /OE (pin 12) | VPP: pin 31 (word_size); /PGM: pin 10 (Acts as /OE) | ✓ |

## 24-pin EEPROM Family (28Cxx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 28C16 |  | 2KB | 11 (A0-A10) | /CE (pin 18), /OE (pin 20), /WRITE (pin 21) | None | ✓ |

## 28-pin EEPROM Family (28Cxx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 28C64 |  | 8KB | 13 (A0-A12) | /BUSY (pin 1), /CE (pin 20), /OE (pin 22), /WRITE (pin 27) | None | ✓ |
| 28C256 |  | 32KB | 15 (A0-A14) | /CE (pin 20), /OE (pin 22), /WRITE (pin 27) | None | ✓ |

## 32-pin EEPROM Family (28Cxx)

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 28C512 |  | 64KB | 16 (A0-A15) | /CE (pin 22), /OE (pin 24), /WRITE (pin 30) | None | ✓ |

## RAM Chips

| Chip Type | Aliases | Size | Address Lines | Control Lines | Programming | Supported |
|-----------|---------|------|---------------|---------------|-------------|-----------|
| 6116 | 2016 | 2KB | 11 (A0-A10) | /CE (pin 18), /OE (pin 20), /WRITE (pin 21) | None | ✓ |

## Pin Function Comparison

### 24-pin Package

| Pin | 2316 | 2332 | 2364 | 2704 | 2708 | 2716 | 28C16 | 6116 | 2732 |
|-----|------|------|------|------|------|------|------|------|------|
| 1 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 |
| 2 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 |
| 3 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 |
| 4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 |
| 5 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 |
| 6 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 |
| 7 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 |
| 8 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 |
| 9 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 |
| 10 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 |
| 11 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 |
| 12 | GND | GND | GND | GND | GND | GND | GND | GND | GND |
| 13 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 |
| 14 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 |
| 15 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 |
| 16 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 |
| 17 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 |
| 18 | CS2 | A11 | A11 | /CE+VPP | /CE+VPP | /CE | /CE | /CE | /CE+PE |
| 19 | A10 | A10 | A10 | VDD | VDD | A10 | A10 | A10 | A10 |
| 20 | CS1 | CS1 | CS1 | /OE+PE | /OE+PE | /OE+PE | /OE | /OE | /OE+VPP |
| 21 | CS3 | CS2 | A12 | VBB | VBB | VPP | /WRITE | /WRITE | A11 |
| 22 | A9 | A9 | A9 | GND | A9 | A9 | A9 | A9 | A9 |
| 23 | A8 | A8 | A8 | A8 | A8 | A8 | A8 | A8 | A8 |
| 24 | VCC | VCC | VCC | VCC | VCC | VCC | VCC | VCC | VCC |

### 28-pin Package

| Pin | 23128 | 23256 | 23QL384 | 23512 | 23QL512 | 231024 | 2764 | 28C64 | 27128 | 27256 | 28C256 | 27512 |
|-----|------|------|------|------|------|------|------|------|------|------|------|------|
| 1 | NC | NC | NC | A15 | NC | A15 | VPP | /BUSY | VPP | VPP | A14 | A15 |
| 2 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 |
| 3 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 |
| 4 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 |
| 5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 |
| 6 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 |
| 7 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 |
| 8 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 |
| 9 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 |
| 10 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 |
| 11 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 |
| 12 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 |
| 13 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 |
| 14 | GND | GND | GND | GND | GND | GND | GND | GND | GND | GND | GND | GND |
| 15 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 |
| 16 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 |
| 17 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 |
| 18 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 |
| 19 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 |
| 20 | CS1 | CS1 | A15 | CS1 | A15 | CS1 | /CE | /CE | /CE | /CE | /CE | /CE+PE |
| 21 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 |
| 22 | CS2 | CS2 | CS1 | CS2 | CS1 | A16 | /OE | /OE | /OE | /OE+PE | /OE | /OE+VPP |
| 23 | A11 | A11 | A11 | A11 | A11 | A11 | A11 | A11 | A11 | A11 | A11 | A11 |
| 24 | A9 | A9 | A9 | A9 | A9 | A9 | A9 | A9 | A9 | A9 | A9 | A9 |

### 32-pin Package

| Pin | 23C1001 | 23C1010 | 28C512 | 27C010 | 27C301 | 27C020 | 27C040 | SST39SF040 | 27C080 |
|-----|------|------|------|------|------|------|------|------|------|
| 1 | NC | NC | NC | VPP | VPP | VPP | VPP | A18 | A19 |
| 2 | A16 | A16 | NC | A16 | /OE | A16 | A16 | A16 | A16 |
| 3 | A15 | A15 | A15 | A15 | A15 | A15 | A15 | A15 | A15 |
| 4 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 | A12 |
| 5 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 | A7 |
| 6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 | A6 |
| 7 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 | A5 |
| 8 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 | A4 |
| 9 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 | A3 |
| 10 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 | A2 |
| 11 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 | A1 |
| 12 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 | A0 |
| 13 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 | D0 |
| 14 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 | D1 |
| 15 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 | D2 |
| 16 | GND | GND | GND | GND | GND | GND | GND | GND | GND |
| 17 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 | D3 |
| 18 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 | D4 |
| 19 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 | D5 |
| 20 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 | D6 |
| 21 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 | D7 |
| 22 | /CE | /CE | /CE | /CE | /CE | /CE | /CE+/PGM | /CE | /CE+/PGM |
| 23 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 | A10 |
| 24 | /OE | /OE | /OE | /OE | A16 | /OE | /OE | /OE | /OE+VPP |

### 40-pin Package

| Pin | 27C200 | 27C400 |
|-----|------|------|
| 1 | NC | A18 |
| 2 | A8 | A8 |
| 3 | A7 | A7 |
| 4 | A6 | A6 |
| 5 | A5 | A5 |
| 6 | A4 | A4 |
| 7 | A3 | A3 |
| 8 | A2 | A2 |
| 9 | A1 | A1 |
| 10 | /CE | /CE+/PGM |
| 11 | GND | GND |
| 12 | /OE | /OE |
| 13 | D0 | D0 |
| 14 | D8 | D8 |
| 15 | D1 | D1 |
| 16 | D9 | D9 |
| 17 | D2 | D2 |
| 18 | D10 | D10 |
| 19 | D3 | D3 |
| 20 | D11 | D11 |
| 21 | VCC | VCC |
| 22 | D4 | D4 |
| 23 | D12 | D12 |
| 24 | D5 | D5 |

## Detailed Pinouts

### 2316 - 2KB mask ROM with 3 configurable CS lines

**Package:** 24-pin DIP  
**Capacity:** 2048 bytes  
**Control:** 3 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A10) | 8,7,6,5,4,3,2,1,23,22,19 | 11 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| CS2 | 18 | Configurable polarity |
| CS3 | 21 | Configurable polarity |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 2332 - 4KB mask ROM with 2 configurable CS lines

**Package:** 24-pin DIP  
**Capacity:** 4096 bytes  
**Control:** 2 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A11) | 8,7,6,5,4,3,2,1,23,22,19,18 | 12 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| CS2 | 21 | Configurable polarity |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 2364 - 8KB mask ROM with 1 configurable CS line

**Package:** 24-pin DIP  
**Capacity:** 8192 bytes  
**Control:** 1 configurable CS line  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A12) | 8,7,6,5,4,3,2,1,23,22,19,18,21 | 13 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 23128 - 16KB mask ROM with 3 configurable CS lines

**Package:** 28-pin DIP  
**Capacity:** 16384 bytes  
**Control:** 3 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A13) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26 | 14 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| CS2 | 22 | Configurable polarity |
| CS3 | 27 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 23256 - 32KB mask ROM with 2 configurable CS lines

**Package:** 28-pin DIP  
**Capacity:** 32768 bytes  
**Control:** 2 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A14) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27 | 15 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| CS2 | 22 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 23QL384 - A composite ROM type, serving a combined 23256 and 23128 for the Sinclair QL, with a single configured CS line and is de-selected when A14 & A15 are both high

**Package:** 28-pin DIP  
**Capacity:** 49152 bytes  
**Control:** 1 configurable CS line  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A15) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27,20 | 16 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 22 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 23512 - 64KB mask ROM with 2 configurable CS lines

**Package:** 28-pin DIP  
**Capacity:** 65536 bytes  
**Control:** 2 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A15) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27,1 | 16 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| CS2 | 22 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 23QL512 - A composite ROM type, serving two combined 23256s for the Sinclair QL, with a single configured CS line at pin 22, and A15 at pin 20 instead of pin 1

**Package:** 28-pin DIP  
**Capacity:** 65536 bytes  
**Control:** 1 configurable CS line  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A15) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27,20 | 16 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 22 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 231024 - 128KB mask ROM with 1 configurable CS line

**Package:** 28-pin DIP  
**Capacity:** 131072 bytes  
**Control:** 1 configurable CS line  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A16) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27,1,22 | 17 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CS1 | 20 | Configurable polarity |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 23C1001 - 128KB mask programmed ROM with fixed active-low CE/OE, plus 2 programmable CS lines

**Package:** 32-pin DIP  
**Capacity:** 131072 bytes  
**Control:** 4 configurable CS lines  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A16) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2 | 17 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| CS1 | 31 | Configurable polarity |
| CS2 | 30 | Configurable polarity |
| OE | 24 | Active low |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 23C1010 - 128KB mask ROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 131072 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A16) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2 | 17 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 2704 - 512B EPROM with multiple supply voltages

**Package:** 24-pin DIP  
**Capacity:** 512 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A8) | 8,7,6,5,4,3,2,1,23 | 9 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| VPP | 18 | Low during read during read |
| PE | 20 | Low during read during read |
| VCC | 24 | +5V |
| VDD | 19 | +12V |
| VBB | 21 | -5V |
| GND | 12 | 0V |
| GND | 22 | 0V |

### 2708 - 1KB EPROM with multiple supply voltages

**Package:** 24-pin DIP  
**Capacity:** 1024 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A9) | 8,7,6,5,4,3,2,1,23,22 | 10 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| VPP | 18 | Low during read during read |
| PE | 20 | Low during read during read |
| VCC | 24 | +5V |
| VDD | 19 | +12V |
| VBB | 21 | -5V |
| GND | 12 | 0V |

### 2716 - 2KB EPROM with fixed active-low CE/OE

**Package:** 24-pin DIP  
**Capacity:** 2048 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A10) | 8,7,6,5,4,3,2,1,23,22,19 | 11 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| VPP | 21 | VCC during read during read |
| PE | 20 | Low during read during read |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 28C16 - 2KB EEPROM with fixed active-low CE/OE

**Package:** 24-pin DIP  
**Capacity:** 2048 bytes  
**Control:** /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A10) | 8,7,6,5,4,3,2,1,23,22,19 | 11 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| WRITE | 21 | Active low |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 6116 - 2KB (2048 x 8-bit) Static RAM with fixed active-low CE/OE/WE

**Package:** 24-pin DIP  
**Capacity:** 2048 bytes  
**Control:** /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A10) | 8,7,6,5,4,3,2,1,23,22,19 | 11 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| WRITE | 21 | Active low |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 2732 - 4KB EPROM with fixed active-low CE and shared OE/VPP

**Package:** 24-pin DIP  
**Capacity:** 4096 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A11) | 8,7,6,5,4,3,2,1,23,22,19,21 | 12 address lines |
| Data (D0-D7) | 9,10,11,13,14,15,16,17 | 8 data lines |
| CE | 18 | Active low |
| OE | 20 | Active low |
| VPP | 20 | Acts as /OE during read |
| PE | 18 | Low during read during read |
| VCC | 24 | +5V |
| GND | 12 | 0V |

### 2764 - 8KB EPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 8192 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A12) | 10,9,8,7,6,5,4,3,25,24,21,23,2 | 13 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CE | 20 | Active low |
| OE | 22 | Active low |
| VPP | 1 | VCC during read during read |
| /PGM | 27 | High during read during read |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 28C64 - 8KB EEPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 8192 bytes  
**Control:** /BUSY, /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A12) | 10,9,8,7,6,5,4,3,25,24,21,23,2 | 13 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| BUSY | 1 | Active low |
| CE | 20 | Active low |
| OE | 22 | Active low |
| WRITE | 27 | Active low |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 27128 - 16KB EPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 16384 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A13) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26 | 14 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CE | 20 | Active low |
| OE | 22 | Active low |
| VPP | 1 | VCC during read during read |
| /PGM | 27 | High during read during read |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 27256 - 32KB EPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 32768 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A14) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27 | 15 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CE | 20 | Active low |
| OE | 22 | Active low |
| VPP | 1 | VCC during read during read |
| PE | 22 | Low during read during read |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 28C256 - 32KB EEPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 32768 bytes  
**Control:** /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A14) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,1 | 15 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CE | 20 | Active low |
| OE | 22 | Active low |
| WRITE | 27 | Active low |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 27512 - 64KB EPROM with fixed active-low CE/OE

**Package:** 28-pin DIP  
**Capacity:** 65536 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A15) | 10,9,8,7,6,5,4,3,25,24,21,23,2,26,27,1 | 16 address lines |
| Data (D0-D7) | 11,12,13,15,16,17,18,19 | 8 data lines |
| CE | 20 | Active low |
| OE | 22 | Active low |
| VPP | 22 | VCC during read during read |
| PE | 20 | Low during read during read |
| VCC | 28 | +5V |
| GND | 14 | 0V |

### 28C512 - 64KB EEPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 65536 bytes  
**Control:** /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A15) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3 | 16 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| WRITE | 30 | Active low |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C010 - 128KB EPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 131072 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A16) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2 | 17 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| VPP | 1 | x during read |
| /PGM | 31 | x during read |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C301 - 128KB EPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 131072 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A16) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,24 | 17 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 2 | Active low |
| VPP | 1 | x during read |
| /PGM | 31 | x during read |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C020 - 256KB EPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 262144 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A17) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2,30 | 18 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| VPP | 1 | x during read |
| /PGM | 31 | x during read |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C200 - 0.25MB mask programmed EPROM with fixed active-low CE/OE

**Package:** 40-pin DIP  
**Capacity:** 262144 bytes  
**Control:** /BYTE, /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A17) | 29,9,8,7,6,5,4,3,2,40,39,38,37,36,35,34,33,32 | 18 address lines |
| Data (D0-D7) | 13,15,17,19,22,24,26,28,14,16,18,20,23,25,27,29 | 8 data lines |
| BYTE | 31 | Active low |
| CE | 10 | Active low |
| OE | 12 | Active low |
| VCC | 21 | +5V |
| GND | 11 | 0V |
| GND | 30 | 0V |

### 27C040 - 512KB EPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 524288 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A18) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2,30,31 | 19 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| VPP | 1 | x during read |
| /PGM | 22 | Acts as /OE during read |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C400 - 0.5MB EPROM with fixed active-low CE/OE

**Package:** 40-pin DIP  
**Capacity:** 524288 bytes  
**Control:** /BYTE, /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A18) | 29,9,8,7,6,5,4,3,2,40,39,38,37,36,35,34,33,32,1 | 19 address lines |
| Data (D0-D7) | 13,15,17,19,22,24,26,28,14,16,18,20,23,25,27,29 | 8 data lines |
| BYTE | 31 | Active low |
| CE | 10 | Active low |
| OE | 12 | Active low |
| VPP | 31 | word_size during read |
| /PGM | 10 | Acts as /OE during read |
| VCC | 21 | +5V |
| GND | 11 | 0V |
| GND | 30 | 0V |

### SST39SF040 - 512KB flash with fixed active-low CE/OE and different pinout to 27C040

**Package:** 32-pin DIP  
**Capacity:** 524288 bytes  
**Control:** /CE, /OE, /WRITE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A18) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2,30,1 | 19 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| WRITE | 31 | Active low |
| VCC | 32 | +5V |
| GND | 16 | 0V |

### 27C080 - 1MB EPROM with fixed active-low CE/OE

**Package:** 32-pin DIP  
**Capacity:** 1048576 bytes  
**Control:** /CE, /OE  

| Function | Pins | Notes |
|----------|------|-------|
| Address (A0-A19) | 12,11,10,9,8,7,6,5,27,26,23,25,4,28,29,3,2,30,31,1 | 20 address lines |
| Data (D0-D7) | 13,14,15,17,18,19,20,21 | 8 data lines |
| CE | 22 | Active low |
| OE | 24 | Active low |
| VPP | 24 | Acts as /OE during read |
| /PGM | 22 | Acts as /OE during read |
| VCC | 32 | +5V |
| GND | 16 | 0V |

