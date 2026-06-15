// Query generated roms.c

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "roms-test.h"
#include "json-config.h"

// The address mangler uses CSx, not /CE or /OE.  Where /CE and /OE are used
// instead, this address mangler refers to them as CS1 and CS2.

typedef struct {
    uint8_t addr_pins[MAX_ADDR_LINES];
    uint8_t cs1_pin;
    uint8_t cs2_pin;
    uint8_t cs3_pin;
    uint8_t x1_pin;
    uint8_t x2_pin;
    int initialized;
} address_mangler_t;
static address_mangler_t address_mangler;

static void init_address_mangler(
    const json_config_t* config,
    const sdrr_rom_type_t rom_type,
    address_mangler_t *mangler
) {
    // Initialize
    mangler->initialized = 0;
    mangler->cs1_pin = 255;
    mangler->cs2_pin = 255;
    mangler->cs3_pin = 255;
    mangler->x1_pin = 255;
    mangler->x2_pin = 255;
    memset(mangler->addr_pins, 255, sizeof(mangler->addr_pins));

    // Set CS pins b ased on ROM type
    switch (rom_type) {
        case CHIP_TYPE_2316:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_2316;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_2316;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_2316;
            break;

        case CHIP_TYPE_2332:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_2332;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_2332;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_2332;
            break;

        case CHIP_TYPE_2364:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_2364;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_2364;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_2364;
            if (config->rom.pin_count == 28) {
                mangler->cs1_pin = config->mcu.pins.addr[16];
            }
            break;

        case CHIP_TYPE_23128:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_23128;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_23128;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_23128;
            break;

        case CHIP_TYPE_23256:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_23256;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_23256;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_23256;
            break;

        case CHIP_TYPE_23512:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_23512;
            mangler->cs2_pin = config->mcu.pins.cs2.pin_23512;
            break;

        case CHIP_TYPE_231024:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_231024;
            break;

        case CHIP_TYPE_2704:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2704;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2704;
            if (config->rom.pin_count == 28) {
                mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
                mangler->cs2_pin = config->mcu.pins.ce.pin_2764;
            }
            break;

        case CHIP_TYPE_2708:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2708;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2708;
            if (config->rom.pin_count == 28) {
                mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
                mangler->cs2_pin = config->mcu.pins.ce.pin_2764;
            }
            break;

        case CHIP_TYPE_2716:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2716;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2716;
            if (config->rom.pin_count == 28) {
                mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
                mangler->cs2_pin = config->mcu.pins.ce.pin_2764;
            }
            break;

        case CHIP_TYPE_2732:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2732;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2732;
            if (config->rom.pin_count == 28) {
                mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
                mangler->cs2_pin = config->mcu.pins.ce.pin_2764;
            }
            break;

        case CHIP_TYPE_2764:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2764;
            break;

        case CHIP_TYPE_27128:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27128;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27128;
            break;

        case CHIP_TYPE_27256:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27256;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27256;
            break;

        case CHIP_TYPE_27512:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27512;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27512;
            break;

        case CHIP_TYPE_27C010:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c010;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c010;
            break;

        case CHIP_TYPE_27C020:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c020;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c020;
            break;

        case CHIP_TYPE_SST39SF040:
        case CHIP_TYPE_27C040:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c040;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c040;
            break;

        case CHIP_TYPE_27C080:
            mangler->cs1_pin = config->mcu.pins.cs1.pin_27c080;
            mangler->cs2_pin = config->mcu.pins.ce.pin_27c080;
            mangler->cs3_pin = config->mcu.pins.oe.pin_27c080;
            break;

        case CHIP_TYPE_27C301:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c301;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c301;
            break;

        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c400;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c400;
            break;

        case CHIP_TYPE_28C16:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2704;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2704;
            mangler->cs3_pin = config->mcu.pins.cs2.pin_2332;
            break;

        case CHIP_TYPE_28C64:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2764;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_23128;
            break;

        case CHIP_TYPE_28C256:
            mangler->cs1_pin = config->mcu.pins.ce.pin_2764;
            mangler->cs2_pin = config->mcu.pins.oe.pin_2764;
            mangler->cs3_pin = config->mcu.pins.cs3.pin_23128;
            break;

        case CHIP_TYPE_28C512:
            mangler->cs1_pin = config->mcu.pins.ce.pin_27c010;
            mangler->cs2_pin = config->mcu.pins.oe.pin_27c010;
            mangler->cs3_pin = config->mcu.pins.addr[17];
            break;

        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            mangler->cs1_pin = config->mcu.pins.oe.pin_27512;
            break;

        default:
            printf("Error: Unsupported ROM type %d\n", rom_type);
            exit(1);
            break;
    }


    memcpy(address_mangler.addr_pins, config->mcu.pins.addr, sizeof(address_mangler.addr_pins));

    // There is a special case for 24 pin ROMs - the 2732.  It has A11 as pin
    // 21, whereas the other ROM types have it at pin 18.  For the 2732
    // therefore we swap the A11 and A12 pins.
    if ((rom_type == CHIP_TYPE_2732) && (config->rom.pin_count == 24)) {
        // Find logical A11 and A12 pins
        uint8_t pin_a11 = address_mangler.addr_pins[11];
        uint8_t pin_a12 = address_mangler.addr_pins[12];
        // Swap them
        address_mangler.addr_pins[11] = pin_a12;
        address_mangler.addr_pins[12] = pin_a11;
#if defined(DEBUG_TEST)
        printf("    Note: Swapped A11 and A12 pins %d/%d for 2732 ROM type\n", pin_a11, pin_a12);
#endif // DEBUG_TEST
    } else if (rom_type == CHIP_TYPE_28C256) {
        uint8_t temp = address_mangler.addr_pins[14];
        address_mangler.addr_pins[14] = address_mangler.addr_pins[15];
        address_mangler.addr_pins[15] = temp;
    } else if (rom_type == CHIP_TYPE_27C301 || rom_type == CHIP_TYPE_SST39SF040) {
        // Special case for 32 pin ROMs, the 27C301 and SST39SF040.
        // It has A16 (27C301) and A18 (SST39SF040) after all the other pins,
        // not at the beginning
        // Find the max address pin
        uint8_t max_addr_pin = 0;
        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] != 255) {
                if (address_mangler.addr_pins[ii] > max_addr_pin) {
                    max_addr_pin = address_mangler.addr_pins[ii];
                }
            }
        }
        if (rom_type == CHIP_TYPE_27C301) {
            // fire-28-a has A16 at pin 24, fire-28-b has it at pin 16
            if (address_mangler.addr_pins[18] == 24) {
                // fire-32-a
                address_mangler.addr_pins[16] = max_addr_pin + 1; // A16 is the next pin after the max address pin
            } else if (address_mangler.addr_pins[18] == 16) {
                // fire-32-b
                address_mangler.addr_pins[16] = max_addr_pin + 2; // A16 is after A19 after the last pin
                // Not used for this ROM type
                address_mangler.addr_pins[17] = 255;
                address_mangler.addr_pins[18] = 255;
            } else {
                printf("Error: Unexpected A[18] pin %d for 27C301 ROM type\n", address_mangler.addr_pins[18]);
                assert(0 && "Unexpected config for 27C301");
            }
        } else if (rom_type == CHIP_TYPE_SST39SF040) {
            // fire-32-b has A18 at pin 16
            if (address_mangler.addr_pins[18] == 16) {
                // fire-32-b
                address_mangler.addr_pins[18] = max_addr_pin + 1; // A18 is the next pin after the max address pin
            } else {
                printf("Error: Unexpected A[18] pin %d for SST39SF040 ROM type\n", address_mangler.addr_pins[18]);
                assert(0 && "Unexpected config for SST39SF040 - not supported on fire-32-a");
            }
        }
    } else if (rom_type == CHIP_TYPE_2364 && config->rom.pin_count == 28) {
        uint8_t pin_a11 = address_mangler.addr_pins[11];
        address_mangler.addr_pins[11] = config->mcu.pins.cs1.pin_231024;
        address_mangler.addr_pins[12] = pin_a11;
    } else if (rom_type == CHIP_TYPE_23QL512 || rom_type == CHIP_TYPE_23QL384) {
        address_mangler.addr_pins[15] = config->mcu.pins.ce.pin_27512;
        address_mangler.addr_pins[16] = 255;
    }

    address_mangler.x1_pin = config->mcu.pins.x1;
    address_mangler.x2_pin = config->mcu.pins.x2;
    address_mangler.initialized = 1;
}

