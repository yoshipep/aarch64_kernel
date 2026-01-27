#include "asm/asmdefs.h"
#include "asm/system.h"

.section .text.boot

/*
 * Kernel entry point
 * x0: The address of the dtb
 */
ENTRY(_start)
    /* Mask all interrupts */
    msr DAIFSet, #0b1111
    mov x8, x0
    /* Enable floating point instructions */
    ldr x0, =(CPACR_EL1_FPEN0 | CPACR_EL1_FPEN1)
    msr CPACR_EL1, x0
    isb sy
    /* Set up stack */
    ldr x30, =stack_top
    mov sp, x30
    /* 1. Load the interrupt vector address into VBAR_EL1 */
    adr x0, evt
    msr VBAR_EL1, x0
    isb sy
    msr DAIFClr, #0b0010
    /* 2. Parse the dtb and initialize found devices */
    mov x0, x8
    bl kmain
    mov x8, #0xff
    svc #0
    b .
ENDPROC(_start)
