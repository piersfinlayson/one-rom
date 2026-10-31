// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef PIODMA_H
#define PIODMA_H

#if defined(DEBUG_LOGGING)
#define APIO_LOG_ENABLE DEBUG
#endif // DEBUG_LOGGING

#include <apio.h>
#include "piodma/dmareg.h"

// Address and the CS/Data PIOs are in different blocks, as they may need
// different GPIO_BASE settings.  All other PIO state machines and instructions
// are allocated with ora_claim_pio().
#define BLOCK_ADDR                  1
#define BLOCK_CS_DATA               2
#define SM_ADDR_READ                0
#define SM_DATA_OUTPUT              0
#define SM_DATA_WRITE               1

// IRQs

// Used by the RBCP address monitor PIOs to signal CS going active
#define ADDR_MONITOR_IRQ            0 

// DMA channels

// Used for address read/data write (PIO ROM serving)
#define DMA_CH_ADDR_READ            0
#define DMA_CH_DATA_WRITE           1

// Used for address monitor to send addresses to CPU, for RBCP
#define DMA_CH_ADDR_MONITOR         2

// Used for copying ROM images to RAM
#define DMA_COPY_CHANNEL            15

// A PIO block's bits in runtime info's pio_block_claims.  Each block has 4
// state machine bits and 1 GPIOBASE bit.
#define PIO_CLAIM_SMS_SHIFT(BLOCK)    ((BLOCK) * PIO_CLAIM_BLOCK_1_SMS_SHIFT)
#define PIO_CLAIM_GPIOBASE_16(BLOCK)  (PIO_CLAIM_BLOCK_0_GPIOBASE_16 << (BLOCK))

// A block's claimed state machines, bit N for state machine N.
static inline uint32_t pio_block_claimed_sms(uint32_t block) {
    return (RUNTIME->pio_block_claims >> PIO_CLAIM_SMS_SHIFT(block)) & PIO_CLAIM_BLOCK_0_SMS;
}

// Whether anything in a block is claimed, which fixes the block's GPIOBASE.
static inline uint8_t pio_block_in_use(uint32_t block) {
    return (pio_block_claimed_sms(block) != 0u) || (RUNTIME->pio_instr_claims[block] != 0u);
}

// The GPIOBASE recorded for a block.  Only valid for a block in use.
static inline ora_pio_gpiobase_t pio_block_gpiobase(uint32_t block) {
    return (RUNTIME->pio_block_claims & PIO_CLAIM_GPIOBASE_16(block))
        ? ORA_PIO_GPIOBASE_16 : ORA_PIO_GPIOBASE_0;
}

// The bits for num_instrs instructions starting at first, in a PIO instruction
// claim.  num_instrs is 1 to 32.
static inline uint32_t pio_instr_run(uint32_t first, uint32_t num_instrs) {
    return ((num_instrs >= 32u) ? 0xFFFFFFFFu : ((1u << num_instrs) - 1u)) << first;
}

// The DMA channels the firmware uses.  Bit N is channel N.  SRAM emulation
// (pioram.c) uses channels 0 to 3 (0xF).
#define DMA_FIRMWARE_CHANNELS       (0x000Fu | (1u << DMA_COPY_CHANNEL))

#endif // PIODMA_H