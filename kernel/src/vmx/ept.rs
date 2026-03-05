//! Extended Page Tables (EPT) for guest physical address translation.
//!
//! 4-level page table structure identical to regular x86_64 paging but with
//! EPT-specific flags (Read/Write/Execute instead of Present/Writable/etc).
//! Used for both hardware VMX (EPTP in VMCS) and software emulation
//! (translate() called by instruction emulator).

#![allow(dead_code)]

use crate::memory::{PhysAddr, PAGE_SIZE, hhdm_offset};
use crate::memory::frame;
use alloc::vec::Vec;

/// EPT entry flags (Intel SDM Vol 3, 28.3.2).
#[derive(Clone, Copy, Debug)]
pub struct EptFlags(pub u64);

impl EptFlags {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const EXECUTE: Self = Self(1 << 2);
    /// Memory type WB (writeback) in bits 5:3
    pub const MEMORY_TYPE_WB: Self = Self(6 << 3);
    /// Ignore PAT memory type
    pub const IGNORE_PAT: Self = Self(1 << 6);

    /// Standard RWX + WB for normal guest pages
    pub const RWX_WB: Self = Self(
        Self::READ.0 | Self::WRITE.0 | Self::EXECUTE.0 | Self::MEMORY_TYPE_WB.0
    );

    pub const fn bits(self) -> u64 { self.0 }
    pub const fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
}

impl core::ops::BitOr for EptFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

/// Address mask: bits 12-51 hold the physical frame number
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// A single EPT page table entry.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct EptEntry(u64);

impl EptEntry {
    pub const fn empty() -> Self { Self(0) }

    pub fn is_present(&self) -> bool {
        // EPT entry is present if any of R/W/X bits are set
        self.0 & 0x7 != 0
    }

    pub fn addr(&self) -> PhysAddr {
        PhysAddr(self.0 & ADDR_MASK)
    }

    pub fn set(&mut self, addr: PhysAddr, flags: EptFlags) {
        self.0 = (addr.0 & ADDR_MASK) | flags.0;
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }

    pub fn raw(&self) -> u64 { self.0 }
}

/// 4-level EPT page table (512 entries, 4K aligned).
#[repr(C, align(4096))]
pub struct EptTable {
    pub entries: [EptEntry; 512],
}

/// EPT context for a single virtual machine.
pub struct EptContext {
    /// Physical address of the PML4 (top-level EPT table).
    pml4_phys: PhysAddr,
    /// Number of mapped guest pages.
    mapped_pages: usize,
    /// Track allocated intermediate table frames for cleanup.
    allocated_frames: Vec<PhysAddr>,
}

/// EPT operation errors.
#[derive(Debug)]
pub enum EptError {
    AllocationFailed,
    AlreadyMapped,
    NotMapped,
}

impl EptContext {
    /// Create a new empty EPT context with a fresh PML4.
    pub fn create() -> Result<Self, EptError> {
        let pml4_phys = frame::allocate_frame().ok_or(EptError::AllocationFailed)?;

        // Zero the PML4 via HHDM
        let pml4_virt = pml4_phys.0 + hhdm_offset();
        unsafe {
            core::ptr::write_bytes(pml4_virt as *mut u8, 0, PAGE_SIZE);
        }

        let mut allocated = Vec::new();
        allocated.push(pml4_phys);

        Ok(Self {
            pml4_phys,
            mapped_pages: 0,
            allocated_frames: allocated,
        })
    }

    /// Map a guest physical page to a host physical page.
    pub fn map_page(&mut self, guest_phys: u64, host_phys: PhysAddr, flags: EptFlags) -> Result<(), EptError> {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx   = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx   = ((guest_phys >> 12) & 0x1FF) as usize;

        // Walk or create: PML4 -> PDPT
        let pdpt_phys = self.walk_or_create(self.pml4_phys, pml4_idx)?;
        // PDPT -> PD
        let pd_phys = self.walk_or_create(pdpt_phys, pdpt_idx)?;
        // PD -> PT
        let pt_phys = self.walk_or_create(pd_phys, pd_idx)?;

        // Set the leaf entry
        let pt_virt = pt_phys.0 + hhdm_offset();
        let pt = unsafe { &mut *(pt_virt as *mut EptTable) };

        if pt.entries[pt_idx].is_present() {
            return Err(EptError::AlreadyMapped);
        }

        pt.entries[pt_idx].set(host_phys, flags);
        self.mapped_pages += 1;
        Ok(())
    }

