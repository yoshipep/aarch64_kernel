//! Identity mapping and MMU bring-up
//!
//! Builds a temporary identity map (VA = PA) covering the kernel image and MMIO, then enables the MMU. The kernel is
//! linked at a physical address, so keeping VA == PA lets execution continue unchanged across the MMU-enable transition
//! — no relocation needed yet. This map is torn down later once a proper high-half kernel mapping takes over.

use core::arch::asm;
use core::ptr::addr_of_mut;

use crate::kernel::mm::frame_alloc;
use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::pgtable::{
    Descriptor, DescriptorType, PUD_SIZE, PAGE_SIZE, Pgd, Pmd, Pte, Pud, TableDescriptor,
    kimage_va,
};
use crate::kernel::phys_addr::PhysAddr;
use crate::kernel::sysreg::{sctlr, tcr};
use crate::{println, utilities};

use super::pgtable::LeafDescriptor;
use super::pgtable_hwdef::*;

unsafe extern "C" {
    static mut __kernel_start: u8;
    static mut __text_start: u8;
    static mut __text_end: u8;
    static mut __rodata_start: u8;
    static mut __rodata_end: u8;
    static mut __data_start: u8;
    static mut __data_end: u8;
    static mut __idmap_l0: u8;
    static mut __idmap_l1: u8;
    static mut __kernel_end: u8;
    static mut __stack_top: u8;
}

/// Physical address of a linker symbol. The kernel is linked at its physical load address, so the symbol's value is
/// already its PA.
fn sym_pa(sym: *mut u8) -> PhysAddr {
    PhysAddr::new(sym as u64)
}

/// Returns the physical address of the next-level table that `va` falls under, creating it first if needed.
///
/// Looks up `D`'s slot for `va` in `table`. If that descriptor is invalid, allocates and zeroes a new page, installs a
/// `Table` descriptor pointing at it, and returns it; if it's already valid, returns the table it points at.
///
/// # Arguments
///
/// * `table` - physical address of the current-level table (a `Pgd`, `Pud` or `Pmd` page, depending on `D`)
/// * `va` - the virtual address being mapped
fn get_or_alloc_table<D: TableDescriptor>(table: PhysAddr, va: u64) -> PhysAddr {
    unsafe {
        // Compute the slot into pmd table
        let slot = table.as_mut_ptr::<D>().add(D::index(va));
        // Extract the descriptor from the pointed addr. Read it as volatile
        let descriptor = slot.read_volatile();
        // If it is invalid means this ptedir has not been allocated so we do it
        if !descriptor.is_valid() {
            let page = frame_alloc::alloc();
            if page == PhysAddr::new(0) {
                panic!("get_or_alloc_table: out of frames for page tables");
            }
            utilities::mem::memset(page.as_mut_ptr(), 0x0, PAGE_SIZE);

            let mut next_lvl_desc = D::invalid();
            next_lvl_desc.set_type(DescriptorType::Table);
            next_lvl_desc.set_attrs(table::UXNTABLE | table::APTABLE0);
            next_lvl_desc.set_output_address(page);
            core::ptr::write_volatile(slot, next_lvl_desc);

            return page;
        }

        descriptor.output_address()
    }
}

/// Maps one 4 KiB page: `va` -> `pa`, creating any missing intermediate tables.
///
/// Walks `root` down to the Pte table for `va` (`get_or_alloc_table` at the Pgd, Pud and Pmd levels), then writes the
/// leaf `Pte`. The memory type (`NormalWb`) and shareability (`Inner`) are the same for every kernel region, so they
/// are fixed here; only the permissions vary per call.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `va` - virtual address of the page
/// * `pa` - physical address of the page it maps to
/// * `ap` - read/write permission for EL1/EL0
/// * `attrs` - extra attribute bits, e.g. `leaf::PXN | leaf::UXN | leaf::AF`
fn map_page(root: PhysAddr, va: u64, pa: PhysAddr, ap: leaf::Ap, attrs: u64) {
    // Perform the page walk and create intermediate pages if not exist
    let pud_table = get_or_alloc_table::<Pgd>(root, va);
    let pmd_table = get_or_alloc_table::<Pud>(pud_table, va);
    let pte_table = get_or_alloc_table::<Pmd>(pmd_table, va);
    unsafe {
        let slot = pte_table.as_mut_ptr::<Pte>().add(Pte::index(va));
        let descriptor = slot.read_volatile();

        // If the pte returned is not invalid that means there is something mapped there, just
        // panic
        if descriptor.is_valid() {
            panic!("map_page: page already mapped");
        }

        // Build the pte entry and write it in the table
        let mut pte = Pte::invalid();
        pte.set_type(DescriptorType::Page);
        pte.set_shareability(leaf::Shareability::Inner);
        pte.set_ap(ap);
        pte.set_mair_range(MairIdx::NormalWb);
        pte.set_attrs(attrs);
        pte.set_output_address(pa);
        core::ptr::write_volatile(slot, pte);
    }
}

