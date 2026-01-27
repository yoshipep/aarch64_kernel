//! GICv3 interrupt controller driver
//!
//! This module provides functions to initialize and configure the ARM GICv3 interrupt controller.
//! It manages both the Distributor (GICD) for Shared Peripheral Interrupts (SPIs) and the
//! Redistributor (GICR) for Private Peripheral Interrupts (PPIs) and Software Generated
//! Interrupts (SGIs).
//!
//! The driver uses a global `Gicv3` instance accessed through public wrapper functions.
//! Base addresses are discovered from the device tree during boot.

use core::arch::asm;
use core::ptr::addr_of_mut;

use crate::kernel::device;
use crate::utilities::convert;
use crate::utilities::mmio;

/// Maximum number of cells in a GIC interrupt specifier
///
/// The GICv3 binding requires `#interrupt-cells` to be at least 4
/// (see `Documentation/devicetree/bindings/interrupt-controller/arm,gic-v3.yaml`).
pub const MAX_INTERRUPT_CELLS: usize = 4;

/* --- GICD (Distributor) Constants --- */
/// Distributor Control Register
const GICD_CTLR: usize = 0x000;
/// Enable non secure Group 1 interrupts bit
const GICD_CTLR_GRP1NS: u32 = 0b10;
/// Enable secure Group 1 interrupts bit
const GICD_CTLR_GRP1S: u32 = 0b100;
/// Interrupt Set-Enable Register
const GICD_ISENABLER: usize = 0x100;
/// Interrupt Priority Registers
const GICD_IPRIORITYR: usize = 0x400;
/// Interrupt Configuration Registers
const GICD_ICFGR: usize = 0xC00;
/// Interrupt Routing Registers
const GICD_IROUTER: usize = 0x6100;
/// Interrupt Group Registers
const GICD_IGROUPR: usize = 0x080;

/* --- GICR (Redistributor) Constants --- */
/// SGI Frame offset
const GICR_SGI_BASE: usize = 0x10000; // Offset from RD_base to SGI & PPI frame
/// Redistributor Wake Register
const GICR_WAKER: usize = 0x0014;
/// Processor sleep bit. Indicates whether the Redistributor can assert the **WakeRequest**
/// signal
const GICR_WAKER_PSLEEP: u32 = 0b10;
/// Children asleep bit. Indicates whether the connected PE is quiescent
const GICR_WAKER_CASLEEP: u32 = 0b100;
/// Interrupt Priority Registers
const GICR_IPRIORITYR: usize = 0x400;
/// Interrupt Group Register 0
const GICR_IGROUPR0: usize = 0x080;
/// Interrupt Set-Enable Register 0
const GICR_ISENABLER0: usize = 0x100;
/// Interrupt Configuration Register
const GICR_ICFGR: usize = 0xC00;

/// Global GICv3 instance holding the distributor and redistributor base addresses
static mut GIC: Gicv3 = Gicv3::new();

