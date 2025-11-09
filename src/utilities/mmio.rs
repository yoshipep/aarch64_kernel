//! Memory-mapped I/O utilities
//!
//! This module provides safe wrappers around volatile memory operations for
//! interacting with memory-mapped hardware registers. All functions use
//! volatile reads and writes to ensure the compiler doesn't optimize away
//! hardware accesses.

use core::ptr::{read_volatile, write_volatile};

/// Reads a 32-bit value from a MMIO register
///
/// Reads the value of the MMIO register `base` + `offset`
pub fn read_mmio32(base: usize, offset: usize) -> u32 {
    unsafe {
        let ptr = (base as *const u8).add(offset) as *const u32;
        return read_volatile(ptr);
    }
}

/// Writes a 32-bit value to a MMIO register
///
/// Write the value `value` of the MMIO register `base` + `offset`
pub fn write_mmio32(base: usize, offset: usize, value: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        write_volatile(ptr, value);
    }
}

/// Set bits of a 32 bit MMIO register
///
/// Set the bits `bits` of the MMIO register `base` + `offset`
pub fn set_mmio_bits32(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val | bits);
    }
}

/// Clear bits of a 32 bit MMIO register
///
/// Clear the bits `bits` of the MMIO register `base` + `offset`
pub fn clear_mmio_bits32(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val & !bits);
    }
}
