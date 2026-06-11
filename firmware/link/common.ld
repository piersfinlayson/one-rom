/* This is the main One ROM linker script. */
/* */
/* It is included via a build specific linker script that is auto-generated and */
/* looks like this: */
/* */
/* INCLUDE "common_vars.ld" */
/* */
/* MEMORY */
/* { */
/*     FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 2048K */
/*     RAM (xrw)  : ORIGIN = 0x20000000, LENGTH = 520K */
/* } */
/*  */
/* _Ram_Rom_Image_Start = ORIGIN(RAM) + 0x10000; */
/* _Ram_Rom_Image_Size = 0x10000; */
/*  */
/* INCLUDE "common.ld" */
/* */

/* Entry Point */
ENTRY(Reset_Handler)

SECTIONS
{
    /* -------------- */
    /* Flash sections */
    /* -------------- */

    /* Reset and interrupt vectors - must come first*/
    INCLUDE "flash_isr_vector.ld"

    /* Boot block - must come next (is auto-generated and RP2350 specific) */
    INCLUDE "platform_boot_block.ld"

    /* Next, at offset 0x200 is the onerom_info_t structure, containing info */
    /* about the specific firmware build, pointers to ROM metadata, etc */
    INCLUDE "flash_onerom_info.ld"

    /* Next goes text. */
    INCLUDE "flash_text.ld"

    /* Rest of the code comes next */
    INCLUDE "flash_rodata.ld"

    /* Mark the end of main portion of the firmware - later stuff is going */
    /* to get put in the firmware section as well - specifically data: */
    /* - onerom_runtime_info */
    /* - data */
    /* These are stored in the firmware (flash) and copied to RAM at startup */
    /* in vector.c.*/
    _flash_main_firmware_end = .;

    /* After the firmware, still in flash, comes the ROM image metadata. */
    /* This is put at a specific offset (currently 48KB). */
    INCLUDE "flash_metadata.ld"
    /* Mark the end of metadata */
    _flash_metadata_end = .;

    /* Ensure firmware binary is at least 64KB even if no ROM images, by */
    /* placing a single 0xFF byte at the 64KB - 1 byte mark */
    .padding_to_rom_data (ORIGIN(FLASH) + 0x10000 - 1) :
    {
        BYTE(0xFF)
    } >FLASH

    /* And then the ROM data itself.  This is also put at a specific offset, */
    /* currently 64KB. */
    INCLUDE "flash_rom_data.ld"

    /* This is the end of flash - nothing follows ROM data.  This means that */
    /* firmware is always 48KB + size of ROM images.  As ROM images are */
    /* either 16KB or 64KB, the firmware.bin is always a multiple of 16KB. */

    /* Mark end of flash */
    _flash_end = .;

    /* ------------ */
    /* RAM sections */
    /* ------------ */

    /* Reserved space in RAM to store the ROM image, so it can be looked-up */
    /* directly from RAM.  This is specified so on the RP2350 we can set it */
    /* to a specific SRAM bank, to try and avoid bus contention. */
    INCLUDE "ram_rom_image.ld"

    /* Next include space for any functions that may be copied to RAM. It */
    /* doesn't really matter where these go. */
    /* */
    /* Remove, at least for now, to allow RP2350 access to 512KB RAM */
    /* This will break the RAM function feature */
    /* */
    /* INCLUDE "ram_functions.ld" */

    /* This is onerom_runtime_info_t.  This is pointed to by onerom_info_t, */
    /* so that it is easy to analyse RAM and find it.  It contains useful */
    /* runtime information about One ROM, like what ROM is being served and, */
    /* if so configured, the number of ROM accesses. */
    INCLUDE "ram_runtime_info.ld"

    /* RAM data sections - data and bss */
    INCLUDE "ram_data.ld"

    /* BSS section - RAM to be zeroed on startup */
    INCLUDE "ram_bss.ld"

    ASSERT(. <= 0x20081000, "Error: RAM data/bss overlaps system plugin data region")

    /* Stack section */
    INCLUDE "stack.ld"

    /* Remove information from standard libraries */
    /DISCARD/ :
    {
        libc.a ( * )
        libm.a ( * )
        libgcc.a ( * )
    }
}

_ram_size = LENGTH(RAM);
_end = ORIGIN(RAM) + _ram_size;

_onerom_runtime_info_location = _end - 8192;

/* Set initial stack pointer to end of RAM, except for reserved space */
_estack = _end;

/* Limit bss - this is all segger, and 4284 bytes as of adding this */
/* 4096 is the UP buffer - reduce that and make clean-firmware; make */
/* to reduce it */
ASSERT(_bss_size <= 4284, "Error: BSS section too large")

/* That leaves < 4K for stack, for both processors */

/* Check data is zero */
ASSERT(SIZEOF(.data) == 0, 
    "Error: Data section is non zero - turn globals into const data")

/* Check enough space for onerom_runtime_info structure */
ASSERT(SIZEOF(.onerom_runtime_info) <= _Onerom_Runtime_Info_Size, 
    "Error: onerom_runtime_info structure too large for allocated space")

ASSERT(_flash_firmware_end <= (ORIGIN(FLASH) + _Onerom_Flash_Size), 
    "Error: Firmware exceeds 48KB reserved space")

/* Check metadata fits in reserved space */
ASSERT(_flash_metadata_end <= (ORIGIN(FLASH) + _Onerom_Flash_Size + _Onerom_Metadata_Size),
    "Error: Metadata exceeds 16KB reserved space")
