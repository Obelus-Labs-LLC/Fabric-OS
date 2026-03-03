//! Per-process address space — PML4 page table management.
//!
//! Each process gets its own PML4 (Level 4 page table). The upper half
//! (entries 256-511) is cloned from the kernel PML4 so kernel code, HHDM,
//! and heap are always accessible. The lower half (entries 0-255) is empty,
//! ready for Phase 7 userspace mappings.
//!
//! In Phase 6, CR3 is stored in the PCB but NOT loaded — no ring 3 switch
//! yet. This builds the infrastructure for Phase 7.

#![allow(dead_code)]

use crate::memory::{PhysAddr, VirtAddr};
use crate::memory::page_table::{PageTable, PageTableEntry, PageTableFlags};
use crate::memory::frame;

/// Errors from address space operations.
#[derive(Debug)]
#[must_use]
pub enum AddressSpaceError {
    /// Could not allocate a frame for the PML4.
    AllocationFailed,
    /// Virtual address is in the kernel half (>= 0xFFFF_8000_0000_0000).
    KernelAddressViolation,
    /// Page already mapped.
    AlreadyMapped,
    /// Page not mapped.
    NotMapped,
}

/// Per-process address space.
pub struct AddressSpace {
    /// Physical address of the PML4 page.
    pml4_phys: PhysAddr,
    /// HHDM-mapped virtual address of PML4 (for kernel access).
    pml4_virt: VirtAddr,
    /// Number of user page frames allocated (for leak tracking).
    user_page_count: usize,
    /// Whether this address space is valid.
    active: bool,
}

impl AddressSpace {
    /// Create a new per-process address space.
    /// Allocates a PML4 frame and clones the kernel upper half.
    pub fn create() -> Result<Self, AddressSpaceError> {
        // Allocate a frame for the PML4
        let pml4_phys = frame::allocate_frame()
            .ok_or(AddressSpaceError::AllocationFailed)?;
        let pml4_virt = pml4_phys.to_virt();

        // Zero the entire PML4
        let pml4 = unsafe { &mut *(pml4_virt.as_u64() as *mut PageTable) };
        pml4.zero();

        // Clone kernel PML4 upper half (entries 256-511)
        let kernel_pml4_phys = Self::read_cr3();
        let kernel_pml4 = unsafe {
            &*(kernel_pml4_phys.to_virt().as_u64() as *const PageTable)
        };

        for i in 256..512 {
            pml4.entries[i] = kernel_pml4.entries[i];
        }

        Ok(Self {
            pml4_phys,
            pml4_virt,
            user_page_count: 0,
            active: true,
        })
    }

    /// Map a user-space page (lower half only, entries 0-255).
    pub fn map_user_page(
        &mut self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), AddressSpaceError> {
        // Verify address is in user half
        if virt.as_u64() >= 0xFFFF_8000_0000_0000 {
            return Err(AddressSpaceError::KernelAddressViolation);
        }

        let user_flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER
            | flags;

        unsafe {
            self.walk_or_create_and_map(virt, phys, user_flags)?;
        }

        self.user_page_count += 1;
        Ok(())
    }

    /// Unmap a user-space page and return its physical frame.
    pub fn unmap_user_page(&mut self, virt: VirtAddr) -> Result<PhysAddr, AddressSpaceError> {
        if virt.as_u64() >= 0xFFFF_8000_0000_0000 {
            return Err(AddressSpaceError::KernelAddressViolation);
        }

        unsafe {
            let entry = self.walk_to_entry(virt)
                .ok_or(AddressSpaceError::NotMapped)?;
            if !entry.is_present() {
                return Err(AddressSpaceError::NotMapped);
            }
            let phys = entry.addr();
            entry.clear();
            self.user_page_count = self.user_page_count.saturating_sub(1);
            Ok(phys)
        }
    }

    /// Destroy this address space, freeing all allocated frames.
    /// Frees user page table frames (PDPT, PD, PT levels) and the PML4 itself.
    /// Does NOT free the user data frames — caller must handle that.
    pub fn destroy(&mut self) {
        if !self.active {
            return;
        }

        // Walk the lower half and free intermediate page table frames
        let pml4 = unsafe { &*(self.pml4_virt.as_u64() as *const PageTable) };

        for i in 0..256 {
            if pml4.entries[i].is_present() {
                let pdpt_phys = pml4.entries[i].addr();
                let pdpt = unsafe { &*(pdpt_phys.to_virt().as_u64() as *const PageTable) };

                for j in 0..512 {
                    if pdpt.entries[j].is_present() {
                        let pd_phys = pdpt.entries[j].addr();
                        let pd = unsafe { &*(pd_phys.to_virt().as_u64() as *const PageTable) };

                        for k in 0..512 {
                            if pd.entries[k].is_present() {
                                // Free PT frame
                                let pt_phys = pd.entries[k].addr();
                                frame::deallocate_frame(pt_phys);
                            }
                        }
                        // Free PD frame
                        frame::deallocate_frame(pd_phys);
                    }
                }
                // Free PDPT frame
                frame::deallocate_frame(pdpt_phys);
            }
        }

        // Free the PML4 frame itself
        frame::deallocate_frame(self.pml4_phys);
        self.active = false;
    }

