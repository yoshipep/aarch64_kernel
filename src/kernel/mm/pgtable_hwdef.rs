pub mod table {
    pub const NSTABLE: u64 = 1 << 63; // next-level table in Non-Secure PA space (TrustZone only)
    pub const APTABLE1: u64 = 1 << 62; // AP override: force read-only for EL0 on the subtree
    pub const APTABLE0: u64 = 1 << 61; // AP override: deny EL0 access on the subtree
    pub const UXNTABLE: u64 = 1 << 60; // force EL0 execute-never on the subtree
    pub const PXNTABLE: u64 = 1 << 59; // force EL1 execute-never on the subtree
}

pub mod leaf {
    // Independent flags — OR together.
    pub const UXN: u64 = 1 << 54; // Unprivileged execute-never (EL0)
    pub const PXN: u64 = 1 << 53; // Privileged execute-never (EL1)
    pub const CONTIGUOUS: u64 = 1 << 52; // hint: 16 adjacent entries share one TLB entry
    pub const DBM: u64 = 1 << 51; // Dirty Bit Management (FEAT_HAFDBS)
    // Guarded Page: EL0 indirect branches to this page must target a BTI landing pad.
    // RES0 / inert without FEAT_BTI — do not set until BTI is actually implemented.
    pub const GP: u64 = 1 << 50; // Guarded Page (BTI)
    // Block descriptor transitional bit (FEAT_BBM): marks a block entry as being safely
    // re-mapped to a different block size without an intervening break-before-make sequence.
    // Valid only on L1/L2 *block* descriptors (Pud/Pmd), not L3 pages (Pte) — RES0 without
    // FEAT_BBM and unused here, since this kernel never resizes a live mapping.
    pub const NT: u64 = 1 << 16; // block-only transitional bit (FEAT_BBM)
    pub const NG: u64 = 1 << 11; // not-Global: TLB entry tagged with the current ASID
    pub const AF: u64 = 1 << 10; // Access Flag: must be 1, else first access faults
    pub const NS: u64 = 1 << 5; // Non-Secure output address (TrustZone only)

    // Bits [58:55]: ignored by hardware, reserved for OS-defined PTE flags (e.g. software
    // dirty-bit tracking, swap-entry encoding) once a use is assigned to them. Unused for now.
    pub const SW_RESERVED_SHIFT: u64 = 55;
    pub const SW_RESERVED_MASK: u64 = 0b1111 << SW_RESERVED_SHIFT;

    // Mutually-exclusive fields — pick exactly one. Modeled as enums so an invalid
    // encoding can't be built; the discriminant is the field value already in place.

    /// Shareability domain — bits \[9:8\] of a leaf descriptor. `0b01` is reserved and therefore has no variant here.
    #[derive(Clone, Copy)]
    #[repr(u64)]
    pub enum Shareability {
        /// Private to this CPU. Use for Device memory.
        NonShareable = 0b00 << 8,
        /// Coherent with DMA masters / outer caches.
        Outer = 0b10 << 8,
        /// Coherent across all CPUs. Use for Normal memory.
        Inner = 0b11 << 8,
    }

    /// Data access permission — bits \[7:6\] of a leaf descriptor
    #[derive(Clone, Copy)]
    #[repr(u64)]
    pub enum Ap {
        /// EL1 read/write, EL0 no access
        RwEl1 = 0b00 << 6,
        /// EL1 and EL0 read/write
        RwAll = 0b01 << 6,
        /// EL1 read-only, EL0 no access
        RoEl1 = 0b10 << 6,
        /// EL1 and EL0 read-only
        RoAll = 0b11 << 6,
    }
}
