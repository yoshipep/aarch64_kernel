use crate::kernel::device;
use crate::utilities::convert;

const MAGIC: u32 = 0xd00dfeed;
const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32 = 0x00000002;
const FDT_PROP: u32 = 0x00000003;
const FDT_NOP: u32 = 0x00000004;
const FDT_END: u32 = 0x00000009;
const MAX_DEVICES: usize = 256;

static mut DEVICE_COUNT: usize = 0;

static mut DEVICE_TABLE: [device::PlatformDevice; MAX_DEVICES] =
    [device::PlatformDevice::new(); MAX_DEVICES];

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
    /// Parse header from DTB address (handles big-endian conversion)
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

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FdtPropHeader {
    pub len: u32,
    pub nameoff: u32,
    // We can't efficiently add data until we implement a memory allocator
}

impl FdtPropHeader {
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
                    stack[stack_depth] = DEVICE_COUNT;
                }
                stack_depth += 1;
            }
            FDT_END_NODE => {
                // End of current node. Store device in table, reset counters
                device.prop_count = prop_id;
                unsafe {
                    DEVICE_TABLE[DEVICE_COUNT] = device;
                    DEVICE_COUNT += 1;
                }

                prop_id = 0;
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
                device.properties[prop_id] = prop;
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
}
