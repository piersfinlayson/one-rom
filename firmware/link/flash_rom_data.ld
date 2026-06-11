/* onerom_rom_set_t, onerom_rom_info_t, ROM filenames and ROM data */
.rom_data (ORIGIN(FLASH) + _Onerom_Flash_Size + _Onerom_Metadata_Size) :
{
    . = ALIGN(4);
    _rom_images_start = .;
    KEEP(*(.rom_data))
    . = ALIGN(4);
    _rom_images_end = .;
} > FLASH