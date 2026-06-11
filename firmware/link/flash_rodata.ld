/* General read-only data */
.rodata :
{
    . = ALIGN(4);
    _rodata_start = .;
    KEEP(*(.rodata));

        . = ALIGN(4);
    __yield_wait_for_resume_start = .;
    KEEP(*(.yield_wait_for_resume));
    . = ALIGN(4);
    __yield_wait_for_resume_end = .;

    KEEP(*(.rodata*));
    . = ALIGN(4);
    _rodata_end = .;
} >FLASH

/* 64 matches #define in plugin.c */
ASSERT((__yield_wait_for_resume_end - __yield_wait_for_resume_start) <= 64,
    "Error: yield_wait_for_resume exceeds YIELD_BUF_SIZE")
