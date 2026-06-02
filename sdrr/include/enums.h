// Contains enums used by the One ROM firmware.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef ENUMS_H
#define ENUMS_H

// ROM type enumeration
typedef enum {
    CHIP_TYPE_2316 = 0,
    CHIP_TYPE_2332 = 1,
    CHIP_TYPE_2364 = 2,
    CHIP_TYPE_23128 = 3,
    CHIP_TYPE_23256 = 4,
    CHIP_TYPE_23512 = 5,
    CHIP_TYPE_2704 = 6,
    CHIP_TYPE_2708 = 7,
    CHIP_TYPE_2716 = 8,
    CHIP_TYPE_2732 = 9,
    CHIP_TYPE_2764 = 10,
    CHIP_TYPE_27128 = 11,
    CHIP_TYPE_27256 = 12,
    CHIP_TYPE_27512 = 13,
    CHIP_TYPE_231024 = 14,
    CHIP_TYPE_27C010 = 15,  // And 23C1010
    CHIP_TYPE_27C020 = 16,
    CHIP_TYPE_27C040 = 17,
    CHIP_TYPE_27C080 = 18,
    CHIP_TYPE_27C400 = 19,
    CHIP_TYPE_6116 = 20,
    CHIP_TYPE_27C301 = 21,
    CHIP_TYPE_SYSTEM_PLUGIN = 22,
    CHIP_TYPE_USER_PLUGIN = 23,
    CHIP_TYPE_PIO_PLUGIN = 24,
    CHIP_TYPE_SST39SF040 = 25,
    CHIP_TYPE_28C16 = 26,
    CHIP_TYPE_28C64 = 27,
    CHIP_TYPE_28C256 = 28,
    CHIP_TYPE_28C512 = 29,
    CHIP_TYPE_23QL512 = 30,        // Not a real chip, but serves the Sinclair QL's combined 23256+23128, covering the whole 64KB address space
    CHIP_TYPE_23QL384 = 31,        // Not a real chip, but serves the Sinclair QL's combined 23256+23128, covering $0000-$BFFF (48KB)
    CHIP_TYPE_23C1001 = 32,
    CHIP_TYPE_27C200 = 33,
    NUM_CHIP_TYPES,
    INVALID_CHIP_TYPE = 0xFF
} sdrr_rom_type_t;
_Static_assert(sizeof(sdrr_rom_type_t) == 1, "sdrr_rom_type_t must be 1 byte");
#define CHIP_TYPE_23C1010 CHIP_TYPE_27C010

extern const char* const chip_type_strings[NUM_CHIP_TYPES];
#if defined(ONEROM_CONSTANTS)
const char * const chip_type_strings[NUM_CHIP_TYPES] = {
    "2316",
    "2332",
    "2364",
    "23128",
    "23256",
    "23512",
    "2704",
    "2708",
    "2716",
    "2732",
    "2764",
    "27128",
    "27256",
    "27512",
    "231024",
    "27C010",
    "27C020",
    "27C040",
    "27C080",
    "27C400",
    "6116",
    "27C301",
    "System Plugin",
    "User Plugin",
    "PIO Plugin",
    "SST39SF040",
    "28C16",
    "28C64",
    "28C256",
    "28C512",
    "23QL512",
    "23QL384",
    "23C1001",
    "27C200",
};
_Static_assert(sizeof(chip_type_strings)/sizeof(chip_type_strings[0]) == NUM_CHIP_TYPES,
               "chip_type_strings size doesn't match NUM_CHIP_TYPES");
#endif

// CS state enumeration
typedef enum {
    CS_ACTIVE_LOW,
    CS_ACTIVE_HIGH,
    CS_NOT_USED,
} sdrr_cs_state_t;
_Static_assert(sizeof(sdrr_cs_state_t) == 1, "sdrr_cs_state_t must be 1 byte");

typedef enum {
    BIT_MODE_8  = 0x01,
    BIT_MODE_16 = 0x02,
} bit_modes_t;
_Static_assert(sizeof(bit_modes_t) == 1, "bit_modes_t must be 1 byte");

typedef enum {
    FIRE_SERVE_CPU = 0x00,
    FIRE_SERVE_PIO = 0x01,
} fire_serve_modes_t;
_Static_assert(sizeof(fire_serve_modes_t) == 1, "fire_serve_modes_t must be 1 byte");

