#ifndef __ASSEMBLER__
#error "This file should only be included in asm files"
#endif // __ASSEMBLER__

#ifndef MACRO_H_
#define MACRO_H_

/* clang-format off */

/* Macro to save gpr registers when taking an exception */
.macro save_gpr_regs_on_exc
	stp x27, x28, [sp, #-16]!
	stp x25, x26, [sp, #-16]!
	stp x23, x24, [sp, #-16]!
	stp x21, x22, [sp, #-16]!
	stp x19, x20, [sp, #-16]!
	stp x17, x18, [sp, #-16]!
	stp x15, x16, [sp, #-16]!
	stp x13, x14, [sp, #-16]!
	stp x11, x12, [sp, #-16]!
	stp x9, x10, [sp, #-16]!
	stp x7, x8, [sp, #-16]!
	stp x5, x6, [sp, #-16]!
	stp x3, x4, [sp, #-16]!
	stp x1, x2, [sp, #-16]!
	stp x0, xzr, [sp, #-16]!
.endm

/* Macro to restore gpr registers when returning from a syscall. This macro does not restore x0
 * as it is used to return the value of the requested syscall */
.macro restore_gpr_regs_on_swi
	ldp xzr, xzr, [sp], #16
	ldp x1, x2, [sp], #16
	ldp x3, x4, [sp], #16
	ldp x5, x6, [sp], #16
	ldp x7, x8, [sp], #16
	ldp x9, x10, [sp], #16
	ldp x11, x12, [sp], #16
	ldp x13, x14, [sp], #16
	ldp x15, x16, [sp], #16
	ldp x17, x18, [sp], #16
	ldp x19, x20, [sp], #16
	ldp x21, x22, [sp], #16
	ldp x23, x24, [sp], #16
	ldp x25, x26, [sp], #16
	ldp x27, x28, [sp], #16
.endm

/* Macro to restore gpr registers when returning from an exception */
.macro restore_gpr_regs_on_exc
	ldp x0, xzr, [sp], #16
	ldp x1, x2, [sp], #16
	ldp x3, x4, [sp], #16
	ldp x5, x6, [sp], #16
	ldp x7, x8, [sp], #16
	ldp x9, x10, [sp], #16
	ldp x11, x12, [sp], #16
	ldp x13, x14, [sp], #16
	ldp x15, x16, [sp], #16
	ldp x17, x18, [sp], #16
	ldp x19, x20, [sp], #16
	ldp x21, x22, [sp], #16
	ldp x23, x24, [sp], #16
	ldp x25, x26, [sp], #16
	ldp x27, x28, [sp], #16
.endm

/* Macro to get the current CPU offset */
.macro get_this_cpu_offset, dst
    mrs \dst, TPIDR_EL1
.endm

/* Macro to set the current CPU offset */
.macro set_this_cpu_offset, src
    msr TPIDR_EL1, \src
.endm

/* Macro to allocate stack space */
.macro alloc_stack, space
    sub sp, sp, \space
.endm

/* Macro to deallocate stack space */
.macro dealloc_stack, space
    add sp, sp, \space
.endm

#endif // MACRO_H_
