// Define the address of the UART device (MMIO)
// In C: volatile uint8_t *uart = ...
struct UartPl011 {
    base_addr: *mut u32,
    base_clock: u32,
    baudrate: u32,
    data_bits: u8,
    stop_bits: u8,
}

const DR_OFF: isize = 0x00;
const FR_OFF: isize = 0x18;
const FR_BUSY: u32 = 1 << 3;
const IBRD_OFF: isize = 0x24;
const FBRD_OFF: isize = 0x28;
const LCR_OFF: isize = 0x2c;
const LCR_FEN: u32 = 1 << 4;
const LCR_STP2: u32 = 1 << 3;
const CR_OFF: isize = 0x30;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXEN: u32 = 1 << 8;
const IMSC_OFF: isize = 0x38;
const DMACR_OFF: isize = 0x48;

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

fn read_reg(offset: isize) -> u32 {
    unsafe {
        return *(UART.base_addr.offset(offset));
    }
}

fn write_reg(offset: isize, val: u32) {
    unsafe {
        let mut contents: u32 = *(UART.base_addr.offset(offset));
        // Mask to save the bits that we are not overwriting
        contents &= !val;
        contents |= val;
        *(UART.base_addr.offset(offset)) = contents;
    }
}

#[unsafe(no_mangle)]
pub fn configure_uart() {
    // 1. Disable the UART
    disable_uart();
    // 2. Wait for the end of TX
    loop {
        if uart_ready() {
            break;
        }
    }
    // 3. Flush TX FIFO
    uart_flush_tx_fifo();
    // 4. Set speed
    set_uart_low_speed();
    // 5. Configure the data frame format
    // 5.1 Word length: bits 5 and 6
    let mut cfg: u32 = 0;
    unsafe {
        cfg |= (((UART.data_bits - 1) & 0x3) << 5) as u32;
        // 5.2 Use 1 or 2 stop bits: bit LCR_STP2
        if UART.stop_bits == 2 {
            cfg |= LCR_STP2;
        }
    }
    uart_write_lcr(cfg);
    // 6. Mask all interrupts
    uart_write_msc(0x7ff);
    // 7. Disable DMA
    uart_write_dmacr(0);
    // 8. Enable TX
    uart_write_cr(CR_TXEN);
    // 9. Enable UART
    uart_write_cr(CR_UARTEN);
}

fn disable_uart() {
    uart_write_cr(CR_UARTEN);
}

fn uart_ready() -> bool {
    return (read_reg(FR_OFF) & FR_BUSY) == 0;
}

fn uart_flush_tx_fifo() {
    write_reg(LCR_OFF, LCR_FEN);
}

fn uart_write_lcr(cfg: u32) {
    write_reg(LCR_OFF, cfg);
}

fn uart_write_cr(cfg: u32) {
    write_reg(CR_OFF, cfg);
}

fn uart_write_msc(mask: u32) {
    write_reg(IMSC_OFF, mask);
}

fn uart_write_dmacr(mask: u32) {
    write_reg(DMACR_OFF, mask);
}

fn uart_set_speed() {
    unsafe {
        let baud_div = (UART.base_clock * 1000) / (16 * UART.baudrate);
        let ibrd = baud_div / 1000;
        let fbrd = (((baud_div % 1000) * 64 + 500) / 1000) as u32;

        write_reg(IBRD_OFF, ibrd & 0xffff);
        write_reg(FBRD_OFF, fbrd & 0x3f);
    }
}

fn set_uart_low_speed() {
    write_reg(IBRD_OFF, (1 << 16) - 1);
    write_reg(FBRD_OFF, 0);
}

fn putchar(c: u8) {
    unsafe {
        loop {
            if uart_ready() {
                break;
            }
        }
        *(UART.base_addr.offset(DR_OFF) as *mut u8) = c;
    }
}

pub fn print(s: &[u8]) {
    for &c in s {
        putchar(c);
    }
}
