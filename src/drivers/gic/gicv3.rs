//! GICv3 interrupt controller driver
//!
//! This module provides functions to initialize and configure the ARM GICv3 interrupt controller. It manages both the
//! Distributor (GICD) for Shared Peripheral Interrupts (SPIs) and the Redistributor (GICR) for Private Peripheral
//! Interrupts (PPIs) and Software Generated Interrupts (SGIs).
//!
//! The driver uses a global `Gicv3` instance accessed through public wrapper functions. Base addresses are discovered
//! from the device tree during boot.

use core::arch::asm;
use core::ptr::addr_of_mut;

use crate::kernel::device;
use crate::kernel::sysreg::icc;
use crate::utilities::convert;
use crate::utilities::mmio;

/// Maximum number of cells in a GIC interrupt specifier
///
/// The GICv3 binding requires `#interrupt-cells` to be at least 4 (see the Linux kernel's
/// `Documentation/devicetree/bindings/interrupt-controller/arm,gic-v3.yaml`).
pub const MAX_INTERRUPT_CELLS: usize = 4;

/// Priority mask that lets every interrupt through (0xFF is the lowest threshold)
const PRIORITY_MASK_ALL: u8 = 0xFF;

/* --- GICD (Distributor) register offsets --- */
#[allow(dead_code)]
mod gicd {
    pub const CTLR: usize = 0x000; // Distributor Control
    pub const IGROUPR: usize = 0x080; // Interrupt Group
    pub const ISENABLER: usize = 0x100; // Interrupt Set-Enable
    pub const IPRIORITYR: usize = 0x400; // Interrupt Priority
    pub const ICFGR: usize = 0xC00; // Interrupt Configuration
    pub const IROUTER: usize = 0x6100; // Interrupt Routing
}

/* --- GICR (Redistributor) register offsets --- */
#[allow(dead_code)]
mod gicr {
    pub const SGI_BASE: usize = 0x10000; // Offset from RD_base to the SGI/PPI frame
    pub const WAKER: usize = 0x0014; // Redistributor Wake (in the RD frame)
    /* The following live inside the SGI/PPI frame (RD_base + SGI_BASE) */
    pub const IGROUPR0: usize = 0x080; // Interrupt Group 0
    pub const ISENABLER0: usize = 0x100; // Interrupt Set-Enable 0
    pub const IPRIORITYR: usize = 0x400; // Interrupt Priority
    pub const ICFGR: usize = 0xC00; // Interrupt Configuration
}

/* --- GICD_CTLR bits ---
 * The layout depends on how many Security states the Distributor supports (Arm IHI 0069H.b 12.9.4).
 * QEMU virt runs without EL3 (no `secure=on`), so there is a single Security state and every access
 * -- including ours from Non-secure EL1 -- sees the single-Security-state view:
 *     [0] EnableGrp0   [1] EnableGrp1   [4] ARE   [6] DS   [7] E1NWF   [31] RWP
 * The two-Security-state views differ, so these constants are not portable to a system with EL3:
 *     Secure:     [1] EnableGrp1NS  [2] EnableGrp1S  [4] ARE_S  [5] ARE_NS
 *     Non-secure: [0] EnableGrp1    [1] EnableGrp1A  [4] ARE_NS
 */
#[allow(dead_code)]
mod gicd_ctlr {
    pub const ENABLE_GRP0: u32 = 1 << 0; // Enable Group 0 interrupts
    pub const ENABLE_GRP1: u32 = 1 << 1; // Enable Group 1 interrupts
    // Affinity Routing Enable. Do NOT write it: RAO/WI when GICv2 backwards compatibility is
    // absent (so already in effect here), and changing it 0 -> 1 once a group is enabled is
    // UNPREDICTABLE.
    pub const ARE: u32 = 1 << 4;
}

