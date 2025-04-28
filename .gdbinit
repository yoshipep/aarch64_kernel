set confirm off
set architecture aarch64
file kernel.elf
target remote 127.0.0.1:1234
layout asm
layout regs
