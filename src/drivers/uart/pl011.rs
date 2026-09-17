//! A driver for the PL011 UART serial port
//!
//! This module provides functions to initialize, configure, and interact with a PL011 UART device.
//!
//! ## Design
//!
//! The driver uses a mixed model for handling communication:
//!
//! - **Transmission (TX):** Writing characters (`putchar`, `print`) is done via **polling**. The code will wait in a
//!   loop until the UART's transmit buffer is ready to accept a new character.
//!
//! - **Reception (RX):** Receiving characters is **interrupt-driven**. The interrupt handler (defined in
//!   `exceptions.rs`) reads the incoming byte and `push` it into the global `RX_BUFFER`. The `getchar` function then
//!   safely reads from this buffer.
//!
//! ## Concurrency
//!
//! The global `RX_BUFFER` is shared between the UART and any kernel code that calls `getchar`. To prevent race
//! conditions and deadlocks, it is protected by the interrupt safe `Mutex` from `crate::irq_safe_mutex`

use core::ptr::addr_of_mut;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::drivers::gic::gicv3;
use crate::ipc::irq_safe_mutex::Mutex;
use crate::kernel::device;
use crate::kernel::dtb;
use crate::utilities::convert;
use crate::utilities::mmio;

/// The size of the circular buffer used for receiving UART data
const UART_BUFFER_SIZE: usize = 256;

/// A circular buffer for storing incoming UART data
///
/// This buffer is designed to be written to by the UART interrupt handler and read from the kernel's main execution
/// context
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
/// This buffer is protected by the interrupt-safe `Mutex` to allow for safe, concurrent access from both the UART
/// interrupt handler (the producer) and the kernel's character-reading functions (the consumer)
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

/// Early console base address (used before DTB-based driver initialization)
///
/// The bootloader/firmware is expected to have already configured the UART at this address. The early console just
/// writes to it — no hardware setup.
#[cfg(feature = "qemu-virt")]
const EARLY_BASE: usize = 0x0900_0000;

/* --- PL011 register offsets --- */
mod reg {
    pub const DR: usize = 0x00; // UARTDR: Data Register
    pub const FR: usize = 0x18; // UARTFR: Flag Register
    pub const IBRD: usize = 0x24; // UARTIBRD: Integer Baud Rate Divisor
    pub const FBRD: usize = 0x28; // UARTFBRD: Fractional Baud Rate Divisor
    pub const LCR_H: usize = 0x2c; // UARTLCR_H: Line Control Register
    pub const CR: usize = 0x30; // UARTCR: Control Register
    pub const IMSC: usize = 0x38; // UARTIMSC: Interrupt Mask set/clear Register
    pub const ICR: usize = 0x44; // UARTICR: Interrupt Clear Register
    pub const DMACR: usize = 0x48; // UARTDMACR: DMA Control Register
}

/* --- UARTFR: flag register --- */
#[allow(dead_code)]
mod fr {
    pub const BUSY: u32 = 1 << 3; // Transmitter busy
    pub const RXFE: u32 = 1 << 4; // RX FIFO empty
    pub const TXFF: u32 = 1 << 5; // TX FIFO full
    pub const RXFF: u32 = 1 << 6; // RX FIFO full
    pub const TXFE: u32 = 1 << 7; // TX FIFO empty
}

/* --- UARTLCR_H: line control --- */
#[allow(dead_code)]
mod lcr_h {
    pub const PEN: u32 = 1 << 1; // Parity enable
    pub const EPS: u32 = 1 << 2; // Even parity select
    pub const STP2: u32 = 1 << 3; // 2 stop bits (0 = 1 stop bit)
    pub const FEN: u32 = 1 << 4; // Enable FIFOs

    /// Word Length, bits \[6:5\]
    #[derive(Clone, Copy)]
    #[repr(u32)]
    pub enum WordLen {
        Bits5 = 0b00 << 5,
        Bits6 = 0b01 << 5,
        Bits7 = 0b10 << 5,
        Bits8 = 0b11 << 5,
    }
}

/* --- UARTCR: control --- */
#[allow(dead_code)]
mod cr {
    pub const UARTEN: u32 = 1 << 0; // UART enable
    pub const LBE: u32 = 1 << 7; // Loopback enable
    pub const TXE: u32 = 1 << 8; // Transmit enable
    pub const RXE: u32 = 1 << 9; // Receive enable
}

/* --- Baud-rate divisor fixed-point layout ---
 * BAUDDIV is a 22.6 fixed-point value (4 * UARTCLK / baud): the integer part goes in
 * UARTIBRD.BAUD_DIVINT [15:0], the 6-bit fraction in UARTFBRD.BAUD_DIVFRAC [5:0].
 */
#[allow(dead_code)]
mod baud {
    pub const FRAC_BITS: u32 = 6; // width of UARTFBRD.BAUD_DIVFRAC
    pub const INT_MASK: u32 = 0xffff; // width of UARTIBRD.BAUD_DIVINT
    pub const FRAC_MASK: u32 = 0x3f; // mask for the 6-bit fractional part
}

