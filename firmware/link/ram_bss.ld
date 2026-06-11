/* Uninitialized data */
.bss :
{
    . = ALIGN(4);
    _sbss = .;
    *(.bss)
    *(.bss*)
    *(COMMON)
    . = ALIGN(4);
    _ebss = .;
} >RAM

_bss_size = _ebss - _sbss;