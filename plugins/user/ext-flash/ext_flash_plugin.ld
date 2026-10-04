/*
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 *
 * Linker script for the external flash tester plugin.
 *
 * The stock plugin script, with sections added to bracket the routines that
 * have to run from RAM.  They stay in flash here and the plugin copies the one
 * it needs onto its stack, so the RAM they cost is the size of the largest one
 * for the length of one call, rather than the sum of all of them for the life
 * of the plugin.  A user plugin has 1KB for its static RAM and its stack
 * together, and the sum does not fit in it.
 */

/* 
 * Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
 *
 * MIT License
 */

/*
 * One ROM plugin linker script
 *
 * Links a plugin binary as a flat, position-dependent image at the start of
 * its plugin region.  The build system selects the region via
 * --defsym PLUGIN_TYPE=<type>.
 *
 * The plugin header must be defined in the plugin source using
 * ORA_DEFINE_SYSTEM_PLUGIN() or ORA_DEFINE_USER_PLUGIN() from ora/plugin.h.
 */

INCLUDE onerom_linker_constants_generated.ld

SYSTEM_PLUGIN_TYPE = 0;
USER_PLUGIN_TYPE = 1;

SYSTEM_PLUGIN_BASE = ORA_FLASH_CS0_BASE_ADDR + ORA_SYSTEM_PLUGIN_OFFSET;
USER_PLUGIN_BASE = ORA_FLASH_CS0_BASE_ADDR + ORA_USER_PLUGIN_OFFSET;

TOP_OF_SRAM = 0x20082000;

SYSTEM_PLUGIN_DATA_BASE = 0x20081000;
SYSTEM_PLUGIN_DATA_LIMIT = 0x20081800;
SYSTEM_PLUGIN_STACK_TOP = 0x20081C00; /* 1KB stack, grows downwards and may collide with data after 1KB */
USER_PLUGIN_DATA_BASE = 0x20081C00;
USER_PLUGIN_DATA_LIMIT = 0x20081E00;  /* Arbitrary - up to plugin to avoid blowing its own stack */
USER_PLUGIN_STACK_TOP = 0x20082000;   /* 1KB shared between stack and static RAM */

ASSERT(DEFINED(PLUGIN_TYPE) && (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE || PLUGIN_TYPE == USER_PLUGIN_TYPE),
    "ERROR: PLUGIN_TYPE must be defined as 0 (system) or 1 (user)")

PLUGIN_BASE = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_BASE : USER_PLUGIN_BASE;
DATA_BASE = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_DATA_BASE : USER_PLUGIN_DATA_BASE;
DATA_LIMIT = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_DATA_LIMIT : USER_PLUGIN_DATA_LIMIT;
STACK_TOP = (PLUGIN_TYPE == SYSTEM_PLUGIN_TYPE) ? SYSTEM_PLUGIN_STACK_TOP : USER_PLUGIN_STACK_TOP;

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

        /* Routines the plugin copies onto its stack and runs from there.      */
        /* Each is bracketed so the plugin can find how much to copy, and each */
        /* holds its entry point first, followed by whatever that entry calls, */
        /* so the copy takes a self-contained blob.                            */
        . = ALIGN(4);
        __stage_probe_start = .;
        KEEP(*(.stage_probe))
        . = ALIGN(4);
        __stage_probe_end = .;

        __stage_qspi_start = .;
        KEEP(*(.stage_qspi))
        . = ALIGN(4);
        __stage_qspi_end = .;

        __stage_op_start = .;
        KEEP(*(.stage_op))
        . = ALIGN(4);
        __stage_op_end = .;

        __stage_m1_start = .;
        KEEP(*(.stage_m1))
        . = ALIGN(4);
        __stage_m1_end = .;

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

/* Each staged routine is copied onto the stack into a buffer the C sizes from
 * the matching Makefile number.  Only the linker has each routine's actual
 * size, so it checks the two agree here.  A routine that
 * has grown past its buffer fails the link, naming itself, rather than
 * overrunning a 1KB stack on the device.
 */
ASSERT(__stage_probe_end - __stage_probe_start <= STAGE_PROBE_WORDS * 4,
       "ERROR: .stage_probe is larger than STAGE_PROBE_WORDS")
ASSERT(__stage_qspi_end - __stage_qspi_start <= STAGE_QSPI_WORDS * 4,
       "ERROR: .stage_qspi is larger than STAGE_QSPI_WORDS")
ASSERT(__stage_op_end - __stage_op_start <= STAGE_OP_WORDS * 4,
       "ERROR: .stage_op is larger than STAGE_OP_WORDS")
ASSERT(__stage_m1_end - __stage_m1_start <= STAGE_M1_WORDS * 4,
       "ERROR: .stage_m1 is larger than STAGE_M1_WORDS")