/* --- Interrupt bits: identical layout in IMSC / RIS / MIS / ICR --- */
#[allow(dead_code)]
mod int {
    pub const RX: u32 = 1 << 4; // Receive Interrupt Mask
    pub const TX: u32 = 1 << 5; // Transmit Interrupt Mask
    pub const RT: u32 = 1 << 6; // Receive timeout Interrupt Mask
    pub const FE: u32 = 1 << 7; // Framing error Interrupt Mask
    pub const PE: u32 = 1 << 8; // Parity error Interrupt Mask
    pub const BE: u32 = 1 << 9; // Break error Interrupt Mask
    pub const OE: u32 = 1 << 10; // Overrun error Interrupt Mask
}

/// The global, mutable instance representing the system's UART device
static mut UART: UartPl011 = UartPl011::new();

impl UartPl011 {
    /// Const constructor for static initialization
    pub const fn new() -> Self {
        Self {
            base_addr: core::ptr::null_mut(),
            base_clock: 0,
            baudrate: 115200,
            data_bits: 8,
            stop_bits: 1,
        }
    }

    /// Initialize with hardware-specific details
    ///
    /// # Arguments
    ///
    /// * `base_addr` - MMIO base address of the UART's register block
    /// * `base_clock` - input clock frequency (Hz) driving the UART, used by `set_speed` to compute the baud-rate
    ///   divisor
    pub fn init(&mut self, base_addr: *mut u32, base_clock: u32) {
        self.base_addr = base_addr;
        self.base_clock = base_clock;
    }

    /// Configure the UART hardware registers
    pub fn configure(&self) {
        // 1. Disable the UART
        mmio::write_mmio32(self.base_addr as usize, reg::CR, 0);

        // 2. Wait for the end of TX
        while (mmio::read_mmio32(self.base_addr as usize, reg::FR) & fr::BUSY) != 0 {}

        // 3. Flush RX/TX fifos
        mmio::clear_mmio_bits32(self.base_addr as usize, reg::LCR_H, lcr_h::FEN);

        // 4. Set speed
        self.set_speed();

        // 5. Configure the data frame format
        // 5.1 Word length: bits [6:5]
        let word_len = match self.data_bits {
            5 => lcr_h::WordLen::Bits5,
            6 => lcr_h::WordLen::Bits6,
            7 => lcr_h::WordLen::Bits7,
            _ => lcr_h::WordLen::Bits8,
        };

        let mut lcr_val = word_len as u32;
        // 5.2 Use 1 or 2 stop bits
        if self.stop_bits == 2 {
            lcr_val |= lcr_h::STP2;
        }

        // 6. Enable FIFOs
        lcr_val |= lcr_h::FEN;
        mmio::write_mmio32(self.base_addr as usize, reg::LCR_H, lcr_val);

        // 7. Enable RX interrupt
        mmio::set_mmio_bits32(self.base_addr as usize, reg::IMSC, int::RX);

        // 8. Disable DMA (RXDMAE/TXDMAE cleared; 0 is also the reset value)
        mmio::write_mmio32(self.base_addr as usize, reg::DMACR, 0);

        // 9. Enable TX, RX and UART (TXE+UARTEN required to transmit, RXE+UARTEN to receive)
        mmio::set_mmio_bits32(
            self.base_addr as usize,
            reg::CR,
            cr::UARTEN | cr::TXE | cr::RXE,
        );
    }

    /// Set baud rate divisor registers
    fn set_speed(&self) {
        let baud_div = 4 * self.base_clock / self.baudrate;

        mmio::write_mmio32(
            self.base_addr as usize,
            reg::IBRD,
            (baud_div >> baud::FRAC_BITS) & baud::INT_MASK,
        );

        mmio::write_mmio32(
            self.base_addr as usize,
            reg::FBRD,
            baud_div & baud::FRAC_MASK,
        );
    }

    /// Write a single byte
    ///
    /// If the UART has not been initialized yet (base address is null), falls back to the early console base address.
    pub fn putchar(&self, c: u8) {
        let base = if self.base_addr.is_null() {
            EARLY_BASE
        } else {
            self.base_addr as usize
        };

        while (mmio::read_mmio32(base, reg::FR) & fr::TXFF) != 0 {}

        mmio::write_mmio32(base, reg::DR, c as u32);
    }
}

/// Initializes the global UART struct with hardware-specific details
fn init_uart(base_addr: *mut u32, base_clock: u32) {
    unsafe {
        (*addr_of_mut!(UART)).init(base_addr, base_clock);
    }
}

/// Configures the UART hardware registers for operation
///
/// This function performs the hardware specific setup sequence for the PL011 UART, including setting the baud rate,
/// data format and enabling interrupts
#[unsafe(no_mangle)]
pub fn configure_uart() {
    unsafe {
        (*addr_of_mut!(UART)).configure();
    }
}

/// Writes a single byte to the UART data register
///
/// This function will block and spin until the UART's TX FIFO has space
pub fn putchar(c: u8) {
    unsafe {
        (*addr_of_mut!(UART)).putchar(c);
    }
}

