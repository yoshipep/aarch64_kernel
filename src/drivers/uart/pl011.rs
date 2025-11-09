//! A driver for the PL011 UART serial port
//!
//! This module provides functions to initialize, configure, and interact with a PL011 UART device.
//!
//! ## Design
//!
//! The driver uses a mixed model for handling communication:
//!
//! - **Transmission (TX):** Writing characters (`putchar`, `print`) is done via **polling**. The
//!   code will wait in a loop until the UART's transmit buffer is ready to accept a new character.
//!
//! - **Reception (RX):** Receiving characters is **interrupt-driven**. The interrupt handler (defined
//!   in `exceptions.rs`) reads the incoming byte and `push` it into the global `RX_BUFFER`. The
//!   `getchar` function then safely reads from this buffer.
//!
//! ## Concurrency
//!
//! The global `RX_BUFFER` is shared between the UART and any kernel code that calls `getchar`. To
//! prevent race conditions and deadlocks, it is protected by the interrupt safe `Mutex` from
//! `crate::irq_safe_mutex`

use core::sync::atomic::AtomicUsize;

use crate::ipc::irq_safe_mutex::Mutex;
use crate::utilities::mmio;
use core::sync::atomic::Ordering;

/// The size of the circular buffer used for receiving UART data
const UART_BUFFER_SIZE: usize = 256;

/// A circular buffer for storing incoming UART data
///
/// This buffer is designed to be written to by the UART interrupt handler and read from the
/// kernel's main execution context
pub struct UartBuffer {
    /// The underlying array for the buffer
    buffer: [u8; UART_BUFFER_SIZE],
    /// The index where the next byte will be written
    head: AtomicUsize,
    /// The index from which the next byte will be read
    tail: AtomicUsize,
}

/// Global static instance of the UART RX buffer.
///
/// This buffer is protected by the interrupt-safe `Mutex` to allow for safe, concurrent access
/// from both the UART interrupt handler (the producer) and the kernel's character-reading
/// functions (the consumer)
pub static RX_BUFFER: Mutex<UartBuffer> = Mutex::new(UartBuffer {
    buffer: [0; UART_BUFFER_SIZE],
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
});

impl UartBuffer {
    /// Pushes a byte into the circular buffer
    pub fn push(&mut self, byte: u8) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % UART_BUFFER_SIZE;

        if next_head == self.tail.load(Ordering::Relaxed) {
            return false;
        }

        self.buffer[head] = byte;
        self.head.store(next_head, Ordering::Relaxed);
        true
    }

    /// Pops a byte from the circular buffer
    fn pop(&mut self) -> Option<u8> {
        let byte;
        let next_tail;
        let tail = self.tail.load(Ordering::Relaxed);

        if self.head.load(Ordering::Relaxed) == tail {
            return None;
        }

        byte = self.buffer[tail];
        next_tail = (tail + 1) % UART_BUFFER_SIZE;
        self.tail.store(next_tail, Ordering::Relaxed);
        Some(byte)
    }
}

// Define the address of the UART device (MMIO)
// In C: volatile uint8_t *uart = ...
/// A struct holding the configuration and state of the PL011 UART device
struct UartPl011 {
    /// The base memory mapped address of the UART registers
    base_addr: *mut u32,
    /// The base clock frequency of the UART peripheral
    base_clock: u32,
    /// The configured baud rate
    baudrate: u32,
    /// The number of data bits
    data_bits: u8,
    /// The number of stop bits
    stop_bits: u8,
}

/* --- PL011 UART Register Constants --- */
const DR_OFF: usize = 0x00;
const FR_OFF: usize = 0x18;
const FR_BUSY: u32 = 1 << 3;
const FR_TXFE: u32 = 1 << 5;
const IBRD_OFF: usize = 0x24;
const FBRD_OFF: usize = 0x28;
const LCR_OFF: usize = 0x2c;
const LCR_FEN: u32 = 1 << 4;
const LCR_STP2: u32 = 1 << 3;
const CR_OFF: usize = 0x30;
const CR_UARTEN: u32 = 1 << 0;
const CR_RXEN: u32 = 1 << 9;
const IMSC_OFF: usize = 0x38;
const IMSC_RXIM: u32 = 1 << 4;
pub const ICR_OFF: usize = 0x44;
pub const ICR_RXIC: u32 = 1 << 4;
const DMACR_OFF: usize = 0x48;

