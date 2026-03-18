use core::arch::asm;
use core::ptr::{addr_of, addr_of_mut};

use crate::println;

use super::bits::*;
use super::pgtable::{
    L1_SIZE_PER_ENTRY, Pte, mark_block_desc, mark_table_desc, set_block_attrs, set_mair_range,
    set_next_lvl_table_addr, set_table_attrs,
};

unsafe extern "C" {
    static __kernel_start: u8;
    static __stack_top: u8;
static mut __idmap_l0: u8;
    static mut __idmap_l1: u8;
}

pub fn setup_identity_mapping() {
    // As kernel is mapped at 0x50000000, and MMIO is at 0x8000000-0x90000000, we use L0 and L1
    // descriptors, so we cover the entire space by using huge pages
    unsafe {
        // We will use those symbols to setup pages accordingly
        let kernel_code_start = addr_of!(__kernel_start) as usize;
        let kernel_stack_top = addr_of!(__stack_top) as usize;
        let kernel_addr_range = kernel_stack_top - kernel_code_start;
        // For now we hardcode mmio range
        let mmio_addr_range: usize = 0x09000000 - 0x08000000;
        let pages = (kernel_addr_range + mmio_addr_range) / L1_SIZE_PER_ENTRY;
        let idmap_l0_ptr = addr_of_mut!(__idmap_l0) as *mut u8;
        println!("idmap_l0 addr {:?}", idmap_l0_ptr);
        // 1. Mark the entry as Table Descriptor, it will cover 512 GiB
        mark_table_desc(idmap_l0_ptr as *mut Pte);
        // 2. Set attributes
        set_table_attrs(idmap_l0_ptr as *mut Pte, TABLE_UXNTABLE | TABLE_APTABLE0);
        let idmap_l1_ptr = addr_of_mut!(__idmap_l1) as *mut u8;
        println!("idmap_l1 addr {:?}", idmap_l1_ptr);
        // 3. Set next level entry
        set_next_lvl_table_addr(idmap_l0_ptr as *mut Pte, idmap_l1_ptr as *const u64);
        let ranges: [u64; 2] = [MAIR_IDX_DEVICE as u64, MAIR_IDX_NORMAL_WB as u64];
        let mut j = 0;
        // We will set up at least 2 ranges, (NGNRNE and CACHEABLE). If we have to use more
        // than two ranges, the remaining will use CACHEABLE range. When setting up the final
        // mapping, we will use each configured MAIR range
        for i in 0..pages.max(2) {
            let off = (idmap_l1_ptr as *mut Pte).offset(i as isize);
            // 0. Clear the descriptor
            *off = 0;
            // 1. Mark as block descriptor
            mark_block_desc(off);
            // 2. Setup MAIR range
            set_mair_range(off, ranges[j]);
            // 3. Set attributes
            if j == 0 {
                // MMIO desc
                set_block_attrs(
                    off,
                    DESC_UXN | DESC_PXN | DESC_AF | DESC_SH_NONE | DESC_AP_RW_EL1,
                );
            } else {
                // Normal desc
                set_block_attrs(off, DESC_UXN | DESC_AF | DESC_SH_INNER | DESC_AP_RW_EL1);
            }
            // 4. Set output address (identity map: VA = PA = i * 1 GiB)
            set_next_lvl_table_addr(off, (i * SZ_1G) as *const u64);
            if j < 1 {
                j += 1;
            }
        }
        load_ttbr0(idmap_l0_ptr as *const u64);
    }
    // We can now safely enable MMU
    enable_mmu();
}

#[inline(always)]
fn configure_tcr() {
    unsafe {
        asm!(
            "mov x0, #0x10",
            "mov x1, #0x1",
            "orr x0, x0, x1, LSL #8",
            "orr x0, x0, x1, LSL #10",
            "mov x1, #0x3",
            "orr x0, x0, x1, LSL #12",
            "mov x1, #0x10",
            "orr x0, x0, x1, LSL #16",
            "mov x1, #0x1",
            "orr x0, x0, x1, LSL #23",
            "orr x0, x0, x1, LSL #31",
            "orr x0, x0, x1, LSL #33",
            "msr tcr_el1, x0",
            "isb sy",
            options(nostack, nomem, preserves_flags)
        );
    }
}

#[inline(always)]
fn enable_mmu() {
    configure_tcr();
    unsafe {
        asm!(
            "mrs {tmp}, sctlr_el1",
            "orr {tmp}, {tmp}, {mmu_bit}",
            "msr sctlr_el1, {tmp}",
            "isb sy",
            mmu_bit = in(reg) SCTLR_ELX_MMU,
            tmp = out(reg) _,
            options(nostack, nomem, preserves_flags)
        );
    }
}

#[inline(always)]
fn load_ttbr0(base: *const u64) {
    unsafe {
        asm!(
            "msr ttbr0_el1, {tmp}",
            "isb sy",
            "tlbi vmalle1",
            "dsb nsh",
            "isb sy",
            tmp = in(reg) base,
            options(nostack, preserves_flags)
        );
    }
}