void create_address_mangler(const json_config_t* config, const sdrr_rom_type_t rom_type) {
    init_address_mangler(config, rom_type, &address_mangler);

    // Now renamp address/CS/X pins if they're not in the 0..15 range
    if (config->rom.pin_count == 24) {
        if ((config->mcu.ports.data_port == config->mcu.ports.addr_port) && (config->mcu.pins.data[0] < 8)) {
            // If data and address ports are the same, and data lines are 0-7, then
            // address lines must be higher 8-23.  Subtract 8 off them so thare are 0-15.
            for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
                if (address_mangler.addr_pins[ii] != 255) {
                    address_mangler.addr_pins[ii] -= 8;
                }
            }

            // And the CS and X lines too
            if (address_mangler.cs1_pin != 255) {
                address_mangler.cs1_pin -= 8;
            }
            if (address_mangler.cs2_pin != 255) {
                address_mangler.cs2_pin -= 8;
            }
            if (address_mangler.cs3_pin != 255) {
                address_mangler.cs3_pin -= 8;
            }
            if (address_mangler.x1_pin != 255) {
                address_mangler.x1_pin -= 8;
            }
            if (address_mangler.x2_pin != 255) {
                address_mangler.x2_pin -= 8;
            }
        }
    } else if (config->rom.pin_count == 28) {
#if defined(RP235X)
        // RP235X: CS pins ARE part of address space for 28 pin ROMs for
        // 231024, but not for other 28 pin chip types.

        // Find the minimum across address AND CS pins
        uint8_t min_pin = 255;
        uint8_t max_pins;

        // <231024 have 16 pins, 231024/2364 have 18 pins
        // 23QL384 and 23QL512 have 17 pins.
        if (rom_type == CHIP_TYPE_231024 || rom_type == CHIP_TYPE_2364) {
            max_pins = 18;
        } else if (rom_type == CHIP_TYPE_23QL512 || rom_type == CHIP_TYPE_23QL384) {
            max_pins = 17;
        } else {
            max_pins = 16;
        }

        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (ii < max_pins) {
                if (address_mangler.addr_pins[ii] < min_pin) {
                    min_pin = address_mangler.addr_pins[ii];
                }
            } else {
                // Ensure any pins above max are set to 255 so mangler
                // doesn't try to use them
                address_mangler.addr_pins[ii] = 255;
            }
        }

        if (rom_type == CHIP_TYPE_231024 || rom_type == CHIP_TYPE_2364) {
            if (address_mangler.cs1_pin != 255 && address_mangler.cs1_pin < min_pin) {
                min_pin = address_mangler.cs1_pin;
            }
            if (address_mangler.cs2_pin != 255 && address_mangler.cs2_pin < min_pin) {
                min_pin = address_mangler.cs2_pin;
            }
            if (address_mangler.cs3_pin != 255 && address_mangler.cs3_pin < min_pin) {
                min_pin = address_mangler.cs3_pin;
            }

            // Subtract minimum from all CS pins
            if (address_mangler.cs1_pin != 255) {
                address_mangler.cs1_pin -= min_pin;
            }
            if (address_mangler.cs2_pin != 255) {
                address_mangler.cs2_pin -= min_pin;
            }
            if (address_mangler.cs3_pin != 255) {
                address_mangler.cs3_pin -= min_pin;
            }
        }

        // Subtract minimum from all address pins
        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] != 255) {
                address_mangler.addr_pins[ii] -= min_pin;
            }
        }
        
