// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#pragma once
#include <stdint.h>
#include <epio.h>

void *ffi_runtime_info_ptr(void);
uint32_t ffi_runtime_info_size(void);
uint8_t ffi_limp_mode(void);
uint8_t ffi_pios_enabled(void);
uint8_t ffi_image_sel(void);

// The serving algorithms and address window the current ROM slot is running.
//
// A test needs the address state machine's sampled pin window to know whether a
// chip select transition changes the SRAM index (window covers it) or only
// gates the data output drivers (it does not) — the two have very different
// CS-to-valid-data costs.  Reported from the live slot config rather than
// re-derived host side, so it cannot drift from what the firmware is running.
typedef struct ffi_serving_alg_t {
    uint8_t addr_alg;       // onerom_alg_addr_t
    uint8_t cs_alg;         // onerom_alg_cs_t
    uint8_t data_alg;       // onerom_alg_data_t
    uint8_t addr_window_base;   // first GPIO the address SM samples
    uint8_t addr_window_pins;   // how many GPIOs it samples
} ffi_serving_alg_t;

// Returns 1 and fills `out` when a ROM slot is being served, 0 otherwise.
uint8_t ffi_serving_alg(ffi_serving_alg_t *out);
void ffi_epio_setup_sram(epio_t *epio);
void ffi_epio_setup_dma_chain(epio_t *epio, uint8_t word_size);
void ffi_epio_arm_monitor(epio_t *epio);
void ffi_set_logging(uint8_t enabled);