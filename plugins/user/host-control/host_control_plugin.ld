/* 
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 */

INCLUDE onerom_linker_constants_generated.ld

USER_PLUGIN_BASE = ORA_FLASH_CS0_BASE_ADDR + ORA_USER_PLUGIN_OFFSET;
TOP_OF_SRAM = 0x20082000;

USER_PLUGIN_DATA_BASE = 0x20081C00;
USER_PLUGIN_DATA_LIMIT = 0x20081E00;  /* Arbitrary - up to plugin to avoid blowing its own stack */
USER_PLUGIN_STACK_TOP = 0x20082000;   /* 1KB shared between stack and static RAM */

PLUGIN_BASE = USER_PLUGIN_BASE;
DATA_BASE = USER_PLUGIN_DATA_BASE;
DATA_LIMIT = USER_PLUGIN_DATA_LIMIT;
STACK_TOP = USER_PLUGIN_STACK_TOP;

SECTIONS
{
    . = PLUGIN_BASE;

    .plugin_header : {
        KEEP(*(.plugin_header))
        . = ALIGN(4);
    }

    ASSERT(SIZEOF(.plugin_header) > 0, "ERROR: .plugin_header section is empty")

    .text : {
        *(.text*)
        *(.rodata*)
        . = ALIGN(4);
        __flash_erase_fn_start = .;
        KEEP(*(.flash_erase_fn*))
        . = ALIGN(4);
        __flash_erase_fn_end = .;
        __text_end = .;
    }

    /* 0xFF from the end of the plugin to the end of the user plugin region,
     * so programming the plugin leaves its NV storage erased.  The linker
     * doesn't output a section that only holds fill so the last byte is
     * written explicitly. */
    .padding (__text_end + SIZEOF(.ramfunc) + SIZEOF(.data)) : {
        FILL(0xFF);
        . = ABSOLUTE(USER_PLUGIN_BASE + ORA_USER_PLUGIN_SIZE - 1);
        BYTE(0xFF)
    }

    .ring_buf DATA_BASE (NOLOAD) : {
        *(.ring_buf)
        . = ALIGN(4);
    }

    .ramfunc : AT(__text_end) {
        __ramfunc_start = .;
        *(.ramfunc*)
        . = ALIGN(4);
        __ramfunc_end = .;
    }
    __ramfunc_load = LOADADDR(.ramfunc);

    .data (__ramfunc_end) : AT(__text_end + SIZEOF(.ramfunc)) {
        __data_start = .;
        *(.data*)
        . = ALIGN(4);
        __data_end = .;
    }
    __data_load = LOADADDR(.data);

    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss*)
        . = ALIGN(4);
        __bss_end = .;
    }
    ASSERT(. <= DATA_LIMIT, "ERROR: .ramfunc + .data + .bss exceed DATA_LIMIT")

    /DISCARD/ : {
        *(.ARM.exidx*)
        *(.ARM.extab*)
    }
}

__nv_storage_start = USER_PLUGIN_BASE + ORA_USER_PLUGIN_SIZE - 4K;
ASSERT(__text_end + SIZEOF(.ramfunc) + SIZEOF(.data) <= __nv_storage_start,
       "ERROR: plugin code/data overlaps NV storage region")
ASSERT(__flash_erase_fn_end - __flash_erase_fn_start > 0, "ERROR: flash erase code is empty")

/* NV_ERASE_FN_MAX comes from the Makefile, which gives the same value to the
 * compiler to size the buffer the routine is copied into. */
ASSERT(__flash_erase_fn_end - __flash_erase_fn_start <= NV_ERASE_FN_MAX,
    "ERROR: flash erase code exceeds NV_ERASE_FN_MAX")

/* PLUGIN_STATIC_MAX comes from the Makefile.  Everything static the plugin has
 * sits between DATA_BASE and __bss_end, and the rest of the window is stack. */
ASSERT(__bss_end - DATA_BASE <= PLUGIN_STATIC_MAX,
    "ERROR: static data exceeds PLUGIN_STATIC_MAX, leaving less stack")