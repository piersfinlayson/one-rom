// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#define ONEROM_CONSTANTS

#include "include.h"

// Metadata strings to include in the binary
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

#define STRINGIFY(x) #x
#define TOSTRING(x) STRINGIFY(x)
#define ONEROM_VERSION_STRING \
    "v" TOSTRING(ONEROM_VERSION_MAJOR) \
    "." TOSTRING(ONEROM_VERSION_MINOR) \
    "." TOSTRING(ONEROM_VERSION_PATCH)
const char version_str[] = ONEROM_VERSION_STRING;
const uint32_t version_str_len = sizeof(ONEROM_VERSION_STRING);

// Indexed by rp235x_variant_t, gives the maximum GPIO number for that variant.
const uint8_t max_gpios[2] = {
    48,
    30
};

// Algorithm length arrays
const uint8_t alg_cs_params_len[NUM_CS_ALGS] = {
    ALG_CS0_PARAMS_LEN,
    ALG_CS1_PARAMS_LEN,
    ALG_CS2_PARAMS_LEN
};
const uint8_t alg_addr_params_len[NUM_ADDR_ALGS] = {
    ALG_ADDR0_PARAMS_PRE_LIST_LEN
};
const uint8_t alg_data_params_len[NUM_DATA_ALGS] = {
    ALG_DATA0_PARAMS_LEN,
    ALG_DATA1_PARAMS_LEN
};
const uint8_t alg_dma_params_len[NUM_DMA_ALGS] = {
    ALG_DMA0_PARAMS_LEN
};

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
STATIC_ASSERT(sizeof(chip_type_strings)/sizeof(chip_type_strings[0]) == NUM_CHIP_TYPES,
               "chip_type_strings size doesn't match NUM_CHIP_TYPES");
