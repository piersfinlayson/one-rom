.text :
{
    . = ALIGN(4);
    _text_start = .;

    *(.text)
    *(.text*)

    . = ALIGN(4);
    _text_end = .;
} >FLASH