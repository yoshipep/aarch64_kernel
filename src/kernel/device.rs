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
}

impl Default for PlatformDevice {
    fn default() -> Self {
        Self::new()
    }
}