#endif // RP235X
#if defined(STM32F4)
        // STM32F4: CS pins are NOT part of address space for 28 pin ROMs
        // Only left shift address pins
        uint8_t min_addr_pin = 255;
        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] < min_addr_pin) {
                min_addr_pin = address_mangler.addr_pins[ii];
            }
        }

        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] != 255) {
                address_mangler.addr_pins[ii] -= min_addr_pin;
            }
        }
#endif // STM32F4
    } else if ((config->rom.pin_count == 40) || (config->rom.pin_count == 32)) {
        // 32/40 pins ROMs.  CS pins are NOT part of address space for 32/40 pin
        // ROMs.  Only left shift address pins.
        uint8_t min_addr_pin = 255;
        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] < min_addr_pin) {
                min_addr_pin = address_mangler.addr_pins[ii];
            }
        }

        printf("Min address pin for 32/40 pin ROM type is %d\n", min_addr_pin);
        if (rom_type == CHIP_TYPE_27C301 && min_addr_pin == 19) {
            // fire-32-b
            // Special case it
            min_addr_pin -= 1;
        }

        for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
            if (address_mangler.addr_pins[ii] != 255) {
                address_mangler.addr_pins[ii] -= min_addr_pin;
            }
        }

        if (rom_type == CHIP_TYPE_28C512) {
            if (address_mangler.cs3_pin != 255) {
                address_mangler.cs3_pin -= min_addr_pin;
            }
        }
    } else {
        printf("Error: Unsupported pin count %d for address mangler\n", config->rom.pin_count);
        exit(1);
    }

