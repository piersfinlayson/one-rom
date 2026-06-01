// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// PIO tester program
//
// This works by:
// - Building the firmware with a test stub.
// - Executing the firmware up to the point where it is about to start serving
//   ROM data, at which point it returns to the test code.
// - Checking that limp mode hasn't been entered, and that the PIO SMs have
//   been enabled.
// - Driving the address and approrpriate CS lines to read the entire ROM
//   configuration, checking the data read matches the expected data for each
//   address.
//
// Current limitations:
// - Doesn't support 40 pin ROMs, and may need enhancing for 32 pin support.
// - Doesn't support testing multiple ROM sets - only tests the first ROM set
//   in the configuration.
// - Doesn't support testing multi-ROM sets, or dyanmically banked ROMs.

#define TEST_MAIN_C

#include <stdio.h>

#include <include.h>
#include <apio.h>
#include <epio.h>
#include <test/stub.h>
#include <test/func.h>

static int check_rom_read(
    epio_t *epio,
    uint8_t set_index,
    uint8_t rom_index,
    uint32_t addr,
    uint8_t bit_mode
);
static void setup_test_infra(void);
static epio_t *start_epio(void);

// Cycle counts to use for tests.  At 150MHz, each cycle is 6.67ns.
//
// Strictly these are more aggressive than the real world, as in the real
// world we have 2 cycles before reading a changed GPIO state due to meta-
// stability handling.
//
// The figures are explicitly tuned as low as possible so changes to the PIOs
// which slow down byte serving will break the tests.
//
// When explicit meta-stability handling is added to epio, these timings will
// need to be relaxed.

// The CS active/inactive loop generally only takes 3 cycles, but may take up
// to 6 in the 2332 case (i.e. non-contiguous CS pins).
#define TST_CYCLES_BEFORE_START             173 // A random number
#define TST_CYCLES_ADDR_BEFORE_CS_ACTIVE    6   // 40ns (+ CS delay)
#define TST_CYCLES_CS_ACTIVE_TO_DATA_READY  6   // 40ns
#define TST_CYCLES_AFTER_READ               6   // 40ns

// Additional delay required for multi-ROM sets as the address can only be
// validly retrieved after CS has gone active.  This allows for another
// address read -> DMA -> data write chain
#define TST_CYCLES_MULTI_ROM_CS_ACTIVE_TO_DATA_READY 12 // 80ns

// The following 27C400 figures are tuned to be just long enough to work with
// the current algorithms, so the tests will break if the algorithms are
// changed without updating these figures. 

#if !defined(FORCE_16_BIT) || (FORCE_16_BIT == 0)
// An additional delay is required for 27C400 when 16bit mode isn't forced as
// the address read loop is deliberately slowed down to 7 cycles to give /BYTE
// mode handling time to complete
#define TST_CYCLES_27C400_ADDR_BEFORE_CS_ACTIVE         13  // 86.7ns (+ CS delay)

// Additional delay required for 27C400 as, due to /BYTE handling, it has a
// longer delay before the byte is correctly served (specifically in /BYTE
// low mode)
#define TST_CYCLES_27C400_BYTE_CS_ACTIVE_TO_DATA_READY  9   // 60ns

#else // FORCE_16_BIT

#define TST_CYCLES_27C400_ADDR_BEFORE_CS_ACTIVE         8   // 53.3ns (+ CS delay)

// BYTE mode not supported - required so code compiles, but set it to a
// nonsensical value
#define TST_CYCLES_27C400_BYTE_CS_ACTIVE_TO_DATA_READY  0

#endif // FORCE_16_BIT

