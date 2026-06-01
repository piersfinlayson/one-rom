// Contains constants

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#define ONEROM_CONSTANTS

#include "include.h"

// Metadata strings to include in the binary
const char mcu_variant[] = MCU_VARIANT;
const char license[] = "MIT License";
const char project_url[] = "https://onerom.org";
const char copyright[] = "Copyright (c) 2026";
const char author[] = "Piers Finlayson <piers@piers.rocks>";
const char product[] = "One ROM";
const char log_divider[] = "-----";
const char inverted[] = "~";
const char r2364[] = "2364";
const char r2332[] = "2332";
const char r2316[] = "2316";
const char unknown[] = "unknown";
const char cs_low[] = "0";
const char cs_high[] = "1";
const char cs_na[] = "-";
const char flash[] = "Flash";
const char ram[] = "RAM";
const char rom_type[] = "ROM type";
const char stm32_bootloader_mode[] = "STM32 bootloader mode";
const char disabled[] = "disabled";
const char enabled[] = "enabled";
const char oscillator[] = "Oscillator";
#if defined(BOOT_LOGGING)
const char * const port_names[] = {"NONE", "A", "B", "C", "D", "0"};
const char * const cs_values[] = {"Active Low", "Active High", "-"};
#endif // BOOT_LOGGING
const uint32_t chip_size_from_type[NUM_CHIP_TYPES] = {
    2048,   // CHIP_TYPE_2316
    4096,   // CHIP_TYPE_2332
    8192,   // CHIP_TYPE_2364
    16384,  // CHIP_TYPE_23128
    32768,  // CHIP_TYPE_23256
    65536,  // CHIP_TYPE_23512
    512,    // CHIP_TYPE_2704
    1024,   // CHIP_TYPE_2708
    2048,   // CHIP_TYPE_2716
    4096,   // CHIP_TYPE_2732
    8192,   // CHIP_TYPE_2764
    16384,  // CHIP_TYPE_27128
    32768,  // CHIP_TYPE_27256
    65536,  // CHIP_TYPE_27512
    131072, // CHIP_TYPE_231024
    131072, // CHIP_TYPE_27C010
    262144, // CHIP_TYPE_27C020
    524288, // CHIP_TYPE_27C040
    1048576,// CHIP_TYPE_27C080
    524288, // CHIP_TYPE_27C400
    2048,   // CHIP_TYPE_6116
    131072, // CHIP_TYPE_27C301
    65536,  // CHIP_TYPE_SYSTEM_PLUGIN
    65536,  // CHIP_TYPE_USER_PLUGIN
    65536,  // CHIP_TYPE_PIO_PLUGIN
    524288, // CHIP_TYPE_SST39SF040
    2048,   // CHIP_TYPE_28C16
    8192,   // CHIP_TYPE_28C64
    32768,  // CHIP_TYPE_28C256
    65536,  // CHIP_TYPE_28C512
    65536,  // CHIP_TYPE_23QL512
    49152,  // CHIP_TYPE_23QL384
    131072, // CHIP_TYPE_23C1001
    262144, // CHIP_TYPE_27C200
};
#define STRINGIFY(x) #x
#define TOSTRING(x) STRINGIFY(x)
#define SDRR_VERSION_STRING \
    "v" TOSTRING(SDRR_VERSION_MAJOR) \
    "." TOSTRING(SDRR_VERSION_MINOR) \
    "." TOSTRING(SDRR_VERSION_PATCH)
const char version_str[] = SDRR_VERSION_STRING;
const uint32_t version_str_len = sizeof(SDRR_VERSION_STRING);