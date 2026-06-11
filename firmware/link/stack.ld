/* Reserve stack space */
._user_heap_stack :
{
    . = ALIGN(8);
    . = . + _Min_Stack_Size;
    . = ALIGN(8);
} >RAM