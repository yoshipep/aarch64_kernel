use core::arch::asm;

use crate::kernel::mm::bits::{
    MAIR_DEVICE_NGNRNE, MAIR_IDX_DEVICE, MAIR_IDX_NORMAL_NC, MAIR_IDX_NORMAL_WB, MAIR_NORMAL_NC,
    MAIR_NORMAL_WB,
};

#[inline(always)]
fn configure_mair_range(conf: u64, range: usize) {
    let conf_shifted = conf << (range * 8);
    unsafe {
        asm!(
            "mrs {tmp}, mair_el1",
            "orr {tmp}, {tmp}, {conf}",
            "msr mair_el1, {tmp}",
            "isb sy",
            conf = in(reg) conf_shifted,
            tmp = out(reg) _,
            options(nostack, nomem, preserves_flags)
        );
    }
}

pub fn setup_mair_ranges() {
    // Device: non-Gathering, non-Reordering, no-EarlyWriteACK
    configure_mair_range(MAIR_DEVICE_NGNRNE, MAIR_IDX_DEVICE);
    // Normal cacheable: write-back cacheable, inner shareable
    configure_mair_range(MAIR_NORMAL_WB, MAIR_IDX_NORMAL_WB);
    // Normal non-cacheable: outer non-cacheable, inner non-cacheable
    configure_mair_range(MAIR_NORMAL_NC, MAIR_IDX_NORMAL_NC);
}
