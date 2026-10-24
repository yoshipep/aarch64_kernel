//! Page-table mapping API
//!
//! Builds mappings in a translation table hierarchy, from small single-purpose pieces up to whole ranges. Each layer
//! is made of the one below:
//!
//! - `read_entry`, `write_entry`: access one slot of a table page.
//! - `alloc_table`, `lookup_table`, `map_table`, `map_leaf`: one level, one job (read, allocate or write one entry).
//! - `get_or_alloc_table`: follow a table descriptor, creating the next table if it is missing.
//! - `map_page`, `map_pmd_block`, `map_pud_block`: walk down to the right level and install one leaf.
//! - `map_range`: map a whole range, choosing the largest block that fits at every step.
//!
//! The functions work on table pages through their physical address, so they run with the MMU off (early boot) and
//! never need the tables to be mapped anywhere. They know nothing about where a range comes from, so the kernel image
//! and, later, the linear map and `ioremap` can all build their mappings with `map_range`.

use crate::kernel::mm::frame_alloc;
use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::pgtable::{
    Descriptor, DescriptorType, LeafDescriptor, Pgd, Pmd, Pte, Pud, TableDescriptor, PAGE_SIZE, PMD_SIZE, PUD_SIZE,
};
use crate::kernel::mm::pgtable_hwdef::leaf::{Ap, Shareability};
use crate::kernel::mm::pgtable_hwdef::table;
use crate::kernel::phys_addr::PhysAddr;
use crate::kernel::virt_addr::VirtAddr;
use crate::utilities;

/// The attributes of a mapping that vary per region: memory type, shareability and permissions.
///
/// Applied by `map_leaf` to every leaf descriptor it writes.
#[derive(Clone, Copy)]
pub struct Prot {
    /// MAIR_EL1 slot: Normal write-back for RAM, Device for MMIO
    pub mair: MairIdx,
    /// Shareability domain of the mapped memory
    pub sh: Shareability,
    /// Read/write permission for EL1 and EL0
    pub ap: Ap,
    /// Extra attribute bits, e.g. `leaf::PXN | leaf::UXN | leaf::AF`
    pub attrs: u64,
}

/// Reads the `index`-th descriptor of the translation table at `table`.
///
/// The access is volatile: the table is also read by the hardware walker, so the compiler must not elide, merge or
/// reorder it. This is the one place where a table's physical address becomes a pointer to dereference.
///
/// # Arguments
///
/// * `table` - physical address of a table page whose entries have the type `D`
/// * `index` - entry number within the table, `0..PTRS_PER_TABLE` (usually `D::index(va)`)
///
/// # Returns
///
/// The descriptor stored in that slot, valid or not.
pub fn read_entry<D: Descriptor>(table: PhysAddr, index: usize) -> D {
    unsafe {
        let slot = table.as_mut_ptr::<D>().add(index);
        slot.read_volatile()
    }
}

/// Writes `desc` to the `index`-th slot of the translation table at `table`.
///
/// The access is volatile, for the same reason as `read_entry`. It does not issue any barrier: making the store
/// visible to the walker is up to the caller (`dsb ishst` once the table is complete).
///
/// # Arguments
///
/// * `table` - physical address of a table page whose entries have the type `D`
/// * `index` - entry number within the table, `0..PTRS_PER_TABLE` (usually `D::index(va)`)
/// * `desc` - the descriptor to store
pub fn write_entry<D: Descriptor>(table: PhysAddr, index: usize, desc: D) {
    unsafe {
        let slot = table.as_mut_ptr::<D>().add(index);
        core::ptr::write_volatile(slot, desc);
    }
}

/// Allocates a frame for a translation table and zeroes it, so every descriptor in it is invalid.
///
/// The frame allocator returns pages filled with a debug pattern, so the zeroing is not optional.
///
/// # Returns
///
/// The physical address of the new table page.
///
/// # Panics
///
/// If the frame allocator is out of frames.
pub fn alloc_table() -> PhysAddr {
    let table = frame_alloc::alloc();
    if table == PhysAddr::new(0) {
        panic!("alloc_table: out of frames for page tables");
    }

    unsafe {
        utilities::mem::memset(table.as_mut_ptr(), 0x0, PAGE_SIZE);
    }

    table
}

