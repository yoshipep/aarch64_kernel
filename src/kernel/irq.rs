//! Exception handling module
//!
//! Handles synchronous exceptions, IRQs, FIQs, and SErrors taken at EL1. Assembly entry code (`src/asm/vectors.S`)
//! saves register state to the stack and calls into the matching handler here by name —
//! `do_sync`/`do_irq`/`do_fiq`/`do_serror` for the current-EL vectors this kernel actually expects, `do_bad_*` for
//! exceptions taken from an unexpected EL, which just dump diagnostics and panic.

use crate::drivers::timer::arch_timer;
use crate::drivers::uart::pl011;
use crate::{print, println};

/// CPU register state at the time of an exception
///
/// This struct is a typed view over raw stack memory — it is never constructed in Rust, only read through a `&Regs`
/// reference handed in from assembly (`x0` at the call to `do_bad_sync` etc., set by `save_regs` in
/// `vectors.S`/`macro.h`'s `save_gpr_regs_on_exc`). Reordering this struct without updating the assembly (or vice
/// versa) silently reads the wrong register.
///
/// `x29`/`x30` are pushed by each vector entry in `vectors.S` itself — immediately before `bl save_regs` — not by
/// `save_regs`. Every one of the 8 vectors follows that same `stp x29, x30, [sp, #-16]!` then `bl save_regs` pattern
/// with nothing in between, which is what makes their position here reliable despite living outside `save_regs`'s own
/// pushes.
///
/// # Fields
///
/// - `elr`: Exception Link Register - return address
/// - `spsr`: Saved Program Status Register - saved processor state
/// - `esr`: Exception Syndrome Register - describes the exception cause
/// - `_xzr1`, `_xzr2`: architectural zero-register padding — `stp` needs a register pair, and `esr`/`x0` have no
///   natural odd-numbered partner, so the zero register fills the slot. Always reads as 0; not real saved data.
/// - `x0-x30`: General-purpose registers
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Regs {
    elr: u64,
    spsr: u64,
    esr: u64,
    _xzr1: u64,
    x0: u64,
    _xzr2: u64,
    x1: u64,
    x2: u64,
    x3: u64,
    x4: u64,
    x5: u64,
    x6: u64,
    x7: u64,
    x8: u64,
    x9: u64,
    x10: u64,
    x11: u64,
    x12: u64,
    x13: u64,
    x14: u64,
    x15: u64,
    x16: u64,
    x17: u64,
    x18: u64,
    x19: u64,
    x20: u64,
    x21: u64,
    x22: u64,
    x23: u64,
    x24: u64,
    x25: u64,
    x26: u64,
    x27: u64,
    x28: u64,
    x29: u64,
    x30: u64,
}

impl Regs {
    /// Register names for iteration
    const NAMES: [&'static str; 34] = [
        "x0 ", "x1 ", "x2 ", "x3 ", "x4 ", "x5 ", "x6 ", "x7 ", "x8 ", "x9 ", "x10", "x11", "x12",
        "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22", "x23", "x24", "x25",
        "x26", "x27", "x28", "x29", "x30", "esr", "elr", "spsr",
    ];

    /// Convert registers to an array for easy iteration
    pub fn as_array(&self) -> [u64; 34] {
        [
            self.x0, self.x1, self.x2, self.x3, self.x4, self.x5, self.x6, self.x7, self.x8,
            self.x9, self.x10, self.x11, self.x12, self.x13, self.x14, self.x15, self.x16,
            self.x17, self.x18, self.x19, self.x20, self.x21, self.x22, self.x23, self.x24,
            self.x25, self.x26, self.x27, self.x28, self.x29, self.x30, self.esr, self.elr,
            self.spsr,
        ]
    }

