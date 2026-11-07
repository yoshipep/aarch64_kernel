//! Mapping of device registers (MMIO) into the kernel address space
//!
//! RAM is reached through the linear map, which is Normal cacheable memory. Device registers need different memory
//! attributes (Device-nGnRE) and are not part of RAM, so each device block gets its own mapping, at a virtual address
//! allocated here at run time. `ioremap` returns an `IoMem` for it, the only type that can access the registers.
//!
//! # Where it lives
//!
//! The region is a top-level slot of its own in TTBR1, right after the kernel image. A Pgd entry covers 512 GiB
//! (`1 << 39`), and the entry index is bits `[47:39]` of the address:
//!
//! | Pgd index | Start | Owner |
//! |---|---|---|
//! | `0x000`..`0x0FF` | `0xFFFF_0000_0000_0000` (`PAGE_OFFSET`) | linear map of RAM |
//! | `0x100` | `0xFFFF_8000_0000_0000` (`KIMAGE_VADDR`) | kernel image |
//! | `0x101` | `0xFFFF_8080_0000_0000` (`IOREMAP_START`) | `ioremap` |
//! | `0x102`..`0x1FF` | | unassigned |
//!
//! The rule is that everything below the image belongs to the linear map, so it can grow as far as the physical
//! address size allows without the layout being rechecked. Linux does it differently: `ioremap` and `vmalloc` share
//! one large region. A shared region can replace this one once the kernel has a heap.
//!
//! # Allocation
//!
//! Virtual ranges are tracked in a small static table of areas, searched first-fit, with one unmapped guard page
//! after each area so an overrun faults instead of reaching the next device. The table is static because there is no
//! heap yet; it is to be replaced by a heap-based allocator when `kmalloc` exists.

use crate::ipc::irq_safe_mutex::Mutex;
use crate::kernel::io_mem::IoMem;
use crate::kernel::meminfo;
use crate::kernel::mm::identity::kernel_pgd;
use crate::kernel::mm::mair::MairIdx;
use crate::kernel::mm::map;
use crate::kernel::mm::pgtable::{KIMAGE_VADDR, PAGE_MASK, PAGE_SIZE, PA_BITS, PGD_SIZE};
use crate::kernel::mm::pgtable_hwdef::leaf;
use crate::kernel::phys_addr::PhysAddr;
use crate::kernel::virt_addr::VirtAddr;
use crate::pr_info;
use crate::utilities::mmio;

/// First virtual address of the `ioremap` region: the Pgd slot after the one holding the kernel image, i.e. index
/// `0x101`, `0xFFFF_8080_0000_0000`. It is derived from the image's slot, not from `IOREMAP_SIZE`, so the region can
/// change size without moving.
pub const IOREMAP_START: u64 = KIMAGE_VADDR + PGD_SIZE as u64;
/// Size of the `ioremap` region in bytes: one Pgd entry, 512 GiB.
pub const IOREMAP_SIZE: u64 = PGD_SIZE as u64;
/// First address past the `ioremap` region.
pub const IOREMAP_END: u64 = IOREMAP_START + IOREMAP_SIZE;

// The region must not overlap the kernel image's Pgd slot. Fails the build if the layout constants are edited
// inconsistently, instead of failing at boot.
const _: () = assert!(IOREMAP_START >= KIMAGE_VADDR + PGD_SIZE as u64);

/// Number of areas the table can hold, so at most this many devices can be mapped at once.
const MAX_AREAS: usize = 16;

/// A range of the `ioremap` region that is in use.
#[derive(Copy, Clone)]
pub struct Area {
    /// First address of the area, page-aligned
    start: VirtAddr,
    /// Length of the area in bytes, a whole number of pages, not counting the guard page that follows it
    size: usize,
}

/// The areas in use, `None` for a free slot. Slots are not kept in any order.
static AREAS: Mutex<[Option<Area>; MAX_AREAS]> = Mutex::new([None; MAX_AREAS]);

