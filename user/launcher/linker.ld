ENTRY(_start)

SECTIONS
{
    . = 0x4000000000;

    .text : ALIGN(0x1000)
    {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(0x1000)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(0x1000)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(0x1000)
    {
        *(.bss .bss.*)
        *(COMMON)
    }
}