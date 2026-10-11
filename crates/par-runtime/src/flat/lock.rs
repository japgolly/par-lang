//! A minimal spin lock for the runtime's tiny critical sections.
//!
//! The runtime's locks guard a few moves of nodes in and out, are practically
//! never contended, and are taken extremely often. `std::sync::Mutex` is too
//! heavy for that on some platforms (on macOS, it lazily allocates a pthread
//! mutex), and compare-and-swap based locks turned out considerably slower on
//! Apple silicon than locking with a swap, which is what this does.

use std::cell::UnsafeCell;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: access to `value` is serialized by `locked`.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    #[inline(always)]
    pub(crate) fn lock(&self) -> SpinLockGuard<'_, T> {
        while self.locked.swap(true, Acquire) {
            self.wait();
        }
        SpinLockGuard { lock: self }
    }

    pub(crate) fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        if self.locked.swap(true, Acquire) {
            None
        } else {
            Some(SpinLockGuard { lock: self })
        }
    }

    #[cold]
    fn wait(&self) {
        let mut spins = 0u32;
        while self.locked.load(Relaxed) {
            if spins < 100 {
                spins += 1;
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }
}

impl<T: Debug> Debug for SpinLock<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.try_lock() {
            Some(value) => f.debug_tuple("SpinLock").field(&*value).finish(),
            None => f.write_str("SpinLock(<locked>)"),
        }
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the lock.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the lock.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    #[inline(always)]
    fn drop(&mut self) {
        self.lock.locked.store(false, Release);
    }
}
