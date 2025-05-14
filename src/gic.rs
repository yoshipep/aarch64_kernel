const GICR_WAKER_OFF: isize = 0x0014;
const GICR_WAKER_PSLEEP: u32 = 1 << 1;
const GICR_WAKER_CASLEEP: u32 = 1 << 2;
const GICD_ISENABLER_OFF: isize = 0x0100;

fn read_reg(base_addr: *mut u32, offset: isize) -> u32 {
    unsafe {
        return *(base_addr.offset(offset));
    }
}

fn write_reg(base_addr: *mut u32, offset: isize, val: u32, mask: u32) {
    unsafe {
        let mut contents: u32 = read_reg(base_addr, offset);
        contents &= mask;
        contents |= val;
        *(base_addr.offset(offset)) = contents;
    }
}

#[unsafe(no_mangle)]
pub fn init_gic_redistributor(base_addr: *mut u32) {
    write_reg(base_addr, GICR_WAKER_OFF, 0, !GICR_WAKER_PSLEEP);
    loop {
        let status = read_reg(base_addr, GICR_WAKER_OFF);
        if status & GICR_WAKER_CASLEEP == 0 {
            break;
        }
    }
}

#[unsafe(no_mangle)]
pub fn enable_interrupt(base_addr: *mut u32, id: u32) {
    let idx: isize = (id / 32) as isize * 4;
    let bit = id % 32;
    write_reg(base_addr, GICD_ISENABLER_OFF + idx, 1, !bit);
}