/// Reads a single byte from the interrupt-driven RX buffer
///
/// # Returns
///
/// `Some(byte)` if a byte was available, `None` if the RX buffer is currently empty.
pub fn getchar() -> Option<u8> {
    return RX_BUFFER.lock_irqsafe(|rx| rx.pop());
}

/// Drains the pending RX byte into the buffer and clears the RX interrupt
///
/// Called from the UART IRQ handler; keeps all PL011 register access inside the driver.
pub fn handle_rx_interrupt() {
    let base = get_base_addr();

    RX_BUFFER.lock_irqsafe(|rx| {
        let ch = mmio::read_mmio32(base, reg::DR) as u8;
        let _ = rx.push(ch);
    });

    mmio::write_mmio32(base, reg::ICR, int::RX);
}

/// Returns the UART base address
pub fn get_base_addr() -> usize {
    unsafe { (*addr_of_mut!(UART)).base_addr as usize }
}

/// Zero-sized writer that implements `core::fmt::Write` for the PL011 UART
pub struct UartWriter;

impl core::fmt::Write for UartWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &c in s.as_bytes() {
            putchar(c);
        }
        Ok(())
    }
}

/// Helper function used by the `print!` and `println!` macros
#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    UartWriter.write_fmt(args).unwrap();
}

/// Formats and writes to the UART, without a trailing newline
///
/// This crate's own `print!`, not `std`'s — a `#![no_std]` kernel has no standard library to import it from. Same
/// argument syntax as the familiar macro; routes through [`_print`] to the PL011 driver.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::drivers::uart::pl011::_print(format_args!($($arg)*))
    };
}

/// Formats and writes to the UART, with a trailing newline
///
/// This crate's own `println!` — see [`print!`] for why one is needed at all in `#![no_std]`.
#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($fmt:expr) => { $crate::print!(concat!($fmt, "\n")) };
    ($fmt:expr, $($arg:tt)*) => { $crate::print!(concat!($fmt, "\n"), $($arg)*) };
}

/// Sets up the PL011 UART from device tree properties
///
/// Parses the device's DTB properties to extract:
/// - Base address from the `reg` property
/// - Interrupt configuration from the `interrupts` property (configures as SPI in the GIC)
/// - Clock frequency from the `clocks` property (follows phandle to clock node)
///
/// After extracting these values, initializes and configures the UART hardware.
pub fn setup(dev: &device::PlatformDevice) {
    let mut addr: u64 = 0;
    let mut freq: u32 = 0;
    let mut interrupt_info: [u32; gicv3::MAX_INTERRUPT_CELLS] = [0; gicv3::MAX_INTERRUPT_CELLS];
    // Get #address-cells from parent (size_cells not needed for UART)
    let (addr_cells, _) = dev.get_parent_cells();
    // Parse reg property for base address
    if let Some(reg_prop) = dev.find_property("reg") {
        for i in 0..addr_cells as usize {
            let cell = convert::read_be_u32(reg_prop.value, i * 4);
            addr = (addr << 32) | cell as u64;
        }
    }

    // Parse interrupts property
    if let Some(int_prop) = dev.find_property("interrupts") {
        if let Some(intc) = dtb::find_interrupt_parent(dev) {
            // Get #interrupt-cells from interrupt controller
            let mut interrupt_cells: u32 = 3; // Default for GICv3
            if let Some(cells_prop) = intc.find_property("#interrupt-cells") {
                interrupt_cells = convert::read_be_u32(cells_prop.value, 0);
            }

            // Read interrupt specifier cells
            for i in 0..interrupt_cells.min(gicv3::MAX_INTERRUPT_CELLS as u32) {
                interrupt_info[i as usize] = convert::read_be_u32(int_prop.value, (i * 4) as usize);
            }

            // interrupt_info[0] = irq_type (0 = SPI, 1 = PPI)
            // interrupt_info[1] = interrupt_number
            // interrupt_info[2] = flags (trigger type)
            if interrupt_info[0] == 0 {
                let spi_id = 32 + interrupt_info[1];
                // bits 0-1: edge trigger (1=rising, 2=falling)
                // bits 2-3: level trigger (4=high, 8=low)
                if (interrupt_info[2] & 0x3) != 0 {
                    gicv3::set_spi_trigger_edge(spi_id);
                } else {
                    gicv3::set_spi_trigger_level(spi_id);
                }
                gicv3::set_spi_priority(spi_id, 0x00);
                gicv3::set_spi_group1(spi_id);
                gicv3::set_spi_routing(spi_id, 0); // Route to core 0
                gicv3::enable_spi(spi_id);
            }
        }
    }

    // Parse clocks property for clock frequency
    if let Some(clocks_prop) = dev.find_property("clocks") {
        let phandle_id = convert::read_be_u32(clocks_prop.value, 0);
        if let Some(clock_node) = dtb::find_device_by_phandle(phandle_id) {
            if let Some(freq_prop) = clock_node.find_property("clock-frequency") {
                freq = convert::read_be_u32(freq_prop.value, 0);
            }
        }
    }

    init_uart(addr as *mut u32, freq);
    configure_uart();
}