/// The global, mutable instance representing the system's UART device
static mut UART: UartPl011 = UartPl011 {
    base_addr: core::ptr::null_mut(),
    base_clock: 0,
    baudrate: 0,
    data_bits: 0,
    stop_bits: 0,
};

/// Initializes the global UART struct with hardware-specific details
#[unsafe(no_mangle)]
pub fn init_uart(base_addr: *mut u32, base_clock: u32, baudrate: u32) {
    unsafe {
        UART = UartPl011 {
            base_addr: base_addr,
            base_clock: base_clock,
            baudrate: baudrate,
            data_bits: 8,
            stop_bits: 1,
        };
    }
}

/// Configures the UART hardware registers for operation
///
/// This function performs the hardware specific setup sequence for the PL011 UART, including
/// setting the baud rate, data format and enabling interrupts
#[unsafe(no_mangle)]
pub fn configure_uart() {
    let mut lcr_val: u32 = 0;

    // 1. Disable the UART
    unsafe {
        mmio::write_mmio32(UART.base_addr as usize, CR_OFF, 0);
        // 2. Wait for the end of TX
        while (mmio::read_mmio32(UART.base_addr as usize, FR_OFF) & FR_BUSY) != 0 {}
        // 3.Flush RX/TX fifos
        mmio::clear_mmio_bits32(UART.base_addr as usize, LCR_OFF, LCR_FEN);
    }

    // 4. Set speed
    uart_set_speed();
    // 5. Configure the data frame format
    unsafe {
        // 5.1 Word length: bits 5 and 6
        lcr_val |= ((UART.data_bits as u32 - 1) & 0x3) << 5;
        // 5.2 Use 1 or 2 stop bits: bit LCR_STP2
        if UART.stop_bits == 2 {
            lcr_val |= LCR_STP2;
        }
        // 6 Enable FIFOs
        lcr_val |= LCR_FEN;
    }

    unsafe {
        mmio::write_mmio32(UART.base_addr as usize, LCR_OFF, lcr_val);
        // 7. Enable RX interrupt
        mmio::set_mmio_bits32(UART.base_addr as usize, IMSC_OFF, IMSC_RXIM);
        // 8. Disable DMA
        mmio::write_mmio32(UART.base_addr as usize, DMACR_OFF, 0x01);
        // 9. Enable RX and UART
        mmio::set_mmio_bits32(UART.base_addr as usize, CR_OFF, CR_UARTEN | CR_RXEN);
    }
}

/// Calculates and sets the baud rate divisor registers
fn uart_set_speed() {
    let baud_div;

    unsafe {
        baud_div = 4 * UART.base_clock / UART.baudrate;
        mmio::write_mmio32(UART.base_addr as usize, IBRD_OFF, (baud_div >> 6) & 0xffff);
        mmio::write_mmio32(UART.base_addr as usize, FBRD_OFF, baud_div & 0x3f);
    }
}

/// Writes a single byte to the UART data register
///
/// This function will block and spin until the UART's TX FIFO has space
pub fn putchar(c: u8) {
    unsafe {
        while (mmio::read_mmio32(UART.base_addr as usize, FR_OFF) & FR_TXFE) != 0 {}
        mmio::write_mmio32(UART.base_addr as usize, DR_OFF, c as u32);
    }
}

/// Reads a single byte from the interrupt-driven RX buffer
pub fn getchar() -> Option<u8> {
    return RX_BUFFER.lock_irqsafe(|rx| rx.pop());
}

/// Prints a byte slice to the UART
pub fn print(s: &[u8]) {
    for &c in s {
        putchar(c);
    }
}

/// Prints a byte slice plus the newline character `\n` to the UART
pub fn println(s: &[u8]) {
    print(s);
    print(b"\n");
}
