//! Shared system-register bitfield constants
//!
//! Named bits for AArch64 system registers used across the kernel's GIC, timer, and mm code, factored out here so each
//! driver doesn't redefine the same fields.

/// `SCTLR_EL1` bits
pub mod sctlr {
    /// MMU enable
    pub const MMU: u64 = 1 << 0;
    /// Alignment check
    pub const A: u64 = 1 << 1;
    /// Stage 1 cacheability
    pub const C: u64 = 1 << 2;
    /// SP alignment check
    pub const SA: u64 = 1 << 3;
    /// Write execute never
    pub const WXN: u64 = 1 << 19;
    /// Endianness of data accesses
    pub const EE: u64 = 1 << 25;
}

/// `TCR_EL1` bits
pub mod tcr {
    /// Bits \[5:0\]: 48-bit VA (TTBR0)
    pub const T0SZ_48: u64 = 16;
    /// Bits \[9:8\]: inner WB, read/write-allocate
    pub const IRGN0_WBWA: u64 = 0b01 << 8;
    /// Bits \[11:10\]: outer WB, read/write-allocate
    pub const ORGN0_WBWA: u64 = 0b01 << 10;
    /// Bits \[13:12\]: inner shareable
    pub const SH0_INNER: u64 = 0b11 << 12;
    /// Bits \[15:14\]: 4KB granule (TTBR0) = `0b00`
    pub const TG0_4K: u64 = 0;
    /// Bits \[21:16\]: 48-bit VA (TTBR1)
    pub const T1SZ_48: u64 = 16 << 16;
    /// Bit \[23\]: disable TTBR1 walks
    pub const EPD1: u64 = 1 << 23;
    /// Bits \[31:30\]: 4KB granule (TTBR1) — `0b10`, not `0b00`
    pub const TG1_4K: u64 = 0b10 << 30;
    /// Bits \[34:32\]: 44-bit PA (16 TB)
    pub const IPS_44: u64 = 0b100 << 32;
}

/// GICv3 CPU interface registers (ICC_*), accessed via MRS/MSR
pub mod icc {
    /// `ICC_IGRPEN1_EL1.Enable`: enable Group 1 interrupts
    pub const IGRPEN1_ENABLE: u64 = 1 << 0;
}

/// ENABLE/IMASK/ISTATUS are identical in every generic-timer control register: `CNTP_CTL_EL0`, `CNTV_CTL_EL0`,
/// `CNTHP_CTL_EL2`, `CNTPS_CTL_EL1`.
pub mod timer_ctl {
    /// Timer enabled
    pub const ENABLE: u64 = 1 << 0;
    /// 1 = Interrupt masked
    pub const IMASK: u64 = 1 << 1;
    /// Read-only: timer condition met
    pub const ISTATUS: u64 = 1 << 2;
}
