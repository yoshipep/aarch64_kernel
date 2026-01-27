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
    pub fn init(&mut self, base_addr: *mut u32, base_clock: u32) {
        self.base_addr = base_addr;
        self.base_clock = base_clock;
    }

    /// Set baud rate
    pub fn set_baudrate(&mut self, baudrate: u32) {
        self.baudrate = baudrate;
    }

    /// Set data bits
    pub fn set_data_bits(&mut self, data_bits: u8) {
        self.data_bits = data_bits;
    }

    /// Set stop bits
    pub fn set_stop_bits(&mut self, stop_bits: u8) {
        self.stop_bits = stop_bits;
    }

    /// Configure the UART hardware registers
    pub fn configure(&self) {
        // 1. Disable the UART
        mmio::write_mmio32(self.base_addr as usize, CR_OFF, 0);
        // 2. Wait for the end of TX
        while (mmio::read_mmio32(self.base_addr as usize, FR_OFF) & FR_BUSY) != 0 {}
        // 3. Flush RX/TX fifos
        mmio::clear_mmio_bits32(self.base_addr as usize, LCR_OFF, LCR_FEN);

        // 4. Set speed
        self.set_speed();

        // 5. Configure the data frame format
        let mut lcr_val: u32 = 0;
        // 5.1 Word length: bits 5 and 6
        lcr_val |= ((self.data_bits as u32 - 1) & 0x3) << 5;
        // 5.2 Use 1 or 2 stop bits: bit LCR_STP2
        if self.stop_bits == 2 {
            lcr_val |= LCR_STP2;
        }
        // 6. Enable FIFOs
        lcr_val |= LCR_FEN;

        mmio::write_mmio32(self.base_addr as usize, LCR_OFF, lcr_val);
        // 7. Enable RX interrupt
        mmio::set_mmio_bits32(self.base_addr as usize, IMSC_OFF, IMSC_RXIM);
        // 8. Disable DMA
        mmio::write_mmio32(self.base_addr as usize, DMACR_OFF, 0x01);
        // 9. Enable RX and UART
        mmio::set_mmio_bits32(self.base_addr as usize, CR_OFF, CR_UARTEN | CR_RXEN);
    }

    /// Set baud rate divisor registers
    fn set_speed(&self) {
        let baud_div = 4 * self.base_clock / self.baudrate;
        mmio::write_mmio32(self.base_addr as usize, IBRD_OFF, (baud_div >> 6) & 0xffff);
        mmio::write_mmio32(self.base_addr as usize, FBRD_OFF, baud_div & 0x3f);
    }

    /// Write a single byte
    pub fn putchar(&self, c: u8) {
        while (mmio::read_mmio32(self.base_addr as usize, FR_OFF) & FR_TXFE) != 0 {}
        mmio::write_mmio32(self.base_addr as usize, DR_OFF, c as u32);
    }
}

/// Initializes the global UART struct with hardware-specific details
fn init_uart(base_addr: *mut u32, base_clock: u32) {
    unsafe {
        (*addr_of_mut!(UART)).init(base_addr, base_clock);
    }
}

/// Sets the baud rate (call before configure_uart)
pub fn set_baudrate(baudrate: u32) {
    unsafe {
        (*addr_of_mut!(UART)).set_baudrate(baudrate);
    }
}

/// Sets the number of data bits (call before configure_uart)
pub fn set_data_bits(data_bits: u8) {
    unsafe {
        (*addr_of_mut!(UART)).set_data_bits(data_bits);
    }
}

/// Sets the number of stop bits (call before configure_uart)
pub fn set_stop_bits(stop_bits: u8) {
    unsafe {
        (*addr_of_mut!(UART)).set_stop_bits(stop_bits);
    }
}

/// Configures the UART hardware registers for operation
///
/// This function performs the hardware specific setup sequence for the PL011 UART, including
/// setting the baud rate, data format and enabling interrupts
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
pub fn getchar() -> Option<u8> {
    return RX_BUFFER.lock_irqsafe(|rx| rx.pop());
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

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::drivers::uart::pl011::_print(format_args!($($arg)*))
    };
}

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
                gicv3::set_spi_group(spi_id);
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