/// Why `ioremap` refused a request.
#[derive(Debug)]
pub enum IoremapError {
    /// The requested size was zero.
    ZeroSize,
    /// `pa + size` wraps around the end of the physical address space.
    Overflow,
    /// The range reaches past the physical address size the MMU is configured for (`PA_BITS`).
    OutOfRange,
    /// The range overlaps RAM, which is reached through the linear map and must not get a second, Device mapping.
    IsRam,
    /// No free range in the `ioremap` region, or no free slot in the area table.
    NoVirtualSpace,
}

/// Reserves a range of virtual addresses in the `ioremap` region and records it in the area table.
///
/// First-fit: the lowest address where `size` bytes fit without touching an existing area, leaving one unmapped guard
/// page after the new area (the guard is not part of the recorded size). The whole search and the insertion happen
/// under one lock, so two callers can't be given the same range. This only reserves addresses; nothing is mapped.
///
/// # Arguments
///
/// * `size` - length in bytes, a non-zero multiple of the page size
///
/// # Returns
///
/// The page-aligned start of the reserved range, or `IoremapError::NoVirtualSpace` if the region or the table is full.
///
/// # Panics
///
/// In debug builds, if `size` is zero or not page-aligned (`ioremap` guarantees both before calling).
pub fn alloc_va(size: usize) -> Result<VirtAddr, IoremapError> {
    debug_assert!(size > 0 && size & !PAGE_MASK == 0);
    let needed = size as u64 + PAGE_SIZE as u64;
    let mut candidate = IOREMAP_START;

    AREAS.lock_irqsafe(|areas| -> Result<VirtAddr, IoremapError> {
        'rescan: loop {
            // Iterate all over the areas excluding the 'None' ones
            for area in areas.iter().flatten() {
                // The end of the area includes a page guard for our checks
                let area_end = area.start.as_u64() + area.size as u64 + PAGE_SIZE as u64;

                // The test is half-open, on the whole new range and not only on its first byte:
                //
                //   area:       [area_start ........ area_end)
                //   candidate:      [candidate ........ candidate + needed)
                //
                // They overlap if they share any address, which is `candidate < area_end` and
                // `area_start < candidate + needed`. The second one is what catches a candidate that starts before
                // the area but reaches into it.
                if candidate < area_end && area.start.as_u64() < candidate + needed {
                    candidate = area_end;
                    continue 'rescan;
                }
            }

            break;
        }

        if candidate + needed > IOREMAP_END {
            return Err(IoremapError::NoVirtualSpace);
        }

        // Try to find a free slot
        let Some(slot) = areas.iter_mut().find(|slot| slot.is_none()) else {
            return Err(IoremapError::NoVirtualSpace);
        };

        let addr = VirtAddr::new(candidate);
        *slot = Some(Area { start: addr, size });

        Ok(addr)
    })
}

