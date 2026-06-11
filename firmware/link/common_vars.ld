/* Stack size */
_Min_Stack_Size = 0x800;    /* 2KB, 1KB for each core */

/* Space for storing functions in RAM */
_Ram_Func_Size = 0x1000; /* 4KB */

/* Space for onerom_runtime_info_t in RAM */
_Onerom_Runtime_Info_Size = 0x100; /* 256 bytes */

/* Space for onerom_info_t and other stuff required by main_loop when running */
/* from RAM */
_Onerom_Info_Ram_Size = 0x100; /* 256 bytes */ 

/* Offset from start of RAM for onerom_info_t */
_Onerom_Info_Offset = 0x200;

/* Amount of flash reserved at start for firmware */
_Onerom_Flash_Size = 0xC000;

/* Amount of flash reserved after firmware for metadata */
_Onerom_Metadata_Size = 0x4000; 
