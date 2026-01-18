#[inline]
pub fn read_be_u16(ptr: *const u8, offset: usize) -> u16 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u16;
        u16::from_be(value_ptr.read_unaligned())
    }
}

#[inline]
pub fn read_be_u32(ptr: *const u8, offset: usize) -> u32 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u32;
        u32::from_be(value_ptr.read_unaligned())
    }
}

#[inline]
pub fn read_be_u64(ptr: *const u8, offset: usize) -> u64 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u64;
        u64::from_be(value_ptr.read_unaligned())
    }
}

#[inline]
pub fn read_le_u16(ptr: *const u8, offset: usize) -> u16 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u16;
        u16::from_le(value_ptr.read_unaligned())
    }
}

#[inline]
pub fn read_le_u32(ptr: *const u8, offset: usize) -> u32 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u32;
        u32::from_le(value_ptr.read_unaligned())
    }
}

#[inline]
pub fn read_le_u64(ptr: *const u8, offset: usize) -> u64 {
    unsafe {
        let value_ptr = ptr.add(offset) as *const u64;
        u64::from_le(value_ptr.read_unaligned())
    }
}
