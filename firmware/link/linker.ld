/*
 linker.ld

 Top-level One ROM firmware linker script.
*/

INCLUDE "common_vars.ld"

MEMORY
{
    FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 2048K
    RAM (xrw)  : ORIGIN = 0x20000000, LENGTH = 520K
}

_Ram_Rom_Image_Start = ORIGIN(RAM);
_Ram_Rom_Image_Size = 0x80000;  /* 512 KB */

INCLUDE "common.ld"