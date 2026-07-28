// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// TODO:
// - Any pulled address lines need to be specified and pulled.  Includes X
//   pins.
// - Need ability to invert /BYTE

#if defined(TEST_BUILD)
#define APIO_LOG_IMPL  1
#endif // TEST_BUILD

#include "include.h"

#include "piodma/piodma.h"

// Structure used to hold GPIO configuration for setup_serving_gpios
typedef struct {
    uint8_t base_data_pin;
    uint8_t num_data_pins;

    uint8_t base_addr_pin;
    uint8_t num_addr_pins;

    uint8_t base_cs_pin;
    uint8_t num_cs_pins;
    uint8_t ignore_cs_index;

    uint8_t byte_pin;

    uint8_t num_pulls;
    const uint8_t *pulls;

    uint8_t num_overrides;
    const uint8_t *overrides;
} gpio_init_t;

// Forward declarations
static int validate_serving_algs(const onerom_rom_slot_t *slot);
static int setup_serving_gpios(const onerom_rom_slot_t *slot);
static int setup_serving_pios(const onerom_rom_slot_t *slot, uint32_t rom_table_addr);
static int setup_serving_dma(const onerom_rom_slot_t *slot, uint32_t rom_table_addr);
static void start_serving_pios(void);

#if BLOCK_CS_DATA == 0
#define DATA_GPIO_CTRL_FUNC   GPIO_CTRL_FUNC_PIO0
#elif BLOCK_CS_DATA == 1
#define DATA_GPIO_CTRL_FUNC   GPIO_CTRL_FUNC_PIO1
#elif BLOCK_CS_DATA == 2
#define DATA_GPIO_CTRL_FUNC   GPIO_CTRL_FUNC_PIO2
#else
#error "Invalid block for CS/Data PIO"
#endif

int piorom2(void) {
    const onerom_rom_slot_t *slot = RUNTIME->current_rom_slot;
    const uint32_t rom_table_addr = (uint32_t)(uintptr_t)RUNTIME->rom_table;
    int rc;

    // Validate the serving algorithm configuration.  This allows subsequent
    // functions to avoid error checking.  This is a no-op when TURBO_BOOT is
    // defined.
    rc = validate_serving_algs(slot);
    if (rc != 0) {
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return rc;
    }

    rc = setup_serving_gpios(slot);
    if (rc != 0) {
        return rc;
    }

    rc = setup_serving_dma(slot, rom_table_addr);
    if (rc != 0) {
        return rc;
    }

    rc = setup_serving_pios(slot, rom_table_addr);
    if (rc != 0) {
        return rc;
    }

    start_serving_pios();

    return 0;
}

static uint8_t retrieve_gpio_init(const onerom_rom_slot_t *slot, gpio_init_t *gpio_init) {
    gpio_init->base_data_pin = 0xFF;
    gpio_init->base_addr_pin = 0xFF;
    gpio_init->base_cs_pin = 0xFF;
    gpio_init->byte_pin = 0xFF;
    gpio_init->num_addr_pins = 0;
    gpio_init->num_data_pins = 0;
    gpio_init->num_cs_pins = 0;
    gpio_init->ignore_cs_index = 0xFF;
    gpio_init->num_pulls = 0;
    gpio_init->pulls = NULL;
    gpio_init->num_overrides = 0;
    gpio_init->overrides = NULL;

    const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;
    gpio_init->base_data_pin = cs_alg->base_data_pin + cs_alg->gpio_base;
    gpio_init->num_data_pins = cs_alg->num_data_pins;
    gpio_init->base_cs_pin = cs_alg->base_cs_pin + cs_alg->gpio_base;
    gpio_init->num_cs_pins = cs_alg->num_cs_pins;

    // Get data, CS and byte pins from the CS algorithm
    switch (slot->alg->alg_cs->alg) {
        case ALG_CS_0: {
            const onerom_alg_cs0_param_t *params = (const onerom_alg_cs0_param_t *)slot->alg->alg_cs->params;
            // byte_pin is GPIO_NONE when the chip has no /BYTE pin.  Adding the
            // base to that would wrap it round into a real GPIO, which
            // setup_serving_gpios() would then reconfigure, so only offset a
            // pin that is actually present.
            if (params->byte_pin != GPIO_NONE) {
                gpio_init->byte_pin = params->byte_pin + cs_alg->gpio_base;
            }
            break;
        }

        case ALG_CS_1: {
            const onerom_alg_cs1_param_t *params = (const onerom_alg_cs1_param_t*)slot->alg->alg_cs->params;
            gpio_init->ignore_cs_index = params->cs_ignore_index;
            break;
        }

        case ALG_CS_2: {
            // No additional configuration to handle
            break;
        }

        default:
            // Unreachable as we validate the algorithm earlier
            return -1;
            break;
    }

    // Get the address pins from the address algorithm
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    gpio_init->base_addr_pin = addr_alg->base_addr_pin + addr_alg->gpio_base;
    gpio_init->num_addr_pins = addr_alg->num_addr_pins;
    switch (slot->alg->alg_addr->alg) {
        case ALG_ADDR_0: {
            // No additional configuration to handle
            break;
        }

        default:
            // Unreachable as we validate the algorithm earlier
            return -1;
            break;
    }

    // No need to get data pins - we have them from the CS pins

    // Get pull configs
    if (slot->alg->gpio_pull_config != NULL && slot->alg->gpio_pull_config != (void*)0xFFFFFFFF) {
        const onerom_alg_pull_config_t *pull_config = slot->alg->gpio_pull_config;
        gpio_init->num_pulls = pull_config->param_len;
        gpio_init->pulls = pull_config->params;
    }

    // Get override configs
    if (slot->alg->gpio_override_config != NULL && slot->alg->gpio_override_config != (void*)0xFFFFFFFF) {
        const onerom_alg_override_config_t *override_config = slot->alg->gpio_override_config;
        gpio_init->num_overrides = override_config->param_len;
        gpio_init->overrides = override_config->params;
    }

    return 0;
}

