use core::ptr::addr_of_mut;

use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::pgtable_hwdef::leaf::{Ap, Shareability};
use crate::kernel::phys_addr::PhysAddr;

unsafe extern "C" {
    static mut __kernel_start: u8;
}

/// Width of a virtual address: 48 bits, so the TTBR0 region is `[0, 2^48)` and the TTBR1 region is
/// `[0xFFFF_0000_0000_0000, 2^64)`. Must match `T0SZ`/`T1SZ` in TCR_EL1 (`64 - VA_BITS = 16`).
pub const VA_BITS: usize = 48;

/// Virtual address where the kernel image starts in the TTBR1 (high) half. The image is mapped contiguously from here,
/// so `kimage_va(pa)` is `KIMAGE_VADDR` plus the offset from `__kernel_start`.
pub const KIMAGE_VADDR: u64 = 0xFFFF_8000_0000_0000;

// Output-address (physical) field width in a descriptor. Distinct from VA_BITS: it lives on the
// output side of translation and diverges from VA_BITS under LPA2 (52-bit). 4KB granule, non-LPA2.
const OA_BITS: usize = 48;

/// log2 of the page size (4 KiB granule).
pub const PAGE_SHIFT: usize = 12;

/// Size of a page in bytes (4 KiB).
pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

/// Mask that clears the offset within a page: `addr & PAGE_MASK` rounds `addr` down to a page boundary.
pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

/// Entries per translation table: one page of 8-byte descriptors (512).
pub const PTRS_PER_TABLE: usize = PAGE_SIZE / size_of::<u64>();

/// Index bits per level, log2 of `PTRS_PER_TABLE` (9). Each level's `SHIFT` is the one below plus this.
pub const TABLE_SHIFT: usize = PTRS_PER_TABLE.ilog2() as usize;

/// Mask of the output-address field in a descriptor: bits \[47:12\], i.e. `OA_BITS` wide and page-aligned. The bits
/// above and below hold attributes.
pub const ADDR_MASK: u64 = ((1u64 << OA_BITS) - 1) & !(PAGE_SIZE as u64 - 1);

/// Bit position of the Pud index in a virtual address (30); same value as `Pud::SHIFT`.
pub const PUD_SHIFT: usize = Pud::SHIFT;

/// Bytes covered by one Pud entry (1 GiB): the size of a Pud `Block` mapping or of the Pmd table a `Table` points to.
pub const PUD_SIZE: usize = 1 << PUD_SHIFT;

/// Type field (bits \[1:0\]) of a descriptor. The meaning of `0b11` depends on the level, see `DescriptorType::Table`.
#[repr(u64)]
pub enum DescriptorType {
    /// Unmapped; the rest of the descriptor is ignored by the hardware
    Invalid = 0b00,
    /// Maps a whole 1 GiB (Pud) or 2 MiB (Pmd) region directly
    Block = 0b01,
    /// Maps a 4 KiB page (Pte); the same encoding is a `Table` descriptor at Pgd/Pud/Pmd
    Page = 0b11,
}

impl DescriptorType {
    // Table descriptors share the Page encoding (type bits [1:0] = 0b11);
    // the table-vs-page distinction is the level, not the type field.
    #[allow(non_upper_case_globals)]
    pub const Table: Self = Self::Page;
}

// Behavior shared by every descriptor, whatever its level or kind.
pub trait Descriptor: Sized {
    const DESC_TYPE_MASK: u64 = 0b11;

    /// Bit position in a virtual address where this level's 9-bit table index starts (Pgd 39, Pud 30, Pmd 21, Pte 12)
    const SHIFT: usize;

    /// Returns the index of `va`'s entry in a table of this level: bits `[SHIFT + 8 : SHIFT]` of `va`
    fn index(va: u64) -> usize {
        // provided method, no `self`
        ((va >> Self::SHIFT) & (PTRS_PER_TABLE as u64 - 1)) as usize
    }

    /// Builds a descriptor directly from a raw 64-bit value, with no validation.
    ///
    /// Low-level escape hatch that bypasses the typed builders. For normal construction prefer `invalid()` followed by
    /// the typed setters (`set_type`, `set_output_address`, and the level's leaf/table setters), which enforce valid
    /// field encodings.
    fn new(raw: u64) -> Self;

    fn raw(&self) -> u64;

    fn raw_mut(&mut self) -> &mut u64;

    /// Builds a fresh descriptor with all bits zero — the `Invalid` type encoding (bit 0 = 0). A constructor, not a
    /// mutator: it doesn't invalidate an existing descriptor, it creates a new empty one, to be filled in with
    /// `set_type` and the level's other setters.
    #[inline]
    fn invalid() -> Self {
        Self::new(0)
    }

    /// Returns whether the descriptor's valid bit (bit 0) is set
    #[inline]
    fn is_valid(&self) -> bool {
        self.raw() & 0b1 != 0
    }

    /// Sets the descriptor's type field (bits \[1:0\]) to `type_`, leaving other bits untouched
    #[inline]
    fn set_type(&mut self, type_: DescriptorType) {
        *self.raw_mut() =
            (*self.raw_mut() & !Self::DESC_TYPE_MASK) | (type_ as u64 & Self::DESC_TYPE_MASK);
    }

    /// Raw-ORs a bitmask of attribute bits into the descriptor.
    ///
    /// Escape hatch for the independent attribute flags (e.g. `leaf::UXN | leaf::AF`); it neither clears nor validates
    /// anything. For the pick-one fields prefer the typed setters (`set_shareability`, `set_ap`, `set_mair_range`),
    /// which guarantee valid encodings.
    #[inline]
    fn set_attrs(&mut self, attrs: u64) {
        *self.raw_mut() |= attrs;
    }