/// Maps a block of device registers into the `ioremap` region and returns a handle to it.
///
/// Rejects an empty or wrapping range and any range that overlaps RAM (RAM is reached through the linear map, and a
/// second mapping of it with different attributes would be a mismatched alias). Otherwise it reserves a virtual range
/// with `alloc_va`, then maps whole pages of the device with Device-nGnRE attributes, never executable, in the kernel
/// root table. The physical address does not need to be page-aligned: the mapping starts at its page, and the handle
/// points at the same offset inside that page.
///
/// # Arguments
///
/// * `pa` - physical address of the first register of the block
/// * `size` - length of the block in bytes, as the DTB `reg` property gives it
///
/// # Returns
///
/// An `IoMem` for `[pa, pa + size)`, or an `IoremapError`: `ZeroSize`, `Overflow` if the arithmetic on the range wraps,
/// `OutOfRange` if it reaches past the physical address size (`PA_BITS`), `IsRam` if it overlaps RAM, `NoVirtualSpace`
/// if the region or the area table is full.
///
/// # Panics
///
/// If `map_range` runs out of frames for page tables or finds the virtual pages already mapped. Those are kernel bugs
/// or fatal conditions, not something a driver can recover from, so they are not errors.
pub fn ioremap(pa: PhysAddr, size: usize) -> Result<IoMem, IoremapError> {
    let (ram_start, ram_size) = meminfo::ram_range();
    let ram_end = ram_start + ram_size as u64;

    if size == 0 {
        return Err(IoremapError::ZeroSize);
    }

    let base = pa.align_down(PAGE_SIZE as u64);
    let offset = pa - base;

    // Rounding up to whole pages can overflow even when pa + size did not (a range ending in the last page of the
    // address space), so both the rounding and the end of the mapped range are checked.
    let Some(len) = offset.checked_add(size as u64) else {
        return Err(IoremapError::Overflow);
    };
    let Some(map_size) = len.checked_next_multiple_of(PAGE_SIZE as u64) else {
        return Err(IoremapError::Overflow);
    };
    let Some(map_end) = base.as_u64().checked_add(map_size) else {
        return Err(IoremapError::Overflow);
    };

    // The MMU only translates addresses below 1 << PA_BITS (TCR_EL1.IPS). map_end is an exclusive end, so it can be
    // equal to that limit, and only a larger value is out of range.
    if map_end > 1u64 << PA_BITS {
        return Err(IoremapError::OutOfRange);
    }

    // The pages that will be mapped are what must not overlap RAM, not just the bytes asked for.
    let map_end = PhysAddr::new(map_end);
    if base < ram_end && ram_start < map_end {
        return Err(IoremapError::IsRam);
    }

    // SH is ignored for Device memory (always Outer Shareable, Arm ARM D8.6); same value as the idmap's Device block
    let prot = map::Prot {
        ap: leaf::Ap::RwEl1,
        mair: MairIdx::Device,
        sh: leaf::Shareability::NonShareable,
        attrs: leaf::PXN | leaf::UXN | leaf::AF,
    };

    // The final '?' will return error if alloc_va returns error, otherwise va will be the returned
    // va
    let va = alloc_va(map_size as usize)?;
    map::map_range(kernel_pgd(), va, base, map_size, prot);

    Ok(IoMem::new(va + offset, size))
}

/// Checks `ioremap`: that a mapped device block reads the same as through the identity map, and that bad requests
/// are refused.
///
/// Must run after the MMU is on, `TTBR1` is loaded and the linear map is live. The mapping test maps the PL011 and
/// compares its PrimeCell ID registers (`0xFE0` to `0xFFC`, read-only and constant) through the new `IoMem` and
/// through the identity map, which is still there. The error tests ask for an empty range, a range in RAM and a range
/// past the physical address size, and expect `ZeroSize`, `IsRam` and `OutOfRange`. A mismatch panics; a missing
/// mapping shows up as a data abort instead.
pub fn check_ioremap() {
    const UART_BASE: PhysAddr = PhysAddr::new(0x0900_0000);
    let va = ioremap(UART_BASE, PAGE_SIZE).expect("check_ioremap: ioremap of the PL011 failed");

    // The eight PrimeCell ID registers: PeriphID0-3 at 0xFE0..=0xFEC and PCellID0-3 at 0xFF0..=0xFFC
    for offset in (0xFE0..=0xFFC).step_by(4) {
        let read_ioremap = va.read32(offset);
        let read_idmap = mmio::read_mmio32(0x0900_0000, offset);

        if read_idmap != read_ioremap {
            panic!(
                "check_ioremap: PL011 ID register at {offset:#x} reads {read_ioremap:#x} through the ioremap mapping \
                 and {read_idmap:#x} through the identity map"
            );
        }
    }

    // Bad requests are refused before anything is reserved or mapped, so these leave no trace in the area table
    let result = ioremap(UART_BASE, 0);
    if !matches!(result, Err(IoremapError::ZeroSize)) {
        panic!("check_ioremap: an empty range returned {result:?} instead of ZeroSize");
    }

    let (ram_start, _) = meminfo::ram_range();
    let result = ioremap(ram_start, PAGE_SIZE);
    if !matches!(result, Err(IoremapError::IsRam)) {
        panic!("check_ioremap: a range in RAM returned {result:?} instead of IsRam");
    }

    let result = ioremap(PhysAddr::new(1 << PA_BITS), PAGE_SIZE);
    if !matches!(result, Err(IoremapError::OutOfRange)) {
        panic!("check_ioremap: a range past the physical address size returned {result:?} instead of OutOfRange");
    }

    pr_info!(
        "ioremap",
        "read-back OK (PL011 ID registers via the ioremap and identity mappings, three bad requests refused)"
    );
}
