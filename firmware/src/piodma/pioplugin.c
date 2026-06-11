// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Plugin routines dependent on PIO function

#include "include.h"
#include "piodma/piodma.h"

#if REAL_HARDWARE

static void pio_setup_address_monitor_pios() {
    const onerom_rom_slot_t *slot = RUNTIME->current_rom_slot;

    APIO_ASM_INIT();

    // Use the same block as the ROM serving CS/Data PIO, starting from
    // where it left off
    uint8_t cs_data_block = GET_PIO_BLOCK_INFO(RUNTIME->cs_data_pio_block_info);
    uint8_t cs_data_sm_pos = GET_PIO_BLOCK_INSTR_LEN(RUNTIME->cs_data_pio_block_info);
    APIO_SET_BLOCK_FROM_VAR(cs_data_block, cs_data_sm_pos);

    //
    // SM 0: CS Monitor
    //
    if (CHECK_PIO_SM_INFO(RUNTIME->cs_data_pio_sm_info, SM_ADDR_MONITOR_CS_MONITOR)) {
        ERR("CS monitor SM already in use");
        return;
    }
    APIO_SET_SM(SM_ADDR_MONITOR_CS_MONITOR);
    const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;
    if (cs_alg->gpio_base == 0) {
        APIO_GPIOBASE_0();
    } else {
        APIO_GPIOBASE_16();
    }
    uint8_t base_cs_pin = cs_alg->base_cs_pin;
    uint8_t num_cs_pins = cs_alg->num_cs_pins;
    switch (cs_alg->alg) {
        case ALG_CS_0: {
            // All CS pins contiguous - CS active == zero
            APIO_WRAP_BOTTOM();
            APIO_LABEL_NEW(cs_inactive);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            const onerom_alg_cs0_param_t *params = (const onerom_alg_cs0_param_t *)cs_alg->params;
            if (params->serve_cs_low_0 == 0) {
                // CS active == zero
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_inactive)));
            } else {
                // CS active == non-zero (pins inverted)
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
                APIO_ADD_INSTR(APIO_MOV_X_PINS);
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_inactive)));
            }

            APIO_ADD_INSTR(APIO_IRQ_SET(ADDR_MONITOR_IRQ));

            APIO_LABEL_NEW(cs_active);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_WRAP_TOP();
            if (params->serve_cs_low_0 == 0) {
                // CS inactive == non-zero
                APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_active)));
            } else {
                // CS inactive == zero (pins inverted)
                APIO_ADD_INSTR(APIO_JMP_X_DEC(APIO_LABEL(cs_active)));
            }

            // If multi-ROM mode, use only the first ROM's CS pins
            if ((params->first_rom_num_cs_pins > 0) && (params->first_rom_num_cs_pins < 0xFF)) {
                base_cs_pin = params->first_rom_cs_base;
                num_cs_pins = params->first_rom_num_cs_pins;
            }
            break;
        }

        case ALG_CS_1: {
            const onerom_alg_cs1_param_t *params = (const onerom_alg_cs1_param_t *)cs_alg->params;

            APIO_LABEL_NEW(cs_inactive);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_LABEL_NEW_OFFSET(check2, 2);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check2)));
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_LABEL_NEW_OFFSET(check3, 2);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check3)));
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_LABEL_NEW_OFFSET(check4, 2);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(check4)));
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_LABEL_NEW_OFFSET(cs_active, 2);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(cs_active)));
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));

            // cs_active:
            APIO_ADD_INSTR(APIO_IRQ_SET(ADDR_MONITOR_IRQ));

            APIO_WRAP_BOTTOM();
            APIO_LABEL_NEW(test_if_inactive);
            APIO_ADD_INSTR(APIO_MOV_X_PINS);
            APIO_ADD_INSTR(APIO_JMP_NOT_X(APIO_LABEL(test_if_inactive)));
            APIO_WRAP_TOP();
            APIO_ADD_INSTR(APIO_JMP_X_NOT_Y(APIO_LABEL(cs_inactive)));
            // cs_pin_2nd_match still active - wrap back to test_if_inactive

            // Preload Y with the mask of the ignored CS pin
            APIO_TXF = (1 << params->cs_ignore_index);
            APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
            APIO_SM_EXEC_INSTR(APIO_MOV_Y_OSR);
            break;
        }

        // 23QL384 is not currently supported
        case ALG_CS_2:
        default:
            ERR("Unsupported CS algorithm: %d", cs_alg->alg);
            break;
    }

    APIO_SM_CLKDIV_SET(cs_alg->clkdiv_int, cs_alg->clkdiv_frac);
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_IN_COUNT(num_cs_pins) |
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(base_cs_pin)
    );
    APIO_SM_JMP_TO_START();

    APIO_END_BLOCK_FROM(cs_data_sm_pos);

    //
    // SM 1: Address read monitor
    //
    uint8_t addr_block = GET_PIO_BLOCK_INFO(RUNTIME->addr_pio_block_info);
    uint8_t addr_sm_pos = GET_PIO_BLOCK_INSTR_LEN(RUNTIME->addr_pio_block_info);
    APIO_SET_BLOCK_FROM_VAR(addr_block, addr_sm_pos);

    // There is currently only a single address algorithm
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    if (cs_alg->gpio_base != addr_alg->gpio_base) {
        // TODO
        // This happens for 32 and 40 pin ROMs - needs an enhancement to
        // add them to the regular PIO blocks.
        ERR("Address/Data GPIO base mismatch");
        return;
    }
    APIO_SET_SM(SM_ADDR_MONITOR_ADDR_READ);

    APIO_ADD_INSTR(APIO_WAIT_IRQ_HIGH(ADDR_MONITOR_IRQ));
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_IN_PINS(addr_alg->num_addr_pins));

    APIO_SM_CLKDIV_SET(1, 0);
    APIO_SM_EXECCTRL_SET(0);
    APIO_SM_SHIFTCTRL_SET(
        APIO_AUTOPUSH        |
        APIO_PUSH_THRESH(addr_alg->num_addr_pins) |
        APIO_IN_SHIFTDIR_L
    );
    APIO_SM_PINCTRL_SET(
        APIO_IN_BASE(addr_alg->base_addr_pin)
    );
    APIO_SM_JMP_TO_START();

    APIO_END_BLOCK_FROM(addr_sm_pos);

    return;
}

