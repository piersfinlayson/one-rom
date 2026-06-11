/* onerom_info contains firmware configuration info */
.onerom_info :
{
    . = ORIGIN(FLASH) + _Onerom_Info_Offset;
    . = ALIGN(4);
    _onerom_info_start = .;
    KEEP(*(.onerom_info))
    . = ALIGN(4);
    _onerom_info_end = .;
} >FLASH