//! Raw memory primitives
//!
//! Generic, subsystem-agnostic operations on raw memory — same kind of thing as `mmio`'s volatile accessors or
//! `convert`'s byte conversions, just at the granularity of whole buffers rather than single values.

/// Fills `len` bytes starting at `ptr` with `val`.
///
/// # Safety
///
/// - `ptr` must be valid for writes of `len` bytes.
/// - `ptr` must be properly aligned for `u8` (i.e. no alignment requirement).
pub unsafe fn memset(ptr: *mut u8, val: u8, len: usize) {
    if len == 0 {
        return;
    }

    let mut remaining = 0;

    while remaining < len {
        unsafe {
            let addr_ptr = ptr.add(remaining);
            let addr = addr_ptr.addr();

            // Case 1: 64-bit aligned
            if ((remaining + size_of::<u64>()) <= len) && (addr & (align_of::<u64>() - 1) == 0) {
                let word = u64::from_ne_bytes([val; size_of::<u64>()]);
                *(addr_ptr as *mut u64) = word;

                remaining += size_of::<u64>();
                continue;
            }

            // Case 2: 32-bit aligned
            if ((remaining + size_of::<u32>()) <= len) && (addr & (align_of::<u32>() - 1) == 0) {
                let word = u32::from_ne_bytes([val; size_of::<u32>()]);
                *(addr_ptr as *mut u32) = word;

                remaining += size_of::<u32>();
                continue;
            }

            // Case 3: 16-bit aligned
            if ((remaining + size_of::<u16>()) <= len) && (addr & (align_of::<u16>() - 1) == 0) {
                let word = u16::from_ne_bytes([val; size_of::<u16>()]);
                *(addr_ptr as *mut u16) = word;

                remaining += size_of::<u16>();
                continue;
            }

            // Case 4: 8-bit aligned
            if ((remaining + size_of::<u8>()) <= len) && (addr & (align_of::<u8>() - 1) == 0) {
                *addr_ptr = val;

                remaining += size_of::<u8>();
                continue;
            }
        }
    }
}