/// GICv3 interrupt controller state
///
/// Holds the MMIO base addresses for the GIC Distributor (GICD) and Redistributor (GICR)
/// regions. These are populated during device tree parsing and used by all GIC operations.
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
            mmio::set_mmio_bits32(
                self.dist_addr,
                GICD_CTLR,
                GICD_CTLR_GRP1S | GICD_CTLR_GRP1NS,
            );
            asm!("dsb sy", options(nostack, nomem));
        }
    }
    /// Initializes the GIC Redistributor
    pub fn init_gic_redistributor(&self) {
        unsafe {
            mmio::clear_mmio_bits32(self.redist_addr, GICR_WAKER, GICR_WAKER_PSLEEP);
            asm!("dsb sy", options(nostack, nomem));
            while (mmio::read_mmio32(self.redist_addr, GICR_WAKER) & GICR_WAKER_CASLEEP) != 0 {}
        }
    }

    /// Sets the priority for the given PPI/SGI
    ///
    /// Sets the priority `prio` to the given PPI/SGI `id`
    pub fn set_ppi_priority(&self, id: u32, prio: u8) {
        unsafe {
            let sgi_base = self.redist_addr + GICR_SGI_BASE;
            let reg_index = id / 4;
            let reg_offset = (reg_index * 4) as usize;
            let byte_index_in_reg = id % 4;
            let bit_shift = byte_index_in_reg * 8;
            let prio_reg_addr = sgi_base + GICR_IPRIORITYR + reg_offset;
            let mut reg_val = mmio::read_mmio32(prio_reg_addr, 0);
            let mask: u32 = !(0xFF << bit_shift);
            reg_val &= mask;
            reg_val |= (prio as u32) << bit_shift;
            mmio::write_mmio32(prio_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Assigns the PPI/SGI to Group 1
    ///
    /// Assigns the PPI/SGI `id` to Group 1
    pub fn set_ppi_group(&self, id: u32) {
        unsafe {
            mmio::set_mmio_bits32(self.redist_addr + GICR_SGI_BASE, GICR_IGROUPR0, 1 << id);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Enables the PPI/SGI
    ///
    /// Enables the PPI/SGI with the given `id`
    pub fn enable_ppi(&self, id: u32) {
        unsafe {
            mmio::set_mmio_bits32(self.redist_addr + GICR_SGI_BASE, GICR_ISENABLER0, 1 << id);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Sets the priority of interrupts
    ///
    /// Sets the priority `prio` to the interrupt `id`
    pub fn set_spi_priority(&self, id: u32, prio: u8) {
        unsafe {
            let reg_index = id / 4;
            let reg_offset = (reg_index * 4) as usize;
            let byte_index_in_reg = id % 4;
            let bit_shift = byte_index_in_reg * 8;
            let prio_reg_addr = self.dist_addr + GICD_IPRIORITYR + reg_offset;
            let mut reg_val = mmio::read_mmio32(prio_reg_addr, 0);
            let mask: u32 = !(0xFF << bit_shift);
            reg_val &= mask;
            reg_val |= (prio as u32) << bit_shift;
            mmio::write_mmio32(prio_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Sets level-sensitive trigger mode for the SPI
    ///
    /// Configures the interrupt `id` to be level-sensitive (0b00 in ICFGR)
    pub fn set_spi_trigger_level(&self, id: u32) {
        unsafe {
            let reg_index = id / 16;
            let reg_offset = (reg_index * 4) as usize;
            let bit_shift = (id % 16) * 2;
            let cfg_reg_addr = self.dist_addr + GICD_ICFGR + reg_offset;
            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            let mask: u32 = !(0b11 << bit_shift);
            reg_val &= mask;
            // Level-sensitive: bits = 0b00
            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Sets edge-triggered mode for the SPI
    ///
    /// Configures the interrupt `id` to be edge-triggered (0b10 in ICFGR)
    pub fn set_spi_trigger_edge(&self, id: u32) {
        unsafe {
            let reg_index = id / 16;
            let reg_offset = (reg_index * 4) as usize;
            let bit_shift = (id % 16) * 2;
            let cfg_reg_addr = self.dist_addr + GICD_ICFGR + reg_offset;
            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            let mask: u32 = !(0b11 << bit_shift);
            reg_val &= mask;
            // Edge-triggered: bits = 0b10
            reg_val |= 0b10 << bit_shift;
            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Enables forwarding of the interrupt to the CPU interface
    ///
    /// Enables forwarding of the interrupt `id` in the GIC distributor
    pub fn enable_spi(&self, id: u32) {
        unsafe {
            let reg_index = id / 32;
            let reg_offset = (reg_index * 4) as usize;
            let enabler_reg_addr = self.dist_addr + GICD_ISENABLER + reg_offset;
            let bit_to_set = 1 << (id % 32);
            mmio::write_mmio32(enabler_reg_addr, 0, bit_to_set);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Provides routing information for the SPI
    ///
    /// When affinity routing is enabled, provides routing information for the SPI with id `id`. It
    /// defines the routing mode by writting the value `core_affinity` into the corresponding register
    pub fn set_spi_routing(&self, id: u32, core_affinity: u64) {
        unsafe {
            let router_reg_addr = self.dist_addr + GICD_IROUTER + (8 * id as usize);
            let router_ptr = router_reg_addr as *mut u64;
            core::ptr::write_volatile(router_ptr, core_affinity);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Assigns the SPI to the Group 1
    ///
    /// Assigns the SPI `id` to the Group 1
    pub fn set_spi_group(&self, id: u32) {
        unsafe {
            let reg_index = id / 32;
            let reg_offset = (reg_index * 4) as usize;
            let group_reg_addr = self.dist_addr + GICD_IGROUPR + reg_offset;
            let bit_to_set = 1 << (id % 32);
            mmio::set_mmio_bits32(group_reg_addr, 0, bit_to_set);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Sets level-sensitive trigger mode for the PPI
    ///
    /// Configures the interrupt `id` to be level-sensitive (0b00 in ICFGR)
    pub fn set_ppi_trigger_level(&self, id: u32) {
        unsafe {
            let reg_index = id / 16;
            let reg_offset = (reg_index * 4) as usize;
            let bit_shift = (id % 16) * 2;
            let sgi_base = self.redist_addr + GICR_SGI_BASE;
            let cfg_reg_addr = sgi_base + GICR_ICFGR + reg_offset;
            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            let mask: u32 = !(0b11 << bit_shift);
            reg_val &= mask;
            // Level-sensitive: bits = 0b00
            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }

    /// Sets edge-triggered mode for the PPI
    ///
    /// Configures the interrupt `id` to be edge-triggered (0b10 in ICFGR)
    pub fn set_ppi_trigger_edge(&self, id: u32) {
        unsafe {
            let reg_index = id / 16;
            let reg_offset = (reg_index * 4) as usize;
            let bit_shift = (id % 16) * 2;
            let sgi_base = self.redist_addr + GICR_SGI_BASE;
            let cfg_reg_addr = sgi_base + GICR_ICFGR + reg_offset;
            let mut reg_val = mmio::read_mmio32(cfg_reg_addr, 0);
            let mask: u32 = !(0b11 << bit_shift);
            reg_val &= mask;
            // Edge-triggered: bits = 0b10
            reg_val |= 0b10 << bit_shift;
            mmio::write_mmio32(cfg_reg_addr, 0, reg_val);
            asm!("dsb sy", options(nostack, nomem));
        }
    }
}

/// Initializes the GIC with the given distributor and redistributor addresses
///
/// Stores the base addresses and initializes both the distributor (enables Group 1
/// interrupts and affinity routing) and redistributor (wakes the PE from sleep).
fn init_gic(dist_addr: usize, redist_addr: usize) {
    unsafe {
        (*addr_of_mut!(GIC)).dist_addr = dist_addr;
        (*addr_of_mut!(GIC)).redist_addr = redist_addr;
        (*addr_of_mut!(GIC)).init_gic_distributor();
        (*addr_of_mut!(GIC)).init_gic_redistributor();
    }
}

// Public wrapper functions for SPI (distributor) access

/// Enables forwarding of the SPI `id` in the GIC distributor
pub fn enable_spi(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).enable_spi(id);
    }
}

/// Sets the priority of SPI `id` in the distributor
pub fn set_spi_priority(id: u32, prio: u8) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_priority(id, prio);
    }
}

/// Sets level-sensitive trigger mode for SPI `id`
pub fn set_spi_trigger_level(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_trigger_level(id);
    }
}

/// Sets edge-triggered mode for SPI `id`
pub fn set_spi_trigger_edge(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_trigger_edge(id);
    }
}

/// Assigns SPI `id` to Group 1
pub fn set_spi_group(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_group(id);
    }
}

/// Sets the affinity routing for SPI `id`
pub fn set_spi_routing(id: u32, core_affinity: u64) {
    unsafe {
        (*addr_of_mut!(GIC)).set_spi_routing(id, core_affinity);
    }
}

// Public wrapper functions for PPI/SGI (redistributor)

/// Sets the priority of PPI/SGI `id` in the redistributor
pub fn set_ppi_priority(id: u32, prio: u8) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_priority(id, prio);
    }
}

/// Assigns PPI/SGI `id` to Group 1 in the redistributor
pub fn set_ppi_group(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_group(id);
    }
}

/// Enables PPI/SGI `id` in the redistributor
pub fn enable_ppi(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).enable_ppi(id);
    }
}

/// Sets level-sensitive trigger mode for PPI `id`
pub fn set_ppi_trigger_level(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_trigger_level(id);
    }
}

/// Sets edge-triggered mode for PPI `id`
pub fn set_ppi_trigger_edge(id: u32) {
    unsafe {
        (*addr_of_mut!(GIC)).set_ppi_trigger_edge(id);
    }
}

/// Sets an interrupt mask
///
/// Sets the interrupt mask `priority`. Interrupts with a higher priority than `priority` will be signaled to the PE
pub fn set_priority_mask(priority: u8) {
    unsafe {
        asm!("msr ICC_PMR_EL1, {}", in(reg) priority as u64, options(nostack, nomem));
    }
}

/// Enable the Group 1 interrupts
pub fn enable_grp1_ints() {
    unsafe {
        asm!(
            "mrs {tmp}, ICC_IGRPEN1_EL1",
            "orr {tmp}, {tmp}, #1",
            "msr ICC_IGRPEN1_EL1, {tmp}",
            "isb sy",
            tmp = out(reg) _,
            options(nostack, nomem)
        );
    }
}

/// Sets up the GICv3 from device tree properties
///
/// Parses the `reg` property to extract the distributor (GICD) and redistributor (GICR)
/// base addresses, initializes the GIC hardware, sets the CPU interface priority mask
/// to accept all priorities, and enables Group 1 interrupts.
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
    set_priority_mask(0xff);
    enable_grp1_ints();
}
