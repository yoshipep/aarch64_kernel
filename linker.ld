ENTRY(_start)
SECTIONS {
    . = 0x40100000;
    .ktext.init : { boot.o(.text) }
    .ktext : { *(.text) }
    .kdata : { *(.data) }
    .kbss : { *(.bss COMMON) }
    . = ALIGN(8);
    . += 0x1000;
    stack_top = .;
}
