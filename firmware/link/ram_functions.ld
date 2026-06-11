/* Reserve space for RAM functions such as main_loop*/
.ram_func :
{
    . = ALIGN(4);
    _ram_func_start = .;
    . = . + _Ram_Func_Size;
    . = ALIGN(4);
    _ram_func_end = .;
} >RAM

/* Reserve space for data required by RAM functions */
.onerom_info_ram :
{
    . = ALIGN(4);
    _onerom_info_ram_start = .;
    . = . + _Onerom_Info_Ram_Size;
    . = ALIGN(4);
    _onerom_info_ram_end = .;
}