use core::arch::asm;
use core::ptr::addr_of_mut;

use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::pgtable::{Descriptor, DescriptorType, L1_SIZE_PER_ENTRY, Pgd, Pud};
use crate::kernel::sysreg::{sctlr, tcr};
use crate::println;

use super::pgtable::LeafDescriptor;
use super::pgtable_hwdef::*;

unsafe extern "C" {
    static mut __idmap_l0: u8;
    static mut __idmap_l1: u8;
}

pub fn setup_identity_mapping() {
    // As kernel is mapped at 0x50000000, and MMIO is at 0x8000000-0x90000000, we use L0 and L1
    // descriptors, so we cover the entire space by using huge pages
    unsafe {
        let idmap_pgd_ptr = addr_of_mut!(__idmap_l0) as *mut u64;
        println!("idmap_pgd addr {:?}", idmap_pgd_ptr);

        let mut pgd = Pgd::invalid();

        // 1. Mark the entry as Table Descriptor, it will cover 512 GiB
        pgd.set_type(DescriptorType::Table);

        // 2. Set attributes
        pgd.set_attrs(table::UXNTABLE | table::APTABLE0);

        // 3. Set next level entry
        let idmap_pud_ptr = addr_of_mut!(__idmap_l1) as *mut u64;
        println!("idmap_pud addr {:?}", idmap_pud_ptr);
        pgd.set_output_address(idmap_pud_ptr as u64);
        *idmap_pgd_ptr = pgd.raw();

        // We will set up at least 2 ranges, (NGNRNE and CACHEABLE). If we have to use more
        // than two ranges, the remaining will use CACHEABLE range. When setting up the final
        // mapping, we will use each configured MAIR range
        /* Descriptor for device memory */
        let mut off = idmap_pud_ptr.offset(0);
        let mut pud_dev = Pud::invalid();

        // 1.1 Mark as block descriptor
        pud_dev.set_type(DescriptorType::Block);

        // 2.1 Setup MAIR range
        pud_dev.set_mair_range(MairIdx::Device);

        // 3.1 Set attributes
        pud_dev.set_attrs(leaf::UXN | leaf::PXN | leaf::AF);
        pud_dev.set_shareability(leaf::Shareability::NonShareable);
        pud_dev.set_ap(leaf::Ap::RwEl1);

        // 4.1 Set output address (identity map: VA = PA = n * 1 GiB)
        pud_dev.set_output_address(0);
        *off = pud_dev.raw();

        /* Descriptor for normal memory */
        off = idmap_pud_ptr.offset(1);
        let mut pud_nwb = Pud::invalid();

        // 1.2 Mark as block descriptor
        pud_nwb.set_type(DescriptorType::Block);

        // 2.2 Setup MAIR range
        pud_nwb.set_mair_range(MairIdx::NormalWb);

        // 3.2 Set attributes
        pud_nwb.set_attrs(leaf::UXN | leaf::AF);
        pud_nwb.set_shareability(leaf::Shareability::Inner);
        pud_nwb.set_ap(leaf::Ap::RwEl1);

        // 4.2 Set output address (identity map: VA = PA = i * 1 GiB)
        pud_nwb.set_output_address(L1_SIZE_PER_ENTRY as u64);
        *off = pud_nwb.raw();

        load_ttbr0(idmap_pgd_ptr as u64);
    }
    // We can now safely enable MMU
    enable_mmu();
}

#[inline(always)]
fn configure_tcr(tcr: u64) {
    unsafe {
        asm!(
            "msr tcr_el1, {tcr}",
            "isb sy",
            tcr = in(reg) tcr,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
fn enable_mmu() {
    configure_tcr(
        tcr::T0SZ_48
            | tcr::IRGN0_WBWA
            | tcr::ORGN0_WBWA
            | tcr::SH0_INNER
            | tcr::TG0_4K
            | tcr::T1SZ_48
            | tcr::EPD1
            | tcr::TG1_4K
            | tcr::IPS_44,
    );
    unsafe {
        asm!(
            "mrs {tmp}, sctlr_el1",
            "orr {tmp}, {tmp}, {mmu_bit}",
            "msr sctlr_el1, {tmp}",
            "isb sy",
            mmu_bit = in(reg) sctlr::MMU,
            tmp = out(reg) _,
            options(nostack, preserves_flags)
        );
    }
}

#[inline(always)]
fn load_ttbr0(base: u64) {
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
