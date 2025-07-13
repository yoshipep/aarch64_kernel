use core::ptr::write_volatile;
use core::sync::atomic::AtomicUsize;

use crate::sync::Mutex;
use crate::utilities::{read_mmio, set_mmio_bits, write_mmio};
use core::sync::atomic::Ordering;

const UART_BUFFER_SIZE: usize = 256;

pub struct UartBuffer {
    buffer: [u8; UART_BUFFER_SIZE],
    head: AtomicUsize,
    tail: AtomicUsize,
}

pub static RX_BUFFER: Mutex<UartBuffer> = Mutex::new(UartBuffer {
    buffer: [0; UART_BUFFER_SIZE],
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
});

impl UartBuffer {
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

    fn pop(&mut self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);

        if self.head.load(Ordering::Relaxed) == tail {
            return None;
        }

        let byte = self.buffer[tail];
        let next_tail = (tail + 1) % UART_BUFFER_SIZE;
        self.tail.store(next_tail, Ordering::Relaxed);
        Some(byte)
    }
}

// Define the address of the UART device (MMIO)
// In C: volatile uint8_t *uart = ...
struct UartPl011 {
    base_addr: *mut u32,
    base_clock: u32,
    baudrate: u32,
    data_bits: u8,
    stop_bits: u8,
}

const DR_OFF: usize = 0x00;
const FR_OFF: usize = 0x18;
const FR_BUSY: u32 = 1 << 3;
const IBRD_OFF: usize = 0x24;
const FBRD_OFF: usize = 0x28;
const LCR_OFF: usize = 0x2c;
const LCR_FEN: u32 = 1 << 4;
const LCR_STP2: u32 = 1 << 3;
const CR_OFF: usize = 0x30;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXEN: u32 = 1 << 8;
const IMSC_OFF: usize = 0x38;
const IMSC_RXIM: u32 = 1 << 4;
const IMSC_TXIM: u32 = 1 << 5;
const DMACR_OFF: usize = 0x48;

static mut UART: UartPl011 = UartPl011 {
    base_addr: core::ptr::null_mut(),
    base_clock: 0,
    baudrate: 0,
    data_bits: 0,
    stop_bits: 0,
};

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

#[unsafe(no_mangle)]
pub fn configure_uart() {
    unsafe {
        write_mmio(UART.base_addr as usize, CR_OFF, 0);
    }
    set_uart_low_speed();
    let mut lcr_val: u32 = 0;
    unsafe {
        lcr_val |= ((UART.data_bits as u32 - 1) & 0x3) << 5;
        lcr_val |= LCR_FEN;
    }
    unsafe {
        write_mmio(UART.base_addr as usize, LCR_OFF, lcr_val);
    }
    // 6. Mask all interrupts
    unsafe {
        write_mmio(UART.base_addr as usize, IMSC_OFF, 0x00);
        set_mmio_bits(UART.base_addr as usize, IMSC_OFF, IMSC_RXIM);
        // set_mmio_bits(UART.base_addr as usize, IMSC_OFF, IMSC_TXIM);
    }
    // 9. Enable UART
    unsafe {
        let cr_val = CR_UARTEN | CR_TXEN | (1 << 9);
        set_mmio_bits(UART.base_addr as usize, CR_OFF, cr_val);
    }
}

// #[unsafe(no_mangle)]
// pub fn configure_uart() {
//     // 1. Disable the UART
//     unsafe {
//         clear_mmio_bits(UART.base_addr as usize, CR_OFF, CR_UARTEN);
//     }
//     // 2. Wait for the end of TX
//     loop {
//         if uart_ready() {
//             break;
//         }
//     }
//     // 3. Flush TX FIFO
//     unsafe {
//         set_mmio_bits(UART.base_addr as usize, LCR_OFF, LCR_FEN);
//     }
//     // 4. Set speed
//     set_uart_low_speed();
//     // 5. Configure the data frame format
//     // 5.1 Word length: bits 5 and 6
//     let mut cfg: u32 = 0;
//     unsafe {
//         cfg |= (((UART.data_bits - 1) & 0x3) << 5) as u32;
//         // 5.2 Use 1 or 2 stop bits: bit LCR_STP2
//         if UART.stop_bits == 2 {
//             cfg |= LCR_STP2;
//         }
//     }
//     unsafe {
//         write_mmio(UART.base_addr as usize, LCR_OFF, cfg & 0xff37);
//     }
//     // 6. Mask all interrupts
//     unsafe {
//         write_mmio(UART.base_addr as usize, IMSC_OFF, 0x00);
//         set_mmio_bits(UART.base_addr as usize, IMSC_OFF, IMSC_RXIM);
//         set_mmio_bits(UART.base_addr as usize, IMSC_OFF, IMSC_TXIM);
//     }
//     // 7. Disable DMA
//     unsafe {
//         clear_mmio_bits(UART.base_addr as usize, DMACR_OFF, 0x7);
//     }
//     // 8. Enable TX
//     unsafe {
//         set_mmio_bits(UART.base_addr as usize, CR_OFF, CR_TXEN);
//     }
//     // 9. Enable UART
//     unsafe {
//         set_mmio_bits(UART.base_addr as usize, CR_OFF, CR_UARTEN);
//     }
// }

#[unsafe(no_mangle)]
fn uart_ready() -> bool {
    unsafe {
        return (read_mmio(UART.base_addr as usize, FR_OFF) & FR_BUSY) == 0;
    }
}

fn uart_set_speed() {
    unsafe {
        let baud_div = (UART.base_clock * 1000) / (16 * UART.baudrate);
        let ibrd = baud_div / 1000;
        let fbrd = (((baud_div % 1000) * 64 + 500) / 1000) as u32;
        write_mmio(UART.base_addr as usize, IBRD_OFF, ibrd);
        write_mmio(UART.base_addr as usize, FBRD_OFF, fbrd & 0x3f);
    }
}

fn set_uart_low_speed() {
    unsafe {
        write_mmio(UART.base_addr as usize, IBRD_OFF, (1 << 16) - 1);
        write_mmio(UART.base_addr as usize, FBRD_OFF, 0);
    }
}

pub fn putchar(c: u8) {
    unsafe {
        loop {
            // if uart_ready() {
            //     break;
            // }
            if (read_mmio(UART.base_addr as usize, FR_OFF) & (1 << 5)) == 0 {
                break;
            }
        }
        let addr = UART.base_addr as *mut u8;
        write_volatile(addr.add(DR_OFF), c);
    }
}

pub fn getchar() -> Option<u8> {
    RX_BUFFER.lock_irqsafe(|rx| rx.pop())
}

pub fn print(s: &[u8]) {
    for &c in s {
        putchar(c);
    }
}
