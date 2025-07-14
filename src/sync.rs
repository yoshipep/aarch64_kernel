use core::arch::asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

#[inline(always)]
fn disable_interrupts() -> u64 {
    let daif: u64;
    unsafe {
        asm!("mrs {}, daif", out(reg) daif);
        asm!("msr daifset, #0x2");
    }
    daif
}

#[inline(always)]
fn restore_interrupts(daif: u64) {
    unsafe {
        asm!("msr daif, {}", in(reg) daif);
    }
}

pub struct Mutex<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T> Sync for Mutex<T> {}
unsafe impl<T> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

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

    pub fn lock_irqsafe<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let daif_state = disable_interrupts();

        let result = self.lock(f);
        restore_interrupts(daif_state);
        result
    }
}