#if defined(DEBUG_TEST)
    printf("  Address Mangler Configuration:\n");
    printf("    CS1 pin: %d\n", address_mangler.cs1_pin);
    printf("    CS2 pin: %d\n", address_mangler.cs2_pin);
    printf("    CS3 pin: %d\n", address_mangler.cs3_pin);
    printf("    X1 pin: %d\n", address_mangler.x1_pin);
    printf("    X2 pin: %d\n", address_mangler.x2_pin);
    printf("    Address pins mapping (after any left shift to base 0):\n");
    for (int ii = 0; ii < MAX_ADDR_LINES; ii++) {
        if (address_mangler.addr_pins[ii] != 255) {
            printf("      Logical A%d -> GPIO %d\n", ii, address_mangler.addr_pins[ii]);
        }
    }
#endif // DEBUG_TEST
}

static struct {
    uint8_t data_pins[NUM_DATA_LINES];
    int initialized;
} byte_demangler = {0};

void create_byte_demangler(const json_config_t* config) {
    memcpy(byte_demangler.data_pins, config->mcu.pins.data, sizeof(byte_demangler.data_pins));
    if (!strcmp(config->mcu.family, "rp2350")) {
        // See how many data pins are valid
        uint8_t num_valid_data_pins = 0;
        for (int ii = 0; ii < NUM_DATA_LINES; ii++) {
            if (config->mcu.pins.data[ii] < 255) {
                num_valid_data_pins++;
            } else {
                break;
            }
        }
        for (int ii = 0; ii < num_valid_data_pins; ii++) {
            // RP2350 uses a higher byte for data lines, but still expects to
            // read a single byte at a time - the RP2350 hardware takes care
            // of getting the value shifted. 
            byte_demangler.data_pins[ii] = byte_demangler.data_pins[ii] % 8;
        }
    }
    byte_demangler.initialized = 1;
}

// lookup_rom_byte - Simulates the lookup of a byte from the ROM image based on the mangled address
uint8_t lookup_rom_byte(uint8_t set, uint32_t mangled_addr) {  // Removed unused CS parameters
    return rom_set[set].data[mangled_addr];
}

