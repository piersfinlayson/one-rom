/* Reserve space for the onerom_runtime_info structure at the end of RAM */
_onerom_runtime_info_flash = LOADADDR(.onerom_runtime_info);
.onerom_runtime_info : AT(_flash_main_firmware_end)
{
    . = ALIGN(4);
    _onerom_runtime_info_ram = .;
    *(.onerom_runtime_info)
    *(.onerom_runtime_info*)
    . = _onerom_runtime_info_ram + _Onerom_Runtime_Info_Size;
    . = ALIGN(4);
    _onerom_runtime_info_end = .;
} > RAM
