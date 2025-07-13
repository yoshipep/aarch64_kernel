// Disable Rust's standard library and main entry point
// Required for bare-metal development
#![no_main]
#![no_std]

mod uart;
mod exceptions;
mod gic;
mod utilities;
mod sync;

// Required to handle panics manually when `no_std` is enabled
use core::panic::PanicInfo;

// Entry point function (void kmain(void) in C)
// no_mangle: Disables Rust's name mangling
// extern "C": Uses C calling convention
#[unsafe(no_mangle)]
pub extern "C" fn kmain(_fdt_addr: usize) {
    uart::print(b"Hello, from Rust\n");
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
