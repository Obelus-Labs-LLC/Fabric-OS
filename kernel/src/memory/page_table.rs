#![allow(dead_code)]

use crate::memory::PhysAddr;

/// x86_64 page table entry flags
#[derive(Clone, Copy, Debug)]
pub struct PageTableFlags(u64);

impl PageTableFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER: Self = Self(1 << 2);
    pub const WRITE_THROUGH: Self = Self(1 << 3);
    pub const NO_CACHE: Self = Self(1 << 4);
    pub const ACCESSED: Self = Self(1 << 5);
    pub const DIRTY: Self = Self(1 << 6);
    pub const HUGE_PAGE: Self = Self(1 << 7);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(1u64 << 63);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl core::ops::BitOr for PageTableFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for PageTableFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Address mask: bits 12-51 hold the physical page frame number
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// A single page table entry (u64)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn is_present(&self) -> bool {
        self.0 & PageTableFlags::PRESENT.0 != 0
    }

    /// Extract the physical address from bits 12-51
    pub fn addr(&self) -> PhysAddr {
        PhysAddr(self.0 & ADDR_MASK)
    }

    /// Extract the flags (bits 0-11 and 52-63)
    pub fn flags(&self) -> PageTableFlags {
        PageTableFlags(self.0 & !ADDR_MASK)
    }

    /// Set both address and flags
    pub fn set(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        self.0 = (addr.0 & ADDR_MASK) | flags.0;
    }

    /// Clear this entry
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// A page table: 512 entries, 4096-byte aligned
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    /// Zero out all entries
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = PageTableEntry::empty();
        }
    }
}

// --- Virtual address decomposition ---

pub fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

pub fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}

pub fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}

pub fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}
