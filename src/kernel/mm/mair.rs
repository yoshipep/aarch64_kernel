//! MAIR_EL1 configuration
//!
//! MAIR_EL1 is a palette of 8 memory-attribute presets. Each page-table descriptor's AttrIndx field (bits \[4:2\])
//! indexes into this palette rather than encoding the attribute directly — see `MairIdx` for the slot layout this
//! kernel uses.

use core::arch::asm;

/* MAIR_ELx memory attribute encodings */
const MAIR_DEVICE_NGNRNE: u64 = 0x00; // Device: non-Gathering, non-Reordering, no Early Write Acknowledgement
const MAIR_NORMAL_NC: u64 = 0x44; // Normal: outer and inner non-cacheable
const MAIR_NORMAL_WB: u64 = 0xFF; // Normal: outer and inner write-back cacheable, read/write allocate

/// MAIR_ELx slot indices, used in the AttrIndx field of block/page descriptors
pub enum MairIdx {
    /// Device-nGnRnE — non-Gathering, non-Reordering, no Early Write Acknowledgement. Use for MMIO.
    Device = 0,
    /// Normal memory, outer and inner non-cacheable. Use for DMA buffers.
    NormalNC = 1,
    /// Normal memory, outer and inner write-back cacheable, read/write allocate. Use for RAM (kernel code, data,
    /// stack).
    NormalWb = 2,
}

/// Writes one 8-bit attribute encoding into the given MAIR_EL1 slot
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

/// Configures all three MAIR_EL1 slots used by this kernel's page tables
///
/// Must run before any page table that references `MairIdx::Device`, `NormalNC`, or `NormalWb` is walked by the MMU —
/// see `setup_identity_mapping`.
pub fn setup_mair_ranges() {
    // Device: non-Gathering, non-Reordering, no-EarlyWriteACK
    configure_mair_range(MAIR_DEVICE_NGNRNE, MairIdx::Device);

    // Normal cacheable: write-back cacheable, inner shareable
    configure_mair_range(MAIR_NORMAL_WB, MairIdx::NormalWb);

    // Normal non-cacheable: outer non-cacheable, inner non-cacheable
    configure_mair_range(MAIR_NORMAL_NC, MairIdx::NormalNC);
}
