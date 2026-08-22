// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(FLASH_ERASE_H)
#define FLASH_ERASE_H

#include <stdint.h>

typedef void (*flash_exit_xip_fn_t)(void);
typedef void (*flash_range_erase_fn_t)(uint32_t offs, uint32_t count, uint32_t block_size, uint8_t block_cmd);
typedef void (*flash_flush_cache_fn_t)(void);
typedef void (*flash_select_xip_read_mode_fn_t)(uint8_t mode, uint8_t clkdiv);
typedef void (*flash_range_program_fn_t)(uint32_t offs, const uint8_t *data, uint32_t count);
typedef void (*connect_internal_flash_fn_t)(void);

// Mask this core's interrupts, and put them back the way they were.
//
// PRIMASK is saved and restored rather than enabled outright, so a masked
// region may sit inside another one.
//
// A host build has no cpsid/cpsie, and masking is the one part of the commit
// with no host equivalent - so rather than compiling it away, it goes to the
// harness, which counts the depth and refuses a flash call that arrives
// outside a masked region.  Without that the interrupt discipline would be the
// only part of the sequence nothing checks.
#if defined(ORA_HOST_TEST)
uint32_t ora_host_test_irq_disable(void);
void ora_host_test_irq_restore(uint32_t primask);
static inline uint32_t flash_irq_disable(void) {
    return ora_host_test_irq_disable();
}
static inline void flash_irq_restore(uint32_t primask) {
    ora_host_test_irq_restore(primask);
}
#else
static inline uint32_t flash_irq_disable(void) {
    uint32_t primask;
    __asm volatile ("mrs %0, primask \n\t"
                    "cpsid i"
                    : "=r" (primask) :: "memory");
    return primask;
}
static inline void flash_irq_restore(uint32_t primask) {
    __asm volatile ("msr primask, %0" :: "r" (primask) : "memory");
}
#endif

typedef void (*nv_flash_erase_critical_fn_t)(
    flash_exit_xip_fn_t             exit_xip,
    flash_range_erase_fn_t          range_erase,
    flash_range_program_fn_t        range_program,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    uint32_t                        flash_offs,
    const uint8_t                  *data,
    uint32_t                        size,
    uint8_t                         clkdiv
);

void flash_erase_critical(
    flash_exit_xip_fn_t             exit_xip,
    flash_range_erase_fn_t          range_erase,
    flash_range_program_fn_t        range_program,
    flash_flush_cache_fn_t          flush_cache,
    flash_select_xip_read_mode_fn_t select_xip,
    uint32_t                        flash_offs,
    const uint8_t                  *data,
    uint32_t                        size,
    uint8_t                         clkdiv
);

#endif // FLASH_ERASE_H