/// Returns the next-level table that `va` falls under, if the descriptor in `table` points to one.
///
/// Looks up `D`'s slot for `va` in `table`. Never allocates and never writes: a missing table is `None`.
///
/// # Arguments
///
/// * `table` - physical address of the current-level table (a `Pgd`, `Pud` or `Pmd` page, depending on `D`)
/// * `va` - the virtual address being looked up
///
/// # Returns
///
/// The physical address of the next-level table, or `None` if the descriptor is invalid.
///
/// # Panics
///
/// If the descriptor is a block, i.e. `va` is already covered by a larger mapping.
fn lookup_table<D: TableDescriptor>(table: PhysAddr, va: VirtAddr) -> Option<PhysAddr> {
    let descriptor: D = read_entry(table, D::index(va));
    if !descriptor.is_valid() {
        return None;
    }

    if descriptor.raw() & D::DESC_TYPE_MASK != DescriptorType::Table as u64 {
        panic!("lookup_table: block mapping where a table is expected");
    }

    Some(descriptor.next_table())
}

/// Installs a `Table` descriptor in `D`'s slot for `va`, pointing at the table page `next`.
///
/// # Arguments
///
/// * `table` - physical address of the current-level table (a `Pgd`, `Pud` or `Pmd` page, depending on `D`)
/// * `va` - the virtual address whose slot is written
/// * `next` - physical address of the next-level table page
fn map_table<D: TableDescriptor>(table: PhysAddr, va: VirtAddr, next: PhysAddr) {
    let mut desc = D::invalid();
    desc.set_type(DescriptorType::Table);
    // Every table built here is kernel-only: nothing below it may be accessed or executed from EL0.
    desc.set_attrs(table::UXNTABLE | table::APTABLE0);
    desc.set_output_address(next);
    write_entry(table, D::index(va), desc);
}

/// Installs a leaf descriptor in `D`'s slot for `va`, mapping `va` to `pa` with the attributes in `prot`.
///
/// The descriptor type comes from `D::LEAF_TYPE`: a `Block` at Pud and Pmd, a `Page` at Pte. `pa` must be aligned to
/// the size a `D` entry covers.
///
/// # Arguments
///
/// * `table` - physical address of the table whose entries are `D`
/// * `va` - the virtual address whose slot is written
/// * `pa` - physical address of the page or block it maps to
/// * `prot` - memory type, shareability and permissions
///
/// # Panics
///
/// If the slot already holds a valid descriptor.
fn map_leaf<D: LeafDescriptor>(table: PhysAddr, va: VirtAddr, pa: PhysAddr, prot: Prot) {
    debug_assert!(va.is_aligned(1 << D::SHIFT) && pa.is_aligned(1 << D::SHIFT));

    let index = D::index(va);
    let existing: D = read_entry(table, index);
    if existing.is_valid() {
        panic!("map_leaf: slot already mapped");
    }

    let mut leaf = D::invalid();
    leaf.set_type(D::LEAF_TYPE);
    leaf.set_shareability(prot.sh);
    leaf.set_ap(prot.ap);
    leaf.set_mair_range(prot.mair);
    leaf.set_attrs(prot.attrs);
    leaf.set_output_address(pa);
    write_entry(table, index, leaf);
}

/// Returns the physical address of the next-level table that `va` falls under, creating it first if needed.
///
/// A `lookup_table`, falling back to `alloc_table` and `map_table`.
///
/// # Arguments
///
/// * `table` - physical address of the current-level table (a `Pgd`, `Pud` or `Pmd` page, depending on `D`)
/// * `va` - the virtual address being mapped
fn get_or_alloc_table<D: TableDescriptor>(table: PhysAddr, va: VirtAddr) -> PhysAddr {
    if let Some(next) = lookup_table::<D>(table, va) {
        return next;
    }

    let next = alloc_table();
    map_table::<D>(table, va, next);
    next
}