uint32_t create_mangled_address(
    size_t rom_pins,
    uint32_t logical_addr,
    uint8_t cs1,
    uint8_t cs2,
    uint8_t cs3,
    uint8_t x1,
    uint8_t x2
) {
    assert(address_mangler.initialized);
    
    uint32_t mangled = 0;

    uint8_t max_addr_pins;
    
    if (rom_pins == 24) {
        // Strictly these asserts aren't valid for RP2350 as one could use later pins for CS lines,
        // but OK for now
        assert(address_mangler.cs1_pin <= 15);
        assert(cs1 <= 1);
        if (address_mangler.cs2_pin != 255) {
            assert(address_mangler.cs2_pin <= 15);
            // CS2 does not have to be provided
            if (address_mangler.cs3_pin != 255) {
                assert(address_mangler.cs3_pin <= 15);
            }
            // CS3 does not have to be provided
        }
        assert(address_mangler.x1_pin <= 15);
        assert(address_mangler.x2_pin <= 15);
        assert(x1 <= 1);
        assert(x2 <= 1);

        // Set CS selection bits (active low)
        if (cs1 == 1) mangled |= (1 << address_mangler.cs1_pin);
        if (cs2 == 1) mangled |= (1 << address_mangler.cs2_pin);
        if (cs3 == 1) mangled |= (1 << address_mangler.cs3_pin);
        if (x1 == 1)  mangled |= (1 << address_mangler.x1_pin);  
        if (x2 == 1)  mangled |= (1 << address_mangler.x2_pin);

        max_addr_pins = 16;
    } else if (rom_pins == 28) {
    // 28-pin ROMs
        max_addr_pins = 16;
#if defined(RP235X)
        // RP235X: CS lines are part of the address space
        // Map CS bits to their physical pin positions
        if (cs1 != 255) {
            if (cs1 == 1) mangled |= (1 << address_mangler.cs1_pin);
        }
        if (cs2 != 255 && address_mangler.cs2_pin != 255) {
            if (cs2 == 1) mangled |= (1 << address_mangler.cs2_pin);
        }
        if (cs3 != 255 && address_mangler.cs3_pin != 255) {
            if (cs3 == 1) mangled |= (1 << address_mangler.cs3_pin);
        }
        max_addr_pins = 18;
#endif // RP235X
    } else if (rom_pins == 32) {
        max_addr_pins = 19;
    } else if (rom_pins == 40) {
        // 40-pin ROMs
        max_addr_pins = 19;
    } else {
        printf("Error: Unsupported ROM pin count %zu in create_mangled_address\n", rom_pins);
        exit(1);
    }
    
    // Map logical address bits to configured GPIO positions
    for (int i = 0; i < MAX_ADDR_LINES; i++) {
        if (logical_addr & (1 << i)) {
            if (address_mangler.addr_pins[i] > (max_addr_pins-1)) {
                printf("Error: Address pin mapping for logical A%d (%d) is out of range for ROM pin count %zu\n", 
                    i, address_mangler.addr_pins[i], rom_pins);
                exit(1);
            }
            mangled |= (1 << address_mangler.addr_pins[i]);
        }
    }

    return mangled;
}

uint8_t demangle_byte(uint8_t mangled_byte) {
    assert(byte_demangler.initialized);
    
    uint8_t logical = 0;
    
    for (int i = 0; i < 8; i++) {
        assert(byte_demangler.data_pins[i] <= 7);
        if (mangled_byte & (1 << byte_demangler.data_pins[i])) {
            logical |= (1 << i);
        }
    }

    return logical;
}

