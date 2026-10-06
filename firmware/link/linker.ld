/*
 linker.ld

 Top-level One ROM firmware linker script.
*/

INCLUDE "common_vars.ld"

MEMORY
{
    FLASH (rx) : ORIGIN = FLASH_CS0_BASE_ADDR, LENGTH = FIRMWARE_SIZE
    RAM (xrw)  : ORIGIN = 0x20000000, LENGTH = 520K
}

_Ram_Rom_Image_Start = ORIGIN(RAM);
_Ram_Rom_Image_Size = MAX_ROM_IMAGE_SIZE;

INCLUDE "common.ld"