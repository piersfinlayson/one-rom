/* Reserve space for ROM image, using _Ram_Rom_Image_Start as an absolute */
/* address */
.ram_rom_image _Ram_Rom_Image_Start :
{
    . = ALIGN(4);
    _ram_rom_image_start = .;
    . = . + _Ram_Rom_Image_Size;
    . = ALIGN(4);
    _ram_rom_image_end = .;
} >RAM