/*
 platform_boot_block.ld
*/

/* RP2350 specific linker script */
.rp2350_block :
{
    . = ALIGN(4);
    KEEP(*(.rp2350_block))
    . = ALIGN(4);
} >FLASH