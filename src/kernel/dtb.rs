//! Flattened Device Tree (FDT) parser
//!
//! This module parses the Device Tree Blob (DTB) provided by the bootloader or firmware to
//! discover hardware devices and their configuration. The DTB is a binary representation of the
//! device tree, a data structure that describes the hardware topology of the system.
//!
//! ## Parsing Strategy
//!
//! The parser walks the DTB structure block token by token, building a flat device table. Each
//! DTB node becomes a `PlatformDevice` entry with its properties stored directly in the table.
//! A depth stack tracks parent-child relationships. After parsing, `init_devices` matches
//! discovered devices against the configured driver table and calls their setup functions.
//!
//! ## Initialization Order
//!
//! Device initialization is done in two passes:
//! 1. **First pass**: Initialize the interrupt controller (GIC), since other devices need it
//!    to configure their interrupts
//! 2. **Second pass**: Initialize all remaining devices (UART, timer, etc.)

use core;

use crate::kernel::device;
use crate::utilities::convert;

/// DTB magic number (big-endian: 0xd00dfeed)
const MAGIC: u32 = 0xd00dfeed;
/// Token marking the start of a node
const FDT_BEGIN_NODE: u32 = 0x00000001;
/// Token marking the end of a node
const FDT_END_NODE: u32 = 0x00000002;
/// Token marking a property entry
const FDT_PROP: u32 = 0x00000003;
/// Token for padding/alignment (ignored)
const FDT_NOP: u32 = 0x00000004;
/// Token marking the end of the structure block
const FDT_END: u32 = 0x00000009;
/// Maximum number of devices that can be stored in the device table
const MAX_DEVICES: usize = 256;
/// Maximum number of phandle entries
const MAX_HANDLES: usize = 32;

/// Number of devices discovered during DTB parsing
static mut DEVICE_COUNT: usize = 0;

/// Flat table of all devices discovered from the DTB
static mut DEVICE_TABLE: [device::PlatformDevice; MAX_DEVICES] =
    [device::PlatformDevice::new(); MAX_DEVICES];

/// Number of phandle mappings registered
static mut PHANDLE_COUNT: usize = 0;

/// Lookup table mapping phandle values to device table indices
static mut PHANDLE_TABLE: [(u32, usize); MAX_HANDLES] = [(0, 0); MAX_HANDLES];

/// Flattened Device Tree header
///
/// The first 40 bytes of the DTB contain this header, which describes the layout
/// and version of the blob. All fields are stored in big-endian byte order.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FdtHeader {
    pub magic: u32,             // 0x00: Must be 0xd00dfeed
    pub totalsize: u32,         // 0x04: Total DTB size
    pub off_dt_struct: u32,     // 0x08: Offset to structure block
    pub off_dt_strings: u32,    // 0x0C: Offset to strings block
    pub off_mem_rsvmap: u32,    // 0x10: Offset to memory reserve map
    pub version: u32,           // 0x14: DTB version
    pub last_comp_version: u32, // 0x18: Last compatible version
    pub boot_cpuid_phys: u32,   // 0x1C: Boot CPU ID
    pub size_dt_strings: u32,   // 0x20: Strings block size
    pub size_dt_struct: u32,    // 0x24: Structure block size
}

impl FdtHeader {
    /// Parses the FDT header from raw memory at `dtb_addr`
    ///
    /// Reads each field from the DTB base address using big-endian to native conversion.
    pub fn from_be_bytes(dtb_addr: usize) -> Self {
        let ptr = dtb_addr as *const u8;
        Self {
            magic: convert::read_be_u32(ptr, 0x00),
            totalsize: convert::read_be_u32(ptr, 0x04),
            off_dt_struct: convert::read_be_u32(ptr, 0x08),
            off_dt_strings: convert::read_be_u32(ptr, 0x0C),
            off_mem_rsvmap: convert::read_be_u32(ptr, 0x10),
            version: convert::read_be_u32(ptr, 0x14),
            last_comp_version: convert::read_be_u32(ptr, 0x18),
            boot_cpuid_phys: convert::read_be_u32(ptr, 0x1C),
            size_dt_strings: convert::read_be_u32(ptr, 0x20),
            size_dt_struct: convert::read_be_u32(ptr, 0x24),
        }
    }
}