/// Maps every page in `[start, end)` of the kernel image into the high half, `kimage_va(pa)` -> `pa`.
///
/// `start` is rounded down and `end` up to page boundaries, so a region whose linker symbols aren't page-aligned
/// (e.g. the end of `.text`) still covers its last partial page. The virtual address is derived from each page's
/// physical address here, so callers can't pass a mismatched pair.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `start` - physical address of the first byte of the region
/// * `end` - physical address one past the last byte of the region
/// * `ap` - read/write permission for EL1/EL0
/// * `attrs` - extra attribute bits, e.g. `leaf::PXN | leaf::UXN | leaf::AF`
fn map_range(root: PhysAddr, start: PhysAddr, end: PhysAddr, ap: leaf::Ap, attrs: u64) {
    let mut page = start.align_down(PAGE_SIZE as u64);
    while page < end {
        let vaddr = kimage_va(page);
        map_page(root, vaddr, page, ap, attrs);

        page = page + PAGE_SIZE as u64;
    }
}

/// Builds the TTBR1 page tables that map the kernel image at `KIMAGE_VADDR`, and returns the root table.
///
/// Allocates and zeroes a Pgd, then maps each region page by page with its own permissions: `.text` read-only and
/// executable by EL1, `.rodata` read-only, `.data`/`.bss` and the stack read-write, all non-executable. The page
/// between `__kernel_end` and the stack is left unmapped as a guard page. Runs with the MMU off, so tables are written
/// through physical addresses. Ends with a `dsb ishst` so the table writes are complete before the root is published
/// by `load_ttbr1`.
///
/// # Returns
///
/// Physical address of the Pgd, ready for `load_ttbr1`.
///
/// # Panics
///
/// If the frame allocator runs out of pages for the tables.
fn map_kimage() -> PhysAddr {
    let pgdir = frame_alloc::alloc(); // 512 GiB
    if pgdir == PhysAddr::new(0) {
        panic!("map_kimage: out of frames for the TTBR1 root table");
    }

    unsafe {
        utilities::mem::memset(pgdir.as_mut_ptr(), 0x0, PAGE_SIZE);
    }

    // 1. Build the walk for text pages
    map_range(
        pgdir,
        sym_pa(addr_of_mut!(__text_start)),
        sym_pa(addr_of_mut!(__text_end)),
        leaf::Ap::RoEl1,
        leaf::UXN | leaf::AF,
    );

    // 2. Build the walk for rodata pages
    map_range(
        pgdir,
        sym_pa(addr_of_mut!(__rodata_start)),
        sym_pa(addr_of_mut!(__rodata_end)),
        leaf::Ap::RoEl1,
        leaf::PXN | leaf::UXN | leaf::AF,
    );

    // 3. Build the walk for data pages
    map_range(
        pgdir,
        sym_pa(addr_of_mut!(__data_start)),
        sym_pa(addr_of_mut!(__data_end)),
        leaf::Ap::RwEl1,
        leaf::PXN | leaf::UXN | leaf::AF,
    );

    // 4. Build the walk for stack pages. We start a page after __kernel_end so we leave a guard
    //    page in between data and stack sections
    map_range(
        pgdir,
        sym_pa(addr_of_mut!(__kernel_end)) + PAGE_SIZE as u64,
        sym_pa(addr_of_mut!(__stack_top)),
        leaf::Ap::RwEl1,
        leaf::PXN | leaf::UXN | leaf::AF,
    );

    let first_va = kimage_va(sym_pa(addr_of_mut!(__kernel_start)));
    println!(
        "kimage tables: root {:#X}, image VA {:#X} (Pgd {:#X}, Pud {}, Pmd {}, Pte {})",
        pgdir,
        first_va,
        Pgd::index(first_va),
        Pud::index(first_va),
        Pmd::index(first_va),
        Pte::index(first_va),
    );

    unsafe {
        asm!("dsb ishst", options(nostack, preserves_flags));
    }

    pgdir
}

