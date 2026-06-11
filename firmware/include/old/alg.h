// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Contains types for One ROM's ROM serving algorithms

#if !defined(ALG_H)
#define ALG_H

#include <stdint.h>
#include "macros.h"
#include "enums.h"

typedef struct onerom_alg_config_t onerom_alg_config_t;
typedef struct onerom_alg_cs_config_t onerom_alg_cs_config_t;
typedef struct onerom_alg_addr_config_t onerom_alg_addr_config_t;
typedef struct onerom_alg_data_config_t onerom_alg_data_config_t;
typedef struct onerom_alg_dma_config_t onerom_alg_dma_config_t;
typedef struct onerom_alg_pull_config_t onerom_alg_pull_config_t;
typedef struct onerom_alg_override_config_t onerom_alg_override_config_t;

// ROM serving algorithms
//
// Used by pro-processor to indicate which PIO serving algorithm the firmware
// should use to serve a particular ROM slot.
typedef enum {
    ALG_CS_0 = 0,
    ALG_CS_1 = 1,
    ALG_CS_2 = 2,
    NUM_CS_ALGS
} onerom_alg_cs_t;
typedef enum {
    ALG_ADDR_0 = 0,
    NUM_ADDR_ALGS
} onerom_alg_addr_t;
typedef enum {
    ALG_DATA_0 = 0,
    ALG_DATA_1 = 1,
    NUM_DATA_ALGS
} onerom_alg_data_t;
typedef enum {
    ALG_DMA_0 = 0,
    NUM_DMA_ALGS
} onerom_alg_dma_t;
STATIC_ASSERT(sizeof(onerom_alg_cs_t) == 1, "onerom_alg_cs_t must be 1 byte");
STATIC_ASSERT(sizeof(onerom_alg_addr_t) == 1, "onerom_alg_addr_t must be 1 byte");
STATIC_ASSERT(sizeof(onerom_alg_data_t) == 1, "onerom_alg_data_t must be 1 byte");
STATIC_ASSERT(sizeof(onerom_alg_dma_t) == 1, "onerom_alg_dma_t must be 1 byte");

// All serving algorithm configuration for a specific ROM slot.  An instance
// of this structure can be reused by all ROM slots using the same algorithms
// and parameters.
typedef struct onerom_alg_config_t {
    const onerom_alg_cs_config_t * const alg_cs;
    const onerom_alg_addr_config_t * const alg_addr;
    const onerom_alg_data_config_t * const alg_data;
    const onerom_alg_dma_config_t * const alg_dma;
    const onerom_alg_pull_config_t * const gpio_pull_config;
    const onerom_alg_override_config_t * const gpio_override_config;

    // Allow for future expansion.
    uint8_t reserved[2 * 4];
} onerom_alg_config_t;
STATIC_ASSERT(sizeof(onerom_alg_config_t) == 32, "onerom_alg_config_t must be 32 bytes");

// Chip select algorithm configuration
typedef struct onerom_alg_cs_config_t {
    // The chip select algorithm to use
    onerom_alg_cs_t alg;

    // Length of the parameters.
    uint8_t param_len;

    // CLKDIV INT and FRAC values for the SM.
    uint16_t clkdiv_int;
    uint8_t clkdiv_frac;

    uint8_t gpio_base;
    uint8_t base_cs_pin;
    uint8_t num_cs_pins;
    uint8_t base_data_pin;
    uint8_t num_data_pins;
    uint8_t cs_active_delay;
    uint8_t cs_inactive_delay;

    // Configuration parameteres for this algorithm.  The format of these
    // parameters are algortithm specific.
    uint8_t params[];
} onerom_alg_cs_config_t;
STATIC_ASSERT(sizeof(onerom_alg_cs_config_t) == 12, "onerom_alg_cs_config_t must be 2 bytes");

// Address reading algorithm configuration
typedef struct onerom_alg_addr_config_t {
    // The address algorithm to use
    onerom_alg_addr_t alg;

    // Length of the parameters.
    uint8_t param_len;

    // CLKDIV INT and FRAC values for the SM.
    uint16_t clkdiv_int;
    uint8_t clkdiv_frac;

    uint8_t gpio_base;
    uint8_t num_delay_cycles;
    uint8_t base_addr_pin;
    uint8_t num_addr_pins;
    uint8_t num_rom_table_bits;

    // Configuration parameteres for this algorithm.  The format of these
    // parameters are algortithm specific.
    uint8_t params[];
} onerom_alg_addr_config_t;
STATIC_ASSERT(sizeof(onerom_alg_addr_config_t) == 10, "onerom_alg_addr_config_t must be 8 bytes");

// Data serving algorithm configuration
typedef struct onerom_alg_data_config_t {
    // The data algorithm to use
    onerom_alg_data_t alg;

    // Length of the parameters
    uint8_t param_len;

    // CLKDIV INT and FRAC values for the SM.
    uint16_t clkdiv_int;
    uint8_t clkdiv_frac;

    uint8_t gpio_base;
    uint8_t base_data_pin;
    uint8_t word_size;      // In bits - e.g. 8 or 16

    // Configuration parameteres for this algorithm.  The format of these
    // parameters are algortithm specific.
    uint8_t params[];
} onerom_alg_data_config_t;
STATIC_ASSERT(sizeof(onerom_alg_data_config_t) == 8, "onerom_alg_data_config_t must be 8 bytes");

