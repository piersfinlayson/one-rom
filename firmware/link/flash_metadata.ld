/* onerom_metadata_header_t */
.metadata (ORIGIN(FLASH) + _Onerom_Flash_Size) :
{
    . = ALIGN(4);
    _metadata_start = .;
    KEEP(*(.metadata.header))
    . = ALIGN(4);
    KEEP(*(.metadata.data))
    . = ALIGN(4);
    _metadata_end = .;
} > FLASH