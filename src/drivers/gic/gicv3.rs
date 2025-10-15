//! Generic interrupt controller (GICv3) module

use core::arch::asm;

use crate::utilities::io::{clear_mmio_bits, read_mmio, set_mmio_bits, write_mmio};

/* --- GICD (Distributor) Constants --- */
/// Distributor Control Register
const GICD_CTLR: usize = 0x000;
/// Enable non secure Group 1 interrupts bit
const GICD_CTLR_GRP1NS: u32 = 0b10;
/// Enable secure Group 1 interrupts bit
const GICD_CTLR_GRP1S: u32 = 0b100;
/// Affinity routing enable, secure state bit
const GICD_CTLR_ARE_S: u32 = 0b10000; // ARE_S (bit 4), for when DS=1
/// Interrupt Set-Enable Register
const GICD_ISENABLER: usize = 0x100;
/// Interrupt Priority Registers
const GICD_IPRIORITYR: usize = 0x400;
/// Interrupt Configuration Registers
const GICD_ICFGR: usize = 0xC00;
/// Interrupt Routing Registers
const GICD_IROUTER: usize = 0x6100;
/// Interrupt Group Registers
const GICD_IGROUPR: usize = 0x080;

/* --- GICR (Redistributor) Constants --- */
/// SGI Frame offset
const GICR_SGI_BASE: usize = 0x10000; // Offset from RD_base to SGI & PPI frame
/// Redistributor Wake Register
const GICR_WAKER: usize = 0x0014;
/// Processor sleep bit. Indicates whether the Redistributor can assert the **WakeRequest**
/// signal
const GICR_WAKER_PSLEEP: u32 = 0b10;
/// Children asleep bit. Indicates whether the connected PE is quiescent
const GICR_WAKER_CASLEEP: u32 = 0b100;
/// Interrupt Priority Registers
const GICR_IPRIORITYR: usize = 0x400;
/// Interrupt Group Register 0
const GICR_IGROUPR0: usize = 0x080;
/// Interrupt Set-Enable Register 0
const GICR_ISENABLER0: usize = 0x100;

/// Variable to hold if the affinity routing is enabled in the CPU
#[unsafe(no_mangle)]
pub static mut AFFINITY_ENABLED: bool = false;

/// Initializes the GIC Distributor
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

/// Initializes the GIC Redistributor
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

/// Sets an interrupt mask
///
/// Sets the interrupt mask `priority`. Interrupts with a higher priority than `priority` will be signaled to the PE
#[unsafe(no_mangle)]
pub unsafe fn set_priority_mask(priority: u8) {
    unsafe {
        asm!("msr ICC_PMR_EL1, {}", in(reg) priority as u64, options(nostack, nomem));
    }
}

/// Enable the group 0 interrupts
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

/// Enable the Group 1 interrupts
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

/// Sets the priority for the given interrupt
///
/// Sets the priority `prio` to the given interrupt `id`
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

/// Assign interrupts to the Group 1
///
/// Assigns the interrupt `id` to the Group 1
#[unsafe(no_mangle)]
pub unsafe fn set_int_grp(rd_base: usize, id: u32) {
    unsafe {
        set_mmio_bits(rd_base + GICR_SGI_BASE, GICR_IGROUPR0, 1 << id);
        asm!("dsb sy", options(nostack, nomem));
    }
}

/// Enables interrupt
///
/// Enables the interrupt with the given `id`
#[unsafe(no_mangle)]
pub unsafe fn enable_int(rd_base: usize, id: u32) {
    unsafe {
        set_mmio_bits(rd_base + GICR_SGI_BASE, GICR_ISENABLER0, 1 << id);
        asm!("dsb sy", options(nostack, nomem));
    }
}

/// Sets the priority of interrupts
///
/// Sets the priority `prio` to the interrupt `id`
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

/// Sets the trigger mode for the interrupt
///
/// Sets edge trigger or level sensitive mode for the interrupt `id`
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

/// Enables forwarding of the interrupt to the CPU interface
///
/// Enables forwarding of the interrupt `id` in the GIC distributor
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

/// Provides routing information for the SPI
///
/// When affinity routing is enabled, provides routing information for the SPI with id `id`. It
/// defines the routing mode by writting the value `core_affinity` into the corresponding register
#[unsafe(no_mangle)]
pub unsafe fn set_spi_routing(dist_base: usize, id: u32, core_affinity: u64) {
    unsafe {
        let router_reg_addr = dist_base + GICD_IROUTER + (8 * id as usize);
        let router_ptr = router_reg_addr as *mut u64;
        core::ptr::write_volatile(router_ptr, core_affinity);
        asm!("dsb sy", options(nostack, nomem));
    }
}

/// Assigns the SPI to the Group 1
///
/// Assigns the SPI `id` to the Group 1
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
