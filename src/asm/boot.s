.section .text

.global _start

_start:
    # Mask all interrupts
    msr DAIFSet, #0b1111
    # Load the interrupt vector address into VBAR_EL1
    ldr x0, =evt
    msr VBAR_EL1, x0
    isb
    ldr x30, =stack_top
    mov sp, x30
    mov w0, #0x080A0000
    # Initialize the GIC
    # In a near future this parameters will be given by the machine:
    # w0: GIC REDIST base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L174
    bl init_gic_redistributor
    # Enable group 1 NS interrupts
    mrs x0, ICC_IGRPEN1_EL1
    orr x0, x0, #1
    msr ICC_IGRPEN1_EL1, x0
    # Set priority levels
    mrs x0, ICC_PMR_EL1
    orr x0, x0, #0xff
    msr ICC_PMR_EL1, x0
    # In a near future this parameters will be given by the machine:
    # w0: GIC DIST base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L166
    # w1: Interrupt ID: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L229
    mov w0, #0x08000000
    mov w1, #0x1
    bl enable_interrupt
    # Unmask IRQs only
    msr DAIFClr, #0b0010
    # In a near future this parameters will be given by the machine:
    # w0: UART base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L175
    # w1: UART clock frequency: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L323
    # w2: UART baud rate
    mov w0, #0x09000000
    mov w1, #0x3600
    movk w1,#0x16e,LSL #16
    mov w2, #23
    bl init_uart
    bl configure_uart
    svc #0
    bl kmain
    b .