    /// Returns an iterator over (name, value) pairs for all registers
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, u64)> {
        self.as_array()
            .into_iter()
            .zip(Self::NAMES.iter().copied())
            .map(|(val, name)| (name, val))
    }

    /// Print all registers to UART
    pub fn print(&self) {
        println!("\nRegisters:");
        for (name, value) in self.iter() {
            println!("{}: 0x{:016x}", name, value);
        }
    }
}

/// Prints the faulting instruction at the exception address
///
/// Reads and displays the 32-bit instruction at the address stored in the Exception Link Register (ELR), which points
/// to the instruction that caused the exception.
fn print_faulting_instr(elr: u64) {
    let addr = (elr & !3) as *const u32;
    let opcode: u32;
    unsafe {
        opcode = addr.read_volatile();
    }

    print!("Faulting instruction at 0x{:016x}: ", elr);
    for i in 0..4u32 {
        let byte = (opcode >> (i * 8)) as u8;
        if i == 0 {
            print!("[{:02X}]", byte);
        } else {
            print!("{:02X}", byte);
        }
        if i < 3 {
            print!(" ");
        }
    }
    println!();
}

/// Prints all CPU registers from the saved register state
#[inline(always)]
fn print_regs(regs: &Regs) {
    // Print register dump
    regs.print();
}

