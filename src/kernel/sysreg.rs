pub mod sctlr {
    pub const MMU: u64 = 1 << 0; // MMU enable
    pub const A: u64 = 1 << 1; // Alignment check
    pub const C: u64 = 1 << 2; // Stage 1 cacheability
    pub const SA: u64 = 1 << 3; // SP alignment check
    pub const WXN: u64 = 1 << 19; // Write execute never
    pub const EE: u64 = 1 << 25; // Endianness of data accesses
}

pub mod tcr {
    pub const T0SZ_48: u64 = 16; // [5:0]   48-bit VA (TTBR0)
    pub const IRGN0_WBWA: u64 = 0b01 << 8; // [9:8]   inner WB, read/write-allocate
    pub const ORGN0_WBWA: u64 = 0b01 << 10; // [11:10] outer WB, read/write-allocate
    pub const SH0_INNER: u64 = 0b11 << 12; // [13:12] inner shareable
    pub const TG0_4K: u64 = 0; // [15:14] 4KB granule (TTBR0) = 0b00
    pub const T1SZ_48: u64 = 16 << 16; // [21:16] 48-bit VA (TTBR1)
    pub const EPD1: u64 = 1 << 23; // [23]    disable TTBR1 walks
    pub const TG1_4K: u64 = 0b10 << 30; // [31:30] 4KB granule (TTBR1) — 0b10, not 0b00
    pub const IPS_44: u64 = 0b100 << 32; // [34:32] 44-bit PA (16 TB)
}

// GICv3 CPU interface registers (ICC_*), accessed via MRS/MSR
pub mod icc {
    pub const IGRPEN1_ENABLE: u64 = 1 << 0; // ICC_IGRPEN1_EL1.Enable: enable Group 1 interrupts
}

// ENABLE/IMASK/ISTATUS are identical in every generic-timer control register:
// CNTP_CTL_EL0, CNTV_CTL_EL0, CNTHP_CTL_EL2, CNTPS_CTL_EL1.
pub mod timer_ctl {
    pub const ENABLE: u64 = 1 << 0; // Timer enabled
    pub const IMASK: u64 = 1 << 1; // 1 = Interrupt masked
    pub const ISTATUS: u64 = 1 << 2; // Read-only: timer condition met
}
