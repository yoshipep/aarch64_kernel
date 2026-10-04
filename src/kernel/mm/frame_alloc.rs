//! Physical frame allocator
//!
//! Hands out and reclaims physical memory one 4 KiB page (frame) at a time — no variable-size allocation, no
//! fragmentation handling. Every consumer at this stage (page-table pages for the upper-half mapping, `ioremap`'s own
//! page-table pages) needs exactly one frame per request, so a fixed-granularity allocator is a complete fit: it gives
//! up nothing a general-purpose allocator would provide that's actually used here, while avoiding the split/coalesce
//! bookkeeping a variable-size allocator would need. A general-purpose allocator (heap/`kmalloc`-style, or a later
//! buddy-style allocator) is a deliberately separate, future piece of work, layered on top of this one once something
//! concrete needs it — see `TODOWORK.md`.
//!
//! # Design
//!
//! Modeled on xv6's `kalloc.c`: free frames are tracked as a singly-linked stack threaded through the free frames
//! themselves — each free frame's own first 8 bytes hold a pointer to the next free frame, with a single head pointer
//! as the only separate bookkeeping state. This works because free frames are already identity-mapped and writable at
//! init time, and it means the free list needs no allocator of its own to build: `free_frame` pushes a frame onto the
//! front of the list by writing the current head into the frame's own memory, then repointing the head at it;
//! `alloc_frame` pops the front the same way in reverse. Both are O(1) — no traversal.
//!
//! Unlike xv6 (where the kernel is linked at the very base of RAM, so the only free region is "above the kernel"),
//! this kernel is linked at `0x50000000` while RAM starts at `0x40000000` — there is a genuine free region *below*
//! the kernel image too. Initialization frees both ranges:
//! - `[RAM base, kernel image start)`
//! - `[kernel image end / stack top, RAM end)`
//!
//! both as reported by the `/memory` node (via `meminfo`), with the kernel image (`__kernel_start`/`__kernel_end`),
//! the stack (`__stack_top`/`STACK_SIZE`), and the raw DTB blob's own physical range excluded from both.
//!
//! # Non-goals
//!
//! No double-free or use-after-free detection, matching xv6's own `kalloc.c`: there is no adversarial or concurrent
//! boundary yet (single-threaded, no scheduler, only this kernel's own code calls in), so a double-free is a bug to
//! catch while writing this code, not an attack surface to defend against. If that changes (multitasking, less-trusted
//! callers), this is the point to revisit.

use crate::{
    ipc::irq_safe_mutex::Mutex,
    kernel::{mm::pgtable::PAGE_SIZE, phys_addr::PhysAddr},
    pr_info,
};

unsafe extern "C" {
    static mut __kernel_start: u8;
    static mut __stack_top: u8;
}

/// A free physical frame's self-hosted free-list node.
///
/// Every free frame's own first 8 bytes are reinterpreted as this struct — `next` points at the next free frame, or
/// is null for the last one. See the module doc for the full push/pop mechanism.
#[repr(C)]
#[derive(Copy, Clone)]
struct Frame {
    pub next: *mut Frame,
}

/// Head of the free-frame list. The head pointer itself lives *inside* the lock (not next to it) — `Mutex<T>`'s
/// safety only holds if `T` is the only copy of the protected state, reachable exclusively through `lock`/
/// `lock_irqsafe`'s closure.
static FREE_LIST: Mutex<*mut Frame> = Mutex::new(core::ptr::null_mut());

/// Allocates one 4 KiB physical frame.
///
/// Pops the head of the free list. Returns `PhysAddr::new(0)` if none are free — check for that before use, the same
/// null-as-failure convention xv6's `kalloc` uses; address `0` is never itself an allocatable frame on this target
/// (RAM starts at `0x40000000`).
pub fn alloc() -> PhysAddr {
    let mut frame = core::ptr::null_mut();

    unsafe {
        FREE_LIST.lock_irqsafe(|head| {
            frame = *head;
            if !frame.is_null() {
                *head = (*frame).next;
            }
        });

        if !frame.is_null() {
            core::ptr::write_bytes(frame as *mut u8, 0x3E, PAGE_SIZE);
        }

        PhysAddr::new(frame as u64)
    }
}

/// Returns a physical frame to the free list.
///
/// `addr` must be page-aligned and must be a frame previously handed out by `alloc`, or one of the frames this
/// allocator's own `init` seeds the list with. Not validated — see the module doc's Non-goals section.
pub fn free(addr: PhysAddr) {
    unsafe {
        core::ptr::write_bytes(addr.as_mut_ptr::<u8>(), 0xA0, PAGE_SIZE);

        let frame = addr.as_mut_ptr::<Frame>();
        FREE_LIST.lock_irqsafe(|head| {
            (*frame).next = *head;
            *head = frame;
        });
    }
}

/// Builds the initial free list from the reported RAM range, excluding the kernel image, its stack, and the raw DTB
/// blob still in memory at this point.
///
/// `start_ram`/`ram_size` come from the `/memory` node (see `meminfo::ram_range`); `start_dtb`/`end_dtb` bound the
/// DTB blob itself. Kernel/stack bounds come directly from the linker symbols `__kernel_start`/`__stack_top`, not
/// from a parameter. See the module doc for why the free range splits into up to three pieces around these two
/// exclusion windows.
pub fn init(start_ram: PhysAddr, ram_size: usize, start_dtb: PhysAddr, end_dtb: PhysAddr) {
    let ram_end = start_ram + ram_size as u64;

    let kernel_start = PhysAddr::new(core::ptr::addr_of!(__kernel_start) as u64);
    let kernel_end = PhysAddr::new(core::ptr::addr_of!(__stack_top) as u64);

    let mut occupied_ranges = [(kernel_start, kernel_end), (start_dtb, end_dtb)];
    if occupied_ranges[0].0 > occupied_ranges[1].0 {
        occupied_ranges.swap(0, 1);
    }

    pr_info!(
        "frame_alloc",
        "Occupied range 0 [{:#X}-{:#X}]",
        occupied_ranges[0].0,
        occupied_ranges[0].1
    );
    pr_info!(
        "frame_alloc",
        "Occupied range 1 [{:#X}-{:#X}]",
        occupied_ranges[1].0,
        occupied_ranges[1].1
    );

    free_range(start_ram, occupied_ranges[0].0);
    free_range(occupied_ranges[0].1, occupied_ranges[1].0);
    free_range(occupied_ranges[1].1, ram_end);
}

/// Frees every whole page inside `[start, end)`.
///
/// Rounds `start` up and `end` down to page boundaries first: `start_dtb`/`end_dtb` aren't
/// guaranteed page-aligned (unlike the linker symbols), so a partial page at either edge is
/// conservatively left unfreed rather than freeing a page that isn't fully inside the range. If
/// `start` and `end` cross (e.g. two holes that touch or overlap), the loop below simply does
/// nothing for that segment — never frees anything it isn't sure about.
fn free_range(start: PhysAddr, end: PhysAddr) {
    let page_size = PAGE_SIZE as u64;
    let mut page = start.align_up(page_size);
    let end = end.align_down(page_size);

    while page < end {
        free(page);
        page = page + page_size;
    }
}
