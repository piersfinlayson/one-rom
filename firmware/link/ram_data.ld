/* Data to be copied from flash to RAM */
_sidata = LOADADDR(.data);
.data : AT(LOADADDR(.onerom_runtime_info) + SIZEOF(.onerom_runtime_info))
{
    . = ALIGN(4);
    _sdata = .;
    *(.data)
    *(.data*)
    . = ALIGN(4);
    _edata = .;
} >RAM

_flash_firmware_end = LOADADDR(.data) + SIZEOF(.data);

/* Note data is assumed to be zero - size - asserted in common.ld */