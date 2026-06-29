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
void ffi_epio_setup_sram(epio_t *epio);
void ffi_epio_setup_dma_chain(epio_t *epio, uint8_t word_size);
void ffi_set_logging(uint8_t enabled);