static void pio_setup_address_monitor_dma(
    uint8_t dma_ch,
    uint8_t block,
    uint8_t sm_addr_read,
    volatile uint32_t *ring_buf,
    uint8_t ring_size_log2,
    uint8_t data_size
) {
    uint32_t dma_data_size;
    if (data_size == 8) {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_8BIT;
    } else if (data_size == 16) {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_16BIT;
    } else {
        dma_data_size = DMA_CTRL_TRIG_DATA_SIZE_32BIT;
    }

    // SM1 RX FIFO -> ring_buf circular write
    volatile dma_ch_reg_t *dma_reg = DMA_CH_REG(dma_ch);
    dma_reg->read_addr = (uint32_t)&APIO0_SM_RXF(sm_addr_read);
    dma_reg->write_addr = (uint32_t)ring_buf;
    dma_reg->transfer_count = 0xffffffff;
    dma_reg->ctrl_trig =
        DMA_CTRL_TRIG_EN |
        dma_data_size |
        DMA_CTRL_RING_SIZE(ring_size_log2) |
        DMA_CTRL_RING_SEL |
        DMA_CTRL_INCR_WRITE |
        DMA_CTRL_TRIG_CHAIN_TO(dma_ch) |
        DMA_CTRL_TRIG_TREQ_SEL(
            APIO_DREQ_PIO_X_SM_Y_RX(
                block,
                sm_addr_read
            )
        );
}

