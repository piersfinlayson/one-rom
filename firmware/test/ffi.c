// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "test/stub.h"
#include "piodma/piodma.h"
#include <epio.h>
#include <apio.h>

extern onerom_runtime_info_t onerom_runtime_info;

void *ffi_runtime_info_ptr(void) {
    return &onerom_runtime_info;
}

uint32_t ffi_runtime_info_size(void) {
    return (uint32_t)sizeof(onerom_runtime_info);
}

uint8_t ffi_limp_mode(void) {
    return (uint8_t)limp_mode_value;
}

uint8_t ffi_pios_enabled(void) {
    return (uint8_t)_apio_emulated_pio.pios_enabled;
}

void ffi_epio_setup_sram(epio_t *epio) {
    uint64_t *source = get_ram_rom_image_table_aligned();
    epio_sram_set(epio, SRAM_BASE, (uint8_t *)source, RAM_ROM_TABLE_SIZE);
}

void ffi_epio_setup_dma_chain(epio_t *epio, uint8_t word_size) {
    epio_dma_setup_read_pio_chain(
        epio,
        DMA_CH_ADDR_READ,
        BLOCK_ADDR,
        SM_ADDR_READ,
        4,
        BLOCK_CS_DATA,
        SM_DATA_WRITE,
        4,
        word_size
    );
}

extern uint8_t logging_enabled;

void ffi_set_logging(uint8_t enabled) {
    logging_enabled = enabled;
}