// DMA configuration
typedef struct onerom_alg_dma_config_t {
    // The DMA algorithm to use
    onerom_alg_dma_t alg;

    // Length of the parameters
    uint8_t param_len;

    bit_modes_t bit_mode;   // 8 or 16 bit mode
    uint8_t continuous;     // 0 = single shot, 1 = continuous mode

    uint8_t params[];
} onerom_alg_dma_config_t;
STATIC_ASSERT(sizeof(onerom_alg_dma_config_t) == 4, "onerom_alg_dma_config_t must be 4 bytes");

// CS Standard algorithm - parameters
//
// This is the most common algorithm, used for single ROMs with contiguous CS
// pins.  Supports both 8 and 16 bit ROMs.
#define ALG_CS0_PARAMS_LEN 4
typedef struct onerom_alg_cs0_param_t {
    uint8_t serve_cs_low_0;     // 0 = serve when CS pins are low, 1 = high
    uint8_t byte_pin;           //  0xFF if unused

    // The below are only used for RBCP and only filled in for multi-ROM sets
    uint8_t first_rom_cs_base;  // For multi-ROMs, the base CS pin for the first ROM
    uint8_t first_rom_num_cs_pins; // For multi-ROMs, the number of CS pins used by the first ROM (contiguous)
} onerom_alg_cs0_param_t;
STATIC_ASSERT(sizeof(onerom_alg_cs0_param_t) == ALG_CS0_PARAMS_LEN, "onerom_alg_cs0_param_t mis-sized");

// CS non-contig sigle gap algorithm - parameters
//
// Used for single ROMs where there is a break of up to 1 pin in the entire CS
// pin range.
#define ALG_CS1_PARAMS_LEN 1
typedef struct onerom_alg_cs1_param_t {
    uint8_t cs_ignore_index;    // E.g. 1 = ignore second CS pin
} onerom_alg_cs1_param_t;
STATIC_ASSERT(sizeof(onerom_alg_cs1_param_t) == ALG_CS1_PARAMS_LEN, "onerom_alg_cs1_param_t must be 1 byte");

// CS Enable Address Qualified algorithm - parameters
//
// Used where a single global enable pin (such as /OE or /CE) gates output,
// with address qualifier pins providing bank selection. Active when the enable
// pin is asserted AND qualifier pins do not match the inactive pattern.
#define ALG_CS2_PARAMS_LEN 3
typedef struct onerom_alg_cs2_param_t {
    uint8_t base_qualifier_pin;         // Indexed from GPIO_BASE
    uint8_t num_qualifier_pins;         // Including any gap pins
    uint8_t qualifier_inactive_pattern; // Bit pattern when bank not selected (Y preload)
} onerom_alg_cs2_param_t;
STATIC_ASSERT(sizeof(onerom_alg_cs2_param_t) == ALG_CS2_PARAMS_LEN, "onerom_alg_cs2_param_t must be 3 bytes");

// Address standard algorithm - parameters
//
// Used for all ROMs where /BYTE mode need not to be handled.
#define ALG_ADDR0_PARAMS_PRE_LIST_LEN 0
typedef struct onerom_alg_addr0_param_t {
} onerom_alg_addr0_param_t;
STATIC_ASSERT(sizeof(onerom_alg_addr0_param_t) == ALG_ADDR0_PARAMS_PRE_LIST_LEN, "onerom_alg_addr0_param_t must be 0 bytes");

// Data word serving algorithm - parameters
//
// Used for all ROMs where /BYTE mode need not to be handled.
#define ALG_DATA0_PARAMS_LEN 0
typedef struct onerom_alg_data0_param_t {
} onerom_alg_data0_param_t;
STATIC_ASSERT(sizeof(onerom_alg_data0_param_t) == 0, "onerom_alg_data0_param_t must be 0 bytes");

// Data word with byte mode serving algorithm - parameters
//
// Used for all ROMs where /BYTE mode needs to be handled.  Assumes 16 bit
// words, and 16 data pins.
#define ALG_DATA1_PARAMS_LEN 2
typedef struct onerom_alg_data1_param_t {
    uint8_t byte_pin;
    uint8_t a_minus_1_pin;
} onerom_alg_data1_param_t;
STATIC_ASSERT(sizeof(onerom_alg_data1_param_t) == ALG_DATA1_PARAMS_LEN, "onerom_alg_data1_param_t must be 2 bytes");

#define ALG_DMA0_PARAMS_LEN 0
typedef struct onerom_alg_dma0_param_t {
} onerom_alg_dma0_param_t;
STATIC_ASSERT(sizeof(onerom_alg_dma0_param_t) == ALG_DMA0_PARAMS_LEN, "onerom_alg_dma0_param_t must be 0 bytes");

extern const uint8_t alg_cs_params_len[NUM_CS_ALGS];
extern const uint8_t alg_addr_params_len[NUM_ADDR_ALGS];
extern const uint8_t alg_data_params_len[NUM_DATA_ALGS];
extern const uint8_t alg_dma_params_len[NUM_DMA_ALGS];

// List of GPIOs to pull using internal pull-ups or pull-downs
typedef struct onerom_alg_pull_config_t {
    uint8_t param_len;

    // The GPIOs to pull, as absolute GPIO numbers.  The MSB of each byte
    // indicates whether to pull up (1) or down (0).
    uint8_t params[];
} onerom_alg_pull_config_t;

// List of GPIOs to override - either invert or force to read 0 or 1
typedef struct onerom_alg_override_config_t {
    uint8_t param_len;

    // The GPIOs to override, as absolute GPIO numbers.  The top two MSBs of
    // each byte indicate the override type, using gpio_override_t, shifted
    // left by 6 bits.
    uint8_t params[];
} onerom_alg_override_config_t;

#endif // ALG_H