/* --- GICR_WAKER bits --- */
#[allow(dead_code)]
mod gicr_waker {
    pub const PSLEEP: u32 = 1 << 1; // Processor sleep: may assert WakeRequest
    pub const CASLEEP: u32 = 1 << 2; // Children asleep: the connected PE is quiescent
}

/// Interrupt trigger mode — the 2-bit ICFGR field for one interrupt
#[derive(Clone, Copy)]
#[repr(u32)]
enum Trigger {
    Level = 0b00,
    Edge = 0b10,
}

/* --- GIC register-array layout ---
 * Several GIC registers are arrays in which each interrupt owns a fixed-width field, so an
 * interrupt id maps to a (register index, bit shift) pair.
 */
mod layout {
    pub const REG_BYTES: usize = 4; // these register arrays are 32-bit

    // IPRIORITYR: an 8-bit priority per interrupt -> 4 interrupts per register
    pub const PRIORITY_PER_REG: u32 = 4;
    pub const PRIORITY_BITS: u32 = 8;
    pub const PRIORITY_MASK: u32 = 0xFF;

    // ICFGR: a 2-bit config field per interrupt -> 16 interrupts per register
    pub const CONFIG_PER_REG: u32 = 16;
    pub const CONFIG_BITS: u32 = 2;
    pub const CONFIG_MASK: u32 = 0b11;

    // ISENABLER / IGROUPR: a single bit per interrupt -> 32 interrupts per register
    pub const ENABLE_PER_REG: u32 = 32;

    // IROUTER: one 64-bit register per interrupt
    pub const ROUTER_STRIDE: usize = 8;
}

/// Global GICv3 instance holding the distributor and redistributor base addresses
static mut GIC: Gicv3 = Gicv3::new();

/// GICv3 interrupt controller state
///
/// Holds the MMIO base addresses for the GIC Distributor (GICD) and Redistributor (GICR) regions. These are populated
/// during device tree parsing and used by all GIC operations.
struct Gicv3 {
    /// Base address of the GIC Distributor (GICD) registers
    dist_addr: usize,
    /// Base address of the GIC Redistributor (GICR) registers
    redist_addr: usize,
}

impl Gicv3 {
    /// Const constructor for static initialization with zeroed addresses
    pub const fn new() -> Self {
        Self {
            dist_addr: 0,
            redist_addr: 0,
        }
    }

