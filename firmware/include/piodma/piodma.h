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
// different GPIO_BASE settings
#define BLOCK_MONITOR               0
#define BLOCK_ADDR                  1
#define BLOCK_CS_DATA               2
#define SM_ADDR_READ                0
#define SM_ADDR_MONITOR_ADDR_READ   1
#define SM_DATA_OUTPUT              0
#define SM_DATA_WRITE               1
#define SM_ADDR_MONITOR_CS_MONITOR  2

// IRQs

// Used by the RBCP address monitor PIOs to signal CS going active
#define ADDR_MONITOR_IRQ            0 

// DMA channels

// Used for address read/data write (PIO ROM serving)
#define DMA_CH_ADDR_READ            0
#define DMA_CH_DATA_WRITE           1

// Used for address monitor to send addresses to CPU, for RBCP
#define DMA_CH_ADDR_MONITOR         2

#endif // PIODMA_H