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

// The two plugin context slots, read where an interrupt handler on a device
// reads them.
//
// These are returned through a call rather than by binding the runtime info
// struct itself.  Binding it would put its layout, and the layout of every
// struct it contains, across the FFI boundary - and the bindings are parsed
// for the host while also being compiled for wasm, where the pointer width
// differs, so the sizes bindgen asserts do not hold there.
void *ffi_system_plugin_context(void) {
    return onerom_runtime_info.system_plugin_context;
}

void *ffi_user_plugin_context(void) {
    return onerom_runtime_info.user_plugin_context;
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

// The LED engine's frame, which a device reaches through TIMER0 alarm 1.  There
// is no alarm in this process, so a harness stands where the interrupt does:
// it moves the clock to ffi_led_next_deadline() and calls this.
void ffi_led_frame(void) {
    pio_led_frame();
}

// When the engine next wants a frame, in the milliseconds a plugin sees.
// Returns 0 when nothing is animating and no hold is running.
uint8_t ffi_led_next_deadline(uint32_t *ms_out) {
    return pio_led_next_deadline(ms_out);
}

// The last colour the engine sent to the RGB LED, and how many it has sent.
uint32_t ffi_led_last_pixel(uint32_t *count_out) {
    return pio_led_last_pixel(count_out);
}

// Take the engine back to the state it holds before boot sets an LED, so a
// test starts from a known one rather than from what the test before it left.
// Neither LED is driven from here - what a channel is doing is forgotten, not
// turned off - so a test that cares about the pin sets a mode after it.
void ffi_led_reset(void) {
    pio_led_reset();
}

void ffi_set_logging(uint8_t enabled) {
    logging_enabled = enabled;
}