use core::arch::asm;

use crate::utilities::{clear_mmio_bits, read_mmio, set_mmio_bits, write_mmio};

// --- GICD (Distributor) Register Constants ---
const GICD_CTLR: usize = 0x000;
const GICD_CTLR_GRP1NS: u32 = 0b10;
const GICD_CTLR_GRP1S: u32 = 0b100;
const GICD_CTLR_ARE_S: u32 = 0b10000; // ARE_S (bit 4), for when DS=1
const GICD_ENABLE_GRPS: u32 = GICD_CTLR_GRP1S | 1;

// --- GICR (Redistributor) Register Constants ---
const GICR_SGI_BASE: usize = 0x10000; // Offset from RD_base to SGI & PPI page

const GICR_WAKER: usize = 0x0014;
const GICR_WAKER_PSLEEP: u32 = 0b10;
const GICR_WAKER_CASLEEP: u32 = 0b100;

const GICR_IPRIORITYR: usize = 0x400;
const GICR_IGROUPR0: usize = 0x080;
const GICR_ISENABLER0: usize = 0x100;

#[unsafe(no_mangle)]
pub static mut AFFINITY_ENABLED: bool = false;

#[unsafe(no_mangle)]
pub unsafe fn init_gic_distributor(dist_base: usize) {
    unsafe {
        set_mmio_bits(dist_base, GICD_CTLR, GICD_CTLR_GRP1S | GICD_CTLR_GRP1NS);
        asm!("dsb sy", options(nostack, nomem));
        let final_ctlr = read_mmio(dist_base, GICD_CTLR);
        AFFINITY_ENABLED = (final_ctlr & GICD_CTLR_ARE_S) != 0;
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn init_gic_redistributor(rd_base: usize) {
    unsafe {
        clear_mmio_bits(rd_base, GICR_WAKER, GICR_WAKER_PSLEEP);
        asm!("dsb sy", options(nostack, nomem));
        loop {
            if (read_mmio(rd_base, GICR_WAKER) & GICR_WAKER_CASLEEP) == 0 {
                break;
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_priority_mask(priority: u8) {
    unsafe {
        asm!("msr ICC_PMR_EL1, {}", in(reg) priority as u64, options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn enable_grp1_ints() {
    unsafe {
        asm!(
            "mrs {tmp}, ICC_IGRPEN1_EL1",
            "orr {tmp}, {tmp}, #1",
            "msr ICC_IGRPEN1_EL1, {tmp}",
            "isb sy",
            tmp = out(reg) _,
            options(nostack, nomem)
        );
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_int_priority(rd_base: usize, id: u32, prio: u8) {
    unsafe {
        let sgi_base = rd_base + GICR_SGI_BASE;
        let reg_index = id / 4;
        let reg_offset = (reg_index * 4) as usize;
        let byte_index_in_reg = id % 4;
        let bit_shift = byte_index_in_reg * 8;
        let prio_reg_addr = sgi_base + GICR_IPRIORITYR + reg_offset;
        let mut reg_val = read_mmio(prio_reg_addr, 0);
        let mask: u32 = !(0xFF << bit_shift);
        reg_val &= mask;
        reg_val |= (prio as u32) << bit_shift;
        write_mmio(prio_reg_addr, 0, reg_val);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_int_grp(rd_base: usize, id: u32) {
    unsafe {
        set_mmio_bits(rd_base + GICR_SGI_BASE, GICR_IGROUPR0, 1 << id);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn enable_int(rd_base: usize, id: u32) {
    unsafe {
        set_mmio_bits(rd_base + GICR_SGI_BASE, GICR_ISENABLER0, 1 << id);
        asm!("dsb sy", options(nostack, nomem));
    }
}
