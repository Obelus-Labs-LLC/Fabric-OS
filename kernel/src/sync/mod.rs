//! Lock ordering enforcement (TD-003).
//!
//! `OrderedMutex<T, LEVEL>` wraps `spin::Mutex<T>` with a compile-time lock
//! level. In debug builds, runtime checks panic on out-of-order acquisition.
//! In release builds, this is zero-overhead — inlines to plain `spin::Mutex`.
//!
//! **Convention**: Locks must be acquired in strictly ascending level order.
//! Acquiring level N while holding level M where N <= M is a violation.
//!
//! `try_lock()` bypasses ordering checks (IRQ-safe: returns None on contention
//! instead of spinning, and doesn't interact with the ordering tracker).
//!
//! Exempt from ordering (plain `spin::Mutex`):
//! - `SERIAL` — logging, called from everywhere including inside locked scopes
//! - `ALLOCATOR` — frame allocation, called during page table ops under other locks

#![allow(dead_code)]

use core::ops::{Deref, DerefMut};

// ── Lock level constants ────────────────────────────────────────────────────

/// Lock levels — strictly ascending acquisition order required.
pub mod levels {
    pub const HAL: u32 = 0;         // DMA_MANAGER, IRQ_ROUTER, PCI_DRIVER_TABLE, REGISTRY, NIC, ACTIVE_NIC
    pub const INPUT: u32 = 1;       // KEYBOARD_BUFFER, GAMEPAD_TABLE, AUDIO_MIXER, WINDOW_TABLE
    pub const NETWORK: u32 = 2;     // ARP_TABLE, DNS_CACHE, SOCKETS, LOOPBACK
    pub const DISPLAY: u32 = 3;     // DISPLAY, SURFACE_TABLE
    pub const VFS: u32 = 4;         // INODES, MOUNTS, OPEN_FILES, TMPFS, DEVFS
    pub const GOVERNANCE: u32 = 5;  // GOVERNANCE, VM_TABLE
    pub const TABLE: u32 = 6;       // TABLE, BUTLER_STATE
    pub const STORE: u32 = 7;       // STORE, COUNCIL
    pub const SCHEDULER: u32 = 8;   // SCHEDULER
    pub const BUS: u32 = 9;         // BUS
}

// ── Debug-only tracking ─────────────────────────────────────────────────────

#[cfg(debug_assertions)]
mod tracking {
    const MAX_HELD: usize = 8;

    // Single-CPU kernel: static mut is safe with interrupts managed.
    // try_lock() does not interact with tracking, so IRQ handlers are safe.
    static mut LOCK_STACK: [u32; MAX_HELD] = [0; MAX_HELD];
    static mut LOCK_STACK_LEN: usize = 0;

    /// Push a lock level. Panics if ordering is violated.
    pub fn push_level(level: u32) {
        unsafe {
            let len = LOCK_STACK_LEN;
            for i in 0..len {
                if level <= LOCK_STACK[i] {
                    panic!(
                        "[LOCK ORDER] Violation: acquiring level {} while holding level {}",
                        level, LOCK_STACK[i]
                    );
                }
            }
            if len >= MAX_HELD {
                panic!("[LOCK ORDER] Stack overflow: {} nested locks", len);
            }
            LOCK_STACK[len] = level;
            LOCK_STACK_LEN = len + 1;
        }
    }

    /// Remove a lock level from the stack.
    pub fn pop_level(level: u32) {
        unsafe {
            let len = LOCK_STACK_LEN;
            for i in 0..len {
                if LOCK_STACK[i] == level {
                    // Shift remaining entries left
                    let mut j = i;
                    while j + 1 < len {
                        LOCK_STACK[j] = LOCK_STACK[j + 1];
                        j += 1;
                    }
                    LOCK_STACK_LEN = len - 1;
                    return;
                }
            }
            // Not found — defensive no-op
        }
    }
}

// ── OrderedMutex ────────────────────────────────────────────────────────────

/// A mutex with compile-time lock level for ordering enforcement.
///
/// Wraps `spin::Mutex<T>`. In debug builds, `lock()` checks that no
/// currently-held lock has a level >= LEVEL. In release builds, this
/// compiles to exactly `spin::Mutex<T>` with zero overhead.
pub struct OrderedMutex<T, const LEVEL: u32>(spin::Mutex<T>);

// Safety: inherits Send/Sync from spin::Mutex<T>.
unsafe impl<T: Send, const LEVEL: u32> Send for OrderedMutex<T, LEVEL> {}
unsafe impl<T: Send, const LEVEL: u32> Sync for OrderedMutex<T, LEVEL> {}

/// RAII guard returned by `OrderedMutex::lock()`.
/// Pops the lock level from the tracking stack on drop (debug only).
pub struct OrderedMutexGuard<'a, T, const LEVEL: u32> {
    inner: spin::MutexGuard<'a, T>,
}

impl<T, const LEVEL: u32> OrderedMutex<T, LEVEL> {
    /// Create a new ordered mutex (const-constructible for statics).
    pub const fn new(value: T) -> Self {
        Self(spin::Mutex::new(value))
    }

    /// Acquire the lock. Panics in debug builds if lock ordering is violated.
    #[inline]
    pub fn lock(&self) -> OrderedMutexGuard<'_, T, LEVEL> {
        #[cfg(debug_assertions)]
        tracking::push_level(LEVEL);
        OrderedMutexGuard { inner: self.0.lock() }
    }

    /// Try to acquire the lock without blocking.
    ///
    /// Returns `None` if the lock is held. Does NOT participate in ordering
    /// enforcement — safe to use in IRQ handlers.
    #[inline]
    pub fn try_lock(&self) -> Option<spin::MutexGuard<'_, T>> {
        self.0.try_lock()
    }
}

impl<'a, T, const LEVEL: u32> Deref for OrderedMutexGuard<'a, T, LEVEL> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &*self.inner
    }
}

impl<'a, T, const LEVEL: u32> DerefMut for OrderedMutexGuard<'a, T, LEVEL> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.inner
    }
}

impl<'a, T, const LEVEL: u32> Drop for OrderedMutexGuard<'a, T, LEVEL> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        tracking::pop_level(LEVEL);
    }
}
