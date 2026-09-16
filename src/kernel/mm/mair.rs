use core::arch::asm;

/* MAIR_ELx memory attribute encodings */
const MAIR_DEVICE_NGNRNE: u64 = 0x00; // Device: non-Gathering, non-Reordering, no Early Write Acknowledgement
const MAIR_NORMAL_NC: u64 = 0x44; // Normal: outer and inner non-cacheable
const MAIR_NORMAL_WB: u64 = 0xFF; // Normal: outer and inner write-back cacheable, read/write allocate

/* MAIR_ELx slot indices (used in AttrIndx field of block/page descriptors) */
pub enum MairIdx {
    Device = 0,   // slot 0 -> DEVICE_NGNRNE
    NormalNC = 1, // slot 1 -> NORMAL_NC
    NormalWb = 2, // slot 2 -> NORMAL_WB
}

#[inline(always)]
fn configure_mair_range(conf: u64, range: MairIdx) {
    let conf_shifted = conf << (range as u64 * 8);

    unsafe {
        asm!(
            "mrs {tmp}, mair_el1",
            "orr {tmp}, {tmp}, {conf}",
            "msr mair_el1, {tmp}",
            "isb sy",
            conf = in(reg) conf_shifted,
            tmp = out(reg) _,
            options(nostack, preserves_flags)
        );
    }
}

pub fn setup_mair_ranges() {
    // Device: non-Gathering, non-Reordering, no-EarlyWriteACK
    configure_mair_range(MAIR_DEVICE_NGNRNE, MairIdx::Device);

    // Normal cacheable: write-back cacheable, inner shareable
    configure_mair_range(MAIR_NORMAL_WB, MairIdx::NormalWb);

    // Normal non-cacheable: outer non-cacheable, inner non-cacheable
    configure_mair_range(MAIR_NORMAL_NC, MairIdx::NormalNC);
}
