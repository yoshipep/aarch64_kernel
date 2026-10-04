//! `/memory` node parsing
//!
//! Extracts the base address and size of physical RAM from the DTB's `memory` node (matched via `device_type`, since
//! it has no `compatible` property — see `device::MatchCriteria`). This is the source of the RAM layout the frame
//! allocator (`mm::frame_alloc`) needs to know what physical range it's allowed to hand out pages from.

use crate::{
    kernel::{device, phys_addr::PhysAddr},
    pr_info,
    utilities::convert,
};

/// Physical base address and size of RAM, as last reported by `setup`. `(PhysAddr::new(0), 0)` until `setup` has run.
static mut RAM_RANGE: (PhysAddr, usize) = (PhysAddr::new(0), 0);

/// Sets up memory layout information from device tree properties
///
/// Parses the `reg` property to extract the base address and size of physical RAM. `reg` encodes `(address, size)` as
/// big-endian cells, `#address-cells`/`#size-cells` wide respectively (taken from the node's parent, per DTB spec),
/// concatenated back to back — so the size cells start right after the address cells, at byte offset `addr_cells * 4`.
pub fn setup(dev: &device::PlatformDevice) {
    let mut addr = 0;
    let mut size = 0;

    let (addr_cells, size_cells) = dev.get_parent_cells();
    if let Some(reg_prop) = dev.find_property("reg") {
        for i in 0..addr_cells as usize {
            let cell = convert::read_be_u32(reg_prop.value, i * 4);
            addr = (addr << 32) | cell as u64;
        }

        for i in 0..size_cells as usize {
            unsafe {
                let cell = convert::read_be_u32(reg_prop.value.add(addr_cells as usize * 4), i * 4);
                size = (size << 32) | cell as usize;
            }
        }
    }

    pr_info!("meminfo", "Usable ram: [{:#X}-{:#X}]", addr, addr + (size as u64) - 1);

    unsafe {
        RAM_RANGE = (PhysAddr::new(addr), size);
    }
}

/// Returns the physical base address and size of RAM discovered by `setup`.
pub fn ram_range() -> (PhysAddr, usize) {
    unsafe { RAM_RANGE }
}
