//! Platform device abstraction for DTB-discovered hardware.
//!
//! This module provides structures for representing devices discovered from the Device Tree Blob.
//! The design uses a combined match table approach where each entry contains both the matching
//! criteria (`compatible` string) and the setup function pointer.
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
//! 3. Check `compatible` property against match table
//! 4. If matched, call the corresponding `setup_fn`

use crate::drivers::gic::gicv3;
use crate::drivers::timer::arch_timer;
use crate::drivers::uart::pl011;
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
}

impl Default for PlatformDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry in the device match table.
///
/// Combines matching criteria with setup function (unlike Linux which separates `of_device_id`
/// and `platform_driver`). During DTB parsing, the `compatible` property is checked against
/// each entry; on match, `setup_fn` is called with the collected device properties.
pub struct DeviceMatch {
    /// Compatible string to match (e.g., "arm,pl011", "arm,gic-v3")
    pub compatible: &'static str,
    /// Function to call when a matching device is found
    pub setup_fn: fn(&PlatformDevice),
}

/// Table of supported devices, matched against DTB `compatible` strings during initialization
pub static CONFIGURED_DEVICES: [DeviceMatch; 3] = [
    DeviceMatch {
        compatible: "arm,gic-v3",
        setup_fn: gicv3::setup,
    },
    DeviceMatch {
        compatible: "arm,pl011",
        setup_fn: pl011::setup,
    },
    DeviceMatch {
        compatible: "arm,armv7-timer",
        setup_fn: arch_timer::setup,
    },
];
