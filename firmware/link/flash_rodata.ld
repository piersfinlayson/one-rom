/* General read-only data */
.rodata :
{
    . = ALIGN(4);
    _rodata_start = .;
    *(.rodata);

        . = ALIGN(4);
    __yield_wait_for_resume_start = .;
    /* plugin.c copies this section from __yield_wait_for_resume_start to */
    /* __yield_wait_for_resume_end. Nothing else references it, so without */
    /* KEEP --gc-sections removes it. */
    KEEP(*(.yield_wait_for_resume));
    . = ALIGN(4);
    __yield_wait_for_resume_end = .;

    *(.rodata*);
    . = ALIGN(4);
    _rodata_end = .;
} >FLASH

/* 64 matches #define in plugin.c */
ASSERT((__yield_wait_for_resume_end - __yield_wait_for_resume_start) <= 64,
    "Error: yield_wait_for_resume exceeds YIELD_BUF_SIZE")