typedef enum {
    F401DE = 0x0000,  // 96 KB RAM
    F405 = 0x0001,
    F411 = 0x0002,
    F446 = 0x0003,
    F401BC = 0x0004,  // Only 64KB RAM
    RP2350_LINE = 0x0005,
    MCU_LINE_FORCE_UINT16 = 0xFFFF,
} mcu_line_t;
_Static_assert(sizeof(mcu_line_t) == 2, "mcu_line_t must be 2 bytes");

typedef enum {
    STORAGE_8 = 0x00,
    STORAGE_B = 0x01,
    STORAGE_C = 0x02,
    STORAGE_D = 0x03,
    STORAGE_E = 0x04,
    STORAGE_F = 0x05,
    STORAGE_G = 0x06,
    STORAGE_2MB = 0x07,
    MCU_STORAGE_FORFCE_UINT16 = 0xFFFF,
} mcu_storage_t;
_Static_assert(sizeof(mcu_storage_t) == 2, "mcu_storage_t must be 2 bytes");

// Only ports A-D are exposed on the 64-pin STM32F4s.
// RP2350 has port (bank) 0.
typedef enum {
    PORT_NONE = 0x00,
    PORT_A    = 0x01,
    PORT_B    = 0x02,
    PORT_C    = 0x03,
    PORT_D    = 0x04,
    PORT_0    = 0x05,  // RP2350
} sdrr_mcu_port_t;
_Static_assert(sizeof(sdrr_mcu_port_t) == 1, "sdrr_mcu_port_t must be 1 byte");

// Supported RP2350 clock frequencies
#define FIRE_FREQ_STOCK  0xffff
#define FIRE_FREQ_NONE   0
typedef uint16_t fire_freq_t;
_Static_assert(sizeof(fire_freq_t) == 2, "fire_freq_t must be 2 byte");

// Supported STM32F4 clock frequencies
#define ICE_FREQ_STOCK  0xffff
#define ICE_FREQ_NONE   0
typedef uint16_t ice_freq_t;
_Static_assert(sizeof(ice_freq_t) == 2, "ice_freq_t must be 2 byte");

typedef enum {
    FIRE_VREG_0_55V = 0x00,
    FIRE_VREG_0_60V = 0x01,
    FIRE_VREG_0_65V = 0x02,
    FIRE_VREG_0_70V = 0x03,
    FIRE_VREG_0_75V = 0x04,
    FIRE_VREG_0_80V = 0x05,
    FIRE_VREG_0_85V = 0x06,
    FIRE_VREG_0_90V = 0x07,
    FIRE_VREG_0_95V = 0x08,
    FIRE_VREG_1_00V = 0x09,
    FIRE_VREG_1_05V = 0x0A,
    FIRE_VREG_1_10V = 0x0B,
    FIRE_VREG_1_15V = 0x0C,
    FIRE_VREG_1_20V = 0x0D,
    FIRE_VREG_1_25V = 0x0E,
    FIRE_VREG_1_30V = 0x0F,
    FIRE_VREG_1_35V = 0x10,
    FIRE_VREG_1_40V = 0x11,
    FIRE_VREG_1_50V = 0x12,
    FIRE_VREG_1_60V = 0x13,
    FIRE_VREG_1_65V = 0x14,
    FIRE_VREG_1_70V = 0x15,
    FIRE_VREG_1_80V = 0x16,
    FIRE_VREG_1_90V = 0x17,
    FIRE_VREG_2_00V = 0x18,
    FIRE_VREG_2_35V = 0x19,
    FIRE_VREG_2_50V = 0x1A,
    FIRE_VREG_2_65V = 0x1B,
    FIRE_VREG_2_80V = 0x1C,
    FIRE_VREG_3_00V = 0x1D,
    FIRE_VREG_3_15V = 0x1E,
    FIRE_VREG_3_30V = 0x1F,
    FIRE_VREG_NONE = 0xFE,
    FIRE_VREG_STOCK = 0xFF,
} fire_vreg_t;
_Static_assert(sizeof(fire_vreg_t) == 1, "fire_vreg_t must be 1 byte");

#endif // ENUMS_H
