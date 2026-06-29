// Function prototypes

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef FUNCTIONS_H
#define FUNCTIONS_H

#include <stdint.h>
#include <stdarg.h>
//#include "enums.h"
#include "onerom_metadata.h"
#include "api.h"

// main.c
int firmware_main(void);

// utils.c
uint32_t check_sel_pins(uint32_t *sel_mask);
void process_firmware_overrides(const onerom_rom_slot_t *slot);
uint8_t metadata_valid(void);
void update_runtime_from_metadata(void);
void limp_mode(limp_mode_pattern_t pattern);
void copy_func_to_ram(void (*fn)(void), uint32_t ram_addr, size_t size);
void execute_ram_func(uint32_t ram_addr);
void delay(volatile uint32_t count);
uint8_t get_rom_slot_index(uint32_t sel_pins, uint32_t sel_mask, uint8_t plugins);
void preload_rom_image();

// log.c
#if defined(BOOT_LOGGING)
void log_init();
void log_roms();
void do_log(const char *, ...);
void err_log(const char *, ...);
#endif // BOOT_LOGGING
void do_log_v(const char* msg, va_list *args);
void do_err_log_prefix();
#if defined(DEBUG_LOGGING)
void do_debug_log_prefix();
#endif // DEBUG_LOGGING

// rp235x.c external functions
//
// If adding a new platform, these are the functions you need to implement,
// plus those in include/*inlines.h
void platform_specific_init(void);
void setup_vbus_interrupt(void);
void vbus_connect_handler(void);
void setup_clock(void);
void setup_initial_gpios(void);
void setup_mco(void);
uint32_t setup_sel_pins(uint64_t *sel_mask, uint64_t *flip_bits);
uint64_t get_sel_value(uint64_t sel_mask, uint64_t flip_bits);
void disable_sel_pins(void);
void setup_status_led(void);
void blink_pattern(uint32_t on_time, uint32_t off_time, uint8_t repeats);
void enter_bootloader(void);
void platform_logging(void);
void setup_usb_controller(void);
void setup_usb_pll(void);
void setup_adc(void);
void setup_status_led(void);
void blink_pattern(uint32_t on_time, uint32_t off_time, uint8_t repeats);

// pio.c
extern int pio(void);
// piorom.c
extern int piorom2(void);
extern int pioram(
    const onerom_info_t *info,
    onerom_runtime_info_t *runtime,
    uint32_t ram_table_addr
);
extern ora_result_t pio_setup_address_monitor(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
);
uint32_t pio_map_addr_to_phys(const onerom_rom_slot_t *slot, uint32_t logical_addr);
uint32_t pio_map_data_to_phys(const onerom_rom_slot_t *slot, uint32_t logical_data);
ora_result_t pio_demangle_addr(
    const onerom_rom_slot_t *slot,
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
);
uint8_t pio_demangle_data(
    const onerom_rom_slot_t *slot,
    uint8_t physical_data
);
ora_result_t pio_init_knock(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint8_t data_size,
    ora_knock_t *knock
);
ora_result_t pio_wait_for_knock(
    const ora_knock_t *knock,
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    uint32_t flags,
    uint32_t *payload_out,
    uint8_t payload_len,
    volatile uint32_t *start_pos,
    volatile uint32_t **next_read_out
);
ora_result_t pio_reprogram_ram_rom_slot(
    uint8_t slot,
    uint32_t offset,
    const uint8_t *data,
    uint32_t len,
    uint8_t allow_active
);
ora_result_t pio_start_address_monitor(void);
volatile uint32_t * volatile *pio_get_address_monitor_ring_write_pos(void);
ora_result_t pio_get_new_rom_ram_region(uint32_t *addr_out, uint32_t *size_out);
uint8_t pio_get_effective_addr_pins(void);
uint32_t pio_get_rom_region_size(void);
ora_result_t pio_switch_rom_region(uint32_t new_region_addr);
ora_result_t pio_read_ram_rom_slot(
    const onerom_rom_slot_t *rom_slot,
    uint8_t   ram_slot,
    uint32_t  offset,
    uint8_t  *buf,
    uint32_t  len
);
uint8_t pio_get_active_ram_slot(void);

// plugin.c
uint8_t check_plugin_valid(
    const ora_plugin_header_t *header,
    const ora_plugin_type_t expected_type,
    uint8_t index
);
uint8_t initial_plugin_parse(uint8_t *disable_vbus_det, uint8_t *num_plugins);
void ora_launch_plugins(void);
void irq_handler_timer0_irq_0(void);
void irq_handler_usbctrl_irq(void);
ora_result_t ora_get_ram_slot_info(uint8_t ram_slot, uint32_t *addr_out, uint32_t *size_out, uint32_t *rom_type_out);
ora_result_t ora_get_active_ram_slot(uint8_t *ram_slot_out);
#if !REAL_HARDWARE
uint8_t *sram_to_host(uint32_t addr);
// Sets the SRAM buffer pointer used by sram_to_host().  Call after
// epio_from_apio() with epio_get_sram_ptr() to unify the firmware's SRAM
// backing store with epio's, so subsequent firmware writes are immediately
// visible to the running epio simulation.
void set_host_sram_ptr(uint8_t *ptr);
#endif // !REAL_HARDWARE

// pio/dma.c
void dma_copy(
    uint32_t src_addr,
    uint32_t dst_addr,
    size_t size_words
);
uint32_t dma_copy_status(void);

#endif // FUNCTIONS_H