// Convert ROM type number to string
const char* rom_type_to_string(sdrr_rom_type_t rom_type) {
    switch (rom_type) {
        case CHIP_TYPE_2316: return "2316";
        case CHIP_TYPE_2332: return "2332";  
        case CHIP_TYPE_2364: return "2364";
        case CHIP_TYPE_23128: return "23128";
        case CHIP_TYPE_23256: return "23256";
        case CHIP_TYPE_23512: return "23512";
        case CHIP_TYPE_231024: return "231024";
        case CHIP_TYPE_2704: return "2704";
        case CHIP_TYPE_2708: return "2708";
        case CHIP_TYPE_2716: return "2716";
        case CHIP_TYPE_2732: return "2732";
        case CHIP_TYPE_2764: return "2764";
        case CHIP_TYPE_27128: return "27128";
        case CHIP_TYPE_27256: return "27256";
        case CHIP_TYPE_27512: return "27512";
        case CHIP_TYPE_27C010: return "27C010";
        case CHIP_TYPE_27C020: return "27C020";
        case CHIP_TYPE_SST39SF040: return "SST39SF040";
        case CHIP_TYPE_27C040: return "27C040";
        case CHIP_TYPE_27C080: return "27C080";
        case CHIP_TYPE_27C301: return "27C301";
        case CHIP_TYPE_27C400: return "27C400";
        case CHIP_TYPE_28C16:  return "28C16";
        case CHIP_TYPE_28C64:  return "28C64";
        case CHIP_TYPE_28C256: return "28C256";
        case CHIP_TYPE_28C512: return "28C512";
        case CHIP_TYPE_23QL512: return "23QL512";
        case CHIP_TYPE_23QL384: return "23QL384";
        case CHIP_TYPE_23C1001: return "23C1001";
        case CHIP_TYPE_27C200: return "27C200";
        default: return "unknown";
    }
}

uint8_t get_num_cs(sdrr_rom_type_t rom_type) {
    switch (rom_type) {
        case CHIP_TYPE_2316:
        case CHIP_TYPE_23128:
        case CHIP_TYPE_27C080:
            return 3;
        case CHIP_TYPE_2332:
        case CHIP_TYPE_23256:
        case CHIP_TYPE_23512:
        case CHIP_TYPE_2704:
        case CHIP_TYPE_2708:
        case CHIP_TYPE_2716:
        case CHIP_TYPE_2732:
        case CHIP_TYPE_2764:
        case CHIP_TYPE_27128:
        case CHIP_TYPE_27256:
        case CHIP_TYPE_27512:
        case CHIP_TYPE_27C200:
        case CHIP_TYPE_27C400:
        case CHIP_TYPE_27C010:
        case CHIP_TYPE_27C020:
        case CHIP_TYPE_27C040:
        case CHIP_TYPE_SST39SF040:
        case CHIP_TYPE_27C301:
            return 2;
        case CHIP_TYPE_2364:
        case CHIP_TYPE_231024:
        case CHIP_TYPE_23QL512:
        case CHIP_TYPE_23QL384:
            return 1;
        case CHIP_TYPE_28C16:
        case CHIP_TYPE_28C64:
        case CHIP_TYPE_28C256:
        case CHIP_TYPE_28C512:
            return 3;
        case CHIP_TYPE_23C1001:
            return 4;
        default:
            printf("Error: Unsupported ROM type %d in get_num_cs\n", rom_type);
            assert(0 && "Unknown ROM type in num_cs");
            return 0;
    }
}

static const uint8_t cs_combos_1[2][3] = {{0,255,255}, {1,255,255}};
static const uint8_t cs_combos_2[4][3] = {{0,0,255}, {0,1,255}, {1,0,255}, {1,1,255}};
static const uint8_t cs_combos_3[8][3] = {{0,0,0}, {0,0,1}, {0,1,0}, {0,1,1},
                                           {1,0,0}, {1,0,1}, {1,1,0}, {1,1,1}};
static const uint8_t cs_combos_4[16][4] = {
    {0,0,0,0}, {0,0,0,1}, {0,0,1,0}, {0,0,1,1},
    {0,1,0,0}, {0,1,0,1}, {0,1,1,0}, {0,1,1,1},
    {1,0,0,0}, {1,0,0,1}, {1,0,1,0}, {1,0,1,1},
    {1,1,0,0}, {1,1,0,1}, {1,1,1,0}, {1,1,1,1}
};

uint8_t cs_combinations(sdrr_rom_type_t rom_type, uint8_t **combos) {
    uint8_t num_cs = get_num_cs(rom_type);
    switch (num_cs) {
        case 1:
            *combos = (uint8_t *)cs_combos_1;
            return 2;
        case 2:
            *combos = (uint8_t *)cs_combos_2;
            return 4;
        case 3:
            *combos = (uint8_t *)cs_combos_3;
            return 8;
        case 4:
            *combos = (uint8_t *)cs_combos_4;
            return 16;
        default:
            assert(0 && "Unknown number of CS lines in cs_combinations");
            return 0;
    }
}

