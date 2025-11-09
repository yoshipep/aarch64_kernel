//! Printing utilities
//!
//! This module provides functions to format and print integer values.
//! These are particularly useful for debugging and exception handling
//! where standard formatting traits are not available in a `no_std`
//! environment.
//!
//! All functions output directly to the UART using the PL011 driver.

use crate::drivers::uart::pl011;

/// Lookup table for hexadecimal digit conversion
const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Converts a u32 integer to its decimal string representation
///
/// This function converts an unsigned 32-bit integer into ASCII decimal digits,
/// storing them in the provided buffer. The conversion is performed from right
/// to left, and only the portion of the buffer containing the actual digits
/// is returned.
///
pub fn u32_to_str(mut num: u32, buf: &mut [u8; 10]) -> &[u8] {
    let mut i = buf.len();

    if num == 0 {
        buf[i - 1] = b'0';
        return &buf[i - 1..];
    }

    while num > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (num % 10) as u8;
        num /= 10;
    }

    &buf[i..]
}

/// Prints a u64 value as a 16-digit hexadecimal number to UART
///
/// This function formats the value as a zero-padded 16-character hexadecimal
/// string (e.g., `0000000000000042` for the value 66).
pub fn print_hex_u64(mut value: u64) {
    let mut buf = [0u8; 16];

    // Convert to hex digits (right to left)
    for i in (0..16).rev() {
        buf[i] = HEX_CHARS[(value & 0xF) as usize];
        value >>= 4;
    }

    pl011::print(&buf);
}

/// Prints an 8-bit value as a 2-digit hexadecimal number to UART
///
/// This function formats the value as a zero-padded 2-character hexadecimal
/// string using uppercase letters (e.g., `2A` for the value 42).
pub fn print_hex_u8(val: u8) {
    let mut buf = [0u8; 2];

    for i in 0..2 {
        let nibble = ((val >> ((1 - i) * 4)) & 0xF) as u8;
        if nibble < 10 {
            buf[i] = b'0' + nibble;
        } else {
            buf[i] = b'A' + (nibble - 10);
        };
    }

    pl011::print(&buf);
}
