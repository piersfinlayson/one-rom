// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Stand-in for rtt.h in the host test and emulator build.
//
// The real ring and formatter are not built for the host - src/rtt.c is
// REAL_HARDWARE only - so logging goes to stdout instead, and the control
// block is a placeholder that exists only to satisfy globals.c's pointer to
// it.
//
// Note this is not the same thing as firmware/test/rtt/, which builds the real
// src/rtt.c for the host to test it directly.

#if !defined(ONEROM_TEST_RTT_H)
#define ONEROM_TEST_RTT_H

#include <stdint.h>

// Logging macros go to printf and vprintf.
#define onerom_rtt_printf(channel, ...)         printf(__VA_ARGS__)
#define onerom_rtt_vprintf(channel, fmt, args)  vprintf(fmt, *args)

#define ONEROM_RTT_CH_BOOT  0u

#define do_log STUB_LOG

// Placeholder control block, so the test build compiles and runs.
typedef struct {
    uint32_t dummy;
} onerom_rtt_cb_t;
extern onerom_rtt_cb_t _SEGGER_RTT;

#endif // ONEROM_TEST_RTT_H
