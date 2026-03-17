/* Size constants */
pub const SZ_1G: usize = 1 << 30;

/* SCTLR_ELX bits */
pub const SCTLR_ELX_MMU: usize = 1 << 0; // MMU Enable

/* Block/Page Descriptor bits */
pub const DESC_UXN: u64 = 1 << 54; // Unprivileged Execute-Never: EL0 cannot fetch instructions from this block
pub const DESC_PXN: u64 = 1 << 53; // Privileged Execute-Never: EL1 cannot fetch instructions from this block
pub const DESC_CONTIGUOUS: u64 = 1 << 52; // Hint: 16 adjacent aligned entries can be merged into one TLB entry
pub const DESC_DBM: u64 = 1 << 51; // Dirty Bit Management: hardware auto-clears AP[2] on write (FEAT_HAFDBS)
pub const DESC_AF: u64 = 1 << 10; // Access Flag: must be 1, else first access generates a fault
pub const DESC_SH_INNER: u64 = 0b11 << 8; // Inner Shareable: coherent across all CPUs — use for Normal memory
pub const DESC_SH_OUTER: u64 = 0b10 << 8; // Outer Shareable: coherent with DMA masters and outer caches
pub const DESC_SH_NONE: u64 = 0b00 << 8; // Non-Shareable: coherency private to this CPU — use for Device memory
pub const DESC_AP_RW_EL1: u64 = 0b00 << 6; // EL1 read/write, EL0 no access
pub const DESC_AP_RW_ALL: u64 = 0b01 << 6; // EL1 and EL0 read/write
pub const DESC_AP_RO_EL1: u64 = 0b10 << 6; // EL1 read-only, EL0 no access
pub const DESC_AP_RO_ALL: u64 = 0b11 << 6; // EL1 and EL0 read-only
pub const DESC_NS: u64 = 1 << 5; // Non-Secure output address (TrustZone only, ignored at Non-Secure EL1)
pub const DESC_NG: u64 = 1 << 11; // not-Global: TLB entry tagged with current ASID (use for user mappings)

/* MAIR_EL1 memory attribute encodings */
pub const MAIR_DEVICE_NGNRNE: u64 = 0x00; // Device: non-Gathering, non-Reordering, no Early Write Acknowledgement
pub const MAIR_NORMAL_NC: u64 = 0x44; // Normal: outer and inner non-cacheable
pub const MAIR_NORMAL_WB: u64 = 0xFF; // Normal: outer and inner write-back cacheable, read/write allocate

/* MAIR_EL1 slot indices (used in AttrIndx field of block/page descriptors) */
pub const MAIR_IDX_DEVICE: usize = 0; // slot 0 -> MAIR_DEVICE_NGNRNE
pub const MAIR_IDX_NORMAL_NC: usize = 1; // slot 1 -> MAIR_NORMAL_NC
pub const MAIR_IDX_NORMAL_WB: usize = 2; // slot 2 -> MAIR_NORMAL_WB

/* Table Descriptor bits */
pub const TABLE_NSTABLE: u64 = 1 << 63; // Non-Secure table: next-level table is in Non-Secure PA space (TrustZone only, ignored at Non-Secure EL1)
pub const TABLE_APTABLE1: u64 = 1 << 62; // AP override: force read-only for EL0 on all pages in subtree
pub const TABLE_APTABLE0: u64 = 1 << 61; // AP override: deny EL0 access entirely on all pages in subtree
pub const TABLE_UXNTABLE: u64 = 1 << 60; // UXN override: force EL0 execute-never on all pages in subtree
pub const TABLE_PXNTABLE: u64 = 1 << 59; // PXN override: force EL1 execute-never on all pages in subtree