/// Maps one 4 KiB page: `va` -> `pa`, creating any missing intermediate tables.
///
/// Walks `root` down to the Pte table for `va`, then installs a `Page` leaf there.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `va` - virtual address of the page, page aligned
/// * `pa` - physical address of the page it maps to, page aligned
/// * `prot` - memory type, shareability and permissions
fn map_page(root: PhysAddr, va: VirtAddr, pa: PhysAddr, prot: Prot) {
    let pud = get_or_alloc_table::<Pgd>(root, va);
    let pmd = get_or_alloc_table::<Pud>(pud, va);
    let pte = get_or_alloc_table::<Pmd>(pmd, va);
    map_leaf::<Pte>(pte, va, pa, prot);
}

/// Maps one 2 MiB block: `va` -> `pa`, creating any missing intermediate tables.
///
/// Walks `root` down to the Pmd table for `va`, then installs a `Block` leaf there.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `va` - virtual address of the block, 2 MiB aligned
/// * `pa` - physical address of the block it maps to, 2 MiB aligned
/// * `prot` - memory type, shareability and permissions
fn map_pmd_block(root: PhysAddr, va: VirtAddr, pa: PhysAddr, prot: Prot) {
    let pud = get_or_alloc_table::<Pgd>(root, va);
    let pmd = get_or_alloc_table::<Pud>(pud, va);
    map_leaf::<Pmd>(pmd, va, pa, prot);
}

/// Maps one 1 GiB block: `va` -> `pa`, creating any missing intermediate tables.
///
/// Walks `root` down to the Pud table for `va`, then installs a `Block` leaf there.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `va` - virtual address of the block, 1 GiB aligned
/// * `pa` - physical address of the block it maps to, 1 GiB aligned
/// * `prot` - memory type, shareability and permissions
fn map_pud_block(root: PhysAddr, va: VirtAddr, pa: PhysAddr, prot: Prot) {
    let pud = get_or_alloc_table::<Pgd>(root, va);
    map_leaf::<Pud>(pud, va, pa, prot);
}

/// Maps `size` bytes starting at `va` to `pa`, using the largest block that fits at every step.
///
/// At each position, if `va` and `pa` are both 1 GiB aligned and at least 1 GiB remains, it maps a 1 GiB block;
/// else, if both are 2 MiB aligned and at least 2 MiB remains, a 2 MiB block; else a 4 KiB page. So the unaligned
/// edges of a range are mapped with smaller entries and nothing is rounded away.
///
/// # Arguments
///
/// * `root` - physical address of the Pgd table
/// * `va` - virtual address of the start of the range, page aligned
/// * `pa` - physical address of the start of the range, page aligned
/// * `size` - length of the range in bytes, a multiple of the page size
/// * `prot` - memory type, shareability and permissions, the same for the whole range
///
/// # Panics
///
/// If any page in the range is already mapped.
pub fn map_range(root: PhysAddr, va: VirtAddr, pa: PhysAddr, size: u64, prot: Prot) {
    debug_assert!(va.is_aligned(PAGE_SIZE as u64) && pa.is_aligned(PAGE_SIZE as u64));
    debug_assert!(size % PAGE_SIZE as u64 == 0);

    let mut off = 0;
    while off < size {
        let v = va + off;
        let p = pa + off;
        let left = size - off;

        // Largest block that fits: a block needs both addresses aligned to its size and at least that much left.
        // Anything that doesn't fit falls through to the next smaller size, so the unaligned edges of the range end
        // up as pages and nothing is rounded away.
        if v.is_aligned(PUD_SIZE as u64) && p.is_aligned(PUD_SIZE as u64) && left >= PUD_SIZE as u64 {
            map_pud_block(root, v, p, prot);
            off += PUD_SIZE as u64;
        } else if v.is_aligned(PMD_SIZE as u64) && p.is_aligned(PMD_SIZE as u64) && left >= PMD_SIZE as u64 {
            map_pmd_block(root, v, p, prot);
            off += PMD_SIZE as u64;
        } else {
            map_page(root, v, p, prot);
            off += PAGE_SIZE as u64;
        }
    }
}
