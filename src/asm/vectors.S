#include "asm/asmdefs.h"
#include "asm/macro.h"

.equ IS_SVC_MASK, 0x15

.section .text.eh

.global evt
.align 11

evt:
.align 7 /* Current EL SP_EL0 Synchronous */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_bad_sync
	b exception_exit

.align 7 /* Current EL SP_EL0 IRQ */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_bad_irq
	b exception_exit

.align 7 /* Current EL SP_EL0 FIQ */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_bad_fiq
	b exception_exit

.align 7 /* Current EL SP_EL0 SError */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_bad_serror
	b exception_exit

.align 7 /* Current EL SP_ELx Synchronous */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	b sync_handler

.align 7 /* Current EL SP_ELx IRQ */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	b irq_handler

.align 7 /* Current EL SP_ELx FIQ */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_fiq
	b exception_exit

.align 7 /* Current EL SP_ELx SError */
	stp x29, x30, [sp, #-16]!
	bl save_regs
	bl do_serror
	b exception_exit

/* Move the code outside of the evt region */
.org 0x0800
/* x0: Pointer to a struct Regs */
sync_handler:
	/* As we come from save_regs, x1 already has esr_el1 value */
	lsr x1, x1, #26
	mov w2, #IS_SVC_MASK
	and w1, w1, w2
	cbz w1, unhandled
	/* get syscall num from Regs.x8 */
	ldr x0, [x0, #104]
	bl do_sync
	b exit
unhandled:
	mov w0, w1
	bl unimplemented_sync
exit:
	ldp x3, x2, [sp], #16
	/* Drop esr_el1 and xzr */
	ldp xzr, xzr, [sp], #16
	msr spsr_el1, x2
	msr elr_el1, x3
	restore_gpr_regs_on_swi
	ldp x29, x30, [sp], #16
	eret

irq_handler:
	/* Read the interrupt ID */
	mrs x0, ICC_IAR1_EL1
	bl do_irq
	/* Acknowledge the interrupt */
	msr ICC_EOIR1_EL1, x0
exception_exit:
	ldp x3, x2, [sp], #16
	/* Drop esr_el1 and xzr */
	ldp xzr, xzr, [sp], #16
	msr spsr_el1, x2
	msr elr_el1, x3
	restore_gpr_regs_on_exc
	ldp x29, x30, [sp], #16
	eret

ENTRY(save_regs)
	save_gpr_regs_on_exc
/* We save these registers bc if an unhandled exception is taken, we print their values */
	mrs x1, esr_el1
	mrs x2, elr_el1
	mrs x3, spsr_el1
	stp x1, xzr, [sp, #-16]!
	stp x2, x3, [sp, #-16]!
	mov x0, sp
	ret
ENDPROC(save_regs)
