// Function prototypes

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef FUNCTIONS_H
#define FUNCTIONS_H

// main.c
extern uint32_t check_sel_pins(uint32_t *sel_mask);
extern void check_enter_bootloader(uint32_t sel_pins, uint32_t sel_mask);
extern uint8_t metadata_present(const sdrr_info_t *info);
extern void limp_mode(limp_mode_pattern_t pattern);
extern void process_firmware_overrides(
    sdrr_runtime_info_t *runtime_info,
    const sdrr_rom_set_t *set
);
#if !defined(TEST_BUILD)
extern int main(void);
#else // TEST_BUILD
extern int firmware_main(void);
#endif // !TEST_BUILD

// utils.c
extern uint32_t check_sel_pins(uint32_t *sel_mask);
extern void do_log_v(const char* msg, va_list *args);
extern void do_err_log_prefix();
#if defined(DEBUG_LOGGING)
extern void do_debug_log_prefix();
#endif // DEBUG_LOGGING
#if defined(BOOT_LOGGING)
extern void log_init();
extern void log_roms(const onerom_metadata_header_t *metadata);
extern void do_log(const char *, ...);
extern void err_log(const char *, ...);
#endif // BOOT_LOGGING
#if defined(MAIN_LOOP_LOGGING) || defined(DEBUG_LOGGING)
typedef void (*ram_log_fn)(const char*, ...);
#endif // MAIN_LOOP_LOGGING
#if defined(EXECUTE_FROM_RAM)
extern void copy_func_to_ram(void (*fn)(void), uint32_t ram_addr, size_t size);
extern void execute_ram_func(uint32_t ram_addr);
#endif // EXECUTE_FROM_RAM
extern void setup_status_led(void);
extern void delay(volatile uint32_t count);
extern void blink_pattern(uint32_t on_time, uint32_t off_time, uint8_t repeats);

// stm32f4.c and rp235x.c external functions
//
// If adding a new platform, these are the functions you need to implement,
// plus those in include/*inlines.h
extern void platform_specific_init(void);
void setup_vbus_interrupt(void);
void vbus_connect_handler(void);
extern void setup_clock(void);
extern void setup_gpio(void);
extern void setup_mco(void);
extern uint32_t setup_sel_pins(uint64_t *sel_mask, uint64_t *flip_bits);
extern uint64_t get_sel_value(uint64_t sel_mask, uint64_t flip_bits);
extern void disable_sel_pins(void);
extern void setup_status_led(void);
extern void blink_pattern(uint32_t on_time, uint32_t off_time, uint8_t repeats);
extern void enter_bootloader(void);
extern void check_config(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set
);
extern void platform_logging(void);
#if defined(STM32F4)
void dfu(void);
#endif // STM32F4
#if defined(RP235X)
void setup_usb_controller(void);
void setup_usb_pll(void);
void setup_adc(void);
uint8_t initial_plugin_parse(uint8_t *disable_vbus_det);
#endif // RP235X

// pio.c
extern int pio(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
);
// piorom.c
#if defined(RP235X)
extern int piorom(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
);
extern int pioram(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set,
    uint32_t rom_table_addr
);
extern ora_result_t pio_setup_address_monitor(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
);
uint32_t pio_map_addr_to_phys(uint32_t logical_addr);
uint32_t pio_map_data_to_phys(uint32_t logical_data);
ora_result_t pio_demangle_addr(
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
);
uint8_t pio_demangle_data(uint8_t physical_data);
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

// plugin.c
extern uint8_t check_plugin_valid(
    const ora_plugin_header_t *header,
    const ora_plugin_type_t expected_type,
    uint8_t index
);
extern void ora_launch_plugins(const sdrr_info_t *info);
extern void irq_handler_timer0_irq_0(void);
extern void irq_handler_usbctrl_irq(void);
ora_result_t ora_get_ram_slot_info(uint8_t ram_slot, uint32_t *addr_out, uint32_t *size_out, uint32_t *rom_type_out);
ora_result_t ora_get_active_ram_slot(uint8_t *ram_slot_out);
#endif // RP235X

// rom_impl.c
#if !defined(TIMER_TEST) && !defined(TOGGLE_PA4)
extern void main_loop(
    const sdrr_info_t *info,
    sdrr_runtime_info_t *runtime,
    const sdrr_rom_set_t *set
);
extern uint8_t get_rom_set_index(uint32_t sel_pins, uint32_t sel_mask, uint8_t plugins);
extern void* preload_rom_image(const sdrr_runtime_info_t *runtime_info, const sdrr_rom_set_t *set);
#endif // !TIMER_TEST && !TOGGLE_PA4

// test function prototypes
#if defined(TIMER_TEST) || defined(TOGGLE_PA4)
extern void main_loop(void);
#endif // TIMER_TEST || TOGGLE_PA4

#if defined(RP235X)
extern void dma_copy(
    uint32_t src_addr,
    uint32_t dst_addr,
    size_t size_words
);
extern uint32_t dma_copy_status(void);
#endif // RP235X

#endif // FUNCTIONS_H