.include "macros.inc"

.equ IS_SVC_MASK, 0x15
.equ SVC_NR_MASK, 0xFFFF

.section .text

.global evt
.balign 2048

evt:
sync0_trampoline:
    b .

.balign 0x80
irq0_trampoline:
    b .

.balign 0x80
fiq0_trampoline:
    b .

.balign 0x80
serror0_trampoline:
    b .

.balign 0x80
sync_trampoline:
    b sync_handler

.balign 0x80
irq_trampoline:
    b irq_handler

.balign 0x80
fiq_trampoline:
    b .

.balign 0x80
serror_trampoline:
    b .

.balign 0x80
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
