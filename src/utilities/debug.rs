//! Debugging helpers

use core::arch::asm;

/// Traps into an attached debugger via the AArch64 `BRK` instruction.
///
/// Under QEMU's gdbstub, `BRK` is intercepted directly and reported to GDB as a breakpoint hit, without going through
/// this kernel's own exception vector table (`vectors.S`) — so it works even before `VBAR_EL1` is set up.
#[inline(always)]
pub fn breakpoint() {
    unsafe {
        asm!("brk #0", options(nomem, nostack, preserves_flags));
    }
}