    /// Initializes the GIC Distributor
    pub fn init_gic_distributor(&self) {
        unsafe {
            mmio::set_mmio_bits32(self.dist_addr, gicd::CTLR, gicd_ctlr::ENABLE_GRP1);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Initializes the GIC Redistributor
    pub fn init_gic_redistributor(&self) {
        unsafe {
            mmio::clear_mmio_bits32(self.redist_addr, gicr::WAKER, gicr_waker::PSLEEP);
            asm!("dsb sy", options(nostack));
            while (mmio::read_mmio32(self.redist_addr, gicr::WAKER) & gicr_waker::CASLEEP) != 0 {}
        }
    }

    /// Sets the priority of a PPI/SGI in the redistributor
    ///
    /// # Arguments
    ///
    /// * `id` - PPI/SGI interrupt id (0-31)
    /// * `prio` - priority value; lower numbers are higher priority
    pub fn set_ppi_priority(&self, id: u32, prio: u8) {
        unsafe {
            let sgi_base = self.redist_addr + gicr::SGI_BASE;
            let reg_index = id / layout::PRIORITY_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let prio_reg_addr = sgi_base + gicr::IPRIORITYR + reg_offset;
            let bit_shift = (id % layout::PRIORITY_PER_REG) * layout::PRIORITY_BITS;

            let mut reg_val = mmio::read_mmio32(prio_reg_addr, 0);
            reg_val &= !(layout::PRIORITY_MASK << bit_shift);
            reg_val |= (prio as u32) << bit_shift;

            mmio::write_mmio32(prio_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Assigns the PPI/SGI `id` (0-31) to Group 1
    pub fn set_ppi_group1(&self, id: u32) {
        unsafe {
            mmio::set_mmio_bits32(self.redist_addr + gicr::SGI_BASE, gicr::IGROUPR0, 1 << id);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Enables the PPI/SGI with the given `id` (0-31)
    ///
    /// PPI (Private Peripheral Interrupt) and SGI (Software Generated Interrupt) share the redistributor's 0-31 ID
    /// range: PPIs are per-core hardware interrupts (e.g. the local timer), SGIs are software-triggered
    /// (inter-processor interrupts). Both go through the same redistributor registers, so this driver doesn't
    /// distinguish between them.
    pub fn enable_ppi(&self, id: u32) {
        unsafe {
            mmio::set_mmio_bits32(self.redist_addr + gicr::SGI_BASE, gicr::ISENABLER0, 1 << id);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Sets the priority of an SPI in the distributor
    ///
    /// # Arguments
    ///
    /// * `id` - SPI interrupt id (32+)
    /// * `prio` - priority value; lower numbers are higher priority
    pub fn set_spi_priority(&self, id: u32, prio: u8) {
        unsafe {
            let reg_index = id / layout::PRIORITY_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let prio_reg_addr = self.dist_addr + gicd::IPRIORITYR + reg_offset;
            let bit_shift = (id % layout::PRIORITY_PER_REG) * layout::PRIORITY_BITS;

            let mut reg_val = mmio::read_mmio32(prio_reg_addr, 0);
            reg_val &= !(layout::PRIORITY_MASK << bit_shift);
            reg_val |= (prio as u32) << bit_shift;

            mmio::write_mmio32(prio_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Configures the SPI `id` (32+) to be level-sensitive (0b00 in ICFGR)
    pub fn set_spi_trigger_level(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::CONFIG_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let cfg_reg_addr = self.dist_addr + gicd::ICFGR + reg_offset;
            let bit_shift = (id % layout::CONFIG_PER_REG) * layout::CONFIG_BITS;

            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            reg_val &= !(layout::CONFIG_MASK << bit_shift);
            reg_val |= (Trigger::Level as u32) << bit_shift;

            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Configures the SPI `id` (32+) to be edge-triggered (0b10 in ICFGR)
    pub fn set_spi_trigger_edge(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::CONFIG_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let cfg_reg_addr = self.dist_addr + gicd::ICFGR + reg_offset;
            let bit_shift = (id % layout::CONFIG_PER_REG) * layout::CONFIG_BITS;

            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            reg_val &= !(layout::CONFIG_MASK << bit_shift);
            reg_val |= (Trigger::Edge as u32) << bit_shift;

            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Enables forwarding of SPI `id` (32+) in the GIC distributor
    ///
    /// SPI (Shared Peripheral Interrupt) is a hardware interrupt from a device (e.g. this kernel's UART) — routed
    /// through the distributor and, with affinity routing, deliverable to any core. Distinct from a PPI, which is
    /// private to one core.
    pub fn enable_spi(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::ENABLE_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let enabler_reg_addr = self.dist_addr + gicd::ISENABLER + reg_offset;
            let bit_to_set = 1 << (id % layout::ENABLE_PER_REG);

            mmio::write_mmio32(enabler_reg_addr, 0, bit_to_set);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Sets the routing target for an SPI, when affinity routing is enabled
    ///
    /// # Arguments
    ///
    /// * `id` - SPI interrupt id (32+)
    /// * `core_affinity` - target core, encoded as an `ICC_SGI1R_EL1`-style affinity value, written directly into the
    ///   SPI's `IROUTER` register
    pub fn set_spi_routing(&self, id: u32, core_affinity: u64) {
        unsafe {
            let router_reg_addr =
                self.dist_addr + gicd::IROUTER + (layout::ROUTER_STRIDE * id as usize);
            let router_ptr = router_reg_addr as *mut u64;

            core::ptr::write_volatile(router_ptr, core_affinity);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Assigns the SPI `id` (32+) to Group 1
    pub fn set_spi_group1(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::ENABLE_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let group_reg_addr = self.dist_addr + gicd::IGROUPR + reg_offset;
            let bit_to_set = 1 << (id % layout::ENABLE_PER_REG);

            mmio::set_mmio_bits32(group_reg_addr, 0, bit_to_set);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Configures the PPI `id` (0-31) to be level-sensitive (0b00 in ICFGR)
    pub fn set_ppi_trigger_level(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::CONFIG_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let bit_shift = (id % layout::CONFIG_PER_REG) * layout::CONFIG_BITS;
            let sgi_base = self.redist_addr + gicr::SGI_BASE;
            let cfg_reg_addr = sgi_base + gicr::ICFGR + reg_offset;

            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            reg_val &= !(layout::CONFIG_MASK << bit_shift);
            reg_val |= (Trigger::Level as u32) << bit_shift;

            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }

    /// Configures the PPI `id` (0-31) to be edge-triggered (0b10 in ICFGR)
    pub fn set_ppi_trigger_edge(&self, id: u32) {
        unsafe {
            let reg_index = id / layout::CONFIG_PER_REG;
            let reg_offset = reg_index as usize * layout::REG_BYTES;
            let bit_shift = (id % layout::CONFIG_PER_REG) * layout::CONFIG_BITS;
            let sgi_base = self.redist_addr + gicr::SGI_BASE;
            let cfg_reg_addr = sgi_base + gicr::ICFGR + reg_offset;

            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            reg_val &= !(layout::CONFIG_MASK << bit_shift);
            reg_val |= (Trigger::Edge as u32) << bit_shift;

            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack));
        }
    }
}

/// Initializes the GIC with the given distributor and redistributor addresses
///
/// Stores the base addresses and initializes both the distributor (enables Group 1 interrupts — single-Security-state
/// view, see `gicd_ctlr`) and redistributor (wakes the PE from sleep).
///
/// Affinity routing is not enabled here: GICD_CTLR.ARE is RAO/WI when GICv2 backwards compatibility is absent, so it is
/// already in effect and IROUTER writes take effect.
fn init_gic(dist_addr: usize, redist_addr: usize) {
    unsafe {
        (*addr_of_mut!(GIC)).dist_addr = dist_addr;
        (*addr_of_mut!(GIC)).redist_addr = redist_addr;
        (*addr_of_mut!(GIC)).init_gic_distributor();
        (*addr_of_mut!(GIC)).init_gic_redistributor();
    }
}

// Public wrapper functions for SPI (distributor) access

/// Enables forwarding of the SPI `id` (32+) in the GIC distributor
///
/// SPI (Shared Peripheral Interrupt) is a hardware interrupt from a device (e.g. this kernel's UART) — routed through
/// the distributor and, with affinity routing, deliverable to any core. Distinct from a PPI, which is private to one
/// core.
pub fn enable_spi(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).enable_spi(id);
    }
}

/// Sets the priority of an SPI in the distributor
///
/// # Arguments
///
/// * `id` - SPI interrupt id (32+)
/// * `prio` - priority value; lower numbers are higher priority
pub fn set_spi_priority(id: u32, prio: u8) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_priority(id, prio);
    }
}

/// Sets level-sensitive trigger mode for SPI `id` (32+)
pub fn set_spi_trigger_level(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_trigger_level(id);
    }
}

/// Sets edge-triggered mode for SPI `id` (32+)
pub fn set_spi_trigger_edge(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_trigger_edge(id);
    }
}

/// Assigns SPI `id` (32+) to Group 1
pub fn set_spi_group1(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_group1(id);
    }
}

/// Sets the routing target for an SPI, when affinity routing is enabled
///
/// # Arguments
///
/// * `id` - SPI interrupt id (32+)
/// * `core_affinity` - target core, encoded as an `ICC_SGI1R_EL1`-style affinity value
pub fn set_spi_routing(id: u32, core_affinity: u64) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_routing(id, core_affinity);
    }
}

// Public wrapper functions for PPI/SGI (redistributor)

/// Sets the priority of a PPI/SGI in the redistributor
///
/// # Arguments
///
/// * `id` - PPI/SGI interrupt id (0-31)
/// * `prio` - priority value; lower numbers are higher priority
pub fn set_ppi_priority(id: u32, prio: u8) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_priority(id, prio);
    }
}

/// Assigns PPI/SGI `id` (0-31) to Group 1 in the redistributor
pub fn set_ppi_group1(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_group1(id);
    }
}

/// Enables PPI/SGI `id` (0-31) in the redistributor
///
/// PPI (Private Peripheral Interrupt) and SGI (Software Generated Interrupt) share this 0-31 ID range: PPIs are
/// per-core hardware interrupts (e.g. the local timer), SGIs are software-triggered (inter-processor interrupts). Both
/// go through the same redistributor registers, so this driver doesn't distinguish between them.
pub fn enable_ppi(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).enable_ppi(id);
    }
}

/// Sets level-sensitive trigger mode for PPI `id` (0-31)
pub fn set_ppi_trigger_level(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_trigger_level(id);
    }
}

/// Sets edge-triggered mode for PPI `id` (0-31)
pub fn set_ppi_trigger_edge(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_trigger_edge(id);
    }
}

