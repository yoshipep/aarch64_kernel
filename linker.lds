#include "constants.h"
#include "asm/memory.h"

OUTPUT_FORMAT("elf64-littleaarch64")

OUTPUT_ARCH("aarch64")

ENTRY(_start)

MEMORY {
    /* Kernel loaded at 0x50000000 by bootloader (from ELF parsing) */
    RAM : ORIGIN = 0x50000000, LENGTH = 16M
}

SECTIONS {
    . = ORIGIN(RAM);
    __kernel_start = .;
    .text :
    {
        __text_start = .;
        *(.text.boot)
        *(.text.*)
        __text_end = .;
    } > RAM

    __data_start = .;
    .rodata : ALIGN(PAGE_SIZE)
    {
        __rodata_start = .;
        *(.rodata.*)
    __rodata_end = .;
    } > RAM

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    } > RAM

    .bss : ALIGN(PAGE_SIZE)
    {
        _bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(PAGE_SIZE);
        __idmap_l0 = .;
        . += PAGE_SIZE;
        __idmap_l1 = .;
        _bss_end = .;
    } > RAM

    __data_end = .;

    __kernel_end = .;
    . = . + STACK_SIZE;
    . = ALIGN(PAGE_SIZE);
    __stack_top = .;
}
