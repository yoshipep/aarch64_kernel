//! The aarch64_kernel crate
//!
//! This crate is an implementation of an AArch64 bare-metal kernel for learning purposes.

#![no_std]
#![no_main]

use crate::drivers::timer::arch_timer;
use crate::drivers::uart::pl011;
use crate::kernel::dtb;
use core::panic::PanicInfo;

// Public modules
pub mod drivers;
pub mod ipc;
pub mod kernel;
pub mod utilities;

/// Kernel main function
///
/// This is the entry point for the Rust kernel code, called from assembly after hardware
/// initialization. It receives the device tree address as a parameter.
///
/// # Arguments
/// * `dtb_addr` - The address of the Flattened Device Tree (currently unused)
#[unsafe(no_mangle)]
pub extern "C" fn kmain(dtb_addr: usize) {
    dtb::parse_dtb(dtb_addr);
    println!("Hello, from Rust");
    println!("Arming the timer (1000ms)");
    arch_timer::arm_ms(1000);
    loop {
        if let Some(ch) = pl011::getchar() {
            println!("You typed: {}", ch as char);
        }
    }
}

/// Panic handler for no_std environment
///
/// This function is called when the kernel panics. Since we're in a bare-metal environment
/// with no standard library, we must define our own panic behavior.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!("Panic at {}:{}", location.file(), location.line());
    } else {
        println!("Panic!");
    }
    loop {}
}