/// Header preceding each property value in the structure block
///
/// Each `FDT_PROP` token is followed by this 8-byte header containing the property's
/// value length and an offset into the strings block for the property name.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FdtPropHeader {
    pub len: u32,
    pub nameoff: u32,
    // We can't efficiently add data until we implement a memory allocator
}

impl FdtPropHeader {
    /// Parses the property header from raw memory at `prop_addr`
    pub fn from_be_bytes(prop_addr: usize) -> Self {
        let ptr = prop_addr as *const u8;
        Self {
            len: convert::read_be_u32(ptr, 0x00),
            nameoff: convert::read_be_u32(ptr, 0x04),
        }
    }
}

/// Get a string from the strings block by offset
fn get_property_name(dtb_addr: usize, off_dt_strings: usize, offset: u32) -> &'static str {
    let strings_block = dtb_addr + off_dt_strings;
    let str_start = (strings_block + offset as usize) as *const u8;
    let mut len = 0;
    unsafe {
        while *str_start.add(len) != 0 {
            len += 1;
        }

        let slice = core::slice::from_raw_parts(str_start, len);
        core::str::from_utf8_unchecked(slice)
    }
}

/// Parses the Flattened Device Tree at address `dtb`
///
/// Walks the DTB structure block token by token, creating a `PlatformDevice` for each node
/// and storing its properties in the global `DEVICE_TABLE`. A depth stack tracks parent-child
/// relationships so each device can reference its parent. After parsing, calls `init_devices`
/// to match discovered devices against the driver table and initialize them.
#[unsafe(no_mangle)]
pub fn parse_dtb(dtb: usize) {
    let header = FdtHeader::from_be_bytes(dtb);
    if header.magic != MAGIC {
        panic!();
    }

    let structure_block = dtb + header.off_dt_struct as usize;
    let mut off = 0;
    let mut prop_id = 0;
    let mut device = device::PlatformDevice::default();
    let mut stack: [usize; 10] = [0; 10];
    let mut stack_depth = 0;
    loop {
        let token = convert::read_be_u32(structure_block as *const u8, off);
        off += 4;
        match token {
            FDT_BEGIN_NODE => {
                device = device::PlatformDevice::default();
                prop_id = 0;
                if stack_depth > 0 {
                    unsafe {
                        device.parent =
                            &DEVICE_TABLE[stack[stack_depth - 1]] as *const device::PlatformDevice;
                    }
                }

                // Read null-terminated node name. Name starts after the token FDT_BEGIN_NODE
                let name_start = (structure_block + off) as *const u8;
                let mut name_len = 0;
                unsafe {
                    while *name_start.add(name_len) != 0 {
                        name_len += 1;
                    }
                }

                // Convert to string. Pick [name_start, name_start + name_len] bytes
                device.name = unsafe {
                    let slice = core::slice::from_raw_parts(name_start, name_len);
                    core::str::from_utf8_unchecked(slice)
                };
                // Move offset past name + null terminator
                off += name_len + 1;
                // Align to 4-byte boundary
                off = (off + 3) & !3;
                unsafe {
                    DEVICE_TABLE[DEVICE_COUNT] = device;
                    stack[stack_depth] = DEVICE_COUNT;
                    DEVICE_COUNT += 1;
                }
                stack_depth += 1;
            }
            FDT_END_NODE => {
                // Store prop_count in table entry, then pop the stack
                unsafe {
                    DEVICE_TABLE[stack[stack_depth - 1]].prop_count = prop_id;
                }
                stack_depth -= 1;
            }
            FDT_PROP => {
                // Read property data: length and name
                let prop_header = FdtPropHeader::from_be_bytes(structure_block + off);
                // Get the name of the property
                off += 8;
                let mut prop = device::Property::default();
                prop.name =
                    get_property_name(dtb, header.off_dt_strings as usize, prop_header.nameoff);
                prop.len = prop_header.len as usize;
                prop.value = (structure_block + off) as *const u8;

                // Store property directly in DEVICE_TABLE entry
                let dev_idx = stack[stack_depth - 1];
                unsafe {
                    if prop.name == "phandle" {
                        // phandle is always u32, so we can read the id directly
                        let phandle_value = convert::read_be_u32(prop.value, 0);
                        PHANDLE_TABLE[PHANDLE_COUNT] = (phandle_value, dev_idx);
                        PHANDLE_COUNT += 1;
                    }
                    DEVICE_TABLE[dev_idx].properties[prop_id] = prop;
                }
                prop_id += 1;
                // Align to 4-byte boundary
                off += prop.len as usize;
                off = (off + 3) & !3;
            }
            FDT_NOP => {
                // Skip
            }
            FDT_END => {
                break;
            }
            _ => {
                panic!();
            }
        }
    }
    init_devices();
}

