#![allow(dead_code)]

use crate::memory::{PhysAddr, VirtAddr};
use crate::memory::page_table::*;
use crate::memory::frame;

#[derive(Debug)]
pub enum MapError {
    FrameAllocationFailed,
    AlreadyMapped,
}

#[derive(Debug)]
pub enum UnmapError {
    NotMapped,
}

/// Read the PML4 physical address from CR3
fn read_cr3() -> PhysAddr {
    let cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3);
    }
    PhysAddr(cr3 & 0x000F_FFFF_FFFF_F000)
}

/// Flush a single TLB entry
unsafe fn flush_tlb(addr: VirtAddr) {
    core::arch::asm!("invlpg [{}]", in(reg) addr.0, options(nostack, preserves_flags));
}

/// Get a mutable reference to a page table at a physical address via HHDM
unsafe fn table_at(phys: PhysAddr) -> &'static mut PageTable {
    let virt = phys.to_virt();
    &mut *(virt.as_u64() as *mut PageTable)
}

/// Walk or create page table levels, returning the final PT entry
unsafe fn walk_or_create(
    virt: VirtAddr,
    create: bool,
) -> Result<&'static mut PageTableEntry, MapError> {
    let pml4_phys = read_cr3();
    let pml4 = table_at(pml4_phys);

    // Level 4 -> Level 3 (PML4 -> PDPT)
    let pdpt = next_table_or_create(&mut pml4.entries[pml4_index(virt.0)], create)?;

    // Level 3 -> Level 2 (PDPT -> PD)
    let pd = next_table_or_create(&mut pdpt.entries[pdpt_index(virt.0)], create)?;

    // Level 2 -> Level 1 (PD -> PT)
    let pt = next_table_or_create(&mut pd.entries[pd_index(virt.0)], create)?;

    // Return the PT entry
    Ok(&mut pt.entries[pt_index(virt.0)])
}

/// Follow or create the next level table
unsafe fn next_table_or_create(
    entry: &mut PageTableEntry,
    create: bool,
) -> Result<&'static mut PageTable, MapError> {
    if entry.is_present() {
        Ok(table_at(entry.addr()))
    } else if create {
        // Allocate a new frame for the page table
        let frame_addr = frame::allocate_frame().ok_or(MapError::FrameAllocationFailed)?;
        let table = table_at(frame_addr);
        table.zero();

        // Set the entry: present + writable (intermediate tables need both)
        entry.set(frame_addr, PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
        Ok(table)
    } else {
        Err(MapError::FrameAllocationFailed)
    }
}

/// Map a virtual page to a physical frame with given flags
pub fn map(
    virt: VirtAddr,
    phys: PhysAddr,
    flags: PageTableFlags,
) -> Result<(), MapError> {
    unsafe {
        let entry = walk_or_create(virt, true)?;
        if entry.is_present() {
            return Err(MapError::AlreadyMapped);
        }
        entry.set(phys, flags | PageTableFlags::PRESENT);
        flush_tlb(virt);
    }
    Ok(())
}

/// Unmap a virtual page, returning the physical frame it pointed to
pub fn unmap(virt: VirtAddr) -> Result<PhysAddr, UnmapError> {
    unsafe {
        let entry = walk_or_create(virt, false).map_err(|_| UnmapError::NotMapped)?;
        if !entry.is_present() {
            return Err(UnmapError::NotMapped);
        }
        let phys = entry.addr();
        entry.clear();
        flush_tlb(virt);
        Ok(phys)
    }
}

/// Translate a virtual address to its physical address
pub fn translate(virt: VirtAddr) -> Option<PhysAddr> {
    unsafe {
        let entry = walk_or_create(virt, false).ok()?;
        if !entry.is_present() {
            return None;
        }
        let page_phys = entry.addr();
        let offset = virt.0 & 0xFFF;
        Some(PhysAddr(page_phys.0 + offset))
    }
}