    /// Get the physical address of this PML4 (for CR3 loading in Phase 7).
    pub fn cr3(&self) -> PhysAddr {
        self.pml4_phys
    }

    /// Number of user pages mapped.
    pub fn user_page_count(&self) -> usize {
        self.user_page_count
    }

    /// Whether this address space is still valid.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Verify kernel mappings integrity: entries 256-511 match the kernel PML4.
    pub fn verify_kernel_mappings(&self) -> bool {
        let kernel_pml4_phys = Self::read_cr3();
        let kernel_pml4 = unsafe {
            &*(kernel_pml4_phys.to_virt().as_u64() as *const PageTable)
        };
        let pml4 = unsafe {
            &*(self.pml4_virt.as_u64() as *const PageTable)
        };

        for i in 256..512 {
            // Compare raw entry values
            let ours = unsafe { *(&pml4.entries[i] as *const PageTableEntry as *const u64) };
            let theirs = unsafe { *(&kernel_pml4.entries[i] as *const PageTableEntry as *const u64) };
            if ours != theirs {
                return false;
            }
        }
        true
    }

    // --- Internal helpers ---

    /// Read the current kernel CR3.
    fn read_cr3() -> PhysAddr {
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3);
        }
        PhysAddr(cr3 & 0x000F_FFFF_FFFF_F000)
    }

    /// Walk page table levels for a virtual address, creating intermediate tables.
    unsafe fn walk_or_create_and_map(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<(), AddressSpaceError> {
        use crate::memory::page_table::{pml4_index, pdpt_index, pd_index, pt_index};

        let pml4 = &mut *(self.pml4_virt.as_u64() as *mut PageTable);

        // PML4 -> PDPT
        let pdpt = Self::next_or_create(&mut pml4.entries[pml4_index(virt.as_u64())])?;

        // PDPT -> PD
        let pd = Self::next_or_create(&mut pdpt.entries[pdpt_index(virt.as_u64())])?;

        // PD -> PT
        let pt = Self::next_or_create(&mut pd.entries[pd_index(virt.as_u64())])?;

        // Set the final PT entry
        let entry = &mut pt.entries[pt_index(virt.as_u64())];
        if entry.is_present() {
            return Err(AddressSpaceError::AlreadyMapped);
        }
        entry.set(phys, flags);

        Ok(())
    }

    /// Walk page table to find a final entry (no creation).
    unsafe fn walk_to_entry(&self, virt: VirtAddr) -> Option<&'static mut PageTableEntry> {
        use crate::memory::page_table::{pml4_index, pdpt_index, pd_index, pt_index};

        let pml4 = &*(self.pml4_virt.as_u64() as *const PageTable);

        let pml4_e = &pml4.entries[pml4_index(virt.as_u64())];
        if !pml4_e.is_present() { return None; }

        let pdpt = &*(pml4_e.addr().to_virt().as_u64() as *const PageTable);
        let pdpt_e = &pdpt.entries[pdpt_index(virt.as_u64())];
        if !pdpt_e.is_present() { return None; }

        let pd = &*(pdpt_e.addr().to_virt().as_u64() as *const PageTable);
        let pd_e = &pd.entries[pd_index(virt.as_u64())];
        if !pd_e.is_present() { return None; }

        let pt = &mut *(pd_e.addr().to_virt().as_u64() as *mut PageTable);
        Some(&mut pt.entries[pt_index(virt.as_u64())])
    }

    /// Follow or create an intermediate page table.
    unsafe fn next_or_create(
        entry: &mut PageTableEntry,
    ) -> Result<&'static mut PageTable, AddressSpaceError> {
        if entry.is_present() {
            Ok(&mut *(entry.addr().to_virt().as_u64() as *mut PageTable))
        } else {
            let new_frame = frame::allocate_frame()
                .ok_or(AddressSpaceError::AllocationFailed)?;
            let table = &mut *(new_frame.to_virt().as_u64() as *mut PageTable);
            table.zero();
            // Intermediate tables: present + writable + user
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER;
            entry.set(new_frame, flags);
            Ok(table)
        }
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        if self.active {
            self.destroy();
        }
    }
}
