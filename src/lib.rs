//! The aarch64 crate
//!
//! This crate is an implementation of an aarch64 kernel

#![no_std]
#![no_main]
// Required to handle panics manually when `no_std` is enabled
use core::panic::PanicInfo;

pub mod exceptions;
pub mod gic;
pub mod irq_safe_mutex;
pub mod uart;
pub mod utilities;

/// Kernel main function
// no_mangle: Disables Rust's name mangling
#[unsafe(no_mangle)]
// extern "C": Uses C calling convention
pub extern "C" fn kmain(_fdt_addr: usize) {
    uart::println(b"Hello, from Rust");
    loop {
        if let Some(ch) = uart::getchar() {
            uart::print(b"You typed: ");
            uart::putchar(ch);
            uart::print(b"\n");
        }
    }
}

// The ! here specifies that the function doesn't return
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    uart::print(b"Panic!\n");
    loop {}
}
