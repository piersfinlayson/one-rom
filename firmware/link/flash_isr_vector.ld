/* Vector table and code must be in FLASH */
.isr_vector :
{
    . = ALIGN(4);
    _flash_start = .;
    _isr_vector_start = .;
    KEEP(*(.isr_vector))
    . = ALIGN(4);
    _isr_vector_end = .;
} >FLASH