/// Find a device by its phandle value
pub fn find_device_by_phandle(phandle: u32) -> Option<&'static device::PlatformDevice> {
    unsafe {
        for i in 0..PHANDLE_COUNT {
            let (phandle_val, dev_idx) = PHANDLE_TABLE[i];
            if phandle_val == phandle {
                return Some(&DEVICE_TABLE[dev_idx]);
            }
        }
    }
    None
}

/// Find the interrupt parent for a device by walking up the tree
/// Returns the interrupt controller device if found
pub fn find_interrupt_parent(
    dev: &device::PlatformDevice,
) -> Option<&'static device::PlatformDevice> {
    unsafe {
        let mut current = dev.parent as *const device::PlatformDevice;
        while current != core::ptr::null() {
            // Check if current node has interrupt-parent property
            for i in 0..(*current).prop_count {
                let prop = (*current).properties[i];
                if prop.name == "interrupt-parent" {
                    let phandle = convert::read_be_u32(prop.value, 0);
                    return find_device_by_phandle(phandle);
                }
            }
            // Walk up to parent
            current = (*current).parent;
        }
    }
    None
}

/// Check if a compatible property value contains a specific string.
/// Compatible values can have multiple null-separated strings (e.g., "arm,pl011\0arm,primecell\0")
fn compatible_matches(prop: &device::Property, target: &str) -> bool {
    let mut offset = 0;
    while offset < prop.len {
        let str_start = unsafe { prop.value.add(offset) };
        let mut str_len = 0;
        unsafe {
            while offset + str_len < prop.len && *str_start.add(str_len) != 0 {
                str_len += 1;
            }
        }

        let compat_str = unsafe {
            let slice = core::slice::from_raw_parts(str_start, str_len);
            core::str::from_utf8_unchecked(slice)
        };

        if compat_str == target {
            return true;
        }

        offset += str_len + 1;
    }
    false
}

/// Initializes all discovered devices by matching against the driver table
///
/// Runs in two passes:
/// 1. First initializes the GIC (interrupt controller), since other devices depend on it
///    to configure their interrupts
/// 2. Then initializes all remaining devices (UART, timer, etc.)
pub fn init_devices() {
    unsafe {
        // First pass: initialize GIC (interrupt controller must be ready before other devices)
        for i in 0..DEVICE_COUNT {
            let dev = &DEVICE_TABLE[i];
            if let Some(compat_prop) = dev.find_property("compatible") {
                if compatible_matches(compat_prop, "arm,gic-v3") {
                    for match_entry in &device::CONFIGURED_DEVICES {
                        if compatible_matches(compat_prop, match_entry.compatible) {
                            (match_entry.setup_fn)(dev);
                            break;
                        }
                    }
                }
            }
        }

        // Second pass: initialize all other devices
        for i in 0..DEVICE_COUNT {
            let dev = &DEVICE_TABLE[i];
            if let Some(compat_prop) = dev.find_property("compatible") {
                if !compatible_matches(compat_prop, "arm,gic-v3") {
                    for match_entry in &device::CONFIGURED_DEVICES {
                        if compatible_matches(compat_prop, match_entry.compatible) {
                            (match_entry.setup_fn)(dev);
                            break;
                        }
                    }
                }
            }
        }
    }
}