/// Sets the interrupt priority mask (ICC_PMR_EL1)
///
/// # Arguments
///
/// * `priority` - priority threshold; only interrupts with a higher priority (a numerically lower value) than this are
///   signaled to the PE
#[inline(always)]
pub fn set_priority_mask(priority: u8) {
    unsafe {
        asm!("msr ICC_PMR_EL1, {}", in(reg) priority as u64, options(nostack, nomem, preserves_flags));
    }
}

/// Enable the Group 1 interrupts
///
/// GICv3 partitions interrupts into Group 0 (traditionally routed to FIQ, secure-world use) and Group 1 (routed to IRQ
/// — what this kernel's timer and UART interrupts use, assigned via `set_spi_group1`/`set_ppi_group1`). This gates them
/// at the CPU interface (ICC_IGRPEN1_EL1); the distributor-side Group 1 enable is set separately in
/// `init_gic_distributor`.
#[inline(always)]
pub fn enable_grp1_ints() {
    unsafe {
        asm!(
            "mrs {tmp}, ICC_IGRPEN1_EL1",
            "orr {tmp}, {tmp}, {enable}",
            "msr ICC_IGRPEN1_EL1, {tmp}",
            "isb sy",
            enable = in(reg) icc::IGRPEN1_ENABLE,
            tmp = out(reg) _,
            options(nostack, preserves_flags)
        );
    }
}

/// Sets up the GICv3 from device tree properties
///
/// Parses the `reg` property to extract the distributor (GICD) and redistributor (GICR) base addresses, initializes the
/// GIC hardware, sets the CPU interface priority mask to accept all priorities, and enables Group 1 interrupts.
pub fn setup(dev: &device::PlatformDevice) {
    let mut gicd_addr: usize = 0;
    let mut gicr_addr: usize = 0;

    // Get #address-cells and #size_cells
    let (addr_cells, size_cells) = dev.get_parent_cells();
    if let Some(reg_prop) = dev.find_property("reg") {
        for i in 0..addr_cells as usize {
            let cell = convert::read_be_u32(reg_prop.value, i * 4);
            gicd_addr = (gicd_addr << 32) | cell as usize;
        }

        let gicr_off = (addr_cells + size_cells) as usize * 4; // Convert cells to bytes
        for i in 0..addr_cells as usize {
            unsafe {
                let cell = convert::read_be_u32(reg_prop.value.add(gicr_off), i * 4);
                gicr_addr = (gicr_addr << 32) | cell as usize;
            }
        }

        init_gic(gicd_addr, gicr_addr);
    }

    set_priority_mask(PRIORITY_MASK_ALL);
    enable_grp1_ints();
}
