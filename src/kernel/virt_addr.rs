//! Virtual address newtype
//!
//! `VirtAddr` is the counterpart of `PhysAddr` (see `phys_addr.rs`): one canonical type for an address that goes
//! through the MMU, so a virtual address can't be passed where a physical one is expected, or the other way round.
//! That is a real class of bug now that RAM has several virtual aliases: the identity map, the kernel image
//! (`kimage_va`) and the linear map (`phys_to_virt`).
//!
//! Unlike `PhysAddr`, constructing one is checked: with a 48-bit virtual address space (`pgtable::VA_BITS`) only
//! *canonical* addresses are valid, those whose bits `[63:VA_BITS]` (the top 16) are all equal: all zeros selects
//! TTBR0, all ones selects TTBR1. Bit 47 is an ordinary index bit, not part of the check, so both halves have addresses
//! with it set or clear. Anything else always faults on access, so a non-canonical value is a bug at the point it is
//! created, not where it is finally used.

use core::ops::{Add, Sub};

use crate::kernel::mm::pgtable::VA_BITS;

/// A virtual address.
///
/// Deliberately opaque (the inner `u64` is private), like `PhysAddr`: go through the constructors and accessors below.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(u64);

impl VirtAddr {
    /// Whether `addr` is canonical for a `VA_BITS`-bit address space: bits `[63:VA_BITS]` all zeros (TTBR0 half) or
    /// all ones (TTBR1 half). This is the check the hardware makes with `T0SZ = T1SZ = 64 - VA_BITS`, not the x86-64
    /// rule of sign-extending from the top implemented bit.
    pub const fn is_canonical(addr: u64) -> bool {
        let top = addr >> VA_BITS;
        top == 0 || top == u64::MAX >> VA_BITS
    }

    /// Wraps a raw virtual address value.
    ///
    /// # Panics
    ///
    /// In debug builds, if `addr` is not canonical (`debug_assert!`).
    pub const fn new(addr: u64) -> Self {
        debug_assert!(Self::is_canonical(addr));
        Self(addr)
    }

    /// Wraps a raw virtual address value without checking that it is canonical.
    ///
    /// For the rare case where a non-canonical value is built on purpose (e.g. to test the fault path). Prefer `new`.
    pub const fn new_unchecked(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the address of `ptr`.
    pub fn from_ptr<T>(ptr: *const T) -> Self {
        Self::new(ptr as u64)
    }

    /// Returns the raw address value.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the raw address value, widened to `usize`.
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }

    /// Reinterprets this address as a raw pointer, for memory that is known to be mapped at this address in the
    /// current translation regime.
    pub fn as_ptr<T>(&self) -> *const T {
        self.0 as *const T
    }

    /// Mutable counterpart to `as_ptr`.
    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.0 as *mut T
    }

    /// Whether this address is in the TTBR1 (high) half, i.e. bit 63 is set.
    pub const fn is_high_half(&self) -> bool {
        self.0 >> 63 == 1
    }

    /// Rounds up to the next multiple of `align` (`align` must be a power of two).
    pub const fn align_up(&self, align: u64) -> Self {
        Self::new((self.0 + align - 1) & !(align - 1))
    }

    /// Rounds down to the previous multiple of `align` (`align` must be a power of two).
    pub const fn align_down(&self, align: u64) -> Self {
        Self(self.0 & !(align - 1))
    }

    /// Whether this address is already a multiple of `align` (`align` must be a power of two).
    pub const fn is_aligned(&self, align: u64) -> bool {
        self.0 & (align - 1) == 0
    }
}

/// `address + offset -> address`. Takes a plain `u64` offset, not another `VirtAddr`, as for `PhysAddr`. The result
/// goes through `VirtAddr::new`, so stepping past the end of a half (out of the canonical range) is caught.
impl Add<u64> for VirtAddr {
    type Output = Self;

    fn add(self, offset: u64) -> Self {
        Self::new(self.0 + offset)
    }
}

/// `address - offset -> address`
impl Sub<u64> for VirtAddr {
    type Output = Self;

    fn sub(self, offset: u64) -> Self {
        Self::new(self.0 - offset)
    }
}

/// `address - address -> offset`. The distance between two addresses is a size, not an address.
impl Sub<VirtAddr> for VirtAddr {
    type Output = u64;

    fn sub(self, rhs: VirtAddr) -> u64 {
        self.0 - rhs.0
    }
}

impl core::fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl core::fmt::UpperHex for VirtAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::UpperHex::fmt(&self.0, f)
    }
}