/// Builds the identity page tables and enables the MMU
///
/// Maps the first 2 GiB of physical address space 1:1 (VA = PA) using two 1 GiB block descriptors: `[0, 1 GiB)` as
/// Device-nGnRE (covers the GIC/UART MMIO ranges) and `[1 GiB, 2 GiB)` as Normal write-back cacheable (covers the
/// kernel image and stack, linked at `0x50000000`). Assumes the kernel and all MMIO used before this call fit within
/// that layout — see `PUD_SIZE`.
pub fn setup_identity_mapping() {
    // As kernel is mapped at 0x50000000, and MMIO is at 0x8000000-0x90000000, we use L0 and L1
    // descriptors, so we cover the entire space by using huge pages
    unsafe {
        let idmap_pgd_ptr = PhysAddr::new(addr_of_mut!(__idmap_l0) as u64);
        println!("idmap_pgd addr {:#X}", idmap_pgd_ptr);

        let mut pgd = Pgd::invalid();

        // 1. Mark the entry as Table Descriptor, it will cover 512 GiB
        pgd.set_type(DescriptorType::Table);

        // 2. Set attributes
        pgd.set_attrs(table::UXNTABLE | table::APTABLE0);

        // 3. Set next level entry
        let idmap_pud_ptr = PhysAddr::new(addr_of_mut!(__idmap_l1) as u64);
        println!("idmap_pud addr {:#X}", idmap_pud_ptr);
        pgd.set_output_address(idmap_pud_ptr);
        *idmap_pgd_ptr.as_mut_ptr() = pgd.raw();

        // We will set up at least 2 ranges, (NGNRNE and CACHEABLE). If we have to use more
        // than two ranges, the remaining will use CACHEABLE range. When setting up the final
        // mapping, we will use each configured MAIR range
        /* Descriptor for device memory */
        let table = idmap_pud_ptr.as_mut_ptr::<Pud>();
        let mut off = table.add(0);
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
        pud_dev.set_output_address(PhysAddr::new(0));
        *off = pud_dev;

        /* Descriptor for normal memory */
        off = table.add(1);
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
        pud_nwb.set_output_address(PhysAddr::new(PUD_SIZE as u64));
        *off = pud_nwb;

        load_ttbr0(idmap_pgd_ptr);
        load_ttbr1(map_kimage());
    }

    // We can now safely enable MMU
    enable_mmu();
}

/// Writes the Translation Control Register (TCR_EL1)
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

/// Configures TCR_EL1 for the identity map, then sets SCTLR_EL1.M to turn the MMU on
#[inline(always)]
fn enable_mmu() {
    configure_tcr(
        tcr::T0SZ_48
            | tcr::IRGN0_WBWA
            | tcr::ORGN0_WBWA
            | tcr::SH0_INNER
            | tcr::TG0_4K
            | tcr::T1SZ_48
            | tcr::IRGN1_WBWA
            | tcr::ORGN1_WBWA
            | tcr::SH1_INNER
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

/// Loads TTBR0_EL1 with the given page-table base address and invalidates stale TLB entries
#[inline(always)]
fn load_ttbr0(base: PhysAddr) {
    unsafe {
        asm!(
            "msr ttbr0_el1, {tmp}",
            "isb sy",
            "tlbi vmalle1",
            "dsb nsh",
            "isb sy",
            tmp = in(reg) base.as_u64(),
            options(nostack, preserves_flags)
        );
    }
}

/// Loads TTBR1_EL1 with the given page-table base address and invalidates stale TLB entries
#[inline(always)]
fn load_ttbr1(base: PhysAddr) {
    unsafe {
        asm!(
            "msr ttbr1_el1, {tmp}",
            "isb sy",
            "tlbi vmalle1",
            "dsb nsh",
            "isb sy",
            tmp = in(reg) base.as_u64(),
            options(nostack, preserves_flags)
        );
    }
}
