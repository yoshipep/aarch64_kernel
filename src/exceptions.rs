use core::arch::asm;

use crate::{uart, utilities};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Regs {
    x0: u64,
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
    zr: u64,
}

fn u32_to_str(mut num: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut i = buf.len();
    if num == 0 {
        buf[i - 1] = b'0';
        return &buf[i - 1..];
    }

    while num > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
    }

    &buf[i..]
}

#[unsafe(no_mangle)]
pub extern "C" fn do_sync(_regs: *mut Regs, nr: u32) {
    let mut buf = [0u8; 10];
    let nr_str = u32_to_str(nr, &mut buf);
    uart::print(b"Requested syscall: ");
    uart::print(nr_str);
    uart::print(b"\n");
}

#[unsafe(no_mangle)]
pub fn do_irq(id: u32) -> u32 {
    match id {
        30 => {
            uart::print(b"Timer interrupt!\n");
            unsafe {
                asm!(
                    "mrs x0, CNTFRQ_EL0",
                    "msr CNTP_TVAL_EL0, x0",
                    "isb",
                    options(nostack, nomem)
                );
            }
        }
        33 => {
            unsafe {
                uart::RX_BUFFER.lock_irqsafe(|rx| {
                    let ch = utilities::read_mmio(0x9000000, 0) as u8;
                    let _ = rx.push(ch);
                });
                utilities::write_mmio(0x9000000, 0x44, 1 << 4);
            }
        }
        _ => {
            uart::print(b"Unhandled IRQ: ");
            let mut buf = [0u8; 10];
            uart::print(b"Unhandled IRQ: ");
            let id_str = u32_to_str(id, &mut buf);
            uart::print(b"Unhandled IRQ: ");
            uart::print(id_str);
            uart::print(b"\n");
        }
    }
    return id;
}

#[unsafe(no_mangle)]
pub extern "C" fn do_fiq() -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn do_serror() -> ! {
    loop {}
}

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
    uart::print(b"Unimplemented synchronous exception: ");
    uart::print(kind.as_bytes());
    uart::print(b"\n");
}