// addr is a word address in 16-bit mode
static int check_rom_read(
    epio_t *epio,
    uint8_t set_index,
    uint8_t rom_index,
    uint32_t addr,
    uint8_t bit_mode
) {
    uint8_t multi_rom = 0;
    if (rom_set[set_index].rom_count > 1) {
        multi_rom = 1;
    }

    sdrr_rom_type_t rom_type = get_rom_type(set_index, rom_index);

    // Select delay based on ROM type
    uint32_t addr_before_cs_cycles;
    if (rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) {
        addr_before_cs_cycles = TST_CYCLES_27C400_ADDR_BEFORE_CS_ACTIVE;
    } else {
        addr_before_cs_cycles = TST_CYCLES_ADDR_BEFORE_CS_ACTIVE;
    }
    uint32_t cs_to_data_cycles;
    if ((rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) && (bit_mode == 8)) {
        cs_to_data_cycles = TST_CYCLES_27C400_BYTE_CS_ACTIVE_TO_DATA_READY;
    } else if (multi_rom) {
        cs_to_data_cycles = TST_CYCLES_MULTI_ROM_CS_ACTIVE_TO_DATA_READY;
    } else {
         cs_to_data_cycles = TST_CYCLES_CS_ACTIVE_TO_DATA_READY;
    }

    // Check the data pins are undriven before starting
    if (are_cs_active_all_high(set_index, rom_index)) {
        assert(
            rom_type == CHIP_TYPE_2316 ||
            rom_type == CHIP_TYPE_2332 ||
            rom_type == CHIP_TYPE_2364 ||
            rom_type == CHIP_TYPE_23128 ||
            rom_type == CHIP_TYPE_23256 ||
            rom_type == CHIP_TYPE_23512 ||
            rom_type == CHIP_TYPE_231024 ||
            rom_type == CHIP_TYPE_23QL512 ||
            rom_type == CHIP_TYPE_23QL384 ||
            rom_type == CHIP_TYPE_27C080
        );

        if (rom_type == CHIP_TYPE_27C080) {
            // 27C080 is only in this list as only CS1 is marked as active high
            // in the config, but CE and OE are actually active low.  Hence we
            // can safely check that data lines are undriven between reads.
            check_data_pins_undriven(epio, bit_mode);
        }
    } else {
        check_data_pins_undriven(epio, bit_mode);
    }

    // First step is to drive the address GPIOs with CS inactive.
    uint64_t gpios_driven;
    uint64_t gpio_levels;
    get_gpio_drive(
        set_index,
        rom_index,
        addr,
        0,
        &gpios_driven,
        &gpio_levels,
        bit_mode
    );
    epio_drive_gpios_ext(epio, gpios_driven, gpio_levels);
    epio_step_cycles(epio, addr_before_cs_cycles);

    // Now set CS active
    get_gpio_drive(
        set_index,
        rom_index,
        addr,
        1,
        &gpios_driven,
        &gpio_levels,
        bit_mode
    );
    epio_drive_gpios_ext(epio, gpios_driven, gpio_levels);
    //TST_LOG("Address 0x%08X driven with CS active", addr);
    epio_step_cycles(epio, cs_to_data_cycles);

    // Check the data lines are being actively driven
    int rc = 0;
    if (rom_type != CHIP_TYPE_23QL384 || addr < chip_size_from_type[rom_type]) {
        // For 23QL384, the upper 16KB of the 64KB address space is not actually
        // backed by ROM and won't drive data lines, so skip the check in that
        // case.
        check_data_pins_driven(epio, bit_mode);

        // Read the data lines
        uint64_t gpio_out = epio_read_pin_states(epio);
        uint32_t data = get_byte_from_gpio(gpio_out, bit_mode);

        // Test whether we got the expected data
        uint16_t expected_data;
        if (bit_mode == 16) {
            // Construct from the two appropriate bytes in the ROM image
            expected_data = 
                (uint16_t)get_rom_image_data_byte(set_index, rom_index, addr * 2) |
                ((uint16_t)get_rom_image_data_byte(set_index, rom_index, addr * 2 + 1) << 8);
        } else {
            expected_data = get_rom_image_data_byte(set_index, rom_index, addr);
        }
        if (data != expected_data) {
            if (get_progress() < 5) {
                TST_LOG("ROM Read Mismatch: 0x%08X expected 0x%04X got 0x%04X", addr, expected_data, data);
            }
            rc = 1;
        }
    } else {
        // Check the lines are undriven instead
        check_data_pins_undriven(epio, bit_mode);
    }

    // Stop driving GPIOs and step a bit, as it isn't realistic to have the
    // next read drive the address pins immediately.
    get_gpio_drive(
        set_index,
        rom_index,
        -1,
        2,
        &gpios_driven,
        &gpio_levels,
        bit_mode
    );
    epio_drive_gpios_ext(epio, gpios_driven, gpio_levels);
    epio_step_cycles(epio, TST_CYCLES_AFTER_READ);

    inc_progress();

    return rc;
}

static void setup_test_infra(void) {
    setup_addr_pins();
    setup_data_pins();
    setup_rom_images();
    TST_LOG_FILE_CLEAR();
}

static epio_t *start_epio(void) {
    return epio_from_apio();
}

