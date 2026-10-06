// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 Shared PIO routines

#include "include.h"

#if defined(TEST_BUILD)
#define TEST_PIO_C
#else
#define APIO_LOG_IMPL  1
#endif // TEST_BUILD

#include "piodma/piodma.h"

#if REAL_HARDWARE
// The RP2350's atomic set and clear aliases, as offsets from a register.
#define REG_ALIAS_SET   0x2000u
#define REG_ALIAS_CLR   0x3000u

static volatile uint32_t *pio_ctrl(uint8_t block, uint32_t alias) {
    uint32_t base = (block == 0) ? APIO0_BASE : ((block == 1) ? APIO1_BASE : APIO2_BASE);
    return (volatile uint32_t *)(uintptr_t)(base + APIO_CTRL_OFFSET + alias);
}
#endif // REAL_HARDWARE

void pio_enable_sms(uint8_t block, uint8_t sms) {
#if REAL_HARDWARE
    *pio_ctrl(block, REG_ALIAS_SET) = APIO_CTRL_SM_ENABLE(sms);
#else // !REAL_HARDWARE
    _apio_emulated_pio.enabled_sms[block] |= sms;
#endif // REAL_HARDWARE
}

void pio_disable_sms(uint8_t block, uint8_t sms) {
#if REAL_HARDWARE
    *pio_ctrl(block, REG_ALIAS_CLR) = APIO_CTRL_SM_ENABLE(sms);
#else // !REAL_HARDWARE
    _apio_emulated_pio.enabled_sms[block] &= (uint8_t)~sms;
#endif // REAL_HARDWARE
}

void pio_sm_exec(uint8_t block, uint8_t sm, uint16_t instr) {
#if REAL_HARDWARE
    _apio_sm_reg_ptr(block, sm)->instr = instr;
#else // !REAL_HARDWARE
    // Record the instruction as APIO_SM_EXEC_INSTR does, without moving the
    // assembler to this state machine.  epio_update_from_apio() runs it.
    uint8_t *count = &_apio_emulated_pio.pre_instr_count[block][sm];
    _apio_emulated_pio.pre_instr[block][sm][(*count)++] = instr;
#endif // REAL_HARDWARE
}

int pio(void) {
    int rc;

    if (0) {
        DEBUG("PIO RAM Mode");
        uint32_t rom_table_addr = (uint32_t)(uintptr_t)RUNTIME->rom_table;
        rc = pioram(INFO, RUNTIME, rom_table_addr);
    } else {
        DEBUG("PIO ROM Mode");
        rc = piorom2();
    }

    return rc;
}

