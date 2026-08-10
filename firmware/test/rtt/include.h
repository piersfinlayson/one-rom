// Minimal stand-in for firmware/include/include.h, so firmware/src/rtt.c can
// be compiled and exercised on the host.
//
// The control block layout is no longer duplicated here: rtt.h is One ROM's
// own header now, so the test includes the real one and the static asserts in
// it are checked on the host too.

#if !defined(ONEROM_RTT_TEST_INCLUDE_H)
#define ONEROM_RTT_TEST_INCLUDE_H

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#define REAL_HARDWARE 1

// The host cannot execute a Cortex-M dmb.  A compiler barrier preserves the
// ordering the test cares about (data stores before the write_offset store).
#define ONEROM_RTT_DMB() __atomic_signal_fence(__ATOMIC_SEQ_CST)

// Nor can it mask interrupts.  The test is single threaded, so the critical
// section is uncontended and these can be empty; what they must not do is
// change the ring logic under test.
#define ONEROM_RTT_LOCK 1
static inline uint32_t onerom_rtt_lock(void) { return 0u; }
static inline void onerom_rtt_unlock(uint32_t primask) { (void)primask; }

#include "rtt.h"

#endif // ONEROM_RTT_TEST_INCLUDE_H
