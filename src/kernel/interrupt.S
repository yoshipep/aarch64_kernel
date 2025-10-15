.include "macros.inc"

.equ IS_SVC_MASK, 0x15
.equ SVC_NR_MASK, 0xFFFF

.section .text.eh

.global evt
.align 11

evt:
.org 0x0000
sync0_trampoline:
    b .

.org 0x0080
irq0_trampoline:
    b .

.org 0x0100
fiq0_trampoline:
    b .

.org 0x0180
serror0_trampoline:
    b .

.org 0x0200
sync_trampoline:
    b sync_handler

.org 0x0280
irq_trampoline:
    b irq_handler

.org 0x0300
fiq_trampoline:
    b .

.org 0x0380
serror_trampoline:
    b .

.org 0x0400
sync_handler:
    alloc_stack 256
    saveregs
    mrs x0, ESR_EL1
    lsr x1, x0, #26
    mov w2, IS_SVC_MASK
    and w2, w1, w2
    cmp w2, w2
    bne unhandled
    and w1, w0, SVC_NR_MASK
    mov x0, sp
    bl do_sync
    b sync_ret
unhandled:
    mov w0, w1
    bl unimplemented_sync
sync_ret:
    restoreregs
    dealloc_stack 256
    eret

irq_handler:
    alloc_stack 256
    saveregs
    # Read the interrupt ID
    mrs x0, ICC_IAR1_EL1
    bl do_irq
    # Acknowledge the interrupt
    msr ICC_EOIR1_EL1, x0
    restoreregs
    dealloc_stack 256
    eret
