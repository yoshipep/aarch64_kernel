//! ARM Generic Timer driver (non-secure physical timer)
//!
//! This module provides functions to interact with the ARM Generic Timer,
//! specifically the non-secure physical timer accessible at EL1.
//!
//! ## Timer Registers
//!
//! - `CNTFRQ_EL0`: Counter frequency (read-only, set by firmware)
//! - `CNTPCT_EL0`: Physical counter value (read-only, always incrementing)
//! - `CNTP_TVAL_EL0`: Timer value (countdown from this value)
//! - `CNTP_CVAL_EL0`: Compare value (fire when counter reaches this)
//! - `CNTP_CTL_EL0`: Control register (enable, mask, status)

use core::arch::asm;

use crate::drivers::gic::gicv3;
use crate::kernel::device;
use crate::kernel::dtb;
use crate::utilities::convert;

/// CNTP_CTL_EL0 bits
const CTL_ENABLE: u64 = 1 << 0;   // Timer enabled
const CTL_IMASK: u64 = 1 << 1;    // Interrupt masked
const CTL_ISTATUS: u64 = 1 << 2;  // Interrupt status (read-only)

/// Returns the timer frequency in Hz
pub fn get_frequency() -> u64 {
    let freq: u64;
    unsafe {
        asm!("mrs {}, CNTFRQ_EL0", out(reg) freq, options(nostack, nomem));
    }
    freq
}

/// Returns the current counter value
pub fn get_counter() -> u64 {
    let cnt: u64;
    unsafe {
        asm!("mrs {}, CNTPCT_EL0", out(reg) cnt, options(nostack, nomem));
    }
    cnt
}

/// Sets the timer value (countdown)
///
/// The timer will fire when the counter increments by `tval` ticks.
pub fn set_timer_value(tval: u32) {
    unsafe {
        asm!("msr CNTP_TVAL_EL0, {}", in(reg) tval as u64, options(nostack, nomem));
    }
}

/// Gets the current timer value (remaining ticks)
pub fn get_timer_value() -> u32 {
    let tval: u64;
    unsafe {
        asm!("mrs {}, CNTP_TVAL_EL0", out(reg) tval, options(nostack, nomem));
    }
    tval as u32
}

/// Sets the compare value (absolute)
///
/// The timer will fire when the counter reaches `cval`.
pub fn set_compare_value(cval: u64) {
    unsafe {
        asm!("msr CNTP_CVAL_EL0, {}", in(reg) cval, options(nostack, nomem));
    }
}

/// Gets the compare value
pub fn get_compare_value() -> u64 {
    let cval: u64;
    unsafe {
        asm!("mrs {}, CNTP_CVAL_EL0", out(reg) cval, options(nostack, nomem));
    }
    cval
}

/// Reads the control register
fn get_ctl() -> u64 {
    let ctl: u64;
    unsafe {
        asm!("mrs {}, CNTP_CTL_EL0", out(reg) ctl, options(nostack, nomem));
    }
    ctl
}

/// Writes the control register
fn set_ctl(ctl: u64) {
    unsafe {
        asm!("msr CNTP_CTL_EL0, {}", in(reg) ctl, options(nostack, nomem));
        asm!("isb", options(nostack, nomem));
    }
}

/// Enables the timer
pub fn enable() {
    set_ctl(get_ctl() | CTL_ENABLE);
}

/// Disables the timer
pub fn disable() {
    set_ctl(get_ctl() & !CTL_ENABLE);
}

/// Masks the timer interrupt (prevents interrupt from firing)
pub fn mask_interrupt() {
    set_ctl(get_ctl() | CTL_IMASK);
}

/// Unmasks the timer interrupt (allows interrupt to fire)
pub fn unmask_interrupt() {
    set_ctl(get_ctl() & !CTL_IMASK);
}

/// Returns true if the timer condition is met (timer fired)
pub fn is_pending() -> bool {
    (get_ctl() & CTL_ISTATUS) != 0
}

/// Arms the timer to fire after `ticks` counter increments
///
/// This enables the timer and unmasks the interrupt.
pub fn arm(ticks: u32) {
    set_timer_value(ticks);
    set_ctl(CTL_ENABLE); // Enable, unmask (IMASK=0)
}

/// Arms the timer to fire after `ms` milliseconds
///
/// Uses the timer frequency to calculate the appropriate tick count.
pub fn arm_ms(ms: u32) {
    let freq = get_frequency();
    let ticks = (freq / 1000) * ms as u64;
    arm(ticks as u32);
}

/// Rearms the timer with the same interval
///
/// Call this in the interrupt handler to set up the next timer tick.
pub fn rearm(ticks: u32) {
    set_timer_value(ticks);
}

/// Sets up the ARM Generic Timer from device tree properties
///
/// Parses the `interrupts` property to find the non-secure physical timer interrupt
/// (second entry in the timer node's interrupt list), then configures it as a PPI
/// in the GIC redistributor with appropriate trigger mode, priority, and group.
pub fn setup(dev: &device::PlatformDevice) {
    let mut interrupt_info: [u32; gicv3::MAX_INTERRUPT_CELLS] = [0; gicv3::MAX_INTERRUPT_CELLS];
    // Parse interrupts property
    if let Some(int_prop) = dev.find_property("interrupts") {
        if let Some(intc) = dtb::find_interrupt_parent(dev) {
            // Get #interrupt-cells from interrupt controller
            let mut interrupt_cells: u32 = 3; // Default for GICv3
            if let Some(cells_prop) = intc.find_property("#interrupt-cells") {
                interrupt_cells = convert::read_be_u32(cells_prop.value, 0);
            }

            // Read interrupt specifier cells
            let ns_offset = interrupt_cells as usize * 4;
            for i in 0..interrupt_cells.min(gicv3::MAX_INTERRUPT_CELLS as u32) {
                unsafe {
                    interrupt_info[i as usize] =
                        convert::read_be_u32(int_prop.value.add(ns_offset), (i * 4) as usize);
                }
            }

            // interrupt_info[0] = irq_type (0 = SPI, 1 = PPI)
            // interrupt_info[1] = interrupt_number
            // interrupt_info[2] = flags (trigger type)
            if interrupt_info[0] == 1 {
                let ppi_id = 16 + interrupt_info[1];
                // bits 0-1: edge trigger (1=rising, 2=falling)
                // bits 2-3: level trigger (4=high, 8=low)
                if (interrupt_info[2] & 0x3) != 0 {
                    gicv3::set_ppi_trigger_edge(ppi_id);
                } else {
                    gicv3::set_ppi_trigger_level(ppi_id);
                }
                gicv3::set_ppi_priority(ppi_id, 0x00);
                gicv3::set_ppi_group(ppi_id);
                gicv3::enable_ppi(ppi_id);
            }
        }
    }
}
