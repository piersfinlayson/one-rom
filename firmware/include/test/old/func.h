// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Function protoypes

#include <include.h>

// test_gpio.c
extern void get_gpio_drive(
    uint8_t set_index,
    uint8_t rom_index,
    int32_t addr,
    uint8_t cs_active,
    uint64_t *gpios_to_drive,
    uint64_t *gpio_levels,
    uint8_t bit_mode
);
extern void setup_addr_pins(void);
extern void setup_data_pins(void);
extern uint32_t get_byte_from_gpio(uint64_t gpio_in, uint8_t data_bits);
extern void check_data_pins_driven(epio_t *epio, uint8_t bit_mode);
extern void check_data_pins_undriven(epio_t *epio, uint8_t bit_mode);
uint8_t are_cs_active_all_high(uint8_t set_index, uint8_t rom_index);

// test_image.c
extern void setup_rom_images(void);
extern void free_src_rom_images(void);
extern const uint8_t *get_src_rom_image_data(uint8_t set_index, uint8_t image_index);
extern sdrr_rom_type_t get_rom_type(uint8_t set_index, uint8_t image_index);
extern uint32_t get_rom_image_size(uint8_t set_index, uint8_t image_index);
extern uint8_t get_rom_image_data_byte(uint8_t set_index, uint8_t image_index, uint32_t addr);
extern const char *get_rom_image_name(uint8_t set_index, uint8_t image_index);

// test_log.c
extern void stub_log(const char* msg, ...);
extern void clear_log_file(void);
extern void redirect_stdout_to_file(void);
extern void reset_stdout(void);
extern void inc_progress(void);
extern uint32_t get_progress(void);
extern void reset_progress(void);
extern const char *chip_type[NUM_CHIP_TYPES];
extern const char *transform_to_str[];

// stub_rp235x.c
uint8_t stub_set_sel_image(uint8_t image_index);

// Logging macros
#define TST_LOG_FILE_CLEAR clear_log_file
#define TST_LOG_TO_FILE redirect_stdout_to_file
#define TST_LOG_RESET_STDOUT reset_stdout

#define TST_LOG(...) stub_log(__VA_ARGS__)
#if defined(DEBUG_TEST)
#define TST_DBG(...) stub_log(__VA_ARGS__)
#else // !DEBUG_TEST
#define TST_DBG(...)
#endif // DEBUG_TEST
