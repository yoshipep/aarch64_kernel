use core::arch::asm;

use crate::utilities::{clear_mmio_bits, read_mmio, set_mmio_bits, write_mmio};

// --- GICD (Distributor) Register Constants ---
const GICD_CTLR: usize = 0x000;
const GICD_CTLR_GRP1NS: u32 = 0b10;
const GICD_CTLR_GRP1S: u32 = 0b100;
const GICD_CTLR_ARE_S: u32 = 0b10000; // ARE_S (bit 4), for when DS=1
const GICD_ISENABLER: usize = 0x100;
const GICD_IPRIORITYR: usize = 0x400;
const GICD_ICFGR: usize = 0xC00;
const GICD_IROUTER: usize = 0x6100;
const GICD_IGROUPR: usize = 0x080;

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
pub unsafe fn enable_grp0_ints() {
    unsafe {
        asm!(
            "mrs {tmp}, ICC_IGRPEN0_EL1",
            "orr {tmp}, {tmp}, #1",
            "msr ICC_IGRPEN0_EL1, {tmp}",
            "isb sy",
            tmp = out(reg) _,
            options(nostack, nomem)
        );
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

#[unsafe(no_mangle)]
pub unsafe fn set_spi_priority(dist_base: usize, id: u32, prio: u8) {
    unsafe {
        let reg_index = id / 4;
        let reg_offset = (reg_index * 4) as usize;
        let byte_index_in_reg = id % 4;
        let bit_shift = byte_index_in_reg * 8;
        let prio_reg_addr = dist_base + GICD_IPRIORITYR + reg_offset;
        let mut reg_val = read_mmio(prio_reg_addr, 0);
        let mask: u32 = !(0xFF << bit_shift);
        reg_val &= mask;
        reg_val |= (prio as u32) << bit_shift;
        write_mmio(prio_reg_addr, 0, reg_val);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_spi_trigger(dist_base: usize, id: u32) {
    unsafe {
        let reg_index = id / 16;
        let reg_offset = (reg_index * 4) as usize;
        let bit_shift = (id % 16) * 2;
        let cfg_reg_addr = dist_base + GICD_ICFGR + reg_offset;
        let mut reg_val = read_mmio(cfg_reg_addr, 0);
        let mask: u32 = !(0b11 << bit_shift);
        reg_val &= mask;
        write_mmio(cfg_reg_addr, 0, reg_val);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn enable_spi(dist_base: usize, id: u32) {
    unsafe {
        let reg_index = id / 32;
        let reg_offset = (reg_index * 4) as usize;
        let enabler_reg_addr = dist_base + GICD_ISENABLER + reg_offset;
        let bit_to_set = 1 << (id % 32);
        write_mmio(enabler_reg_addr, 0, bit_to_set);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_spi_routing(dist_base: usize, id: u32, core_affinity: u64) {
    unsafe {
        let router_reg_addr = dist_base + GICD_IROUTER + (8 * id as usize);
        let router_ptr = router_reg_addr as *mut u64;
        core::ptr::write_volatile(router_ptr, core_affinity);
        asm!("dsb sy", options(nostack, nomem));
    }
}

#[unsafe(no_mangle)]
pub unsafe fn set_spi_group(dist_base: usize, id: u32) {
    unsafe {
        let reg_index = id / 32;
        let reg_offset = (reg_index * 4) as usize;
        let group_reg_addr = dist_base + GICD_IGROUPR + reg_offset;
        let bit_to_set = 1 << (id % 32);
        set_mmio_bits(group_reg_addr, 0, bit_to_set);
        asm!("dsb sy", options(nostack, nomem));
    }
}