static int test_set(uint8_t set_index) {
    TST_DBG("Testing ROM set %d", set_index);

    uint8_t set_sel_index = stub_set_sel_image(set_index);
    if (set_sel_index != set_index) {
        TST_LOG("WARNING: Insufficient image select jumpers for set %d, testing set %d instead", set_index, set_sel_index);
    }
    set_index = set_sel_index;

    TST_DBG("Launching firmware");

    // Redirect firmware logging to file
    TST_LOG_TO_FILE();

    // Run the firmware - it will return when it hits the infinite loop.
    firmware_main();

    // Re-instate stdout and close the log file
    TST_LOG_RESET_STDOUT();

    if (limp_mode_value != LIMP_MODE_NONE) {
        TST_LOG("Firmware entered limp mode with pattern %d", limp_mode_value);
        return 1;
    }

    if (_apio_emulated_pio.pios_enabled != 1) {
        TST_LOG("PIO programs were not enabled: %d", _apio_emulated_pio.pios_enabled);
        return 1;
    }

    // Start the PIO emulator to run the PIOs/DMAs
    epio_t *epio = start_epio();
    if (!epio) {
        TST_LOG("Failed to initialize EPIO");
        return 1;
    }

    // Copy from the test_ram_rom_image)_table to epio
    uint64_t *source = get_ram_rom_image_table_aligned();
    epio_sram_set(epio, 0x20000000, (uint8_t *)source, 512*1024);

    // Configure the DMA chain
    uint8_t bit_mode = 8;
    sdrr_rom_type_t rom_type = get_rom_type(set_index, 0);
    if (rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) {
        bit_mode = 16;
    }
    epio_dma_setup_read_pio_chain(
        epio,
        0,
        1,
        0,
        4,
        2,
        1,
        4,
        bit_mode
    );

    // Step the emulator some cycles before we start
    TST_DBG("Stepping emulator for %d cycles before starting tests", TST_CYCLES_BEFORE_START);
    epio_step_cycles(epio, TST_CYCLES_BEFORE_START);

    // Now loop through each ROM image and each address
    uint8_t rom_index = 0;
    uint8_t rom_count = rom_set[set_index].rom_count;
    uint32_t failures = 0;
    for (rom_index = 0; rom_index < rom_count; rom_index++) {
        const sdrr_rom_info_t *rom = rom_set[set_index].roms[rom_index];
        rom_type = get_rom_type(set_index, rom_index);

        // Do two passes for 27C400/27C200, one for 16 bit mode, the other in 8-bit mode
#if defined(FORCE_16_BIT) && (FORCE_16_BIT == 1 )
        uint8_t num_passes = 1;
#else // !FORCE_16_BIT
        uint8_t num_passes = (rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) ? 2 : 1;
#endif // FORCE_16_BIT
        for (uint8_t pass = 0; pass < num_passes; pass++) {
            // Figure out whether to test in 8 or 16 bit mode for this pass
            uint8_t pass_bit_mode = ((rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) && pass == 0) ? 16 : 8;

            // Get the ROM size, and halve it for 16-bit mode, to use word addresses
            uint32_t rom_size = get_rom_image_size(set_index, rom_index);
            if (rom_type == CHIP_TYPE_23QL384) {
                // Round up to 64KB (from 48KB) and, in check_rom_read, deal
                // with the fact that the ROM shouldn't serve about 48KB.
                rom_size = 65536;
            }
            uint32_t iter_count = ((rom_type == CHIP_TYPE_27C400 || rom_type == CHIP_TYPE_27C200) && pass_bit_mode == 16)
                ? rom_size / 2
                : rom_size;

            TST_LOG("Testing ROM read for set %d image %d %s %s %d bytes (%d-bit mode) ...",
                set_index, rom_index, chip_type[rom->rom_type], rom->filename, rom_size,
                pass_bit_mode);

            // Run the test for all addresses
            reset_progress();
            uint32_t pass_failures = 0;
            for (uint32_t ii = 0; ii < iter_count; ii++) {
                // We pass in a word address in 16-bit mode
                int r = check_rom_read(epio, set_index, rom_index, ii, pass_bit_mode);
                pass_failures += r;
                failures += r;
            }
            if (pass_failures == 0) {
                TST_LOG("  read %d bytes successfully", rom_size);
            } else {
                TST_LOG("  ERROR - %d/%d ROM bytes read successfully", rom_size - pass_failures, rom_size);
            }
        }
    }

    epio_free(epio);

    return failures == 0 ? 0 : 1;
}

int main(int argc, char *argv[]) {
    (void)argc;
    (void)argv;

    TST_LOG("One ROM Fire Firmware Tester");
    TST_LOG("%s %s", copyright, author);
    TST_LOG("");
    TST_LOG("Build date/time:   %s", sdrr_info.build_date);
    TST_LOG("Firmware version:  %d.%d.%d", sdrr_info.major_version, sdrr_info.minor_version, sdrr_info.patch_version);
    if (sdrr_info.build_number) {
        TST_LOG("Firmware build:    %d", sdrr_info.build_number);
    }
    TST_LOG("Hardware revision: %s", sdrr_info.hw_rev);
    TST_LOG("-----");

    // Set up the test infrastructure, also does some checking of the captured
    // config to make sure it looks sane.
    setup_test_infra();

    // Now test each set in turn.
    assert(sdrr_info.metadata_header->rom_set_count == SDRR_NUM_SETS);
    int rc;
    for (int ii = 0; ii < SDRR_NUM_SETS; ii++) {
        rc = test_set(ii);
        if (rc != 0) {
            break;
        }
    }

    free_src_rom_images();

    return rc;
}