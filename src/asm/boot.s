.section .text

.global _start

_start:
    mov x0, #0x300000
    msr CPACR_EL1, x0
    isb sy
    ldr x30, =stack_top
    mov sp, x30
    # Mask all interrupts
    msr DAIFSet, #0b1111
    # 1. Load the interrupt vector address into VBAR_EL1
    ldr x0, =evt
    msr VBAR_EL1, x0
    isb sy
    # 2. Enable GIC
    # x0: GIC DIST base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L166
    ldr x0, =0x08000000
    bl init_gic_distributor
    # x0: GIC REDIST base address: https://github.com/qemu/qemu/blob/master/hw/arm/virt.c#L174
    ldr x0, =0x080A0000
    bl init_gic_redistributor
    # 3. Enable system register access ICC_SRE_EL1
    mrs x0, ICC_SRE_EL1
    orr x0, x0, #1
    msr ICC_SRE_EL1, x0
    isb sy
    # 4. Set priority mask
    mov x0, #0xff
    bl set_priority_mask
    # 5. Enable Group 1 ints
    bl enable_grp1_ints
    # 6. Set a priority level for the timer
    mov w1, #30
    mov w2, #0x80
    ldr x0, =AFFINITY_ENABLED
    ldr x0, [x0]
    cbz x0, not_enabled
    ldr x0, =0x080A0000
    bl set_int_priority
    b next
not_enabled:
    ldr x0, =0x08000000
    bl set_int_priority
next:
    # 7. Route the interrupt through group 1
    ldr x0, =0x080A0000
    mov w1, #30
    bl set_int_grp
    # 8. Enable the interrupt
    ldr x0, =0x080A0000
    mov w1, #30
    bl enable_int
    mrs x0, CNTFRQ_EL0
    msr CNTP_TVAL_EL0, x0
    mov x0, #0x1
    msr CNTP_CTL_EL0, x0
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
