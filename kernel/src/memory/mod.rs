#![allow(dead_code)]

pub mod frame;
pub mod heap;
pub mod mapper;
pub mod page_table;

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

pub const PAGE_SIZE: usize = 4096;

static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

pub fn init_hhdm(offset: u64) {
    HHDM_OFFSET.store(offset, Ordering::Release);
}

pub fn hhdm_offset() -> u64 {
    HHDM_OFFSET.load(Ordering::Acquire)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(pub u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn to_virt(self) -> VirtAddr {
        VirtAddr(self.0 + hhdm_offset())
    }
}

impl VirtAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn to_phys(self) -> PhysAddr {
        PhysAddr(self.0 - hhdm_offset())
    }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr(0x{:016x})", self.0)
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:016x}", self.0)
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr(0x{:016x})", self.0)
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:016x}", self.0)
    }
}
