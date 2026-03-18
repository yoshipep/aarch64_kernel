pub const PAGE_SHIFT: usize = 12;

pub const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

pub const PAGE_MASK: usize = !(PAGE_SIZE - 1);

pub const L1_SIZE_PER_ENTRY: usize = 1 << 30;

pub type Pte = u64;

const ATTR_IDX_SHIFT: usize = 2;

/* General descriptor helpers */

#[inline]
pub fn mark_page_desc(desc: *mut Pte) {
    unsafe {
        *desc |= 0x03;
    }
}

pub use mark_page_desc as mark_table_desc;

#[inline]
pub fn mark_block_desc(desc: *mut Pte) {
    unsafe {
        *desc |= 0x01;
    }
}

#[inline]
pub fn set_mair_range(desc: *mut Pte, idx: u64) {
    unsafe {
        *desc |= (idx & 0x7) << ATTR_IDX_SHIFT;
    }
}

/* Table Helpers */

#[inline]
pub fn set_table_attrs(desc: *mut Pte, attrs: u64) {
    unsafe {
        *desc |= attrs;
    }
}

#[inline]
pub fn set_next_lvl_table_addr(desc: *mut Pte, addr: *const u64) {
    unsafe {
        *desc |= (addr as u64) & PAGE_MASK as u64;
    }
}

pub use set_table_attrs as set_block_attrs;
