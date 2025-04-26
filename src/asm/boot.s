.section .text

.global _start

_start:
    ldr x30, =stack_top
    mov sp, x30
    # In a near future this parameters will be given by the machine:
    # x0: UART base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L175
    # x1: UART clock frequency: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L323
    # x2: UART baud rate
    mov x0, #0x09000000
    mov x1, #0x3600
    movk x1,#0x16e,LSL #16
    mov x2, #23
    bl init_uart
    bl configure_uart
    bl kmain
    b .
