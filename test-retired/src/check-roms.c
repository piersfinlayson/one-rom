// Functions to check the compiled ROMs against the original files

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "roms-test.h"
#include "json-config.h"

int validate_all_rom_sets(json_config_t *json_config, loaded_rom_t *loaded_roms, rom_config_t *configs, int count) {
    printf("\n=== Validating All ROM Sets ===\n");

    assert(json_config != NULL);
    create_byte_demangler(json_config);

    int total_errors = 0;
    int total_checked = 0;

    int overall_rom_idx = 0;

    size_t rom_pins = json_config->rom.pin_count;
    
    // Validate each ROM set
    for (int set_idx = 0; set_idx < sdrr_rom_set_count; set_idx++) {
        printf("\nValidating ROM set %d (ROM type %d)...\n", set_idx, rom_set[set_idx].roms[0]->rom_type);

        int errors = 0;
        int checked = 0;
        
        uint8_t num_roms = rom_set[set_idx].rom_count;
        sdrr_serve_t serve = rom_set[set_idx].serve;
        const sdrr_rom_info_t *rom = rom_set[set_idx].roms[0];
        const sdrr_rom_type_t rom_type = rom->rom_type;
        const uint8_t num_cs = get_num_cs(rom_type);
        uint8_t *cs_combos;
        uint8_t num_cs_combos = cs_combinations(rom_type, &cs_combos);
        const uint8_t cs_active_level[] = {
            rom->cs1_state == CS_ACTIVE_HIGH ? 1 : 0,
            rom->cs2_state == CS_ACTIVE_HIGH ? 1 : 0,
            rom->cs3_state == CS_ACTIVE_HIGH ? 1 : 0
        };

        // We created the address mangler on a per ROM set basis, as it depends on the ROM type
        // being used.
        create_address_mangler(json_config, rom_type);

        if (num_roms == 1) {
            int loaded_rom_idx = overall_rom_idx;
            printf("- Single ROM set\n");
            printf("  - Testing ROM %d in set %d, pins %zu, CS combos %d\n", 0, set_idx, rom_pins, num_cs_combos);
            printf("    - Type: %d/%s, Name: %s\n", rom_type, configs[overall_rom_idx].type, configs[loaded_rom_idx].filename);

            if (rom_pins == 24) {
                // Loop through the entire ROM address space.
                size_t orig_rom_size = get_expected_rom_size(rom_type);
                for (uint32_t original_addr = 0; original_addr < orig_rom_size; original_addr++) {
                    // Now iterate through the CS combinations
                    uint8_t (*cs_combos_2d)[3] = (uint8_t (*)[3])cs_combos;
                    for (int combo_idx = 0; combo_idx < num_cs_combos; combo_idx++) {
                        uint8_t cs1 = cs_combos_2d[combo_idx][0];
                        uint8_t cs2 = cs_combos_2d[combo_idx][1];
                        uint8_t cs3 = cs_combos_2d[combo_idx][2];

                        // Figure out whether this combination should activate the ROM
                        int cs_active = 1;
                        if (num_cs >= 1) {
                            if (cs1 != cs_active_level[0]) cs_active = 0;
                        }
                        if (num_cs >= 2) {
                            if (cs2 != cs_active_level[1]) cs_active = 0;
                        }
                        if (num_cs >= 3) {
                            if (cs3 != cs_active_level[2]) cs_active = 0;
                        }

#if defined(RP235X)
                        // Test all 4 X combinations
                        for (int x1 = 0; x1 <= 1; x1++) {
                            for (int x2 = 0; x2 <= 1; x2++) {
#else // !RP235X
                        // Non-RP2350: X pins fixed at 0, as 16KB images
                        int x1 = 0, x2 = 0;
                        {
                            {
#endif // RP235X
                                // Get expected value
                                uint8_t expected_byte;
#if defined(RP235X)
                                if (cs_active && x1 == 0 && x2 == 0) {
#else // ! RP235X
                                if (cs_active) {
#endif // RP235X
                                    expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
#if defined(RP235X)
                                } else if (cs_active) {
                                    // If not activated, read the same
                                    expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
#endif // RP235X
                                } else {
                                    // If not activated, read the same
                                    expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
                                }

                                uint32_t mangled_addr = create_mangled_address(rom_pins, original_addr, cs1, cs2, cs3, x1, x2);
                                uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                                uint8_t demangled_byte = demangle_byte(compiled_byte);

                                if ((demangled_byte != expected_byte)) {
                                    if (errors < 5) {
                                        printf("    - MISMATCH at addr 0x%08X (CS1=%d,CS2=%d,CS3=%d,X1=%d,X2=%d): mangled 0x%08X expected 0x%02X, got 0x%02X\n",
                                                original_addr, cs1, cs2, cs3, x1, x2, mangled_addr, expected_byte, demangled_byte);
                                    }
                                    errors++;
                                }

                                checked++;
                            }
                        }
                    }
                }
            } else if (rom_pins == 28) {
#if defined(RP235X)
                // 28 pin ROMs.  0.6.4 only 231024 has CS lines in address space.
                if (rom_type != CHIP_TYPE_231024) {
                    // Hard code a single CS combo.  Note that strictly this
                    // isn't always CS active, e.g. for 23xxx, but we don't
                    // care about that in this test - we assume active/inactive
                    // both get the right byte.
                    num_cs_combos = 1;
                }
                size_t orig_rom_size = get_expected_rom_size(rom_type);
                for (uint32_t original_addr = 0; original_addr < orig_rom_size; original_addr++) {
                    uint8_t (*cs_combos_2d)[3] = (uint8_t (*)[3])cs_combos;
                    for (int combo_idx = 0; combo_idx < num_cs_combos; combo_idx++) {
                        uint8_t cs1 = cs_combos_2d[combo_idx][0];
                        uint8_t cs2 = cs_combos_2d[combo_idx][1];
                        uint8_t cs3 = cs_combos_2d[combo_idx][2];

                        // Determine if this CS combination activates the ROM
                        // Unused.  We expect the same data whether active or not.
                        int cs_active = 1;
                        if (num_cs >= 1) {
                            if (cs1 != cs_active_level[0]) cs_active = 0;
                        }
                        if (num_cs >= 2) {
                            if (cs2 != cs_active_level[1]) cs_active = 0;
                        }
                        if (num_cs >= 3) {
                            if (cs3 != cs_active_level[2]) cs_active = 0;
                        }

                        // Get expected value
                        uint8_t expected_byte;
                        if (cs_active) {
                            expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
                        } else {
                            expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
                        }

                        uint32_t mangled_addr = create_mangled_address(rom_pins, original_addr, cs1, cs2, cs3, 0, 0);
                        uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                        uint8_t demangled_byte = demangle_byte(compiled_byte);

                        if (demangled_byte != expected_byte) {
                            if (errors < 5) {
                                printf("    - MISMATCH at addr 0x%08X (CS1=%d,CS2=%d,CS3=%d): mangled 0x%05X expected 0x%02X, got 0x%02X\n",
                                        original_addr, cs1, cs2, cs3, mangled_addr, expected_byte, demangled_byte);
                            }
                            errors++;
                        }
                        checked++;
                    }
                }
#endif // RP235X
#if defined(STM32F4)
                // Only test appropriate address length for STM32F4
                size_t max_addr = get_expected_rom_size(configs[overall_rom_idx].type_int);
                for (uint32_t logical_addr = 0; logical_addr < max_addr; logical_addr++) {
                    // Get expected value first
                    uint32_t original_addr = logical_addr % loaded_roms[loaded_rom_idx].size;
                    uint8_t expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];

                    uint32_t mangled_addr = create_mangled_address(rom_pins, logical_addr, 0, 255, 255, 0, 0);
                    uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                    uint8_t demangled_byte = demangle_byte(compiled_byte);

                    if (demangled_byte != expected_byte) {
                        if (errors < 5) {
                            printf("    - MISMATCH at logical 0x%08X: expected 0x%02X, got 0x%02X\n", 
                                logical_addr, expected_byte, demangled_byte);
                        }
                        errors++;
                    }
                    checked++;
                }
#endif
            } else if (rom_pins == 32) {
                size_t orig_rom_size = get_expected_rom_size(rom_type);
                for (uint32_t original_addr = 0; original_addr < orig_rom_size; original_addr++) {
                    // Get expected value
                    uint8_t expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
                    uint32_t mangled_addr = create_mangled_address(rom_pins, original_addr, 0, 255, 255, 0, 0);
                    uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                    uint8_t demangled_byte = demangle_byte(compiled_byte);

                    if (demangled_byte != expected_byte) {
                        if (errors < 5) {
                            printf("    - MISMATCH at addr 0x%08X: mangled 0x%08X expected 0x%02X, got 0x%02X\n",
                                    original_addr, mangled_addr, expected_byte, demangled_byte);
                        }
                        errors++;
                    }
                    checked++;
                }
            } else if (rom_pins == 40) {
                // CS pins are not part of the address space for 40 pin ROMs, so just test the entire
                // address space linearly.
                size_t orig_rom_size = get_expected_rom_size(rom_type);
                for (uint32_t original_addr = 0; original_addr < orig_rom_size; original_addr++) {
                    // Get expected value
                    uint8_t expected_byte = loaded_roms[loaded_rom_idx].data[original_addr];
                    uint32_t mangled_addr = create_mangled_address(rom_pins, original_addr, 0, 255, 255, 0, 0);
                    uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                    uint8_t demangled_byte = demangle_byte(compiled_byte);

                    if (demangled_byte != expected_byte) {
                        if (errors < 5) {
                            printf("    - MISMATCH at addr 0x%08X: mangled 0x%08X expected 0x%02X, got 0x%02X\n",
                                    original_addr, mangled_addr, expected_byte, demangled_byte);
                        }
                        errors++;
                    }
                    checked++;
                }
            } else {
                printf("  Error: Unsupported pin count %zu for single ROM set\n", rom_pins);
                return -1;
            }

            overall_rom_idx++;
        } else {
            if (serve == SERVE_ADDR_ON_ANY_CS) {
                printf("- Multi-ROM set\n");
            }  else {
                printf("- Bank switched set\n");
            }
            // Multi-ROM/bank switched set: test all 8 CS combinations
            int cs_combinations[8][3] = {
                // X1 is set to 1 before X2 so the output is more logic
                {0, 0, 0}, {0, 1, 0}, {0, 0, 1}, {0, 1, 1},
                {1, 0, 0}, {1, 1, 0}, {1, 0, 1}, {1, 1, 1}
            };
            
            for (int combo_idx = 0; combo_idx < 8; combo_idx++) {
                int cs1 = cs_combinations[combo_idx][0];
                int x1 = cs_combinations[combo_idx][1]; 
                int x2 = cs_combinations[combo_idx][2];
                
                // Determine which ROM (if any) this combination should activate
                int active_rom = -1;
                
                // Check if this CS combination matches one of the 3 active patterns
                if (serve == SERVE_ADDR_ON_ANY_CS) {
                    // Multi-ROM set
                    for (int rom_idx = 0; rom_idx < rom_set[set_idx].rom_count; rom_idx++) {
                        int loaded_rom_idx = overall_rom_idx + rom_idx;
                        if (loaded_rom_idx >= count) {
                            printf("  Internal error - ran out of ROMs");
                            continue;
                        }
                        
                        int expected_active = (configs[loaded_rom_idx].cs1 == 0) ? 0 : 1;
                        int expected_inactive = (expected_active == 0) ? 1 : 0;
                        
                        if ((rom_idx == 0 && cs1 == expected_active && x1 == expected_inactive && x2 == expected_inactive) ||
                            (rom_idx == 1 && x1 == expected_active && cs1 == expected_inactive && x2 == expected_inactive) ||
                            (rom_idx == 2 && x2 == expected_active && cs1 == expected_inactive && x1 == expected_inactive)) {
                            active_rom = rom_idx;
                            break;
                        }
                    }
#if defined(RP_PIO)
                    // Invert sense of CS1, X1 and X2 when using PIO serving.
                    // We do this after the above logic, which is based on
                    // real CS1/X1/X2 values.
                    cs1 = cs1 ^ 1;
                    x1 = x1 ^ 1;
                    x2 = x2 ^ 1;
#endif // RP_PIO
                } else {
                    // Bank switched set or single ROM set in RP2350 case
                    uint sel_x1, sel_x2;
                    if (!json_config->mcu.pins.x_jumper_pull) {
                        // X1/X2 are pulled high by default, low by jumper, so
                        // flip em
                        sel_x1 = x1 ? 0 : 1;
                        sel_x2 = x2 ? 0 : 1;
                    } else {
                        // X1/X2 are pulled low by default, high by jumper
                        sel_x1 = x1;
                        sel_x2 = x2;
                    }
                    
                    // CS1 state doesn't matter - X1/X2 selects the bank (0-3)
                    int bank = (sel_x2 << 1) | sel_x1;
                    
                    // Wrap around if we have fewer ROMs than banks
                    active_rom = bank % rom_set[set_idx].rom_count;
                }
                
                // Print header for this combination
                if (active_rom >= 0) {
                    int loaded_rom_idx = overall_rom_idx + active_rom;
                    if (loaded_rom_idx < count) {
                        printf("  - ROM %d in set %d - CS1=%d, X1=%d, X2=%d\n", active_rom, set_idx, cs1, x1, x2);
                        printf("    - Type: %s, Name: %s\n", configs[loaded_rom_idx].type, configs[loaded_rom_idx].filename);
                    } else {
                        printf("  - ROM %d in set %d (ERROR: out of bounds)\n", active_rom, set_idx);
                    }
                } else {
                    printf("  - Testing blank section CS1=%d, X1=%d, X2=%d\n", cs1, x1, x2);
                }
                
                // Test all addresses for this combination
                int combo_errors = 0;
                for (uint32_t logical_addr = 0; logical_addr < 8192; logical_addr++) {
                    uint32_t mangled_addr = create_mangled_address(rom_pins, logical_addr, cs1, 255, 255, x1, x2);
                    uint8_t compiled_byte = lookup_rom_byte(set_idx, mangled_addr);
                    uint8_t demangled_byte = demangle_byte(compiled_byte);
                    
                    uint8_t expected;
                    if (active_rom >= 0) {
                        int loaded_rom_idx = overall_rom_idx + active_rom;
                        if (loaded_rom_idx >= count) {
                            expected = 0xAA;
                        } else {
                            uint32_t rom_addr = logical_addr % loaded_roms[loaded_rom_idx].size;
                            expected = loaded_roms[loaded_rom_idx].data[rom_addr];
                        }
                    } else {
                        expected = 0xAA;
                    }
                    
                    if (demangled_byte != expected) {
                        if (combo_errors < 5) {
                            printf("    - MISMATCH at logical 0x%08X: expected 0x%02X, got 0x%02X\n", 
                                logical_addr, expected, demangled_byte);
                        }
                        combo_errors++;
                    }
                    checked++;
                }
                errors += combo_errors;
            }
            
            overall_rom_idx += rom_set[set_idx].rom_count;
        }
        
        char *roms;
        if (num_roms > 1) {
            roms = "ROMs";
        } else {
            roms = "ROM";
        }
        printf("- Result: Set %d: %d %s, %d addresses checked, %d errors\n", set_idx, num_roms, roms, checked, errors);
        total_errors += errors;
        total_checked += checked;
    }
    
    printf("\nOverall validation:\n");
    printf("  Total ROM sets: %d\n", sdrr_rom_set_count);
    printf("  Total ROMs: %d\n", overall_rom_idx);
    printf("  Total addresses checked: %d\n", total_checked);
    printf("  Total errors found: %d\n", total_errors);
    printf("  Result: %s\n", (total_errors == 0) ? "PASS ✓" : "FAIL ✗");

    if (total_errors > 0) {
     return -1;
    } else {
        return 0;
    }
}
