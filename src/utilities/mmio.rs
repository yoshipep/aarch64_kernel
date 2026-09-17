//! Memory-mapped I/O utilities
//!
//! This module provides safe wrappers around volatile memory operations for interacting with memory-mapped hardware
//! registers. All functions use volatile reads and writes to ensure the compiler doesn't optimize away hardware
//! accesses.

use core::ptr::{read_volatile, write_volatile};

/// Reads a 32-bit value from a MMIO register
///
/// # Arguments
///
/// * `base` - base address of the MMIO region
/// * `offset` - byte offset of the register within that region
pub fn read_mmio32(base: usize, offset: usize) -> u32 {
    unsafe {
        let ptr = (base as *const u8).add(offset) as *const u32;
        return read_volatile(ptr);
    }
}

/// Writes a 32-bit value to a MMIO register
///
/// # Arguments
///
/// * `base` - base address of the MMIO region
/// * `offset` - byte offset of the register within that region
/// * `value` - value to write
pub fn write_mmio32(base: usize, offset: usize, value: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        write_volatile(ptr, value);
    }
}

/// Sets (ORs in) bits of a 32-bit MMIO register, leaving other bits untouched
///
/// # Arguments
///
/// * `base` - base address of the MMIO region
/// * `offset` - byte offset of the register within that region
/// * `bits` - bits to set in the register's current value
pub fn set_mmio_bits32(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val | bits);
    }
}

/// Clears (ANDs out) bits of a 32-bit MMIO register, leaving other bits untouched
///
/// # Arguments
///
/// * `base` - base address of the MMIO region
/// * `offset` - byte offset of the register within that region
/// * `bits` - bits to clear from the register's current value
pub fn clear_mmio_bits32(base: usize, offset: usize, bits: u32) {
    unsafe {
        let ptr = (base as *mut u8).add(offset) as *mut u32;
        let current_val = read_volatile(ptr);
        write_volatile(ptr, current_val & !bits);
    }
}