// Convert CS state number to string
const char* cs_state_to_string(int cs_state) {
    switch (cs_state) {
        case CS_ACTIVE_LOW: return "active_low";
        case CS_ACTIVE_HIGH: return "active_high";
        case CS_NOT_USED: return "not_used";
        default: return "unknown";
    }
}

// Get expected ROM size for type
size_t get_expected_rom_size(sdrr_rom_type_t rom_type) {
    switch (rom_type) {
        case CHIP_TYPE_2316: return 2048;
        case CHIP_TYPE_2332: return 4096;
        case CHIP_TYPE_2364: return 8192;
        case CHIP_TYPE_23128: return 16384;
        case CHIP_TYPE_23256: return 32768;
        case CHIP_TYPE_23512: return 65536;
        case CHIP_TYPE_231024: return 131072;
        case CHIP_TYPE_2704: return 512;
        case CHIP_TYPE_2708: return 1024;
        case CHIP_TYPE_2716: return 2048;
        case CHIP_TYPE_2732: return 4096;
        case CHIP_TYPE_2764: return 8192;
        case CHIP_TYPE_27128: return 16384;
        case CHIP_TYPE_27256: return 32768;
        case CHIP_TYPE_27512: return 65536;
        case CHIP_TYPE_27C010: return 131072;
        case CHIP_TYPE_27C020: return 262144;
        case CHIP_TYPE_27C040: return 524288;
        case CHIP_TYPE_SST39SF040: return 524288;
        case CHIP_TYPE_27C080: return 524288;
        case CHIP_TYPE_27C301: return 131072;
        case CHIP_TYPE_27C400: return 524288;
        case CHIP_TYPE_28C16:  return 2048;
        case CHIP_TYPE_28C64:  return 8192;
        case CHIP_TYPE_28C256: return 32768;
        case CHIP_TYPE_28C512: return 65536;
        case CHIP_TYPE_23QL512: return 65536;
        case CHIP_TYPE_23QL384: return 49152;
        case CHIP_TYPE_23C1001: return 131072;
        case CHIP_TYPE_27C200: return 262144;
        default: return 0;
    }
}

sdrr_rom_type_t rom_type_from_string(const char* type_str) {
    if (strcmp(type_str, "2316") == 0) return CHIP_TYPE_2316;
    else if (strcmp(type_str, "2332") == 0) return CHIP_TYPE_2332;
    else if (strcmp(type_str, "2364") == 0) return CHIP_TYPE_2364;
    else if (strcmp(type_str, "23128") == 0) return CHIP_TYPE_23128;
    else if (strcmp(type_str, "23256") == 0) return CHIP_TYPE_23256;
    else if (strcmp(type_str, "23512") == 0) return CHIP_TYPE_23512;
    else if (strcmp(type_str, "231024") == 0) return CHIP_TYPE_231024;
    else if (strcmp(type_str, "2704") == 0) return CHIP_TYPE_2704;
    else if (strcmp(type_str, "2708") == 0) return CHIP_TYPE_2708;
    else if (strcmp(type_str, "2716") == 0) return CHIP_TYPE_2716;
    else if (strcmp(type_str, "2732") == 0) return CHIP_TYPE_2732;
    else if (strcmp(type_str, "2764") == 0) return CHIP_TYPE_2764;
    else if (strcmp(type_str, "27128") == 0) return CHIP_TYPE_27128;
    else if (strcmp(type_str, "27256") == 0) return CHIP_TYPE_27256;
    else if (strcmp(type_str, "27512") == 0) return CHIP_TYPE_27512;
    else if (strcmp(type_str, "27C010") == 0) return CHIP_TYPE_27C010;
    else if (strcmp(type_str, "27C020") == 0) return CHIP_TYPE_27C020;
    else if (strcmp(type_str, "27C040") == 0) return CHIP_TYPE_27C040;
    else if (strcmp(type_str, "SST39SF040") == 0) return CHIP_TYPE_SST39SF040;
    else if (strcmp(type_str, "27C080") == 0) return CHIP_TYPE_27C080;
    else if (strcmp(type_str, "27C301") == 0) return CHIP_TYPE_27C301;
    else if (strcmp(type_str, "27C400") == 0) return CHIP_TYPE_27C400;
    else if (strcmp(type_str, "28C16")  == 0) return CHIP_TYPE_28C16;
    else if (strcmp(type_str, "28C64")  == 0) return CHIP_TYPE_28C64;
    else if (strcmp(type_str, "28C256") == 0) return CHIP_TYPE_28C256;
    else if (strcmp(type_str, "28C512") == 0) return CHIP_TYPE_28C512;
    else if (strcmp(type_str, "23QL512") == 0) return CHIP_TYPE_23QL512;
    else if (strcmp(type_str, "23QL384") == 0) return CHIP_TYPE_23QL384;
    else if (strcmp(type_str, "23C1001") == 0) return CHIP_TYPE_23C1001;
    else if (strcmp(type_str, "27C200") == 0) return CHIP_TYPE_27C200;
    else return -1; // Unknown type
}

