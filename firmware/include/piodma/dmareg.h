// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// RP2350 DMA register definitions

#ifndef DMAREG_H
#define DMAREG_H

#include <stdint.h>

// DMA
#define DMA_BASE               (0x50000000U)

// DMA register offsets
#define DMA_READ_ADDR_OFFSET        (0x00)
#define DMA_WRITE_ADDR_OFFSET       (0x04)
#define DMA_TRANS_COUNT_OFFSET      (0x08)
#define DMA_CTRL_TRIG_OFFSET        (0x0C)
#define DMA_WRITE_ADDR_TRIG_OFFSET  (0x2C)
#define DMA_READ_ADDR_TRIG_OFFSET   (0x3C)

// DMA channel register structure
typedef struct dma_ch_reg {
    uint32_t read_addr;
    uint32_t write_addr;
    uint32_t transfer_count;
    uint32_t ctrl_trig;
} dma_ch_reg_t;

// Macro to access a DMA channel's registers
#define DMA_CH_REG(X)    ((volatile dma_ch_reg_t *)(DMA_BASE + ((X) * 0x40)))

#define DMA_CTRL_TRIG_EN                (1 << 0)
#define DMA_CTRL_TRIG_PRIORITY_HIGH     (1 << 1)
#define DMA_CTRL_TRIG_DATA_SIZE_8BIT    (0 << 2)
#define DMA_CTRL_TRIG_DATA_SIZE_16BIT   (1 << 2)
#define DMA_CTRL_TRIG_DATA_SIZE_32BIT   (2 << 2)
#define DMA_CTRL_INCR_READ              (1 << 4)
#define DMA_CTRL_INCR_WRITE             (1 << 6)
#define DMA_CTRL_TRIG_CHAIN_TO(X)       (((X) & 0xF) << 13)
#define DMA_CTRL_TRIG_TREQ_SEL(X)       (((X) & 0x3F) << 17)
#define DMA_CTRL_TRIG_TREQ_PERM_BITS    0x3f
#define DMA_CTRL_TRIG_TREQ_PERM         DMA_CTRL_TRIG_TREQ_SEL(DMA_CTRL_TRIG_TREQ_PERM_BITS)
#define DMA_CTRL_TRIG_IRQ_QUIET         (1 << 23)

// Macro to access DMA channel X's READ_ADDR register
#define DMA_CH_READ_ADDR(X)    (*(volatile uint32_t *)(DMA_BASE + ((X) * 0x40) + DMA_READ_ADDR_OFFSET))

// Macro to access DMA channel X's READ_ADDR_TRIG register
#define DMA_CH_READ_ADDR_TRIG(X)    (*(volatile uint32_t *)(DMA_BASE + ((X) * 0x40) + DMA_READ_ADDR_TRIG_OFFSET))

// Macro to access DMA channel X's WRITE_ADDR_TRIG register
#define DMA_CH_WRITE_ADDR_TRIG(X)   (*(volatile uint32_t *)(DMA_BASE + ((X) * 0x40) + DMA_WRITE_ADDR_TRIG_OFFSET))

#define DMA_CTRL_RING_SIZE(X)    (((X) & 0xF) << 8)
#define DMA_CTRL_RING_SEL        (1 << 12)

// Macro to enable the DMA controller
#if !TEST_BUILD
#define DMA_ENABLE()    RESET_RESET &= ~RESET_DMA;        \
                        while (!(RESET_DONE & RESET_DMA));
#else // TEST_BUILD
#define DMA_ENABLE()
#endif // !TEST_BUILD

#endif // DMAREG_H