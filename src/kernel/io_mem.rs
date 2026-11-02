//! MMIO window newtype
//!
//! `IoMem` is the handle `ioremap` returns for a mapped device register block. It is a `VirtAddr` that is known to be
//! mapped with Device attributes, plus the length of the mapping, so it can't be mixed with a plain virtual address:
//! a RAM pointer can't be passed where a register block is expected, and a register block can't be dereferenced as if
//! it were RAM. This is the counterpart of Linux's `void __iomem *`, which sparse checks and the compiler doesn't.
//!
//! The register accessors are methods on the handle (not free functions taking a base), so only an `IoMem` can reach
//! them. They take a byte offset within the window and, in debug builds, check it against the mapping length.

use crate::kernel::virt_addr::VirtAddr;
use crate::utilities::mmio;

/// A mapped MMIO register block.
///
/// Opaque like `PhysAddr` and `VirtAddr`: the fields are private, and the only way to get one is `IoMem::new`, which
/// is meant for `ioremap` alone. `Copy` so a driver can keep it in a static; unmapping through `iounmap` does not
/// invalidate copies, as in Linux.
#[derive(Debug, Clone, Copy)]
pub struct IoMem {
    /// Virtual address of the first byte of the window, as returned by `ioremap` (so it keeps the sub-page offset of
    /// the physical address that was mapped).
    base: VirtAddr,
    /// Length of the window in bytes, as requested from `ioremap` (not rounded to pages).
    len: usize,
}

impl IoMem {
    /// Wraps a mapped window.
    ///
    /// Only `ioremap` should call this: it is the one that knows `base..base + len` is mapped with Device attributes.
    ///
    /// # Arguments
    ///
    /// * `base` - virtual address of the first byte of the window
    /// * `len` - length of the window in bytes
    pub(crate) const fn new(base: VirtAddr, len: usize) -> Self {
        Self { base, len }
    }

    /// Returns the virtual address of the first byte of the window.
    pub const fn base(&self) -> VirtAddr {
        self.base
    }

    /// Returns the length of the window in bytes.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Reads a 32-bit register.
    ///
    /// # Arguments
    ///
    /// * `offset` - byte offset of the register within the window
    ///
    /// # Panics
    ///
    /// In debug builds, if the register does not fit in the window (`offset + 4 > len`).
    pub fn read32(&self, offset: usize) -> u32 {
        self.debug_check_bounds(offset);
        mmio::read_mmio32(self.base.as_usize(), offset)
    }

    /// Writes a 32-bit register.
    ///
    /// # Arguments
    ///
    /// * `offset` - byte offset of the register within the window
    /// * `value` - value to write
    ///
    /// # Panics
    ///
    /// In debug builds, if the register does not fit in the window (`offset + 4 > len`).
    pub fn write32(&self, offset: usize, value: u32) {
        self.debug_check_bounds(offset);
        mmio::write_mmio32(self.base.as_usize(), offset, value);
    }

    /// Sets (ORs in) bits of a 32-bit register, leaving the other bits untouched.
    ///
    /// # Arguments
    ///
    /// * `offset` - byte offset of the register within the window
    /// * `bits` - bits to set in the register's current value
    ///
    /// # Panics
    ///
    /// In debug builds, if the register does not fit in the window (`offset + 4 > len`).
    pub fn set_bits32(&self, offset: usize, bits: u32) {
        self.debug_check_bounds(offset);
        mmio::set_mmio_bits32(self.base.as_usize(), offset, bits);
    }

    /// Clears (ANDs out) bits of a 32-bit register, leaving the other bits untouched.
    ///
    /// # Arguments
    ///
    /// * `offset` - byte offset of the register within the window
    /// * `bits` - bits to clear from the register's current value
    ///
    /// # Panics
    ///
    /// In debug builds, if the register does not fit in the window (`offset + 4 > len`).
    pub fn clear_bits32(&self, offset: usize, bits: u32) {
        self.debug_check_bounds(offset);
        mmio::clear_mmio_bits32(self.base.as_usize(), offset, bits);
    }

    /// Checks that a 32-bit register at `offset` fits in the window.
    ///
    /// A no-op in release builds (the `debug_assert!` is compiled out). The sum uses `checked_add` so a huge `offset`
    /// can't overflow and pass the comparison.
    ///
    /// # Arguments
    ///
    /// * `offset` - byte offset of the register within the window
    ///
    /// # Panics
    ///
    /// In debug builds, if `offset + 4 > len`.
    #[inline]
    fn debug_check_bounds(&self, offset: usize) {
        debug_assert!(offset.checked_add(4).is_some_and(|end| end <= self.len));
    }
}