    /// Translate a guest physical address to a host physical address.
    pub fn translate(&self, guest_phys: u64) -> Option<PhysAddr> {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx   = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx   = ((guest_phys >> 12) & 0x1FF) as usize;
        let offset   = guest_phys & 0xFFF;

        let pdpt_phys = self.walk_entry(self.pml4_phys, pml4_idx)?;
        let pd_phys = self.walk_entry(pdpt_phys, pdpt_idx)?;
        let pt_phys = self.walk_entry(pd_phys, pd_idx)?;

        let pt_virt = pt_phys.0 + hhdm_offset();
        let pt = unsafe { &*(pt_virt as *const EptTable) };

        if !pt.entries[pt_idx].is_present() {
            return None;
        }

        Some(PhysAddr(pt.entries[pt_idx].addr().0 + offset))
    }

    /// Unmap a guest physical page. Returns the host physical address that was mapped.
    pub fn unmap_page(&mut self, guest_phys: u64) -> Result<PhysAddr, EptError> {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx   = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx   = ((guest_phys >> 12) & 0x1FF) as usize;

        let pdpt_phys = self.walk_entry(self.pml4_phys, pml4_idx).ok_or(EptError::NotMapped)?;
        let pd_phys = self.walk_entry(pdpt_phys, pdpt_idx).ok_or(EptError::NotMapped)?;
        let pt_phys = self.walk_entry(pd_phys, pd_idx).ok_or(EptError::NotMapped)?;

        let pt_virt = pt_phys.0 + hhdm_offset();
        let pt = unsafe { &mut *(pt_virt as *mut EptTable) };

        if !pt.entries[pt_idx].is_present() {
            return Err(EptError::NotMapped);
        }

        let host_phys = pt.entries[pt_idx].addr();
        pt.entries[pt_idx].clear();
        self.mapped_pages -= 1;
        Ok(host_phys)
    }

    /// Construct the 64-bit EPTP value for the VMCS.
    /// Memory type WB (6), page walk length 3 (4-level), PML4 physical address.
    pub fn eptp(&self) -> u64 {
        let mem_type_wb: u64 = 6; // bits 2:0
        let walk_length: u64 = 3; // bits 5:3 (walk length - 1 = 3 for 4-level)
        mem_type_wb | (walk_length << 3) | (self.pml4_phys.0 & ADDR_MASK)
    }

    /// Get the PML4 physical address.
    pub fn pml4_phys(&self) -> PhysAddr {
        self.pml4_phys
    }

    /// Number of mapped guest pages.
    pub fn mapped_pages(&self) -> usize {
        self.mapped_pages
    }

    /// Destroy this EPT context, freeing all allocated table frames.
    /// Does NOT free the guest data frames (those are managed by VirtualMachine).
    pub fn destroy(&mut self) {
        for phys in self.allocated_frames.drain(..) {
            frame::deallocate_frame(phys);
        }
        self.mapped_pages = 0;
    }

    // --- Private helpers ---

    /// Walk a table entry. If present, return the physical address it points to.
    fn walk_entry(&self, table_phys: PhysAddr, index: usize) -> Option<PhysAddr> {
        let table_virt = table_phys.0 + hhdm_offset();
        let table = unsafe { &*(table_virt as *const EptTable) };
        if table.entries[index].is_present() {
            Some(table.entries[index].addr())
        } else {
            None
        }
    }

    /// Walk or create: if entry at index is not present, allocate a new table.
    fn walk_or_create(&mut self, table_phys: PhysAddr, index: usize) -> Result<PhysAddr, EptError> {
        let table_virt = table_phys.0 + hhdm_offset();
        let table = unsafe { &mut *(table_virt as *mut EptTable) };

        if table.entries[index].is_present() {
            Ok(table.entries[index].addr())
        } else {
            // Allocate a new table frame
            let new_phys = frame::allocate_frame().ok_or(EptError::AllocationFailed)?;
            // Zero it
            let new_virt = new_phys.0 + hhdm_offset();
            unsafe {
                core::ptr::write_bytes(new_virt as *mut u8, 0, PAGE_SIZE);
            }
            // Set the entry with RWX flags (intermediate tables need all permissions)
            let intermediate_flags = EptFlags::READ | EptFlags::WRITE | EptFlags::EXECUTE;
            table.entries[index].set(new_phys, intermediate_flags);
            self.allocated_frames.push(new_phys);
            Ok(new_phys)
        }
    }
}