    /// Sets the descriptor's output-address field to `pa`, masked to `ADDR_MASK`
    #[inline]
    fn set_output_address(&mut self, pa: PhysAddr) {
        debug_assert!(pa.is_aligned(PAGE_SIZE as u64));
        *self.raw_mut() = (*self.raw_mut() & !ADDR_MASK) | (pa.as_u64() & ADDR_MASK);
    }

    /// Returns the descriptor's output-address field (the next-level table or the mapped page/block), masked to
    /// `ADDR_MASK`
    #[inline]
    fn output_address(&self) -> PhysAddr {
        PhysAddr::new(self.raw() & ADDR_MASK)
    }
}

// Extra behavior for table descriptors: they point to the next-level table.
// Implemented for the levels that can hold a table (Pgd, Pud, Pmd), not Pte.
pub trait TableDescriptor: Descriptor {
    /// Points this table descriptor at the next-level table's physical address
    #[inline]
    fn set_next_table(&mut self, table_pa: PhysAddr) {
        self.set_output_address(table_pa);
    }

    /// Returns the physical address of the next-level table this descriptor points to.
    ///
    /// Only meaningful for a valid `Table` descriptor; checked with a `debug_assert!`.
    #[inline]
    fn next_table(&self) -> PhysAddr {
        debug_assert!(self.is_valid() && self.raw() & 0b11 == 0b11);
        self.output_address()
    }
}

// Extra behavior for leaf descriptors (block or page): they carry memory attributes.
// Implemented for the levels that can hold a block/page (Pud, Pmd, Pte), not Pgd.
pub trait LeafDescriptor: Descriptor {
    const ATTR_IDX_SHIFT: u64 = 2;

    /// Sets the AttrIndx field (bits \[4:2\]) to the given MAIR_EL1 slot
    #[inline]
    fn set_mair_range(&mut self, idx: MairIdx) {
        *self.raw_mut() |= (idx as u64) << Self::ATTR_IDX_SHIFT;
    }

    /// Sets the SH field (bits \[9:8\]) to the given shareability domain
    #[inline]
    fn set_shareability(&mut self, sh: Shareability) {
        *self.raw_mut() |= sh as u64;
    }

    /// Sets the AP field (bits \[7:6\]) to the given access-permission level
    #[inline]
    fn set_ap(&mut self, ap: Ap) {
        *self.raw_mut() |= ap as u64;
    }
}

/// L0 table descriptor (covers 512 GiB per entry). Only ever `Invalid` or `Table` — a leaf (`Block`/`Page`) encoding
/// isn't valid at this level.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Pgd(u64);

impl Descriptor for Pgd {
    const SHIFT: usize = Pud::SHIFT + TABLE_SHIFT;

    #[inline]
    fn new(raw: u64) -> Self {
        Pgd(raw)
    }

    #[inline]
    fn raw(&self) -> u64 {
        self.0
    }

    #[inline]
    fn raw_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

impl TableDescriptor for Pgd {}

/// L1 table-or-block descriptor (or `Invalid`, unmapped). `Table` covers 1 GiB per entry (points to an L2 table);
/// `Block` maps 1 GiB directly as a leaf — the huge-page case this kernel's identity map uses.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Pud(u64);

impl Descriptor for Pud {
    const SHIFT: usize = Pmd::SHIFT + TABLE_SHIFT;

    #[inline]
    fn new(raw: u64) -> Self {
        Pud(raw)
    }

    #[inline]
    fn raw(&self) -> u64 {
        self.0
    }

    #[inline]
    fn raw_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

impl TableDescriptor for Pud {}

impl LeafDescriptor for Pud {}

/// L2 table-or-block descriptor (or `Invalid`, unmapped). `Table` covers 2 MiB per entry (points to an L3 table);
/// `Block` maps 2 MiB directly as a leaf (huge page).
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Pmd(u64);

impl Descriptor for Pmd {
    const SHIFT: usize = Pte::SHIFT + TABLE_SHIFT;

    #[inline]
    fn new(raw: u64) -> Self {
        Pmd(raw)
    }

    #[inline]
    fn raw(&self) -> u64 {
        self.0
    }

    #[inline]
    fn raw_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

impl TableDescriptor for Pmd {}

impl LeafDescriptor for Pmd {}

/// L3 page descriptor (covers 4 KiB per entry). Only ever `Invalid` or `Page` — the `Page` type encoding (bits \[1:0\]
/// = `0b11`) means "page" at this level, unlike the same encoding meaning "table" at L0-L2 (see
/// `DescriptorType::Table`).
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Pte(u64);

impl Descriptor for Pte {
    const SHIFT: usize = PAGE_SHIFT;

    #[inline]
    fn new(raw: u64) -> Self {
        Pte(raw)
    }

    #[inline]
    fn raw(&self) -> u64 {
        self.0
    }

    #[inline]
    fn raw_mut(&mut self) -> &mut u64 {
        &mut self.0
    }
}

impl LeafDescriptor for Pte {}

/// Returns the high-half virtual address of a kernel-image physical address: `KIMAGE_VADDR + (pa - __kernel_start)`.
///
/// `pa` must lie inside the kernel image, at or above `__kernel_start`.
pub fn kimage_va(pa: PhysAddr) -> u64 {
    KIMAGE_VADDR + (pa.as_u64() - addr_of_mut!(__kernel_start) as u64)
}

/// Inverse of `kimage_va`: returns the physical address of a kernel-image virtual address, `va - KIMAGE_VADDR +
/// __kernel_start`.
///
/// `va` must lie inside the kernel image mapping, at or above `KIMAGE_VADDR`.
pub fn kimage_pa(va: u64) -> PhysAddr {
    PhysAddr::new(va - KIMAGE_VADDR + addr_of_mut!(__kernel_start) as u64)
}
