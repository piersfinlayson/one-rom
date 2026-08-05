// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "test/ffi.h"
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

// The image-select value the firmware read from the sel pins on this boot.
// Lets a test confirm the firmware selected the image the case drove the pins
// for, rather than trusting the stub's own view of what it drove.
uint8_t ffi_image_sel(void) {
    return (uint8_t)RUNTIME->image_sel;
}

// See ffi.h.  base_addr_pin is an offset within the PIO's GPIOBASE window, so
// the absolute first GPIO sampled is gpio_base + base_addr_pin.
uint8_t ffi_serving_alg(ffi_serving_alg_t *out) {
    if (out == NULL || CURRENT_SLOT == NULL || CURRENT_SLOT->alg == NULL) {
        return 0u;
    }
    const onerom_alg_config_t *alg = CURRENT_SLOT->alg;
    if (alg->alg_addr == NULL || alg->alg_cs == NULL || alg->alg_data == NULL) {
        return 0u;
    }

    out->addr_alg = (uint8_t)alg->alg_addr->alg;
    out->cs_alg = (uint8_t)alg->alg_cs->alg;
    out->data_alg = (uint8_t)alg->alg_data->alg;
    out->addr_window_base =
        (uint8_t)(alg->alg_addr->gpio_base + alg->alg_addr->base_addr_pin);
    out->addr_window_pins = alg->alg_addr->num_addr_pins;

    return 1u;
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

// Address-monitor capture DMA wiring.
//
// The firmware's pio_setup_address_monitor_dma has no DMA registers under
// emulation; it calls monitor_dma_configure_cb (installed by
// ffi_epio_arm_monitor) with the block/SM/ring it CHOSE.  We wire epio's
// capture channel from that choice — so a wrong block choice by the firmware
// is caught — and point the firmware's ring-write-position slot at epio's live
// capture write pointer.
static epio_t *s_monitor_epio;

static void monitor_dma_configure_cb(
    uint8_t src_block,
    uint8_t src_sm,
    void *ring_buf,
    uint8_t ring_size_log2,
    uint8_t data_size
) {
    uint32_t ring_base = SRAM_BASE +
        (uint32_t)((uint8_t *)ring_buf - epio_get_sram_ptr(s_monitor_epio));
    epio_dma_setup_capture_pio_ring(s_monitor_epio, DMA_CH_ADDR_MONITOR,
                                    src_block, src_sm, 1,
                                    ring_base, ring_size_log2, data_size);
    set_host_monitor_write_slot((volatile uint32_t * volatile *)
        epio_dma_capture_write_slot(s_monitor_epio, DMA_CH_ADDR_MONITOR));
}

// Arm the address-monitor emulation seam.  Call once after setup_epio and
// before the firmware configures the address monitor.
void ffi_epio_arm_monitor(epio_t *epio) {
    s_monitor_epio = epio;
    set_host_monitor_dma_configure(monitor_dma_configure_cb);
}

extern uint8_t logging_enabled;

void ffi_set_logging(uint8_t enabled) {
    logging_enabled = enabled;
}