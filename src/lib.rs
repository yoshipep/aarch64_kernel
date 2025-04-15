// Disable Rust's standard library and main entry point
// Required for bare-metal development
#![no_main]
#![no_std]

// Required to handle panics manually when `no_std` is enabled
use core::panic::PanicInfo;

// Define the address of the UART device (MMIO)
// In C: volatile uint8_t *uart = ...
const UART: *mut u8 = 0x0900_0000 as *mut u8;

// Entry point function (void kmain(void) in C)
// no_mangle: Disables Rust's name mangling
// extern "C": Uses C calling convention
#[unsafe(no_mangle)]
pub extern "C" fn kmain() {
    print(b"Hello, from Rust!\n");
}

fn putchar(c: u8) {
    unsafe {
        *UART = c;
    }
}

fn print(s: &[u8]) {
    for &c in s {
        putchar(c);
    }
}

// The ! here specifies that the function doesn't return
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    print(b"Panic!\n");
    loop {}
}