ora_result_t pio_setup_address_monitor(
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    ora_monitor_mode_t mode,
    uint8_t data_size,
    void *reserved
) {
    (void)mode;
    (void)reserved;

    uint32_t bytes_per_entry_log2 = __builtin_ctz(data_size / 8); // 8->0, 16->1, 32->2
    uint32_t ring_size_log2 = ring_entries_log2 + bytes_per_entry_log2;
    uint32_t ring_size = 1u << ring_size_log2;

    // Check ring_buf is valid and aligned to ring size
    if (ring_buf == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (((uintptr_t)ring_buf % ring_size) != 0) {
        return ORA_RESULT_INVALID_SIZE;
    }

    pio_setup_address_monitor_pios();
    pio_setup_address_monitor_dma(
        DMA_CH_ADDR_MONITOR,
        BLOCK_MONITOR,
        SM_ADDR_MONITOR_ADDR_READ,
        ring_buf,
        ring_size_log2,
        data_size
    );

    return ORA_RESULT_OK;
}

static ora_result_t get_addr_pin(
    const onerom_rom_slot_t *slot,
    uint8_t ii,
    uint8_t *pin_out
) {
    // Select the hardware mapping from the first ROM in the slot.
    const onerom_rom_pin_map_t *pin_map = slot->roms[0]->pin_map;

    if (pin_map->addr[ii] >= MAX_GPIOS) {
        return ORA_RESULT_INTERNAL_ERROR;
    }
    *pin_out = pin_map->addr[ii];
    return ORA_RESULT_OK;
}

// This function always returns a mapping from logical to physical pins for
// the first ROM in a multi-ROM slot.
uint32_t pio_map_addr_to_phys(
    const onerom_rom_slot_t *slot,
    uint32_t logical_addr
) {
    uint8_t base = BASE_ADDR_PIN;
    uint8_t num  = NUM_ADDR_PINS;
    uint32_t physical = 0;

    for (uint8_t b = 0; b < num; b++) {
        if (logical_addr & (1u << b)) {
            uint8_t pin;
            if (get_addr_pin(CURRENT_SLOT, b, &pin) == ORA_RESULT_OK) {
                physical |= (1u << (pin - base));
            }
        }
    }

    // In multi-ROM mode CS1 is part of the SRAM address space, and active
    // CS1 = bit SET (inverted).  Always OR in the CS1 bit so back-channel
    // writes land in the correct half of SRAM that the host is reading
    // from.  Also handles multiple CSs
    if ((slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) &&
        (slot->alg->alg_cs->alg == ALG_CS_0)) {
        onerom_alg_cs0_param_t *params = (onerom_alg_cs0_param_t *)slot->alg->alg_cs->params;
        uint8_t base_cs_pin = params->first_rom_cs_base;
        uint8_t num_cs_pins = params->first_rom_num_cs_pins;

        for (int cs_pin = base_cs_pin; cs_pin < (base_cs_pin + num_cs_pins); cs_pin++) {
            if (cs_pin < MAX_GPIOS) {
                physical |= (1u << (cs_pin - base));
            }
        }
    }

    return physical;
}

static ora_result_t get_data_pin(
    const onerom_rom_slot_t *slot,
    uint8_t ii,
    uint8_t *pin_out
) {
    // Select the hardware mapping from the first ROM in the slot.
    const onerom_rom_pin_map_t *pin_map = slot->roms[0]->pin_map;

    if (pin_map->data[ii] >= MAX_GPIOS) {
        return ORA_RESULT_INTERNAL_ERROR;
    }
    *pin_out = pin_map->data[ii];
    return ORA_RESULT_OK;
}

uint32_t pio_map_data_to_phys(
    const onerom_rom_slot_t *slot,
    uint32_t logical_data
) {
    uint8_t base = BASE_DATA_PIN;
    uint32_t physical = 0;

    for (uint8_t b = 0; b < 8; b++) {
        if (logical_data & (1u << b)) {
            uint8_t pin;
            if (get_data_pin(slot, b, &pin) == ORA_RESULT_OK) {
                physical |= (1u << (pin - base));
            }
        }
    }
    return physical;
}

#define MAX_CS_PINS 8
static void get_cs_pins(
    const onerom_rom_slot_t *slot,
    uint8_t *cs_pins_out,
    uint8_t *cs_pins_over_out
) {
    const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;
    switch (cs_alg->alg) {
        case ALG_CS_0:
        case ALG_CS_2: {
            for (int ii = 0; (ii < cs_alg->num_cs_pins) && (ii < MAX_CS_PINS); ii++) {
                cs_pins_out[ii] = cs_alg->base_cs_pin + ii;
            }
            break;
        }

        case ALG_CS_1: {
            onerom_alg_cs1_param_t *params = (onerom_alg_cs1_param_t *)cs_alg->params;
            int jj = 0;
            for (int ii = 0; (ii < cs_alg->num_cs_pins) && (ii < MAX_CS_PINS); ii++) {
                if (ii != params->cs_ignore_index) {
                        cs_pins_out[jj] = cs_alg->base_cs_pin + ii;
                        jj++;
                }
            }
            break;
        }

        default:
            ERR("Unsupported CS algorithm: %d", cs_alg->alg);
            break;
    }

    // Get whether they are overridden
    const onerom_alg_override_config_t *override = slot->alg->gpio_override_config;
    for (int ii = 0; ii < override->param_len; ii++) {
        for (int jj = 0; jj < MAX_CS_PINS; jj++) {
            if ((override->params[ii] & 0x3F) == cs_pins_out[jj]) {
                cs_pins_over_out[jj] = (override->params[ii] >> 6) & 0x03;
            }
        }
    }
}

#define MAX_X_PINS 2
static void get_x_pins(
    const onerom_rom_slot_t *slot,
    uint8_t *x_pins_out,
    uint8_t *x_pins_over_out
) {
    // Get the X pins
    x_pins_out[0] = HW->gpio_x1;
    x_pins_out[1] = HW->gpio_x2;

    // Get whether they are overridden
    const onerom_alg_override_config_t *override = slot->alg->gpio_override_config;
    for (int ii = 0; ii < override->param_len; ii++) {
        for (int jj = 0; jj < MAX_X_PINS; jj++) {
            if ((override->params[ii] & 0x3F) == x_pins_out[jj]) {
                x_pins_over_out[jj] = (override->params[ii] >> 6) & 0x03;
            }
        }
    }
}

ora_result_t pio_demangle_addr(
    const onerom_rom_slot_t *slot,
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
) {
    if (logical_addr_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint8_t base = BASE_ADDR_PIN;
    uint8_t num  = NUM_ADDR_PINS;

    if (check_control_pins) {
        uint8_t x_pins[MAX_X_PINS];
        uint8_t x_pins_override[MAX_X_PINS];
        get_x_pins(slot, x_pins, x_pins_override);

        uint8_t cs_pins[MAX_CS_PINS] = {GPIO_NONE};
        uint8_t cs_pins_override[MAX_CS_PINS] = {0};
        get_cs_pins(slot, cs_pins, cs_pins_override);

        // Test for actice CS pins
        for (int ii = 0; ii < MAX_CS_PINS && cs_pins[ii] < MAX_GPIOS; ii++) {
            uint8_t cs_pin = cs_pins[ii];
            if ((cs_pin >= base) && (cs_pin < (base + num))) {
                uint8_t cs_pin_pos = 1u << (cs_pin - base);
                switch (cs_pins_override[ii]) {
                    case GPIO_OVER_NORMAL:
                        // CS is inactive if 
                        if ((physical_addr & cs_pin_pos) == 0) {
                            return ORA_RESULT_CONTROL_PIN_ACTIVE;
                        }
                        break;

                    case GPIO_OVER_INVERT:
                        if ((physical_addr & cs_pin_pos) != 0) {
                            return ORA_RESULT_CONTROL_PIN_ACTIVE;
                        }
                        break;

                    default:
                        // Ignore other cases
                        break;
                }
            }
        }

        if (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) {
            // Test for active X pins - if any active, reject
            for (int ii = 0; ii < MAX_X_PINS && x_pins[ii] < MAX_GPIOS; ii++) {
                uint8_t x_pin = x_pins[ii];
                if ((x_pin >= base) && (x_pin < (base + num))) {
                    uint8_t x_pin_pos = 1u << (x_pin - base);
                    switch (x_pins_override[ii]) {
                        case GPIO_OVER_NORMAL:
                            if ((physical_addr & x_pin_pos) != 0) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;

                        case GPIO_OVER_INVERT:
                            if ((physical_addr & x_pin_pos) == 0) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;

                        default:
                            // Ignore other cases
                            break;
                    }
                }
            }
        }
    }

    // 23QL512 not supported here, nor are other chip types like the 231024,
    // 2732 - any snowflake chip type
    // TODO - lift restriction
    uint32_t logical = 0;
    for (uint8_t b = 0; b < num; b++) {
        uint8_t pin;
        if (get_addr_pin(slot, b, &pin) == ORA_RESULT_OK) {
            if (physical_addr & (1u << (pin - base))) {
                logical |= (1u << b);
            }
        }
    }

    *logical_addr_out = logical;
    return ORA_RESULT_OK;
}

uint8_t pio_demangle_data(
    const onerom_rom_slot_t *slot,
    uint8_t physical_data
) {
    uint8_t base = BASE_DATA_PIN;
    const onerom_rom_pin_map_t *pin_map = slot->roms[0]->pin_map;
    uint8_t logical = 0;
    for (uint8_t b = 0; b < 8; b++) {
        uint8_t pin = pin_map->data[b];
        if (pin < MAX_GPIOS) {
            if (physical_data & (1u << (pin - base))) {
                logical |= (1u << b);
            }
        }
    }
    return logical;
}

ora_result_t pio_init_knock(
    const uint32_t *knock_seq,
    uint8_t knock_len,
    uint8_t knock_bits,
    uint8_t data_size,
    ora_knock_t *knock
) {
    if (knock_seq == NULL || knock == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (knock_len == 0 || knock_bits == 0 || knock_bits > NUM_ADDR_PINS) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint8_t base = BASE_ADDR_PIN;
    uint8_t pin;
    ora_result_t result;

    knock->mask = 0;
    for (uint8_t i = 0; i < knock_bits; i++) {
        result = get_addr_pin(CURRENT_SLOT, i, &pin);
        if (result != ORA_RESULT_OK) {
            return result;
        }
        knock->mask |= (1u << (pin - base));
    }

    for (uint8_t k = 0; k < knock_len; k++) {
        knock->matches[k] = 0;
        for (uint8_t i = 0; i < knock_bits; i++) {
            result = get_addr_pin(CURRENT_SLOT, i, &pin);
            if (result != ORA_RESULT_OK) {
                return result;
            }
            if (knock_seq[k] & (1u << i)) {
                knock->matches[k] |= (1u << (pin - base));
            }
        }
    }

    // Calculate CS and X pin masks for filtering during knock detection and
    // payload collection
    uint32_t cs_mask = 0;
    uint32_t x_mask = 0;

    // Get CS pins
    uint8_t cs_pins[MAX_CS_PINS] = {GPIO_NONE};
    uint8_t cs_pins_override[MAX_CS_PINS] = {0};
    get_cs_pins(CURRENT_SLOT, cs_pins, cs_pins_override);
    for (int ii = 0; ii < MAX_CS_PINS && cs_pins[ii] < MAX_GPIOS; ii++) {
        uint8_t cs_pin = cs_pins[ii];
        if ((cs_pin >= base) && (cs_pin < (base + NUM_ADDR_PINS))) {
            cs_mask |= 1u << (cs_pin - base);
        }
    }
    if (CURRENT_SLOT->slot_type != ROM_SLOT_TYPE_MULTI_ROM) {
        knock->multi_rom_mode = 0;
    } else {
        uint8_t x1_pin = HW->gpio_x1;
        uint8_t x2_pin = HW->gpio_x2;
        if ((x1_pin < (base + NUM_ADDR_PINS)) && (x1_pin >= base)) {
            x_mask |= 1u << (x1_pin - base);
        }
        if ((x2_pin < (base + NUM_ADDR_PINS)) && (x2_pin >= base)) {
            x_mask |= 1u << (x2_pin - base);
        }
        knock->multi_rom_mode = 1;
    }

    knock->len  = knock_len;
    knock->bits = knock_bits;
    knock->data_size = data_size;
    knock->cs_mask = cs_mask;
    knock->x_mask = x_mask;

    return ORA_RESULT_OK;
}

__attribute__((always_inline)) static inline uint8_t debounce(
    uint32_t entry,
    const ora_knock_t *knock
) {
    // Primary CS debouncing is now done in the address monitor PIO SM
    //if (!knock->multi_rom_mode) {
    //    if (knock->cs_mask && (entry & knock->cs_mask)) return 1;     // CS inactive - bit set (active low, not inverted)
    //} else {
    //    if (knock->cs_mask && !(entry & knock->cs_mask)) return 1;  // CS inactive - bit clear after inversion
    //}

    // So the only thing needed here is filtering if X pin(s) active
    if (knock->x_mask && (entry & knock->x_mask)) return 1;     // X pin active
    return 0;
}

// Written as a macro to allow multiple data sizes
#define KNOCK_DETECT_LOOP(TYPE) do {                                        \
    volatile TYPE *rp = (volatile TYPE *)read_ptr;                          \
    volatile TYPE *rb = (volatile TYPE *)ring_buf;                          \
    while (knock_pos < knock->len) {                                        \
        volatile TYPE *wp = (volatile TYPE *)                               \
            DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;                    \
        while (rp != wp) {                                                  \
            uint32_t entry = (uint32_t)*rp;                                 \
            if (++rp >= rb + ring_entries) rp = rb;                         \
            if (debounce_cs) {                                              \
                if (debounce(entry, knock)) continue;                       \
            }                                                               \
            if ((entry & knock->mask) == knock->matches[knock_pos]) {       \
                knock_pos++;                                                \
                if (knock_pos >= knock->len) break;                         \
            } else {                                                        \
                knock_pos = ((entry & knock->mask) == knock->matches[0])    \
                    ? 1 : 0;                                                \
            }                                                               \
        }                                                                   \
    }                                                                       \
    read_ptr = (volatile uint32_t *)rp;                                     \
} while (0)

#define PAYLOAD_COLLECT_LOOP(TYPE) do {                                     \
    volatile TYPE *rp = (volatile TYPE *)read_ptr;                          \
    volatile TYPE *rb = (volatile TYPE *)ring_buf;                          \
    while (payload_pos < payload_len) {                                     \
        volatile TYPE *wp = (volatile TYPE *)                               \
            DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;                    \
        while (rp != wp && payload_pos < payload_len) {                     \
            uint32_t entry = (uint32_t)*rp;                                 \
            if (++rp >= rb + ring_entries) rp = rb;                         \
            if (debounce_cs) {                                              \
                if (debounce(entry, knock)) continue;                       \
            }                                                               \
            payload_out[payload_pos++] = entry;                             \
        }                                                                   \
    }                                                                       \
    read_ptr = (volatile uint32_t *)rp;                                     \
} while (0)

ora_result_t pio_wait_for_knock(
    const ora_knock_t *knock,
    volatile uint32_t *ring_buf,
    uint8_t ring_entries_log2,
    uint32_t flags,
    uint32_t *payload_out,
    uint8_t payload_len,
    volatile uint32_t *start_pos,
    volatile uint32_t **next_read_out
) {
    // Discard any captures that occurred before we were called.  Do this first
    // to avoid missing bytes, even before testing for a start_pos.
    volatile uint32_t *read_ptr = (volatile uint32_t *)DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;
    if (start_pos != NULL) {
        // We have a start_pos so use that instead.
        read_ptr = start_pos;
    }

    // Next check the args
    if (knock == NULL || ring_buf == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (payload_len > 0 && payload_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    uint32_t ring_entries = 1u << ring_entries_log2;
    uint8_t debounce_cs = (flags & ORA_WAIT_FOR_KNOCK_FLAG_DEBOUNCE_CS) != 0;

    // Knock detection loop
    uint8_t knock_pos = 0;
    switch (knock->data_size) {
        case 8:  KNOCK_DETECT_LOOP(uint8_t);  break;
        case 16: KNOCK_DETECT_LOOP(uint16_t); break;
        default: KNOCK_DETECT_LOOP(uint32_t); break;
    }

    // Payload collection
    uint8_t payload_pos = 0;
    switch (knock->data_size) {
        case 8:  PAYLOAD_COLLECT_LOOP(uint8_t);  break;
        case 16: PAYLOAD_COLLECT_LOOP(uint16_t); break;
        default: PAYLOAD_COLLECT_LOOP(uint32_t); break;
    }

    if (next_read_out != NULL) {
        *next_read_out = read_ptr;
    }
    return ORA_RESULT_OK;
}

ora_result_t pio_reprogram_ram_rom_slot(
    uint8_t slot,
    uint32_t offset,
    const uint8_t *data,
    uint32_t len,
    uint8_t allow_active
) {
    if (data == NULL || len == 0) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Get the SRAM address and size of the target slot
    uint32_t addr, size;
    ora_result_t result = ora_get_ram_slot_info(slot, &addr, &size, NULL);
    if (result != ORA_RESULT_OK) {
        return result;
    }

    // Check the write stays within the slot
    if (offset + len > size) {
        return ORA_RESULT_INVALID_ARG;
    }

    // If allow_active is not set, refuse to write to the currently active slot
    if (!allow_active) {
        uint8_t active_slot;
        result = ora_get_active_ram_slot(&active_slot);
        if (result == ORA_RESULT_OK && active_slot == slot) {
            return ORA_RESULT_SLOT_ACTIVE;
        }
    }

    // Remap logical addresses and data bytes to their physical representations
    // and write to the target slot in SRAM
    uint8_t *sram = (uint8_t *)addr;
    for (uint32_t i = 0; i < len; i++) {
        uint32_t physical_addr = pio_map_addr_to_phys(CURRENT_SLOT, offset + i);
        uint8_t  physical_data = pio_map_data_to_phys(CURRENT_SLOT, data[i]);
        sram[physical_addr] = physical_data;
    }

    return ORA_RESULT_OK;
}

ora_result_t pio_start_address_monitor(void) {
    APIO_ENABLE_SMS(BLOCK_MONITOR, ((1 << SM_ADDR_MONITOR_CS_MONITOR) | (1 << SM_ADDR_MONITOR_ADDR_READ)));

    return ORA_RESULT_OK;
}

volatile uint32_t * volatile *pio_get_address_monitor_ring_write_pos(void) {
    return (volatile uint32_t * volatile *)&DMA_CH_REG(DMA_CH_ADDR_MONITOR)->write_addr;
}

uint8_t pio_get_effective_addr_pins(void) {
    uint8_t effective_addr_pins = NUM_ADDR_PINS;
    if (BIT_MODE == BIT_MODE_16) {
        effective_addr_pins += 1;
    }
    return effective_addr_pins;
}

uint32_t pio_get_rom_region_size(void) {
    return 1u << pio_get_effective_addr_pins();
}

ora_result_t pio_switch_rom_region(uint32_t new_region_addr) {
    // Input validation is the caller's responsibility. ora_set_active_ram_slot
    // validates the slot index and derives a correct address via
    // ora_get_ram_slot_info before calling this function.
    uint8_t effective_addr_pins = pio_get_effective_addr_pins();
    uint8_t rom_table_num_addr_bits = 32 - effective_addr_pins;
    uint32_t high_bits_mask = (1u << rom_table_num_addr_bits) - 1;
    uint32_t rom_table_high_bits = (new_region_addr >> effective_addr_pins) & high_bits_mask;

    // Update the ROM table address in the config to keep it consistent with
    // reality.
    RUNTIME->rom_table = (void *)new_region_addr;

    // Avoid unused variable warnings from APIO implementation causing
    // compile errors.
    // Update the X register in the address read SM with the new RAM table
    // base.  This delays the address read SM by a single cycle, but is an
    // atomic switch.
    APIO_ASM_INIT();
    APIO_SET_BLOCK(BLOCK_ADDR);
    APIO_SET_SM(SM_ADDR_READ);
    APIO_TXF = rom_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);

    // This is the point at which the SRAM region switch takes effect.
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);

    return ORA_RESULT_OK;
}

#endif // REAL_HARDWARE