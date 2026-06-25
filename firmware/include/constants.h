// Contains constants

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef CONSTANTS_H
#define CONSTANTS_H

// Metadata strings to include in the binary
extern const char license[];
extern const char project_url[];
extern const char copyright[];
extern const char author[];
extern const char product[];
extern const char rom_type[];
extern const char log_divider[];
extern const char inverted[];
extern const char r2364[];
extern const char r2332[];
extern const char r2316[];
extern const char unknown[];
extern const char cs_low[];
extern const char cs_high[];
extern const char cs_na[];
extern const char flash[];
extern const char ram[];
extern const char rom_type[];
extern const char stm32_bootloader_mode[];
extern const char disabled[];
extern const char enabled[];
extern const char oscillator[];
extern const char * const port_names[];
extern const char * const cs_values[];
extern const char version_str[];
extern const uint32_t version_str_len;
extern const uint8_t max_gpios[2];
extern const uint8_t alg_cs_params_len[NUM_CS_ALGS];
extern const uint8_t alg_addr_params_len[NUM_ADDR_ALGS];
extern const uint8_t alg_data_params_len[NUM_DATA_ALGS];
extern const uint8_t alg_dma_params_len[NUM_DMA_ALGS];
extern const char* const chip_type_strings[NUM_CHIP_TYPES];

#endif // CONSTANTS_H