// Reports what the ROM serving path uses gpio for on this slot, writing an
// ora_gpio_use_t to *use_out.  ORA_GPIO_USE_FREE means serving does not use the
// GPIO - board-level system pins (status LED, neopixel, VBUS, ext flash CS) are
// not considered here, as they are independent of the active slot.
//
// The classification is derived from retrieve_gpio_init(), the same
// configuration setup_serving_gpios() acts on, so the two cannot drift.
//
// What is reported is the consequence of driving the pin, not the role it
// plays: the data pins are driven by PIO, so taking one over breaks serving
// until reboot, while everything else serving uses is an SIO input that PIO
// keeps reading regardless of its function select, so taking one over is
// reversible.  Naming the role is the host's job.
ora_result_t pio_get_gpio_use(
    const onerom_rom_slot_t *slot,
    uint8_t gpio,
    uint8_t *use_out
) {
    if (slot == NULL || use_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    gpio_init_t gpio_init;
    if (retrieve_gpio_init(slot, &gpio_init) != 0) {
        return ORA_RESULT_INTERNAL_ERROR;
    }

    *use_out = ORA_GPIO_USE_FREE;

    // Data pins - the only serving GPIOs One ROM drives.
    if (gpio_init.base_data_pin < MAX_GPIOS &&
        gpio >= gpio_init.base_data_pin &&
        (gpio - gpio_init.base_data_pin) < gpio_init.num_data_pins) {
        *use_out = ORA_GPIO_USE_SERVING_DRIVEN;
        return ORA_RESULT_OK;
    }

    // CS pins.  The whole span is covered, including the cs_ignore_index gap (a
    // position serving masks out of the select field) and any excess address
    // line folded in as a half-select: every position in the span is part of
    // the select field the CS state machine samples.
    if (gpio_init.base_cs_pin < MAX_GPIOS &&
        gpio >= gpio_init.base_cs_pin &&
        (gpio - gpio_init.base_cs_pin) < gpio_init.num_cs_pins) {
        *use_out = ORA_GPIO_USE_SERVING_READ;
        return ORA_RESULT_OK;
    }

    // The byte pin.  It sits outside the CS span, so it has to be located
    // specifically - otherwise it would fall through to free, and a 27C400
    // could be flipped between byte and word mode unforced.
    if (gpio_init.byte_pin < MAX_GPIOS && gpio == gpio_init.byte_pin) {
        *use_out = ORA_GPIO_USE_SERVING_READ;
        return ORA_RESULT_OK;
    }

    // Address span.  Everything in it is read by the address state machine,
    // including any X pins folded into the span on Multi and Banked slots.
    if (gpio_init.base_addr_pin < MAX_GPIOS &&
        gpio >= gpio_init.base_addr_pin &&
        (gpio - gpio_init.base_addr_pin) < gpio_init.num_addr_pins) {
        *use_out = ORA_GPIO_USE_SERVING_READ;
        return ORA_RESULT_OK;
    }

    return ORA_RESULT_OK;
}

// This function first initializes any GPIOs used by this ROM slot, then
// configures any pull-ups or downs, and any pin overrides.
//
// This is called after validate_serving_
int setup_serving_gpios(const onerom_rom_slot_t *slot) {
    // Retrieve GPIO configuration for this ROM slot
    gpio_init_t gpio_init;
    uint8_t rc = retrieve_gpio_init(slot, &gpio_init);
    if (rc != 0) {
        return rc;
    }

    DEBUG("GPIO: data=%u+%u addr=%u+%u cs=%u+%u(ign=%u) byte=%u pulls=%u ovrd=%u",
        gpio_init.base_data_pin, gpio_init.num_data_pins,
        gpio_init.base_addr_pin, gpio_init.num_addr_pins,
        gpio_init.base_cs_pin, gpio_init.num_cs_pins,
        gpio_init.ignore_cs_index,
        gpio_init.byte_pin,
        gpio_init.num_pulls,
        gpio_init.num_overrides);

    // Data pins: PIO-controlled input/output, 8 mA drive, fast slew
    for (int ii = 0; ii < gpio_init.num_data_pins; ii++) {
        uint8_t pin = ii + gpio_init.base_data_pin;
        if (pin < MAX_GPIOS) {
            // Clear pulls - pull-down enabled by default on output pins
            APIO_GPIO_PULL_NONE(pin);
            APIO_GPIO_INPUT_OUTPUT(pin, BLOCK_CS_DATA);
            APIO_GPIO_DRIVE(pin, APIO_DRIVE_8MA);
            APIO_GPIO_SLEW_FAST(pin);
        }
    }

    // Address pins: input-only, output driver disabled
    if (gpio_init.base_addr_pin < MAX_GPIOS && gpio_init.num_addr_pins < 0xFF) {
        for (int ii = 0; ii < gpio_init.num_addr_pins; ii++) {
            uint8_t pin = ii + gpio_init.base_addr_pin;
            if (pin < MAX_GPIOS) {
                APIO_GPIO_INPUT_ONLY(pin);
            }
        }
    }

    // CS pins: input-only, output driver disabled (except the ignored index)
    if (gpio_init.base_cs_pin < MAX_GPIOS && gpio_init.num_cs_pins < 0xFF) {
        for (int ii = 0; ii < gpio_init.num_cs_pins; ii++) {
            uint8_t pin = ii + gpio_init.base_cs_pin;
            if (pin < MAX_GPIOS && ii != gpio_init.ignore_cs_index) {
                APIO_GPIO_INPUT_ONLY(pin);
            }
        }
    }

    // Byte pin: input-only, output driver disabled
    if (gpio_init.byte_pin < MAX_GPIOS) {
        uint8_t pin = gpio_init.byte_pin;
        APIO_GPIO_INPUT_ONLY(pin);
    }

    // Pull-ups and pull-downs
    if (gpio_init.num_pulls > 0 && gpio_init.pulls != NULL) {
        for (int ii = 0; ii < gpio_init.num_pulls; ii++) {
            uint8_t pin  = gpio_init.pulls[ii] & 0x7F;
            uint8_t high = gpio_init.pulls[ii] & 0x80 ? 1 : 0;
            DEBUG("Pull[%d]: pin=%u %s", ii, pin, high ? "up" : "down");
            if (high) {
                APIO_GPIO_PULL_UP(pin);
            } else {
                APIO_GPIO_PULL_DOWN(pin);
            }
        }
    }

    // Invert or override to always read 0 or 1
    if (gpio_init.num_overrides > 0 && gpio_init.overrides != NULL) {
        for (int ii = 0; ii < gpio_init.num_overrides; ii++) {
            uint8_t pin  = gpio_init.overrides[ii] & 0x3F;
            uint8_t mode = (gpio_init.overrides[ii] & 0xC0) >> 6;
            DEBUG("Override[%d]: pin=%u mode=%u", ii, pin, mode);
            switch (mode) {
                case GPIO_OVER_NORMAL:
                    break;

                case GPIO_OVER_INVERT:
                    APIO_GPIO_INPUT_INVERT(pin);
                    break;

                case GPIO_OVER_LOW:
                    APIO_GPIO_FORCE_INPUT_LOW(pin);
                    break;

                case GPIO_OVER_HIGH:
                    APIO_GPIO_FORCE_INPUT_HIGH(pin);
                    break;

                default:
                    // Unreachable
                    return -1;
            }
        }
    }

    return 0;
}

int validate_serving_algs(const onerom_rom_slot_t *slot) {
    if (!TURBO) {
        if (slot->slot_type == ROM_SLOT_TYPE_PLUGIN_SYSTEM ||
            slot->slot_type == ROM_SLOT_TYPE_PLUGIN_USER ||
            slot->slot_type == ROM_SLOT_TYPE_PLUGIN_PIO) {
                ERR("Cannot serve a plugin");
                return -1;
        }

        // Check we have a top-level algorithms structure
        if ((slot->alg == NULL) || (slot->alg == ((void*)0xFFFFFFFF))) {
            ERR("No serving algs");
            return -1;
        }

        // Check we have each of the required algorithm structures
        //
        // We don't check the pull and override structs, as they are optional.
        const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;
        const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
        const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
        const onerom_alg_dma_config_t *dma_alg = slot->alg->alg_dma;
        if (cs_alg == NULL ||
            addr_alg == NULL ||
            data_alg == NULL ||
            dma_alg == NULL ||
            cs_alg == ((void*)0xFFFFFFFF) ||
            addr_alg == ((void*)0xFFFFFFFF) ||
            data_alg == ((void*)0xFFFFFFFF) ||
            dma_alg == ((void*)0xFFFFFFFF)
        ) {
            ERR("Incomplete serving algs");
            return -1;
        }

        // Check the algorithm values are valid
        if (cs_alg->alg >= NUM_CS_ALGS) {
            ERR("Invalid CS alg: %d", cs_alg->alg);
            return -1;
        }
        if (addr_alg->alg >= NUM_ADDR_ALGS) {
            ERR("Invalid address alg: %d", addr_alg->alg);
            return -1;
        }
        if (data_alg->alg >= NUM_DATA_ALGS) {
            ERR("Invalid data alg: %d", data_alg->alg);
            return -1;
        }
        if (dma_alg->alg >= NUM_DMA_ALGS) {
           ERR("Invalid DMA alg: %d", dma_alg->alg);
            return -1;
        }

        // Check the algorithm parameter lengths are valid
        if (cs_alg->param_len < alg_cs_params_len[cs_alg->alg]) {
            ERR("CS alg params too short: %d < %d", cs_alg->param_len, alg_cs_params_len[cs_alg->alg]);
            return -1;
        }
        if (addr_alg->param_len < alg_addr_params_len[addr_alg->alg]) {
            ERR("Address alg params too short: %d < %d", addr_alg->param_len, alg_addr_params_len[addr_alg->alg]);
            return -1;
        }
        if (data_alg->param_len < alg_data_params_len[data_alg->alg]) {
            ERR("Data alg params too short: %d < %d", data_alg->param_len, alg_data_params_len[data_alg->alg]);
            return -1;
        }
        if (dma_alg->param_len < alg_dma_params_len[dma_alg->alg]) {
            ERR("DMA alg params too short: %d < %d", dma_alg->param_len, alg_dma_params_len[dma_alg->alg]);
            return -1;
        }
    }

    return 0;
}

int setup_serving_pios(const onerom_rom_slot_t *slot, uint32_t rom_table_addr) {
    // The PIO & DMA algorithms to use are specified in the slot configuration
    if (slot->alg == NULL) {
        ERR("No algorithm specified for ROM slot");
        return -1;
    }

    // Get the algorithm parameters
    const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
    uint8_t cs_param_len = cs_alg->param_len;
    //uint8_t addr_param_len = addr_alg->param_len;
    uint8_t data_param_len = data_alg->param_len;
    const uint8_t *cs_params = cs_alg->params;
    //const uint8_t *addr_params = addr_alg->params;
    const uint8_t *data_params = data_alg->params;

    // Enable the PIOs
    APIO_ENABLE_PIOS(); 

    // Set up the PIO assembler
    APIO_ASM_INIT();
    
    // Clear all PIO IRQs
    APIO_CLEAR_ALL_IRQS();

    // Handle the address block first
    APIO_SET_BLOCK(BLOCK_ADDR);
    RUNTIME->addr_pio_block_info = STORE_PIO_BLOCK_INFO(BLOCK_ADDR);
    APIO_SET_SM(SM_ADDR_READ);
    RUNTIME->addr_pio_sm_info = STORE_PIO_SM_INFO(SM_ADDR_READ);
    DEBUG("Addr alg %u: gpio_base=%u base=%u pins=%u tbl_bits=%u delay=%u clkdiv=%u/%u",
        addr_alg->alg, addr_alg->gpio_base,
        addr_alg->base_addr_pin, addr_alg->num_addr_pins,
        addr_alg->num_rom_table_bits, addr_alg->num_delay_cycles,
        addr_alg->clkdiv_int, addr_alg->clkdiv_frac);
    switch (addr_alg->alg) {
        case ALG_ADDR_0: {
                // No parameters
                //if (addr_param_len < ALG_ADDR0_PARAMS_PRE_LIST_LEN ) {
                //    ERR("Address alg error");
                //    limp_mode(LIMP_MODE_INVALID_CONFIG);
                //    return 0;
                //}
                //const onerom_alg_addr0_param_t *params = (const onerom_alg_addr0_param_t *)addr_params;

                // Figure out the ROM table prefix and # bits.  The number
                // of ROM table bits and number of address bits may be
                // different - there may be more ROM table bits than address
                // pins (e.g. 16 bit ROMs), in which case we pad the extra
                // bits with 0s in the PIO algorithm below.
                uint8_t rom_table_prefix_bits = 32 - addr_alg->num_rom_table_bits;
                uint32_t rom_table_prefix = rom_table_addr >> (32 - rom_table_prefix_bits);
                int8_t extra_addr_bits = addr_alg->num_rom_table_bits - addr_alg->num_addr_pins;
                DEBUG("ROM table prefix 0x%08X, # bits: %u", rom_table_prefix, rom_table_prefix_bits);

                // Write the SM instructions
                APIO_WRAP_BOTTOM();
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_IN_X(rom_table_prefix_bits), addr_alg->num_delay_cycles));
                if (extra_addr_bits == 0) {
                    APIO_WRAP_TOP();
                }
                APIO_ADD_INSTR(APIO_IN_PINS(addr_alg->num_addr_pins));
                for (int ii = extra_addr_bits; ii > 0; ii--) {
                // If the number of ROM table bits is more than the number
                // of address pins, pad the input with additional 0s to
                // fill the address.
                    if (ii == 1) {
                        // Last bit, so wrap the top of the loop
                        APIO_WRAP_TOP();
                    }
                    APIO_ADD_INSTR(APIO_IN_NULL(1));
                }

                // Configure the SM registers
                APIO_SM_EXECCTRL_SET(0);
                APIO_SM_SHIFTCTRL_SET(
                    APIO_IN_COUNT(addr_alg->num_addr_pins) |
                    APIO_AUTOPUSH |
                    APIO_PUSH_THRESH(32) |
                    APIO_IN_SHIFTDIR_L |
                    APIO_OUT_SHIFTDIR_L
                );
                APIO_SM_PINCTRL_SET(APIO_IN_BASE(addr_alg->base_addr_pin));

                // Now preload the ROM table RAM address into the X register
                APIO_TXF = rom_table_prefix;
                APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
                APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);
            }
            break;

        default:
            ERR("Invalid address alg: %d", addr_alg->alg);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            return 0;
    }
    APIO_SM_CLKDIV_SET(addr_alg->clkdiv_int, addr_alg->clkdiv_frac);
    APIO_SM_JMP_TO_START();

    APIO_LOG_SM("Address read");

    if (addr_alg->gpio_base == 0) {
        APIO_GPIOBASE_0();
    } else {
        APIO_GPIOBASE_16();
    }

    RUNTIME->addr_pio_block_info |= STORE_PIO_BLOCK_INSTR_LEN(APIO_INSTR_COUNT());
    APIO_END_BLOCK();

    // How the chip select and data PIOs
    APIO_SET_BLOCK(BLOCK_CS_DATA);
    RUNTIME->cs_data_pio_block_info = STORE_PIO_BLOCK_INFO(BLOCK_CS_DATA);

    // Set up the CS PIO algorithm
    APIO_SET_SM(SM_DATA_OUTPUT);
    RUNTIME->cs_data_pio_sm_info = STORE_PIO_SM_INFO(SM_DATA_OUTPUT);

    // Retrieve the common fields
    // Apply the chosen algorithm
    DEBUG("CS alg %u: gpio_base=%u cs=%u+%u data=%u+%u act_dly=%u inact_dly=%u",
        cs_alg->alg, cs_alg->gpio_base,
        cs_alg->base_cs_pin, cs_alg->num_cs_pins,
        cs_alg->base_data_pin, cs_alg->num_data_pins,
        cs_alg->cs_active_delay, cs_alg->cs_inactive_delay);
    switch (cs_alg->alg) {
        case ALG_CS_0: {
            // Get the algorithm parameters
            if (cs_param_len < ALG_CS0_PARAMS_LEN) {
                ERR("CS alg error");
                limp_mode(LIMP_MODE_INVALID_CONFIG);
                return 0;
            }
            const onerom_alg_cs0_param_t *params = (const onerom_alg_cs0_param_t *)cs_params;
            DEBUG("CS0: serve_low=%u byte=%u first_cs=%u first_ncs=%u",
                params->serve_cs_low_0, params->byte_pin,
                params->first_rom_cs_base, params->first_rom_num_cs_pins);

            // Write the SM instructions
            APIO_WRAP_BOTTOM();
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);
            APIO_LABEL_NEW(load_cs);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            if (params->serve_cs_low_0 == 0) {
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(load_cs)));
            } else {
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(load_cs)));
            }
            if (cs_alg->cs_active_delay > 0) {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_active_delay-1)));
            }

            if (params->byte_pin != 0xFF) {
                // Read /BYTE and if low, jump to special code to only set low
                // 8 data pins to outputs
                APIO_LABEL_NEW_OFFSET(byte_low_offset, 4 + (cs_alg->cs_inactive_delay > 0 ? 1 : 0)); 
                APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(byte_low_offset)));
            }

            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);
            APIO_LABEL_NEW(check_cs_gone_inactive);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_WRAP_TOP();
            if (params->serve_cs_low_0 == 0) {
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check_cs_gone_inactive)));
            } else {
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(check_cs_gone_inactive)));
            }
            if (cs_alg->cs_inactive_delay > 0) {
                APIO_WRAP_TOP();
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_inactive_delay-1)));
            }

            // Now the special /BYTE active handling for 16 bit mode.
            if (params->byte_pin != 0xFF) {
                // Set pindirs from Y which is preloaded to 0b11111111, so
                // only low 8 data pins are set to outputs.
                APIO_ADD_INSTR(APIO_MOV_PINDIRS_Y);
                APIO_END();
                APIO_ADD_INSTR(APIO_JMP(APIO_LABEL(check_cs_gone_inactive)));
            }

            // Configure the SM registers
            if (params->byte_pin == 0xFF) {
                APIO_SM_EXECCTRL_SET(0);
            } else {
                APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(params->byte_pin));
            }
            APIO_SM_SHIFTCTRL_SET(
                APIO_IN_COUNT(cs_alg->num_cs_pins) |
                APIO_IN_SHIFTDIR_L
            );
            APIO_SM_PINCTRL_SET(
                APIO_OUT_COUNT(cs_alg->num_data_pins) |
                APIO_OUT_BASE(cs_alg->base_data_pin) |
                APIO_IN_BASE(cs_alg->base_cs_pin)
            );

            if (params->byte_pin != 0xFF) {
                // Preload Y with 0b11111111 for the byte mode handling
                APIO_TXF = 0xFF;
                APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
                APIO_SM_EXEC_INSTR(APIO_MOV_Y_OSR);
            }
        }
        break;

        case ALG_CS_1: {
            // Get the algorithm parameters
            if (cs_param_len < ALG_CS1_PARAMS_LEN) {
                ERR("CS alg error");
                limp_mode(LIMP_MODE_INVALID_CONFIG);
                return 0;
            }
            const onerom_alg_cs1_param_t *params = (const onerom_alg_cs1_param_t *)cs_params;
            DEBUG("CS1: ign_idx=%u", params->cs_ignore_index);

            // Write the SM instructions.
            APIO_LABEL_NEW(inactive_offset);
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);

            // test_if_active:
            APIO_LABEL_NEW(test_if_active_offset);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);

            APIO_LABEL_NEW_OFFSET(active_offset, 2);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(active_offset)));
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(test_if_active_offset)));

            if (cs_alg->cs_active_delay) {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_active_delay - 1)));
            }
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);

            // .wrap_target:
            // test_if_inactive:
            APIO_WRAP_BOTTOM();
            APIO_LABEL_NEW(test_if_inactive_offset);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(test_if_inactive_offset)));
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(inactive_offset)));
            if (cs_alg->cs_inactive_delay) {
                APIO_WRAP_TOP();
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_inactive_delay - 1)));
            }

            // Configure the SM registers
            APIO_SM_EXECCTRL_SET(0);
            APIO_SM_SHIFTCTRL_SET(
                APIO_IN_COUNT(cs_alg->num_cs_pins) |
                APIO_IN_SHIFTDIR_L
            );
            APIO_SM_PINCTRL_SET(
                APIO_OUT_COUNT(cs_alg->num_data_pins) |
                APIO_OUT_BASE(cs_alg->base_data_pin) |
                APIO_IN_BASE(cs_alg->base_cs_pin)
            );

            // Preload Y
            APIO_TXF = (1 << params->cs_ignore_index);
            APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
            APIO_SM_EXEC_INSTR(APIO_MOV_Y_OSR);
        }
        break;

        case ALG_CS_2: {
            if (cs_param_len < ALG_CS2_PARAMS_LEN) {
                ERR("CS alg error");
                limp_mode(LIMP_MODE_INVALID_CONFIG);
                return 0;
            }
            const onerom_alg_cs2_param_t *params = (const onerom_alg_cs2_param_t *)cs_params;
            DEBUG("CS2: qual_base=%u qual_pins=%u inact_pat=0x%02x",
                params->base_qualifier_pin, params->num_qualifier_pins,
                params->qualifier_inactive_pattern);

            // Write the SM instructions

            // Set data pins to inputs, then loop while enable pin is inactive
            APIO_LABEL_NEW(cs2_inactive);
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NULL);
            APIO_LABEL_NEW(cs2_inactive_poll);
            APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(cs2_inactive_poll)));

            // Read qualifier pins; if they don't match the inactive pattern,
            // the bank is selected and CS is active
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_LABEL_NEW_OFFSET(cs2_active, 2);
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs2_active)));
            APIO_ADD_INSTR(APIO_JMP(APIO_LABEL(cs2_inactive_poll)));

            // CS active: set data pins to outputs
            if (cs_alg->cs_active_delay) {
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_active_delay - 1)));
            }
            APIO_ADD_INSTR(APIO_MOV_PINDIRS_NOT_NULL);

            // Poll for enable going inactive or bank being deselected
            APIO_LABEL_NEW(cs2_active_poll);
            APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(cs2_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs2_active_poll)));
            if (cs_alg->cs_inactive_delay) {
                APIO_WRAP_TOP();
                APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_NOP, (cs_alg->cs_inactive_delay - 1)));
            }

            // Configure SM registers
            APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(cs_alg->base_cs_pin));
            APIO_SM_SHIFTCTRL_SET(
                APIO_IN_COUNT(params->num_qualifier_pins) |
                APIO_IN_SHIFTDIR_L
            );
            APIO_SM_PINCTRL_SET(
                APIO_OUT_COUNT(cs_alg->num_data_pins) |
                APIO_OUT_BASE(cs_alg->base_data_pin) |
                APIO_IN_BASE(params->base_qualifier_pin)
            );

            // Preload Y with the qualifier inactive pattern via TXF rather
            // than SET_Y, as the pattern may exceed the 5-bit SET immediate
            APIO_TXF = params->qualifier_inactive_pattern;
            APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
            APIO_SM_EXEC_INSTR(APIO_MOV_Y_OSR);
        }
        break;

        default:
            ERR("Unsupported CS algorithm: %d", cs_alg->alg);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            return 0;
    }
    APIO_SM_CLKDIV_SET(data_alg->clkdiv_int, data_alg->clkdiv_frac);
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("CS/Data output");

    // Set up the data write algorithm
    APIO_SET_SM(SM_DATA_WRITE);
    RUNTIME->cs_data_pio_sm_info |= STORE_PIO_SM_INFO(SM_DATA_WRITE);
    DEBUG("Data alg %u: gpio_base=%u base=%u word=%u clkdiv=%u/%u",
        data_alg->alg, data_alg->gpio_base,
        data_alg->base_data_pin, data_alg->word_size,
        data_alg->clkdiv_int, data_alg->clkdiv_frac);
    switch (data_alg->alg) {
        case ALG_DATA_0: {
            // No parameters
            //if (data_param_len < ALG_DATA0_PARAMS_LEN) {
            //    ERR("Data alg error");
            //    limp_mode(LIMP_MODE_INVALID_CONFIG);
            //    return 0;
            //}
            //const onerom_alg_data0_param_t *params = (const onerom_alg_data0_param_t *)data_params;

            // Write the SM instructions
            APIO_WRAP_BOTTOM();
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_OUT_PINS(data_alg->word_size));

            // Configure the SM registers
            APIO_SM_EXECCTRL_SET(0);
            APIO_SM_SHIFTCTRL_SET(
                APIO_OUT_SHIFTDIR_R |
                APIO_AUTOPULL |
                APIO_PULL_THRESH(data_alg->word_size)
            );
            APIO_SM_PINCTRL_SET(
                APIO_OUT_COUNT(data_alg->word_size) |
                APIO_OUT_BASE(data_alg->base_data_pin)
            );
        }
        break;

        case ALG_DATA_1: {
            // Get the algorithm parameters
            if (data_param_len < ALG_DATA1_PARAMS_LEN) {
                ERR("Data alg error");
                limp_mode(LIMP_MODE_INVALID_CONFIG);
                return 0;
            }
            const onerom_alg_data1_param_t *params = (const onerom_alg_data1_param_t *)data_params;
            DEBUG("Data1: byte=%u a_minus_1=%u", params->byte_pin, params->a_minus_1_pin);

            // Write the SM instructions
            APIO_WRAP_BOTTOM();

            // Read from the TX FIFO
            APIO_ADD_INSTR(APIO_PULL_BLOCK);

            // If /BYTE active mode (high), jump to special byte handling
            APIO_LABEL_NEW_OFFSET(byte_mode_active_offset, 3);
            APIO_ADD_INSTR(APIO_JMP_PIN(APIO_LABEL(byte_mode_active_offset)));

            // 16-bit mode - set all data pins to values from DMA
            APIO_ADD_INSTR(APIO_OUT_PINS(16));

            // If we get here, we're in /BYTE inactive mode and done.  We jump
            // rather than wrapping, as we need the byte mode active code to take
            // no more than 6 cycles (same as address reader SM) or everything gets
            // out of kilter.
            APIO_ADD_INSTR(APIO_JMP(APIO_START_LABEL()));

            // Read the A-1 signalling pin to X
            APIO_ADD_INSTR(APIO_MOV_X_PINS);

            // If X high low (meaning high 8 bits are required) jump to do that
            APIO_LABEL_NEW_OFFSET(high_byte, 4);
            APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(high_byte)));

            // Output low 8 bits
            APIO_ADD_INSTR(APIO_OUT_PINS(8));

            // Output high 8 bits to null
            APIO_ADD_INSTR(APIO_OUT_NULL(8));

            // Jump to start
            APIO_ADD_INSTR(APIO_JMP(APIO_START_LABEL()));

            // First shift low 8 bits to null
            APIO_ADD_INSTR(APIO_OUT_NULL(8));

            // Write high 8 bits to pins, then wrap to save a JMP
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_OUT_PINS(8)); 

            // Configure the SM registers
            APIO_SM_EXECCTRL_SET(APIO_EXECCTRL_JMP_PIN(params->byte_pin));
            APIO_SM_SHIFTCTRL_SET(
                APIO_OUT_SHIFTDIR_R |
                APIO_AUTOPULL |
                APIO_PULL_THRESH(16) |
                APIO_IN_COUNT(1)        // A-1
            );
            APIO_SM_PINCTRL_SET(
                APIO_OUT_BASE(data_alg->base_data_pin) |
                APIO_OUT_COUNT(16) |
                APIO_IN_BASE(params->a_minus_1_pin)
            );
        }
        break;

        default:
            ERR("Unsupported data algorithm: %d", data_alg->alg);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            return 0;
    }
    APIO_SM_CLKDIV_SET(data_alg->clkdiv_int, data_alg->clkdiv_frac);
    APIO_SM_JMP_TO_START();
    APIO_LOG_SM("Data write");

    if (cs_alg->gpio_base == data_alg->gpio_base) {
        if (data_alg->gpio_base == 0) {
            APIO_GPIOBASE_0();
        } else {
            APIO_GPIOBASE_16();
        }
    } else {
        ERR("Address/Data GPIO base mismatch");
        limp_mode(LIMP_MODE_INVALID_CONFIG);
        return 0;
    }

    RUNTIME->cs_data_pio_block_info |= STORE_PIO_BLOCK_INSTR_LEN(APIO_INSTR_COUNT());
    APIO_END_BLOCK();

    return 0;
}

