//! Identity mapping, kernel page tables and MMU bring-up
//!
//! Builds the page tables the kernel boots with and enables the MMU. TTBR0 holds a temporary identity map (VA = PA)
//! covering the kernel image and MMIO: the kernel is linked at a physical address, so keeping VA == PA lets execution
//! continue unchanged across the MMU-enable transition. TTBR1 holds the kernel image at `KIMAGE_VADDR` with its final
//! permissions (`map_kimage`) and the linear map of RAM at `PAGE_OFFSET` (`map_linear`). The identity map is torn down
//! later, once the kernel runs entirely from the high half.

use core::arch::asm;
use core::ptr::addr_of_mut;

use crate::kernel::meminfo;
use crate::kernel::mm::frame_alloc;
use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::map::{self, map_range, write_entry};
use crate::kernel::mm::pgtable::{
    kimage_va, phys_ptr, phys_to_virt, set_linear_map_live, Descriptor, DescriptorType, Pgd, Pmd, Pte, Pud, PAGE_SIZE,
    PUD_SIZE,
};
use crate::kernel::phys_addr::PhysAddr;
use crate::kernel::sysreg::{sctlr, tcr};
use crate::kernel::virt_addr::VirtAddr;
use crate::pr_info;

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

/// Maps `[start, end)` of the kernel image into the high half, `kimage_va(pa)` -> `pa`, with the permissions in `prot`.
///
/// Both ends are rounded outwards to page boundaries, so a region whose linker symbols aren't page-aligned (e.g. the
/// end of `.text`) still covers its last partial page. The virtual address is derived from `start` here, so callers
/// can't pass a mismatched pair.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `start` - physical address of the first byte of the region
/// * `end` - physical address one past the last byte of the region
/// * `prot` - memory type, shareability and permissions
fn map_kimage_region(root: PhysAddr, start: PhysAddr, end: PhysAddr, prot: map::Prot) {
    let start = start.align_down(PAGE_SIZE as u64);
    let end = end.align_up(PAGE_SIZE as u64);
    map::map_range(root, kimage_va(start), start, end - start, prot);
}

/// Builds the TTBR1 page tables that map the kernel image at `KIMAGE_VADDR`, and returns the root table.
///
/// Allocates and zeroes a Pgd, then maps each region with `map::map_range` (largest block that fits) and its own
/// permissions: `.text` read-only and executable by EL1, `.rodata` read-only, `.data`/`.bss` and the stack read-write,
/// all non-executable. Each region is rounded outwards to whole pages first. The page
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
    let kernel_pgd = map::alloc_table();

    // 1. Build the walk for text pages
    let mut prot = map::Prot {
        ap: leaf::Ap::RoEl1,
        mair: MairIdx::NormalWb,
        sh: leaf::Shareability::Inner,
        attrs: leaf::UXN | leaf::AF,
    };

    map_kimage_region(
        kernel_pgd,
        sym_pa(addr_of_mut!(__text_start)),
        sym_pa(addr_of_mut!(__text_end)),
        prot,
    );

    // 2. Build the walk for rodata pages
    prot.attrs = leaf::PXN | leaf::UXN | leaf::AF;

    map_kimage_region(
        kernel_pgd,
        sym_pa(addr_of_mut!(__rodata_start)),
        sym_pa(addr_of_mut!(__rodata_end)),
        prot,
    );

    // 3. Build the walk for data pages
    prot.ap = leaf::Ap::RwEl1;

    map_kimage_region(
        kernel_pgd,
        sym_pa(addr_of_mut!(__data_start)),
        sym_pa(addr_of_mut!(__data_end)),
        prot,
    );

    // 4. Build the walk for stack pages. We start a page after __kernel_end so we leave a guard
    //    page in between data and stack sections
    map_kimage_region(
        kernel_pgd,
        sym_pa(addr_of_mut!(__kernel_end)) + PAGE_SIZE as u64,
        sym_pa(addr_of_mut!(__stack_top)),
        prot,
    );

    let first_va = kimage_va(sym_pa(addr_of_mut!(__kernel_start)));
    pr_info!(
        "kimage",
        "tables: root {:#X}, image VA {:#X} (Pgd {:#X}, Pud {}, Pmd {}, Pte {})",
        kernel_pgd,
        first_va,
        Pgd::index(first_va),
        Pud::index(first_va),
        Pmd::index(first_va),
        Pte::index(first_va),
    );

    unsafe {
        asm!("dsb ishst", options(nostack, preserves_flags));
    }

    kernel_pgd
}

