/* Stack size */
_Min_Stack_Size = 0x800;    /* 2KB, 1KB for each core */

/* Space for storing functions in RAM */
_Ram_Func_Size = 0x1000; /* 4KB */

/* Space for onerom_runtime_info_t in RAM */
_Onerom_Runtime_Info_Size = 0x100; /* 256 bytes */

/* Space for onerom_info_t and other stuff required by main_loop when running */
/* from RAM */
_Onerom_Info_Ram_Size = 0x100; /* 256 bytes */ 

/* Constants declared in the metadata schema, such as ONEROM_INFO_OFFSET */
INCLUDE "onerom_metadata.ld"

/* Address of the metadata region.  onerom_info_t points to it, and a host */
/* tool writes the metadata there. */
_metadata_start = FLASH_CS0_BASE_ADDR + METADATA_OFFSET;
