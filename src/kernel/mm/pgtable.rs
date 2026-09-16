use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::pgtable_hwdef::leaf::{Ap, Shareability};

pub const VA_BITS: usize = 48;

// Output-address (physical) field width in a descriptor. Distinct from VA_BITS: it lives on the
// output side of translation and diverges from VA_BITS under LPA2 (52-bit). 4KB granule, non-LPA2.
const OA_BITS: usize = 48;

pub const PAGE_SHIFT: usize = 12;

pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub const ADDR_MASK: u64 = ((1u64 << OA_BITS) - 1) & !(PAGE_SIZE as u64 - 1);

pub const L1_SIZE_PER_ENTRY: usize = 1 << 30;

#[repr(u64)]
pub enum DescriptorType {
    Invalid = 0b00,
    Block = 0b01,
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

    /// Builds a descriptor directly from a raw 64-bit value, with no validation.
    ///
    /// Low-level escape hatch that bypasses the typed builders. For normal construction prefer
    /// `invalid()` followed by the typed setters (`set_type`, `set_output_address`, and the level's
    /// leaf/table setters), which enforce valid field encodings.
    fn new(raw: u64) -> Self;

    fn raw(&self) -> u64;

    fn raw_mut(&mut self) -> &mut u64;

    #[inline]
    fn invalid() -> Self {
        Self::new(0)
    }

    #[inline]
    fn is_valid(&self) -> bool {
        self.raw() & 0b1 != 0
    }

    #[inline]
    fn set_type(&mut self, type_: DescriptorType) {
        *self.raw_mut() =
            (*self.raw_mut() & !Self::DESC_TYPE_MASK) | (type_ as u64 & Self::DESC_TYPE_MASK);
    }

    /// Raw-ORs a bitmask of attribute bits into the descriptor.
    ///
    /// Escape hatch for the independent attribute flags (e.g. `leaf::UXN | leaf::AF`); it neither
    /// clears nor validates anything. For the pick-one fields prefer the typed setters
    /// (`set_shareability`, `set_ap`, `set_mair_range`), which guarantee valid encodings.
    #[inline]
    fn set_attrs(&mut self, attrs: u64) {
        *self.raw_mut() |= attrs;
    }

    #[inline]
    fn set_output_address(&mut self, pa: u64) {
        *self.raw_mut() = (*self.raw_mut() & !ADDR_MASK) | (pa & ADDR_MASK);
    }
}

// Extra behavior for table descriptors: they point to the next-level table.
// Implemented for the levels that can hold a table (Pgd, Pud, Pmd), not Pte.
pub trait TableDescriptor: Descriptor {
    #[inline]
    fn set_next_table(&mut self, table_pa: u64) {
        self.set_output_address(table_pa);
    }
}

// Extra behavior for leaf descriptors (block or page): they carry memory attributes.
// Implemented for the levels that can hold a block/page (Pud, Pmd, Pte), not Pgd.
pub trait LeafDescriptor: Descriptor {
    const ATTR_IDX_SHIFT: u64 = 2;

    #[inline]
    fn set_mair_range(&mut self, idx: MairIdx) {
        *self.raw_mut() |= (idx as u64) << Self::ATTR_IDX_SHIFT;
    }

    #[inline]
    fn set_shareability(&mut self, sh: Shareability) {
        *self.raw_mut() |= sh as u64;
    }

    #[inline]
    fn set_ap(&mut self, ap: Ap) {
        *self.raw_mut() |= ap as u64;
    }
}

#[derive(Clone, Copy)]
pub struct Pgd(u64);

impl Descriptor for Pgd {
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

#[derive(Clone, Copy)]
pub struct Pud(u64);

impl Descriptor for Pud {
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

#[derive(Clone, Copy)]
pub struct Pmd(u64);

impl Descriptor for Pmd {
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

#[derive(Clone, Copy)]
pub struct Pte(u64);

impl Descriptor for Pte {
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
