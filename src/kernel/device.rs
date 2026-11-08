//! Platform device abstraction for DTB-discovered hardware.
//!
//! This module provides structures for representing devices discovered from the Device Tree Blob.
//! The design uses a combined match table approach where each entry contains both the matching
//! criteria (a `compatible` string, or a `device_type` string for the `cpu`/`memory` nodes that
//! have no `compatible` property) and the setup function pointer.
//!
//! # Linux Kernel Comparison
//!
//! Linux separates this into two structures:
//! - `of_device_id`: matching criteria + optional data pointer
//! - `platform_driver`: probe function + reference to match table
//!
//! We combine both into `DeviceMatch` for simplicity, which is sufficient for a learning kernel
//! with a small number of devices.
//!
//! # Usage
//!
//! 1. Define a static match table with `DeviceMatch` entries
//! 2. During DTB parsing, collect properties into `PlatformDevice`
//! 3. For each `Stage`, in order, check the property named by each of its entries' `MatchCriteria` against every
//!    collected device
//! 4. If matched, call the corresponding `setup_fn`

use crate::drivers::gic::gicv3;
use crate::drivers::timer::arch_timer;
use crate::drivers::uart::pl011;
use crate::kernel::meminfo;
use crate::kernel::phys_addr::PhysAddr;
use crate::utilities::convert;

/// Maximum number of properties per device node.
/// This is a reasonable limit for typical DTB nodes (most have fewer than 10 properties).
/// Linux uses a linked list with dynamic allocation; we use a fixed array since we lack
/// a memory allocator.
const MAX_PROPS: usize = 16;

/// A single property from a DTB node.
///
/// Properties contain the actual device configuration data such as register addresses,
/// interrupt numbers, clock frequencies, etc.
#[derive(Clone, Copy)]
pub struct Property {
    /// Property name (e.g., "reg", "interrupts", "compatible")
    pub name: &'static str,
    /// Pointer to raw property value in DTB memory
    pub value: *const u8,
    /// Length of property value in bytes
    pub len: usize,
}

impl Property {
    /// Const constructor for static initialization
    pub const fn new() -> Self {
        Self {
            name: "",
            value: core::ptr::null(),
            len: 0,
        }
    }
}

impl Default for Property {
    fn default() -> Self {
        Self::new()
    }
}

fn read_cells(ptr: *const u8, offset: usize, cells: u32) -> u64 {
    let mut value = 0;

    for i in 0..cells as usize {
        // Here we read the property from start which carries the correct index, and advance
        // sizeof(u32) bytes
        let cell = convert::read_be_u32(ptr, offset + i * size_of::<u32>());
        value = (value << 32) | cell as u64;
    }

    value
}

/// A platform device discovered from the DTB.
///
/// Each node in the DTB that matches a supported `compatible` string becomes a `PlatformDevice`.
/// Multiple nodes with the same `compatible` (e.g., two virtio_mmio devices) create separate
/// `PlatformDevice` instances, each with their own property values.
#[derive(Clone, Copy)]
pub struct PlatformDevice {
    pub parent: *const PlatformDevice,
    /// Node name from DTB (e.g., "pl011@9000000")
    pub name: &'static str,
    /// Array of properties belonging to this device
    pub properties: [Property; MAX_PROPS],
    /// Number of valid properties in the array
    pub prop_count: usize,
}

impl PlatformDevice {
    /// Const constructor for static initialization
    pub const fn new() -> Self {
        Self {
            parent: core::ptr::null(),
            name: "",
            properties: [Property::new(); MAX_PROPS],
            prop_count: 0,
        }
    }

    /// Find a property by name
    pub fn find_property(&self, name: &str) -> Option<&Property> {
        for i in 0..self.prop_count {
            if self.properties[i].name == name {
                return Some(&self.properties[i]);
            }
        }
        None
    }

    /// Get #address-cells and #size-cells from the device's parent
    /// Returns (address_cells, size_cells), defaults to (2, 1) if not found
    pub fn get_parent_cells(&self) -> (u32, u32) {
        let mut addr_cells: u32 = 2; // Default per DTB spec
        let mut size_cells: u32 = 1; // Default per DTB spec

        if self.parent.is_null() {
            return (addr_cells, size_cells);
        }

        unsafe {
            let parent = &*self.parent;
            for i in 0..parent.prop_count {
                let prop = &parent.properties[i];
                match prop.name {
                    "#address-cells" => {
                        addr_cells = convert::read_be_u32(prop.value, 0);
                    }
                    "#size-cells" => {
                        size_cells = convert::read_be_u32(prop.value, 0);
                    }
                    _ => {}
                }
            }
        }

        (addr_cells, size_cells)
    }

