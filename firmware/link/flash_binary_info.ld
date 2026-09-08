/* The binary info header picotool scans for. */
/* */
/* picotool reads the first 256 words of the image and looks for the start */
/* marker, needing the end marker four words later, so the whole 20 byte */
/* header has to fall below 1KB.  Sitting it here, straight after */
/* onerom_info, puts it around 0x240 with the rest of the window to spare. */
/* The entries, strings and mapping table it points at are ordinary rodata */
/* and can be anywhere, because picotool follows the pointers. */
.binary_info_header :
{
    . = ALIGN(4);
    _binary_info_header_start = .;
    KEEP(*(.binary_info_header))
    . = ALIGN(4);
    _binary_info_header_end = .;
} >FLASH

ASSERT(SIZEOF(.binary_info_header) > 0,
    "ERROR: .binary_info_header is empty - picotool will report no program info")

ASSERT(_binary_info_header_end <= ORIGIN(FLASH) + 1024,
    "ERROR: binary info header sits past the first 1KB, where picotool stops looking")
