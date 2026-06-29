// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Plugin routines dependent on PIO function

#include "include.h"
#include "piodma/piodma.h"

// GPIOs for X1 and X2 within the address span.  GPIO_NONE when absent.
typedef struct {
    uint8_t x1_gpio;
    uint8_t x2_gpio;
} v2_x_pin_gpios_t;

// Returns the 2-bit GPIO input override type for gpio from
// gpio_override_config, or GPIO_OVER_NORMAL (0) if not present or if
// gpio_override_config is NULL.
static uint8_t v2_get_gpio_override(
    const onerom_rom_slot_t *slot,
    uint8_t gpio
) {
    const onerom_alg_override_config_t *ovr = slot->alg->gpio_override_config;
    if (ovr == NULL || ovr == (const onerom_alg_override_config_t *)0xFFFFFFFF) {
        return GPIO_OVER_NORMAL;
    }
    for (uint8_t i = 0; i < ovr->param_len; i++) {
        if ((ovr->params[i] & 0x3Fu) == gpio) {
            return (uint8_t)((ovr->params[i] >> 6u) & 0x03u);
        }
    }
    return GPIO_OVER_NORMAL;
}

// Identifies the X1 and X2 GPIOs within the address span for Multi/Banked
// slots using the hardware metadata arrays HW->gpio_x1[] / HW->gpio_x2[].
//
// For each GPIO g in [addr_base, addr_base + num_addr_pins):
//   - chip address pins (pin_map->addr[]) are skipped.
//   - the full CS range [cs_base, cs_base + num_cs_pins) is skipped; this
//     covers both real CS lines and any cs_ignore_index gap, and applies to
//     all slot types (Banked sets also have CS lines as gap bits in the
//     address span).
//   - remaining GPIOs are matched against HW->gpio_x1[] / HW->gpio_x2[].
//     If no match is found an internal error is logged.
//
// Returns {GPIO_NONE, GPIO_NONE} for Single slots.
static v2_x_pin_gpios_t v2_get_x_pin_gpios(
    const onerom_rom_slot_t *slot,
    uint8_t addr_base
) {
    v2_x_pin_gpios_t result = { GPIO_NONE, GPIO_NONE };
 
    if (slot->slot_type != ROM_SLOT_TYPE_MULTI_ROM &&
        slot->slot_type != ROM_SLOT_TYPE_BANKED_ROM) {
        return result;
    }
 
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    const onerom_alg_cs_config_t   *cs_alg   = slot->alg->alg_cs;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
 
    uint8_t cs_base = cs_alg->gpio_base + cs_alg->base_cs_pin;
    uint8_t cs_end  = cs_base + cs_alg->num_cs_pins;
 
    for (uint8_t g = addr_base;
         g < (uint8_t)(addr_base + addr_alg->num_addr_pins); g++) {
 
        // Skip chip address pins.
        uint8_t is_addr = 0;
        for (uint8_t n = 0; n < MAX_ADDR_PINS; n++) {
            if (pin_map->addr[n] >= GPIO_NONE) break;
            if (pin_map->addr[n] == g) { is_addr = 1; break; }
        }
        if (is_addr) continue;
 
        // Skip CS range (real CS lines and any cs_ignore_index gap).
        if (g >= cs_base && g < cs_end) continue;
 
        // Match against hardware X pin arrays.
        uint8_t matched = 0;
        for (uint8_t j = 0; j < MAX_X_PIN_GPIOS; j++) {
            if (HW->gpio_x1[j] == g) {
                result.x1_gpio = g;
                matched = 1;
                break;
            }
        }
        if (!matched) {
            for (uint8_t j = 0; j < MAX_X_PIN_GPIOS; j++) {
                if (HW->gpio_x2[j] == g) {
                    result.x2_gpio = g;
                    matched = 1;
                    break;
                }
            }
        }
        if (!matched) {
            ERR("v2_get_x_pin_gpios: unidentified GPIO %u in address span "
                "(slot_type=%d)", (unsigned)g, slot->slot_type);
        }
    }
 
    return result;
}