    /// Returns the address and size of the `index`-th entry of the device's `reg` property
    ///
    /// `reg` is a list of `(address, size)` pairs, one per register block of the device (the GIC has two: the
    /// distributor and the redistributor). The width of each pair is given by the parent's `#address-cells` and
    /// `#size-cells`, each cell being a big-endian 32-bit word, so an entry takes `(address cells + size cells) * 4`
    /// bytes. The values are in the form `ioremap` takes.
    ///
    /// # Arguments
    ///
    /// * `index` - which `(address, size)` pair to return, starting at 0
    ///
    /// # Returns
    ///
    /// `Some((address, size))`, or `None` if the device has no `reg` property, if the property is too short to hold
    /// entry `index`, or if the cell counts don't fit in 64 bits (more than 2 cells for the address or for the size).
    pub fn reg(&self, index: usize) -> Option<(PhysAddr, usize)> {
        let (addr_cells, size_cells) = self.get_parent_cells();
        if addr_cells > 2 || size_cells > 2 {
            return None;
        }

        let reg = self.find_property("reg")?;
        let entry_size = (addr_cells + size_cells) as usize * size_of::<u32>();
        // Very unlikely to happen since no dtb has an entry size of 0
        if entry_size == 0 {
            return None;
        }

        let start = entry_size.checked_mul(index)?;
        if start.checked_add(entry_size)? > reg.len {
            return None;
        }

        let addr = read_cells(reg.value, start, addr_cells);
        let size = read_cells(reg.value, start + addr_cells as usize * size_of::<u32>(), size_cells);
        Some((PhysAddr::new(addr), size as usize))
    }
}

impl Default for PlatformDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// Property a `DeviceMatch` entry is matched against.
///
/// `Compatible` is the devicetree spec's primary driver-matching mechanism and covers virtually every real device.
/// `DeviceType` exists only for node types the spec doesn't give a `compatible` property at all — as of the spec, that
/// means `cpu` and `memory` nodes exclusively.
#[derive(Clone, Copy)]
pub enum MatchCriteria {
    /// Match against the node's `compatible` property (possibly one of several null-separated aliases).
    Compatible(&'static str),
    /// Match against the node's `device_type` property. Only meaningful for `cpu`/`memory` nodes.
    DeviceType(&'static str),
}

/// Phase of boot at which a driver's `setup_fn` runs; the stages are run in declaration order by `dtb.rs`.
///
/// Stands in for Linux's initcall levels (`core_initcall`, `arch_initcall`, `device_initcall`, ...): the order is
/// decided at build time by the driver table, with no allocation. It cannot express a dependency between two drivers of
/// the same stage, nor resolve one from the DTB (Linux does that with `of_irq_init`'s parent-first ordering and
/// deferred probing, which need an allocator).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// RAM description; needed before the frame allocator and the MMU.
    Memory,
    /// Interrupt controllers; other devices configure their interrupts through them.
    IrqChip,
    /// Everything else (UART, timer, ...).
    Device,
}

/// Entry in the device match table.
///
/// Combines matching criteria with setup function (unlike Linux which separates `of_device_id`
/// and `platform_driver`). When the entry's `stage` runs, its `criteria` is checked against the
/// matching property of every discovered node; on match, `setup_fn` is called with the collected
/// device properties.
pub struct DeviceMatch {
    /// What property, and value, identifies a matching node.
    pub criteria: MatchCriteria,
    /// Function to call when a matching device is found
    pub setup_fn: fn(&PlatformDevice),
    /// Boot stage at which `setup_fn` runs
    pub stage: Stage,
}

/// Table of supported devices, matched against DTB nodes during initialization
pub static CONFIGURED_DEVICES: [DeviceMatch; 4] = [
    DeviceMatch {
        criteria: MatchCriteria::Compatible("arm,gic-v3"),
        setup_fn: gicv3::setup,
        stage: Stage::IrqChip,
    },
    DeviceMatch {
        criteria: MatchCriteria::Compatible("arm,pl011"),
        setup_fn: pl011::setup,
        stage: Stage::Device,
    },
    DeviceMatch {
        criteria: MatchCriteria::Compatible("arm,armv7-timer"),
        setup_fn: arch_timer::setup,
        stage: Stage::Device,
    },
    DeviceMatch {
        criteria: MatchCriteria::DeviceType("memory"),
        setup_fn: meminfo::setup,
        stage: Stage::Memory,
    },
];
