//! Utilities module
//!
//! This module provides auxiliary functions. Those functions are intended to be used in any module

use core::ptr::{read_volatile, write_volatile};

/// Read a MMIO register
///
/// Reads the MMIO register `base` + `offset`
pub unsafe fn read_mmio(base: usize, offset: usize) -> u32 {
    unsafe {
        let ptr = (base as *const u8).add(offset) as *const u32;
        return read_volatile(ptr);
    }
}

/// Write to MMIO register
///
/// Writes to the MMIO register `base` + `offset` the value `value`
pub unsafe fn write_mmio(base: usize, offset: usize, value: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        write_volatile(ptr, value);
    }
}

/// Set bits of a MMIO register
///
/// Set the bits `bits` of the MMIO register `base` + `offset`
pub unsafe fn set_mmio_bits(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val | bits);
    }
}

/// Clear bits of a MMIO register
///
/// Clear the bits `bits` of the MMIO register `base` + `offset`
pub unsafe fn clear_mmio_bits(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val & !bits);
    }
}