static void pio_setup_address_monitor_pios() {
    const onerom_rom_slot_t *slot = RUNTIME->current_rom_slot;

    // APIO_ASM_CONTINUE() rather than APIO_ASM_INIT(): this function extends
    // the PIO configuration built at boot rather than starting from scratch.
    // In emulation mode APIO_ASM_CONTINUE() is a no-op, preserving the
    // accumulated apio state so that epio_update_from_apio() can pick up the
    // new SM programs written here without disturbing already-running SMs.
    APIO_ASM_CONTINUE();

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
#if !defined(TEST_BUILD)
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
#else // TEST_BUILD
    LOG("DMA setup: ch=%d block=%d sm=%d ring_buf=%p ring_size_log2=%d data_size=%d",
        dma_ch, block, sm_addr_read, (void *)ring_buf, ring_size_log2, data_size);
#endif // !TEST_BUILD
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

// Maps a logical chip address (host-visible A_n values, chip0) to the SRAM
// table index where that address's data is stored.
//
// Word size and A-1:
//   word_size == 8:  logical_addr is a plain byte address. Every address
//                    line, including the least-significant, is a scattered
//                    GPIO in pin_map->addr[]. addr[n] holds A_n's GPIO.
//   word_size == 16: the ROM serves 16-bit words as two adjacent bytes in
//                    the table. The least-significant logical address bit
//                    is A-1 — it selects the low (A-1=0) or high (A-1=1)
//                    byte within a word and is NOT a scattered GPIO: it is
//                    simply bit 0 of the table index. pin_map->addr[] holds
//                    A0..A_n (the word-address lines) only. So the word
//                    address is (logical_addr >> 1), scattered through
//                    addr[], and A-1 (logical_addr & 1) is OR'd back in as
//                    the table-index LSB: index = (scatter(word) << 1) | A-1.
//
// Step 1: scatter logical address bits to their GPIO bit positions.
//   NORMAL:     table bit = logical bit.
//   INVERT:     table bit = ~logical bit.  The GPIO is inverted so the PIO
//               sees ~A_n; the pre-processor indexes the table by that
//               post-override value, so the table bit for A_n = b is ~b.
//   GPIO_OVER_LOW:  table bit = 0 regardless.
//   GPIO_OVER_HIGH: table bit = 1 regardless.
//
// Step 2: CS bits (Multi only).
//   For Multi, chip0's CS line(s) must be 1 in the table index (active-high
//   serving convention; serve_cs_low_0 == 1).  INVERT on a CS line normalises
//   the physical GPIO polarity to produce this state, so the target bit value
//   is always 1 for chip0 CS active and no INVERT check is needed here; only
//   GPIO_OVER_LOW prevents the target being met.
//   Single/Banked sets need no action: CS bits default to 0, which is the
//   correct active-low state for those types.
//
// Step 3: Banked X bits.
//   Currently targets bank 0 only (X bits remain 0).
//   TODO: add explicit bank selection when required.
uint32_t pio_map_addr_to_phys(
    const onerom_rom_slot_t *slot,
    uint32_t logical_addr
) {
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
    uint8_t  addr_base = addr_alg->gpio_base + addr_alg->base_addr_pin;
    uint32_t physical  = 0;

    // For 16-bit serving, A-1 is the least-significant logical address bit
    // and maps directly to the table-index LSB (selecting the low/high byte
    // of the adjacent-byte word pair), rather than to a scattered GPIO in
    // addr[].  Split it off here; addr[] is scattered over the word address
    // (logical_addr >> 1), and a_minus_1 is re-applied as bit 0 at the end.
    uint8_t  word_size = data_alg->word_size;
    uint32_t a_minus_1 = 0;
    uint32_t addr_bits = logical_addr;
    if (word_size == 16u) {
        a_minus_1 = logical_addr & 1u;
        addr_bits = logical_addr >> 1;
    }

    // Step 1: address bits.
    for (uint8_t n = 0; n < MAX_ADDR_PINS; n++) {
        uint8_t gpio = pin_map->addr[n];
        if (gpio >= GPIO_NONE) break;

        uint8_t bit_pos     = gpio - addr_base;
        uint8_t logical_bit = (uint8_t)((addr_bits >> n) & 1u);
        uint8_t override    = v2_get_gpio_override(slot, gpio);

        switch (override) {
            case GPIO_OVER_NORMAL:
                if (logical_bit) physical |= (1u << bit_pos);
                break;
            case GPIO_OVER_INVERT:
                if (!logical_bit) physical |= (1u << bit_pos);
                break;
            case GPIO_OVER_LOW:
                break;
            case GPIO_OVER_HIGH:
                physical |= (1u << bit_pos);
                break;
            default:
                break;
        }
    }

    // Step 2: CS bits (Multi only).
    if (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) {
        const onerom_alg_cs_config_t *cs_alg = slot->alg->alg_cs;

        switch (cs_alg->alg) {
            case ALG_CS_0: {
                const onerom_alg_cs0_param_t *cs_params =
                    (const onerom_alg_cs0_param_t *)cs_alg->params;
                uint8_t cs_base = cs_alg->gpio_base + cs_params->first_rom_cs_base;
                uint8_t cs_end  = cs_base + cs_params->first_rom_num_cs_pins;
                for (uint8_t g = cs_base; g < cs_end; g++) {
                    uint8_t bit_pos  = g - addr_base;
                    uint8_t override = v2_get_gpio_override(slot, g);
                    switch (override) {
                        case GPIO_OVER_NORMAL:
                        case GPIO_OVER_INVERT:
                        case GPIO_OVER_HIGH:
                            physical |= (1u << bit_pos);
                            break;
                        case GPIO_OVER_LOW:
                            ERR("pio_map_addr_to_phys: CS GPIO %u GPIO_OVER_LOW in "
                                "Multi slot; chip0 CS cannot be made active",
                                (unsigned)g);
                            break;
                        default:
                            break;
                    }
                }
                break;
            }
            case ALG_CS_1: {
                const onerom_alg_cs1_param_t *cs_params =
                    (const onerom_alg_cs1_param_t *)cs_alg->params;
                uint8_t cs_base = cs_alg->gpio_base + cs_alg->base_cs_pin;
                for (uint8_t i = 0; i < cs_alg->num_cs_pins; i++) {
                    if (i == cs_params->cs_ignore_index) continue;
                    uint8_t g        = cs_base + i;
                    uint8_t bit_pos  = g - addr_base;
                    uint8_t override = v2_get_gpio_override(slot, g);
                    switch (override) {
                        case GPIO_OVER_NORMAL:
                        case GPIO_OVER_INVERT:
                        case GPIO_OVER_HIGH:
                            physical |= (1u << bit_pos);
                            break;
                        case GPIO_OVER_LOW:
                            ERR("pio_map_addr_to_phys: CS GPIO %u GPIO_OVER_LOW in "
                                "Multi slot", (unsigned)g);
                            break;
                        default:
                            break;
                    }
                }
                break;
            }
            case ALG_CS_2:
            default:
                ERR("pio_map_addr_to_phys: unsupported CS algorithm %d for "
                    "Multi slot", cs_alg->alg);
                // TODO: implement AlgCs2 Multi support
                break;
        }
    }

    // Step 3: Banked X bits — target bank 0 only (X bits remain 0).
    // TODO: add explicit bank selection when required.

    // Step 4: 16-bit A-1 — the byte-within-word select is the table-index
    // LSB.  The scatter above produced the word-address index in the GPIO
    // bit positions; shift it up by one and OR in A-1 so that adjacent
    // logical bytes map to adjacent table offsets.
    if (word_size == 16u) {
        physical = (physical << 1) | a_minus_1;
    }

    return physical;
}

// Maps a logical data byte to the physical byte to store in the SRAM table.
// Scatters logical data bit d to bit position (data_pin_gpios[d] - data_base)
// applying the GPIO override at each position.
uint32_t pio_map_data_to_phys(
    const onerom_rom_slot_t *slot,
    uint32_t logical_data
) {
    const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
    uint8_t  data_base = data_alg->gpio_base + data_alg->base_data_pin;
    uint8_t  num_data  = data_alg->word_size;   /* 8 or 16 */
    uint32_t physical  = 0;
 
    for (uint8_t d = 0; d < num_data; d++) {
        uint8_t gpio = pin_map->data[d];
        if (gpio >= GPIO_NONE) continue;
 
        uint8_t bit_pos     = gpio - data_base;
        uint8_t logical_bit = (uint8_t)((logical_data >> d) & 1u);
        uint8_t override    = v2_get_gpio_override(slot, gpio);
 
        switch (override) {
            case GPIO_OVER_NORMAL:
                if (logical_bit) physical |= (1u << bit_pos);
                break;
            case GPIO_OVER_INVERT:
                if (!logical_bit) physical |= (1u << bit_pos);
                break;
            case GPIO_OVER_LOW:
                break;
            case GPIO_OVER_HIGH:
                physical |= (1u << bit_pos);
                break;
            default:
                break;
        }
    }
 
    return physical;
}

// Converts a ring buffer entry (post-override PIO-visible GPIO bitmap) to a
// logical address, with optional control pin activity checking.
//
// physical_addr is post-override: PIOs always read post-override values from
// IN PINS, so ring buffer entries already reflect PIO-visible values.
//
// Word size and A-1 (mirror of pio_map_addr_to_phys):
//   word_size == 16: the table index is (word_scatter << 1) | A-1, where
//                    A-1 (the byte-within-word select) is the LSB and is NOT
//                    a scattered GPIO in addr[].  This function pulls A-1 off
//                    bit 0, shifts the scattered word address down by one so
//                    the addr[] extraction sees the word index in its
//                    original GPIO bit positions, then re-applies A-1 as the
//                    logical LSB.  word_size == 8: physical_addr is used as-is.
//
// Control pin check (check_control_pins != 0):
//   AlgCs0, Single/Banked: verify all CS pins are in the active-low state (0).
//   AlgCs0, Multi:         verify chip0 CS sub-range is active-high (1), then
//                          verify X pins are inactive (0).
//   AlgCs1, Single/Banked: verify real CS pins (excluding cs_ignore_index)
//                          are in the active-low state (0).
//   AlgCs2:                not currently supported; returns ORA_RESULT_ERROR.
//                          TODO: implement when AlgCs2 address monitor support
//                          is added.
//
// Address extraction:
//   NORMAL:     logical bit = physical bit.
//   INVERT:     logical bit = ~physical bit (ring buffer has post-INVERT value;
//               invert to recover original chip pin value).
//   GPIO_OVER_LOW:  logical bit = 0 (forced; carries no information).
//   GPIO_OVER_HIGH: logical bit = 1.
ora_result_t pio_demangle_addr(
    const onerom_rom_slot_t *slot,
    uint32_t physical_addr,
    uint32_t *logical_addr_out,
    uint8_t check_control_pins
) {
    if (logical_addr_out == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
    const onerom_alg_cs_config_t   *cs_alg   = slot->alg->alg_cs;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
    uint8_t addr_base = addr_alg->gpio_base + addr_alg->base_addr_pin;

    // 16-bit serving: pio_map_addr_to_phys built the table index as
    // (word_scatter << 1) | A-1, where A-1 (the byte-within-word select) is
    // the least-significant bit and is NOT a scattered GPIO in addr[].
    // Reverse that here: pull A-1 off bit 0, then shift the scattered word
    // address down by one so the addr[] extraction below (and the control-
    // pin checks, which read the same shifted frame) see the word index in
    // its original GPIO bit positions.  A-1 is re-applied as the logical
    // LSB at the end.
    uint8_t  word_size = data_alg->word_size;
    uint32_t a_minus_1 = 0;
    if (word_size == 16u) {
        a_minus_1     = physical_addr & 1u;
        physical_addr = physical_addr >> 1;
    }

    if (check_control_pins) {
        switch (cs_alg->alg) {
            case ALG_CS_0: {
                const onerom_alg_cs0_param_t *cs_params =
                    (const onerom_alg_cs0_param_t *)cs_alg->params;
                uint8_t expected_active = cs_params->serve_cs_low_0;

                // For Multi, check only chip0's CS sub-range.  Checking the
                // full CS range would include X pins (which are 0 for a chip0
                // access) and return false positives against expected_active=1.
                uint8_t cs_check_base, cs_check_count;
                if (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) {
                    cs_check_base  = cs_alg->gpio_base
                                   + cs_params->first_rom_cs_base;
                    cs_check_count = cs_params->first_rom_num_cs_pins;
                } else {
                    cs_check_base  = cs_alg->gpio_base + cs_alg->base_cs_pin;
                    cs_check_count = cs_alg->num_cs_pins;
                }

                for (uint8_t i = 0; i < cs_check_count; i++) {
                    uint8_t g            = cs_check_base + i;
                    uint8_t bit_pos      = g - addr_base;
                    uint8_t physical_bit = (uint8_t)((physical_addr >> bit_pos)
                                                      & 1u);
                    uint8_t override     = v2_get_gpio_override(slot, g);

                    switch (override) {
                        case GPIO_OVER_NORMAL:
                        case GPIO_OVER_INVERT:
                            // physical_addr is post-override; compare directly.
                            if (physical_bit != expected_active) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;
                        case GPIO_OVER_LOW:
                            if (expected_active != 0u) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;
                        case GPIO_OVER_HIGH:
                            if (expected_active != 1u) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;
                        default:
                            break;
                    }
                }

                // Multi: reject if any X pin is active (=1), indicating a
                // secondary chip is being addressed.
                if (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) {
                    v2_x_pin_gpios_t xp = v2_get_x_pin_gpios(slot, addr_base);
                    uint8_t x_gpios[2]  = { xp.x1_gpio, xp.x2_gpio };
                    for (uint8_t xi = 0; xi < 2; xi++) {
                        if (x_gpios[xi] >= GPIO_NONE) continue;
                        uint8_t bit_pos = x_gpios[xi] - addr_base;
                        if ((uint8_t)((physical_addr >> bit_pos) & 1u) != 0u) {
                            return ORA_RESULT_CONTROL_PIN_ACTIVE;
                        }
                    }
                }
                break;
            }

            case ALG_CS_1: {
                // AlgCs1 is Single/Banked only; active-low (expected = 0).
                const onerom_alg_cs1_param_t *cs_params =
                    (const onerom_alg_cs1_param_t *)cs_alg->params;
                uint8_t cs_base = cs_alg->gpio_base + cs_alg->base_cs_pin;

                for (uint8_t i = 0; i < cs_alg->num_cs_pins; i++) {
                    if (i == cs_params->cs_ignore_index) continue;
                    uint8_t g            = cs_base + i;
                    uint8_t bit_pos      = g - addr_base;
                    uint8_t physical_bit = (uint8_t)((physical_addr >> bit_pos)
                                                      & 1u);
                    uint8_t override     = v2_get_gpio_override(slot, g);

                    switch (override) {
                        case GPIO_OVER_NORMAL:
                        case GPIO_OVER_INVERT:
                            if (physical_bit != 0u) {
                                return ORA_RESULT_CONTROL_PIN_ACTIVE;
                            }
                            break;
                        case GPIO_OVER_LOW:
                            /* forced to 0 = active; always passes */
                            break;
                        case GPIO_OVER_HIGH:
                            /* forced to 1 = inactive; always fails */
                            return ORA_RESULT_CONTROL_PIN_ACTIVE;
                        default:
                            break;
                    }
                }
                break;
            }

            case ALG_CS_2:
            default:
                ERR("pio_demangle_addr: unsupported CS algorithm %d",
                    cs_alg->alg);
                return ORA_RESULT_ERROR;
                // TODO: implement AlgCs2 support
        }
    }

    // Address extraction.
    uint32_t logical = 0;
    for (uint8_t n = 0; n < MAX_ADDR_PINS; n++) {
        uint8_t gpio = pin_map->addr[n];
        if (gpio >= GPIO_NONE) break;

        uint8_t bit_pos      = gpio - addr_base;
        uint8_t physical_bit = (uint8_t)((physical_addr >> bit_pos) & 1u);
        uint8_t override     = v2_get_gpio_override(slot, gpio);

        switch (override) {
            case GPIO_OVER_NORMAL:
                if (physical_bit) logical |= (1u << n);
                break;
            case GPIO_OVER_INVERT:
                // Invert to recover original chip pin value from post-override
                // ring buffer entry.
                if (!physical_bit) logical |= (1u << n);
                break;
            case GPIO_OVER_LOW:
                /* always 0; contributes 0 */
                break;
            case GPIO_OVER_HIGH:
                logical |= (1u << n);
                break;
            default:
                break;
        }
    }

    // 16-bit: re-apply A-1 as the logical LSB, shifting the recovered word
    // address up by one (inverse of the split at the top of this function).
    if (word_size == 16u) {
        logical = (logical << 1) | a_minus_1;
    }

    *logical_addr_out = logical;
    return ORA_RESULT_OK;
}
 
// ---------------------------------------------------------------------------
// pio_demangle_data
// ---------------------------------------------------------------------------
 
// Reverses the data pin permutation: extracts the logical byte from a
// physically stored (mangled) SRAM table byte.
uint8_t pio_demangle_data(
    const onerom_rom_slot_t *slot,
    uint8_t physical_data
) {
    const onerom_alg_data_config_t *data_alg = slot->alg->alg_data;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
    uint8_t data_base = data_alg->gpio_base + data_alg->base_data_pin;
    uint8_t num_data  = data_alg->word_size;
    uint8_t logical   = 0;
 
    for (uint8_t d = 0; d < num_data; d++) {
        uint8_t gpio = pin_map->data[d];
        if (gpio >= GPIO_NONE) continue;
 
        uint8_t bit_pos      = gpio - data_base;
        uint8_t physical_bit = (physical_data >> bit_pos) & 1u;
        uint8_t override     = v2_get_gpio_override(slot, gpio);
 
        switch (override) {
            case GPIO_OVER_NORMAL:
                if (physical_bit) logical |= (1u << d);
                break;
            case GPIO_OVER_INVERT:
                if (!physical_bit) logical |= (1u << d);
                break;
            case GPIO_OVER_LOW:
                /* always 0; contributes 0 */
                break;
            case GPIO_OVER_HIGH:
                logical |= (1u << d);
                break;
            default:
                break;
        }
    }
 
    return logical;
}


// Precomputes mask and match values for knock detection against ring buffer
// entries (post-override PIO-visible values).
//
// Mask: bit positions of knock_bits address GPIOs.  Forced GPIOs are excluded
// (they carry no information).
//
// Match values: ring buffer entries are post-override, so INVERT bits appear
// flipped relative to logical knock sequence values.
//
// CS mask: bit positions of real CS pins within the address span, for
// debounce filtering.
//
// X mask (Multi only): bit positions of X pins within the address span.
//
// AlgCs2 is not currently supported; returns ORA_RESULT_ERROR.
// TODO: implement AlgCs2 support.
ora_result_t pio_init_knock(
    const uint32_t *knock_seq,
    uint8_t         knock_len,
    uint8_t         knock_bits,
    uint8_t         data_size,
    ora_knock_t    *knock
) {
    if (knock_seq == NULL || knock == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }
    if (knock_len == 0u || knock_bits == 0u || knock_bits > MAX_ADDR_PINS) {
        return ORA_RESULT_INVALID_ARG;
    }
 
    const onerom_rom_slot_t        *slot     = CURRENT_SLOT;
    const onerom_alg_addr_config_t *addr_alg = slot->alg->alg_addr;
    const onerom_alg_cs_config_t   *cs_alg   = slot->alg->alg_cs;
    const onerom_rom_pin_map_t     *pin_map   = slot->roms[0]->pin_map;
    uint8_t addr_base = addr_alg->gpio_base + addr_alg->base_addr_pin;
 
    // Mask.
    knock->mask = 0;
    for (uint8_t i = 0; i < knock_bits; i++) {
        uint8_t gpio = pin_map->addr[i];
        if (gpio >= GPIO_NONE) {
            return ORA_RESULT_INTERNAL_ERROR;
        }
        uint8_t override = v2_get_gpio_override(slot, gpio);
        switch (override) {
            case GPIO_OVER_NORMAL:
            case GPIO_OVER_INVERT:
                knock->mask |= (1u << (gpio - addr_base));
                break;
            case GPIO_OVER_LOW:
            case GPIO_OVER_HIGH:
                /* exclude forced bits from mask */
                break;
            default:
                break;
        }
    }
 
    // Match values.
    for (uint8_t k = 0; k < knock_len; k++) {
        knock->matches[k] = 0;
        for (uint8_t i = 0; i < knock_bits; i++) {
            uint8_t gpio = pin_map->addr[i];
            if (gpio >= GPIO_NONE) continue;
 
            uint8_t bit_pos     = gpio - addr_base;
            uint8_t override    = v2_get_gpio_override(slot, gpio);
            uint8_t logical_bit = (uint8_t)((knock_seq[k] >> i) & 1u);
 
            switch (override) {
                case GPIO_OVER_NORMAL:
                    if (logical_bit) knock->matches[k] |= (1u << bit_pos);
                    break;
                case GPIO_OVER_INVERT:
                    // Ring buffer has ~logical for inverted GPIOs.
                    if (!logical_bit) knock->matches[k] |= (1u << bit_pos);
                    break;
                case GPIO_OVER_LOW:
                case GPIO_OVER_HIGH:
                    /* excluded from mask; excluded from match */
                    break;
                default:
                    break;
            }
        }
    }
 
    // CS mask.
    uint32_t cs_mask = 0;
    switch (cs_alg->alg) {
        case ALG_CS_0: {
            uint8_t cs_base = cs_alg->gpio_base + cs_alg->base_cs_pin;
            for (uint8_t i = 0; i < cs_alg->num_cs_pins; i++) {
                uint8_t g = cs_base + i;
                if (g >= addr_base &&
                    g < (uint8_t)(addr_base + addr_alg->num_addr_pins)) {
                    cs_mask |= (1u << (g - addr_base));
                }
            }
            break;
        }
        case ALG_CS_1: {
            const onerom_alg_cs1_param_t *cs_params =
                (const onerom_alg_cs1_param_t *)cs_alg->params;
            uint8_t cs_base = cs_alg->gpio_base + cs_alg->base_cs_pin;
            for (uint8_t i = 0; i < cs_alg->num_cs_pins; i++) {
                if (i == cs_params->cs_ignore_index) continue;
                uint8_t g = cs_base + i;
                if (g >= addr_base &&
                    g < (uint8_t)(addr_base + addr_alg->num_addr_pins)) {
                    cs_mask |= (1u << (g - addr_base));
                }
            }
            break;
        }
        case ALG_CS_2:
        default:
            ERR("pio_init_knock: unsupported CS algorithm %d", cs_alg->alg);
            return ORA_RESULT_ERROR;
            // TODO: implement AlgCs2 support
    }
 
    // X mask (Multi only).
    uint32_t x_mask = 0;
    if (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM) {
        v2_x_pin_gpios_t xp         = v2_get_x_pin_gpios(slot, addr_base);
        uint8_t          x_gpios[2] = { xp.x1_gpio, xp.x2_gpio };
        for (uint8_t xi = 0; xi < 2; xi++) {
            if (x_gpios[xi] >= GPIO_NONE) continue;
            if (x_gpios[xi] >= addr_base &&
                x_gpios[xi] < (uint8_t)(addr_base + addr_alg->num_addr_pins)) {
                x_mask |= (1u << (x_gpios[xi] - addr_base));
            }
        }
    }
 
    knock->len            = knock_len;
    knock->bits           = knock_bits;
    knock->data_size      = data_size;
    knock->multi_rom_mode = (slot->slot_type == ROM_SLOT_TYPE_MULTI_ROM)
                            ? 1u : 0u;
    knock->cs_mask        = cs_mask;
    knock->x_mask         = x_mask;
 
    return ORA_RESULT_OK;
}

#if !REAL_HARDWARE
// Pointer to an externally-provided SRAM buffer, set by set_host_sram_ptr().
// When non-NULL, sram_to_host() returns into this buffer instead of the
// firmware's own allocation, allowing fw-emulator to unify the two backing
// stores so that all firmware SRAM writes are immediately visible to epio.
static uint8_t *s_host_sram_ptr = NULL;

void set_host_sram_ptr(uint8_t *ptr) {
    s_host_sram_ptr = ptr;
}

uint8_t *sram_to_host(uint32_t addr) {
    if (s_host_sram_ptr != NULL) {
        return s_host_sram_ptr + (addr - SRAM_BASE);
    }
    return (uint8_t *)get_ram_rom_image_table_aligned() + (addr - SRAM_BASE);
}
#endif

// Reads a contiguous logical region of a RAM slot into buf, reversing the
// address and data mappings.  Counterpart to pio_reprogram_ram_rom_slot.
//
// rom_slot provides the pin map and algorithm config for the mappings.
// ram_slot is the RAM slot index to read from.
//
// Reads are always permitted from the active slot; no allow_active flag is
// needed (the caller is responsible for any consistency requirements).
ora_result_t pio_read_ram_rom_slot(
    const onerom_rom_slot_t *rom_slot,
    uint8_t   ram_slot,
    uint32_t  offset,
    uint8_t  *buf,
    uint32_t  len
) {
    if (buf == NULL || len == 0u) {
        return ORA_RESULT_INVALID_ARG;
    }
 
    uint32_t     addr, size;
    ora_result_t result = ora_get_ram_slot_info(ram_slot, &addr, &size, NULL);
    if (result != ORA_RESULT_OK) {
        return result;
    }
 
    if (offset + len > size) {
        return ORA_RESULT_INVALID_ARG;
    }
 
#if REAL_HARDWARE
    const uint8_t *sram = (const uint8_t *)(uintptr_t)addr;
#else
    const uint8_t *sram = sram_to_host(addr);
#endif
    for (uint32_t i = 0u; i < len; i++) {
        uint32_t physical_offset = pio_map_addr_to_phys(rom_slot, offset + i);
        buf[i] = pio_demangle_data(rom_slot, sram[physical_offset]);
    }
 
    return ORA_RESULT_OK;
}

#if !defined(TEST_BUILD)
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
#endif // !TEST_BUILD

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
#if !defined(TEST_BUILD)
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
#else // TEST_BUILD
    (void)knock;
    (void)ring_buf;
    (void)ring_entries_log2;
    (void)flags;
    (void)payload_out;
    (void)payload_len;
    (void)start_pos;
    (void)next_read_out;
    return ORA_RESULT_OK;
#endif // !TEST_BUILD
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
#if REAL_HARDWARE
    uint8_t *sram = (uint8_t *)(uintptr_t)addr;
#else
    uint8_t *sram = sram_to_host(addr);
#endif
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

// Returns the number of bits used to index the ROM table — i.e. the low bits
// of the 32-bit PIO shift-register output that form the SRAM offset.
//
// This equals alg_addr->num_rom_table_bits, which already accounts for the
// extra bit used in 16-bit word mode (num_rom_table_bits = num_addr_pins + 1
// for ALG_DATA_1 / 16-bit, num_addr_pins for 8-bit).  The v1 equivalent was
// NUM_ADDR_PINS + (BIT_MODE == BIT_MODE_16 ? 1 : 0).
uint8_t pio_get_effective_addr_pins(void) {
    return CURRENT_SLOT->alg->alg_addr->num_rom_table_bits;
}
 
// Returns the number of bytes in one ROM table region, i.e. the number of
// table entries addressable by the PIO address SM in a single SRAM window.
uint32_t pio_get_rom_region_size(void) {
    return 1u << pio_get_effective_addr_pins();
}

// Atomically switches the SRAM region being served by updating the X register
// in the address-read SM with the high bits of new_region_addr.
//
// Input validation is the caller's responsibility.  ora_set_active_ram_slot
// validates the slot index and derives a correct address via
// ora_get_ram_slot_info before calling this function.
ora_result_t pio_switch_rom_region(uint32_t new_region_addr) {
    uint8_t  effective_addr_pins    = pio_get_effective_addr_pins();
    uint8_t  rom_table_prefix_bits  = 32u - effective_addr_pins;
    uint32_t high_bits_mask         = (1u << rom_table_prefix_bits) - 1u;
    uint32_t rom_table_high_bits    = (new_region_addr >> effective_addr_pins)
                                      & high_bits_mask;
 
    // Keep RUNTIME consistent with the switch.
    RUNTIME->rom_table = (void *)(uintptr_t)new_region_addr;
 
    // Update the X register in the address-read SM with the new SRAM region
    // base.  This delays the SM by a single cycle but is an atomic switch.
    //
    // APIO_ASM_CONTINUE() rather than APIO_ASM_INIT(): this function modifies
    // a live SM via APIO_SM_EXEC_INSTR without touching any PIO program
    // memory.  In emulation mode APIO_ASM_CONTINUE() is a no-op, preserving
    // the accumulated apio state so that epio_update_from_apio() sees only the
    // two pre_instrs added here and can apply them to the live epio SM without
    // disturbing any other SM state.
    APIO_ASM_CONTINUE();
    APIO_SET_BLOCK(BLOCK_ADDR);
    APIO_SET_SM(SM_ADDR_READ);
    APIO_TXF = rom_table_high_bits;
    APIO_SM_EXEC_INSTR(APIO_PULL_BLOCK);
    APIO_SM_EXEC_INSTR(APIO_MOV_X_OSR);
 
    return ORA_RESULT_OK;
}

uint8_t pio_get_active_ram_slot(void) {
    return RUNTIME->current_ram_slot;
}