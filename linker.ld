ENTRY(_start)

MEMORY {
    RAM : ORIGIN = 0x40008000, LENGTH = 16M
}

SECTIONS {
    . = ORIGIN(RAM);
    .text :
    {
        *(.text.boot)
        *(.text.*)
    } > RAM

    .rodata : ALIGN(4K)
    {
        *(.rodata.*)
    } > RAM

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    } > RAM

    .bss : ALIGN(4K)
    {
        _bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        _bss_end = .;
    } > RAM

    _kernel_end = .;
    . = . + 64K;
    . = ALIGN(64K);
    stack_top = .;
}
