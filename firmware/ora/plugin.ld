/* 
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 */

/*
 * One ROM plugin linker script
 *
 * Links a plugin binary as a flat, position-dependent image at a base address
 * supplied by the build system via --defsym PLUGIN_BASE=<address>.
 *
 * Use 0x10010000 for system plugins and 0x10020000 for user plugins.
 *
 * The plugin header must be defined in the plugin source using
 * ORA_DEFINE_SYSTEM_PLUGIN() or ORA_DEFINE_USER_PLUGIN() from ora/plugin.h.
 */

SYSTEM_PLUGIN_TYPE = 0;
USER_PLUGIN_TYPE = 1;
PIO_PLUGIN_TYPE = 2;

SYSTEM_PLUGIN_BASE = 0x10010000;
USER_PLUGIN_BASE = 0x10020000;
PIO_PLUGIN_BASE = 0x10030000;

TOP_OF_SRAM = 0x20082000;

SYSTEM_PLUGIN_DATA_BASE = 0x20081000;
SYSTEM_PLUGIN_DATA_LIMIT = 0x20081800;
SYSTEM_PLUGIN_STACK_TOP = 0x20081C00; /* 1KB stack, grows downwards and may collide with data after 1KB */
USER_PLUGIN_DATA_BASE = 0x20081C00;
USER_PLUGIN_DATA_LIMIT = 0x20081E00;  /* Arbitrary - up to plugin to avoid blowing its own stack */
USER_PLUGIN_STACK_TOP = 0x20082000;   /* 1KB shared between stack and static RAM */
PIO_PLUGIN_DATA_BASE = 0x20082000;    /* Invalid value - PIO plugins do not get static RAM or stack ownership*/
PIO_PLUGIN_DATA_LIMIT = 0x20082000;
PIO_PLUGIN_STACK_TOP = 0x20082000;

ASSERT(DEFINED(PLUGIN_TYPE) && (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE || PLUGIN_TYPE == USER_PLUGIN_TYPE || PLUGIN_TYPE == PIO_PLUGIN_TYPE),
    "ERROR: PLUGIN_TYPE must be defined as 0 (system) or 1 (user) or 2 (PIO)")

PLUGIN_BASE = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_BASE : (PLUGIN_TYPE == USER_PLUGIN_TYPE) ? USER_PLUGIN_BASE : PIO_PLUGIN_BASE;
DATA_BASE = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_DATA_BASE : (PLUGIN_TYPE == USER_PLUGIN_TYPE) ? USER_PLUGIN_DATA_BASE : PIO_PLUGIN_DATA_BASE;
DATA_LIMIT = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_DATA_LIMIT : (PLUGIN_TYPE == USER_PLUGIN_TYPE) ? USER_PLUGIN_DATA_LIMIT : PIO_PLUGIN_DATA_LIMIT;
STACK_TOP = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_STACK_TOP : (PLUGIN_TYPE == USER_PLUGIN_TYPE) ? USER_PLUGIN_STACK_TOP : PIO_PLUGIN_STACK_TOP;

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
        __text_end = .;
    }

    .ramfunc DATA_BASE : AT(__text_end) {
        __ramfunc_start = .;
        *(.ramfunc*)
        . = ALIGN(4);
        __ramfunc_end = .;
    }
    __ramfunc_load = LOADADDR(.ramfunc);

    .data (__ramfunc_start + SIZEOF(.ramfunc)) : AT(__text_end + SIZEOF(.ramfunc)) {
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