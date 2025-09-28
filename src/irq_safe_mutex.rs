//! An interrupt-safe Spinlock Mutex
//!
//! This module provides a fundamental synchronization primitive. The central component is a `Mutex` that uses a spinlock for mutual exclusion.
//!
//! It's most critical feature is the `lock_irqsafe` method, which is essential for preventing
//! deadlocks between main kernel code and Interrupt Service Routines (ISR).

use core::arch::asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Disable IRQs in the CPU
#[inline(always)]
fn disable_irq() -> u64 {
    let daif: u64;
    unsafe {
        asm!("mrs {}, daif", out(reg) daif);
        asm!("msr daifset, #0x2");
    }
    daif
}

/// Enables interrupts in the CPU
///
/// Enable the interrupts by writting the `daif` state back to the DAIF register
#[inline(always)]
fn restore_interrupts(daif: u64) {
    unsafe {
        asm!("msr daif, {}", in(reg) daif);
    }
}

/// A mutual exclusive (Mutex) primitive based on a spinlock
///
/// This Mutex provides safe interior mutability by ensuring that only one thread can access the
/// contained data at any given time. It uses an atomic boolean flag and a busy-wait loop to
/// achieve this.
pub struct Mutex<T> {
    /// the atomic flag used to control access. `false` means unlocked, `true` locked
    lock: AtomicBool,
    // The data protected by the mutex, wrapped in an `UnsafeCell` to allow mutable access through
    // a shared reference
    data: UnsafeCell<T>,
}

/// Safety: The `Mutex` is safe to share across threads because access to the inner `UnsafeCell` is
/// guarded by the atomic `lock`
unsafe impl<T> Sync for Mutex<T> {}

/// Safety: The `Mutex` is safe to send to another thread, as the data `T` is owned by the Mutex
/// and the lock mechanism is thread-safe
unsafe impl<T> Send for Mutex<T> {}

impl<T> Mutex<T> {
    /// Creates a new `Mutex` in an unlocked state containing the provided data
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    /// Acquires the lock and provides mutable access to the protected data
    ///
    /// # Memory Ordering
    /// - **Acquire**: An `Acquire` memory ordering is used when obtaining the lock. This ensures
    ///     that all memory operations happening *after* acquiring the lock are not reordered to before
    ///     it.
    /// - **Release**: A `Release` memory ordering is used when releasing the lock. This ensures
    ///     that all memory operations happening *before* releasing the lock are not reordered to after
    ///     it.
    pub fn lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let result = f(unsafe { &mut *self.data.get() });
        self.lock.store(false, Ordering::Release);
        result
    }

    /// Acquires the lock in an interrupt-safe manner
    pub fn lock_irqsafe<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let daif_state = disable_irq();
        let result = self.lock(f);
        restore_interrupts(daif_state);
        result
    }
}
