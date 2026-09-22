//! Physical address newtype
//!
//! `PhysAddr` wraps a raw `u64` to give physical addresses one canonical type, used consistently by everything that
//! passes one around (`kmain`, `meminfo`, `dtb`, `mm::frame_alloc`, ...) instead of an ad-hoc mix of `u64`/`usize` that
//! needs casting at every boundary. Deliberately scoped to just this: no `VirtAddr` counterpart exists yet, because
//! nothing in this kernel currently has both a physical and a virtual address for the same thing that could be mixed
//! up — everything is identity-mapped (VA == PA) until the kernel high mapping (Task 5) lands. That's the point to add
//! `VirtAddr` and let the two types stop a real class of bug; before that, a second type would have nothing to guard
//! against.
//!
//! Deliberately not tied to `usize`: `usize` is defined by this *program's* pointer width, which isn't the same
//! concept as the *hardware's* physical-addressing width (see `pgtable::OA_BITS`'s own doc comment on that
//! distinction) — they happen to coincide on this 64-bit target, but a physical address's natural width is a hardware
//! property, not a Rust-target property.

use core::ops::{Add, Sub};

/// A physical address.
///
/// Deliberately opaque (the inner `u64` is private) — go through the constructors/accessors below rather than reading
/// or writing `.0` directly, so every place that touches a physical address goes through the same conversions.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(u64);

impl PhysAddr {
    /// Wraps a raw physical address value.
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// Returns the raw address value.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Returns the raw address value, widened to `usize`.
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }

    /// Reinterprets this address as a raw pointer, for dereferencing memory that's already known to be mapped and
    /// valid at this address (e.g. under the identity map).
    pub fn as_ptr<T>(&self) -> *const T {
        self.0 as *const T
    }

    /// Mutable counterpart to `as_ptr`.
    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.0 as *mut T
    }

    /// Rounds up to the next multiple of `align` (`align` must be a power of two).
    pub const fn align_up(&self, align: u64) -> Self {
        Self((self.0 + align - 1) & !(align - 1))
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

/// `address + offset -> address`. Deliberately takes a plain `u64` offset, not another `PhysAddr` — adding two
/// addresses together isn't a meaningful operation, only offsetting one by a size is.
impl Add<u64> for PhysAddr {
    type Output = Self;

    fn add(self, offset: u64) -> Self {
        Self(self.0 + offset)
    }
}

/// `address - offset -> address`
impl Sub<u64> for PhysAddr {
    type Output = Self;

    fn sub(self, offset: u64) -> Self {
        Self(self.0 - offset)
    }
}

/// `address - address -> offset`. The distance between two addresses is a size, not an address — same shape as
/// pointer subtraction in Rust/C.
impl Sub<PhysAddr> for PhysAddr {
    type Output = u64;

    fn sub(self, rhs: PhysAddr) -> u64 {
        self.0 - rhs.0
    }
}

impl From<u64> for PhysAddr {
    fn from(addr: u64) -> Self {
        Self(addr)
    }
}

impl From<usize> for PhysAddr {
    fn from(addr: usize) -> Self {
        Self(addr as u64)
    }
}

impl core::fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl core::fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::UpperHex::fmt(&self.0, f)
    }
}