/// Maps all of RAM into the linear map at `PAGE_OFFSET`, `phys_to_virt(pa)` -> `pa`, in the TTBR1 tables `root`.
///
/// The range is the DTB `/memory` one (`meminfo::ram_range`), mapped Normal write-back, inner shareable and never
/// executable, with `map::map_range`. It is mapped exactly, never rounded up to a block: a block bigger than RAM would
/// map non-RAM as cacheable Normal memory, which the CPU may speculatively read.
///
/// It is built as three disjoint ranges: the RAM below the image and the RAM above it, read/write for EL1, and the
/// image's `.text` and `.rodata` (rounded out to pages), read-only. Nothing patches code, so the image's alias has no
/// reason to be writable, which would defeat W^X. The split is also what keeps the option open: the alias lives in
/// entries of its own (pages, plus any 2 MiB block that lies fully inside it), so its permissions can change later by
/// rewriting those entries, without splitting a block that also covers other RAM. `map_range`'s largest-fit rule never
/// lets a block cross the end of a range.
///
/// Only RAM is mapped here. MMIO belongs to `ioremap`, and an access to anything else through the linear range is a
/// bug that faults, not something to demand-map.
///
/// TODO: exclude the DTB `no-map` reserved-memory regions. Plain reserved regions (kernel image, DTB, initrd) stay
/// mapped.
///
/// # Arguments
///
/// * `root` - physical address of the TTBR1 Pgd table, the one `map_kimage` built
fn map_linear(root: PhysAddr) {
    // Get the ram range and align it to page boundary
    let (ram_start, size) = meminfo::ram_range();
    let ram_start = ram_start.align_up(PAGE_SIZE as u64);

    // Rounding it up could map a partial page beyond what the DTB says is RAM.
    let ram_end = (ram_start + size as u64).align_down(PAGE_SIZE as u64);

    let text_start = sym_pa(addr_of_mut!(__text_start)).align_down(PAGE_SIZE as u64);
    let rodata_end = sym_pa(addr_of_mut!(__rodata_end)).align_up(PAGE_SIZE as u64);

    let mut prot = map::Prot {
        ap: leaf::Ap::RwEl1,
        mair: MairIdx::NormalWb,
        sh: leaf::Shareability::Inner,
        attrs: leaf::PXN | leaf::UXN | leaf::AF,
    };

    // 1. Map from ram_start to __text_start (before kernel image)
    map_range(root, phys_to_virt(ram_start), ram_start, text_start - ram_start, prot);

    // 2. Map from __rodata_end to ram_end (past kernel image)
    map_range(root, phys_to_virt(rodata_end), rodata_end, ram_end - rodata_end, prot);

    // 3. Map from __text_start to __rodata_end (kernel image)
    prot.ap = leaf::Ap::RoEl1;
    map_range(
        root,
        phys_to_virt(text_start),
        text_start,
        rodata_end - text_start,
        prot,
    );

    unsafe {
        asm!("dsb ishst", options(nostack, preserves_flags));
    }
}

/// Compares the word at `pa` read through the identity map with the same word read through its linear alias.
///
/// Read-only, so it works on any RAM, including the read-only alias of `.text` and `.rodata`. `what` names the
/// location in the panic message.
fn check_linear_word(pa: PhysAddr, what: &str) {
    let linear = phys_to_virt(pa).as_ptr::<u64>();
    unsafe {
        if pa.as_ptr::<u64>().read_volatile() != linear.read_volatile() {
            panic!("check_linear_mapping: {what} differs between the physical address and the linear alias");
        }
    }
}

/// Checks that the linear map reaches the same frames as the identity map.
///
/// Must run after the MMU is on, `TTBR1` is loaded and `set_linear_map_live` has been called. First compares the word
/// at the edges of the three ranges `map_linear` builds (the start of RAM, either side of the image, the end of RAM)
/// through both addresses. Then takes a free frame, writes a value through its physical address and reads it back
/// through `phys_to_virt(pa)`, and does the opposite. Reports the result with `pr_info!("linear", ...)`; a missing
/// mapping shows up as a data abort instead of a mismatch.
pub fn check_linear_mapping() {
    let (ram_start, ram_size) = meminfo::ram_range();
    let ram_end = ram_start + ram_size as u64;
    let text_start = sym_pa(addr_of_mut!(__text_start)).align_down(PAGE_SIZE as u64);
    let rodata_end = sym_pa(addr_of_mut!(__rodata_end)).align_up(PAGE_SIZE as u64);

    check_linear_word(ram_start, "the start of RAM");
    check_linear_word(text_start - 8, "the last word below the image");
    check_linear_word(text_start, "the start of .text");
    check_linear_word(rodata_end - 8, "the last word of the image alias");
    check_linear_word(rodata_end, "the first word above the image");
    check_linear_word(ram_end - 8, "the end of RAM");

    let pa_frame = frame_alloc::alloc();
    if pa_frame == PhysAddr::new(0) {
        panic!("check_linear_mapping: out of frames");
    }

    unsafe {
        let linear_frame = phys_to_virt(pa_frame).as_mut_ptr::<u64>();
        if phys_ptr::<u64>(pa_frame) != linear_frame {
            panic!("check_linear_mapping: phys_ptr does not return the linear alias after set_linear_map_live");
        }

        core::ptr::write_volatile(pa_frame.as_mut_ptr::<u64>(), 0xBEBECAFE);
        if linear_frame.read_volatile() != 0xBEBECAFE {
            panic!("check_linear_mapping: write through the physical address not seen through the linear alias");
        }

        if pa_frame.as_ptr::<u64>().read_volatile() != linear_frame.read_volatile() {
            panic!("check_linear_mapping: frame differs between the physical address and linear map addresses");
        }

        core::ptr::write_volatile(linear_frame, 0xCAFEBEBE);
        if pa_frame.as_ptr::<u64>().read_volatile() != linear_frame.read_volatile() {
            panic!("check_linear_mapping: frame differs between the physical address and linear map addresses");
        }
    }

    frame_alloc::free(pa_frame);

    pr_info!(
        "linear",
        "read-back OK (range edges and a free frame via the identity and linear aliases)"
    );
}