// Enable the SMs
void start_serving_pios(void) {
    APIO_ENABLE_SMS(BLOCK_ADDR, 1 << SM_ADDR_READ);
    APIO_ENABLE_SMS(BLOCK_CS_DATA, ((1 << SM_DATA_OUTPUT) | (1 << SM_DATA_WRITE)));
}

static int setup_serving_dma(const onerom_rom_slot_t *slot, uint32_t rom_table_addr) {
#if !REAL_HARDWARE
    (void)slot;
    (void)rom_table_addr;
#endif // !REAL_HARDWARE
    RUNTIME->dma_pio_ch = STORE_DMA_CH_INFO(DMA_CH_ADDR_READ);
    RUNTIME->dma_pio_ch |= STORE_DMA_CH_INFO(DMA_CH_DATA_WRITE);

    // TODO dynamically figure out which blocks PIO SMs used are in and don't
    // use fixed APIO1/APIO2 macros

#if defined(DEBUG_LOGGING)
    const onerom_alg_dma_config_t *dma_alg = slot->alg->alg_dma;
    DEBUG("DMA alg %u: bit_mode=%u continuous=%u",
        dma_alg->alg, dma_alg->bit_mode, dma_alg->continuous);
#endif // DEBUG_LOGGING
#if REAL_HARDWARE
#if !defined(DEBUG_LOGGING)
    const onerom_alg_dma_config_t *dma_alg = slot->alg->alg_dma;
#endif // DEBUG_LOGGING
    switch (dma_alg->alg) {
        case ALG_DMA_0: {
            volatile dma_ch_reg_t *dma_reg;

            // DMA Channel 0 - Receives ROM table lookup address from PIO1 SM0
            // and sends it onto DMA Channel 1.  Paced by PIO1 SM0 RX FIFO
            // DREQ.
            dma_reg = DMA_CH_REG(DMA_CH_ADDR_READ);
            dma_reg->read_addr = (uint32_t)&APIO1_SM_RXF(SM_ADDR_READ);
            if (!dma_alg->continuous) {
                // When address read is triggerd by IRQ, we only want a single
                // transfer per IRQ.  We need to trigger channel 1 manually.
                dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR_TRIG(DMA_CH_DATA_WRITE);
                dma_reg->transfer_count = 1;
            } else {
                // When address read is not triggered by IRQ, we want
                // continuous transfers to channel 1.  No triggering is
                // necessary, as channel 1 will be paced by the PIO1 SM0 RX
                // FIFO DREQ, like this channel.
                dma_reg->write_addr = (uint32_t)&DMA_CH_READ_ADDR(DMA_CH_DATA_WRITE);
                dma_reg->transfer_count = 0xffffffff;
            }
            dma_reg->ctrl_trig =
                DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(BLOCK_ADDR, SM_ADDR_READ)) |
                DMA_CTRL_TRIG_EN |
                DMA_CTRL_TRIG_DATA_SIZE_32BIT |
                DMA_CTRL_TRIG_CHAIN_TO(DMA_CH_ADDR_READ);

            // DMA Channel 1 - Reads ROM data from memory and sends to PIO0
            // SM3.  Also paced by PIO0 SM1 RX FIF DREQ, so runs in lock-step
            // with channel 0.
            // Pre-load the READ_ADDR register with the first byte of the ROM
            // table.  This byte will never actually get served, as the data
            // lines will be inputs, but it's more valid than setting to 0.
            dma_reg = DMA_CH_REG(DMA_CH_DATA_WRITE);
            dma_reg->read_addr = rom_table_addr;
            dma_reg->write_addr = (uint32_t)&APIO2_SM_TXF(SM_DATA_WRITE);
            uint32_t ctrl_trig = DMA_CTRL_TRIG_EN | DMA_CTRL_TRIG_CHAIN_TO(DMA_CH_ADDR_READ);
            ctrl_trig |= (dma_alg->bit_mode == BIT_MODE_16)
                ? DMA_CTRL_TRIG_DATA_SIZE_16BIT
                : DMA_CTRL_TRIG_DATA_SIZE_8BIT;
            if (!dma_alg->continuous) {
                // When address read is triggerd by IRQ, we only want a single
                // transfer per IRQ.  We need to re-trigger channel 1 manually.
                dma_reg->transfer_count = 1;
                ctrl_trig |= DMA_CTRL_TRIG_TREQ_SEL(DMA_CTRL_TRIG_TREQ_PERM);
            } else {
                // When address read is not triggered by IRQ, we want
                // continuous transfers.
                dma_reg->transfer_count = 0xffffffff;
                ctrl_trig |= DMA_CTRL_TRIG_TREQ_SEL(APIO_DREQ_PIO_X_SM_Y_RX(BLOCK_ADDR, SM_ADDR_READ));
            }
            dma_reg->ctrl_trig = ctrl_trig;
        }
        break;

        default:
            ERR("Unsupported DMA algorithm: %d", dma_alg->alg);
            limp_mode(LIMP_MODE_INVALID_CONFIG);
            return -1;
    }

    BUSCTRL_BUS_PRIORITY |=
    BUSCTRL_BUS_PRIORITY_DMA_R_BIT |
    BUSCTRL_BUS_PRIORITY_DMA_W_BIT;
#endif // REAL_HARDWARE

    return 0;
}