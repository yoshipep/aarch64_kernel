set confirm off
set architecture aarch64
file bootloader/bootloader.elf
add-symbol-file kernel.elf 0x50000000
target remote 127.0.0.1:1234
layout asm
layout regs
focus cmd