/// Checks that the high-half image mapping translates and reaches the same frames as the identity map.
///
/// Must run after the MMU is on and `TTBR1` is loaded. Compares the first word of `.text` and of `.rodata` read
/// through `kimage_va(pa)` with the same word read at `pa`, then writes a value through the high alias of a spare
/// `.data` word and reads it back through its identity address. Reports the result with `pr_info!("kimage", ...)`; a
/// missing mapping shows up as a data abort instead of a mismatch.
pub fn check_kimage_mapping() {
    let id_text = PhysAddr::new(addr_of_mut!(__text_start) as u64);
    let hi_text = kimage_va(id_text).as_mut_ptr::<u64>();
    unsafe {
        if id_text.as_ptr::<u64>().read_volatile() != hi_text.read_volatile() {
            panic!("check_kimage_mapping: .text differs between the identity and high addresses");
        }
    }

    let id_rodata = PhysAddr::new(addr_of_mut!(__rodata_start) as u64);
    let hi_rodata = kimage_va(id_rodata).as_mut_ptr::<u64>();
    unsafe {
        if id_rodata.as_ptr::<u64>().read_volatile() != hi_rodata.read_volatile() {
            panic!("check_kimage_mapping: .rodata differs between the identity and high addresses");
        }
    }

    let id_data = PhysAddr::new(addr_of_mut!(__data_start) as u64);
    let hi_data = kimage_va(id_data).as_mut_ptr::<u64>();
    unsafe {
        if id_data.as_ptr::<u64>().read_volatile() != hi_data.read_volatile() {
            panic!("check_kimage_mapping: .data differs between the identity and high addresses");
        }

        let hi_val = hi_data.read_volatile();
        core::ptr::write_volatile(hi_data, 0xBEBECAFE);
        if id_data.as_ptr::<u64>().read_volatile() != 0xBEBECAFE {
            panic!("check_kimage_mapping: write through the high alias not seen at the identity address");
        }

        core::ptr::write_volatile(hi_data, hi_val);

        let id_val = id_data.as_ptr::<u64>().read_volatile();
        core::ptr::write_volatile(id_data.as_mut_ptr::<u64>(), 0xBEBECAFE);
        if hi_data.read_volatile() != 0xBEBECAFE {
            panic!("check_kimage_mapping: write through the identity alias not seen at the high address");
        }

        core::ptr::write_volatile(id_data.as_mut_ptr::<u64>(), id_val);
    }

    pr_info!(
        "kimage",
        "read-back OK (text, rodata, data via the high and identity aliases)"
    );
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
    let idmap_pgd_ptr = PhysAddr::new(addr_of_mut!(__idmap_l0) as u64);
    pr_info!("idmap", "pgd at {:#X}", idmap_pgd_ptr);

    let mut pgd = Pgd::invalid();

    // 1. Mark the entry as Table Descriptor, it will cover 512 GiB
    pgd.set_type(DescriptorType::Table);

    // 2. Set attributes
    pgd.set_attrs(table::UXNTABLE | table::APTABLE0);

    // 3. Set next level entry
    let idmap_pud_ptr = PhysAddr::new(addr_of_mut!(__idmap_l1) as u64);
    pr_info!("idmap", "pud at {:#X}", idmap_pud_ptr);
    pgd.set_output_address(idmap_pud_ptr);
    write_entry(idmap_pgd_ptr, Pgd::index(VirtAddr::new(0)), pgd);

    // We will set up at least 2 ranges, (NGNRNE and CACHEABLE). If we have to use more
    // than two ranges, the remaining will use CACHEABLE range. When setting up the final
    // mapping, we will use each configured MAIR range
    /* Descriptor for device memory */
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
    write_entry(idmap_pud_ptr, Pud::index(VirtAddr::new(0)), pud_dev);

    /* Descriptor for normal memory */
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
    let ram_base = PUD_SIZE as u64;
    pud_nwb.set_output_address(PhysAddr::new(ram_base));
    write_entry(idmap_pud_ptr, Pud::index(VirtAddr::new(ram_base)), pud_nwb);

    load_ttbr0(idmap_pgd_ptr);
    let kernel_pgd = map_kimage();
    map_linear(kernel_pgd);
    load_ttbr1(kernel_pgd);

    // We can now safely enable MMU
    enable_mmu();

    // Turn on the linear map
    set_linear_map_live();
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