/// Handles synchronous exceptions from an unexpected exception level
///
/// This "bad mode" handler is called when a synchronous exception occurs from an exception level that should not
/// normally generate exceptions. It prints diagnostic information and panics.
#[unsafe(no_mangle)]
pub extern "C" fn do_bad_sync(regs: &Regs) -> ! {
    println!("Bad mode in Synchronous Exception handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}

/// Handles IRQ (Interrupt Request) from an unexpected exception level
///
/// This "bad mode" handler is called when an IRQ occurs from an exception level that should not normally generate
/// interrupts. It prints diagnostic information and panics.
#[unsafe(no_mangle)]
pub extern "C" fn do_bad_irq(regs: &Regs) -> ! {
    println!("Bad mode in IRQ handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}

/// Handles FIQ (Fast Interrupt Request) from an unexpected exception level
///
/// This "bad mode" handler is called when an FIQ occurs from an exception level that should not normally generate fast
/// interrupts. It prints diagnostic information and panics.
#[unsafe(no_mangle)]
pub extern "C" fn do_bad_fiq(regs: &Regs) -> ! {
    println!("Bad mode in FIQ handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}

/// Handles SError (System Error) from an unexpected exception level
///
/// This "bad mode" handler is called when a system error occurs from an exception level that should not normally
/// generate SErrors. It prints diagnostic information and panics.
#[unsafe(no_mangle)]
pub extern "C" fn do_bad_serror(regs: &Regs) -> ! {
    println!("Bad mode in SError handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}

/// Synchronous exception handler for SVC (syscall) instructions
///
/// Currently a stub: prints the requested syscall number and always returns 0. No syscalls are implemented yet.
///
/// # Arguments
///
/// * `nr` - the syscall number requested (from `x8` at the `svc` instruction, extracted by `sync_handler` in
///   `vectors.S` before this is called)
///
/// # Returns
///
/// The syscall's return value, placed back into `x0` for the caller — currently always 0.
#[unsafe(no_mangle)]
pub extern "C" fn do_sync(nr: u32) -> u64 {
    println!("Requested syscall: {}", nr);
    return 0;
}

/// IRQ handler
///
/// Dispatches by interrupt ID: rearms the timer on a timer interrupt, drains the UART RX FIFO on a UART interrupt,
/// otherwise just logs it.
///
/// # Arguments
///
/// * `id` - the interrupt ID read from `ICC_IAR1_EL1` by `irq_handler` in `vectors.S`
///
/// # Returns
///
/// The same `id`, unchanged — the caller writes it back to `ICC_EOIR1_EL1` to acknowledge the interrupt, which is why
/// this returns the ID rather than `()`.
#[unsafe(no_mangle)]
pub fn do_irq(id: u32) -> u32 {
    match id {
        30 => {
            println!("Timer interrupt!");
            arch_timer::rearm(arch_timer::get_frequency() as u32);
        }
        // UART RX interrupt
        33 => {
            pl011::handle_rx_interrupt();
        }
        _ => {
            println!("Unhandled IRQ: {}", id);
        }
    }
    return id;
}

/// Handler for unimplemented synchronous exceptions
///
/// # Arguments
///
/// * `exception_class` - the ESR_EL1.EC field (bits \[31:26\]), identifying why the exception was taken; decoded below
///   into a human-readable reason and printed
#[unsafe(no_mangle)]
pub extern "C" fn unimplemented_sync(exception_class: u32) {
    let kind;

    match exception_class {
        1 => kind = "Trapped WF",
        3 => kind = "Trapped MCR (coproc==0b1111). EC value 0b000000",
        4 => kind = "Trapped MCRR or MRRC (coproc=0b1111). EC value 0b000000",
        5 => kind = "Trapped MCR (coproc==0b1110)",
        6 => kind = "Trapped LDC",
        7 => kind = "SIMD not implemented. EC value 0b000000",
        10 => kind = "Trapped execution of any instruction not covered by other EC values",
        12 => kind = "Trapped MRRC (coproc=0b1110)",
        13 => kind = "Branch Target Exception",
        14 => kind = "Illegal Execution State",
        17 => kind = "SVC in AArch32 state",
        20 => {
            kind = "Trapped MSRR, MRRS or System instruction execution in AArch64 state. EC value 0b000000"
        }
        21 => kind = "SVC in AArch64 state",
        24 => {
            kind = "Trapped MSRR, MRRS or System instruction execution in AArch64 state. EC values 0b000000, 0b000001, 0b000111"
        }
        25 => kind = "Access to SVE functionality. EC value 0b000000",
        27 => kind = "TSTART instruction at EL0",
        28 => kind = "PAC fail",
        29 => kind = "Access to SME functionality. EC value 0b000000",
        32 => kind = "Instruction abort from lower EL",
        33 => kind = "Instruction abort taken without a change in EL",
        34 => kind = "PC alignement fault",
        36 => kind = "Data abort from lower EL",
        37 => kind = "Data abort taken without a change in EL",
        38 => kind = "SP alignement fault",
        39 => kind = "Memory operation exception",
        40 => kind = "FPE from AArch32 state",
        44 => kind = "FPE from AArch64 state",
        45 => kind = "GCS exception",
        47 => kind = "SError exception",
        48 => kind = "Breakpoint exception from a lower EL",
        49 => kind = "Breakpoint exception taken without a change in EL",
        50 => kind = "Software step exception from a lower EL",
        51 => kind = "Software step exception exception taken without a change in EL",
        52 => kind = "Watchpoint exception from a lower EL",
        53 => kind = "Watchpoint exception exception taken without a change in EL",
        56 => kind = "BKPT instruction in AArch32 state",
        60 => kind = "BRK instruction in AArch64 state",
        61 => kind = "Profiling exception",
        _ => kind = "Unknown reason",
    }
    println!("Unimplemented synchronous exception: {}", kind);
}

/// Handles FIQ (Fast Interrupt Request) from the current exception level
///
/// Called when a fast interrupt request is received. Prints diagnostic information and panics (as FIQ handling is not
/// yet implemented).
#[unsafe(no_mangle)]
pub extern "C" fn do_fiq(regs: &Regs) -> ! {
    println!("FIQ handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}

/// Handles SError (System Error) from the current exception level
///
/// Called when a system error occurs (e.g., asynchronous external abort). Prints diagnostic information and panics.
#[unsafe(no_mangle)]
pub extern "C" fn do_serror(regs: &Regs) -> ! {
    println!("SError handler");
    print_faulting_instr(regs.elr);
    print_regs(regs);
    panic!();
}