void print_compiled_rom_info(void) {
    printf("\n=== Compiled ROM Sets Analysis ===\n");
    printf("Total ROM images: %d\n", SDRR_NUM_IMAGES);
    printf("Total ROM sets: %d\n", sdrr_rom_set_count);
    
    // Print details for each ROM set
    for (int set_idx = 0; set_idx < sdrr_rom_set_count; set_idx++) {
        printf("\nROM Set %d:\n", set_idx);
        printf("  Size: %u bytes (%s)\n", rom_set[set_idx].size, 
               (rom_set[set_idx].size == 16384) ? "16KB" : 
               (rom_set[set_idx].size == 65536) ? "64KB" : "other");
        printf("  ROM count: %d\n", rom_set[set_idx].rom_count);
        
        // Expected image size based on ROM count
#if defined(RP235X)
        const char* expected_size = "64KB";
        const size_t expected_size_bytes = 65536;
#else // ! RP235X
        const char* expected_size = (rom_set[set_idx].rom_count == 1) ? "16KB" : "64KB";
        const size_t expected_size_bytes = (rom_set[set_idx].rom_count == 1) ? 16384 : 65536;
#endif // RP235X
        printf("  Expected size: %s", expected_size);
        if (rom_set[set_idx].size == expected_size_bytes) {
            printf(" ✓\n");
        } else {
            printf(" ✗\n");
        }
        
        // Print details for each ROM in this set
        for (int rom_idx = 0; rom_idx < rom_set[set_idx].rom_count; rom_idx++) {
            const sdrr_rom_info_t *rom_info = rom_set[set_idx].roms[rom_idx];
            
            printf("  ROM %d:\n", rom_idx);
#if defined(BOOT_LOGGING)
            printf("    File: %s\n", rom_info->filename);
#endif // BOOT_LOGGING
            printf("    Type: %s (%d)\n", rom_type_to_string(rom_info->rom_type), rom_info->rom_type);
            printf("    CS1: %s (%d)", cs_state_to_string(rom_info->cs1_state), rom_info->cs1_state);
            
            if (rom_info->cs2_state != CS_NOT_USED) {
                printf(", CS2: %s (%d)", cs_state_to_string(rom_info->cs2_state), rom_info->cs2_state);
            }
            if (rom_info->cs3_state != CS_NOT_USED) {
                printf(", CS3: %s (%d)", cs_state_to_string(rom_info->cs3_state), rom_info->cs3_state);
            }
            printf("\n");
            
            // Expected ROM size check
            size_t expected_rom_size = get_expected_rom_size(rom_info->rom_type);
            printf("    Expected ROM size: %zu bytes\n", expected_rom_size);
        }
        
        // Show first 8 bytes of the ROM set data
        printf("  First 8 bytes of mangled set data: ");
        for (size_t j = 0; j < 8 && j < rom_set[set_idx].size; j++) {
            printf("0x%02X ", rom_set[set_idx].data[j]);
        }
        printf("\n");
    }
}
