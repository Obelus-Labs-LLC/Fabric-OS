//! Guest VM lifecycle management.
//!
//! Ties together VMCS, EPT, and the execution engine into VirtualMachine.
//! Provides create, load_code, run, and destroy. Manages guest physical
//! memory allocation via the buddy allocator.

#![allow(dead_code)]

use spin::Mutex;
use alloc::vec::Vec;
use crate::memory::{PhysAddr, PAGE_SIZE, hhdm_offset};
use crate::memory::frame;
use super::vmcs::SoftVmcs;
use super::ept::{EptContext, EptFlags, EptError};
use super::emulate;
use super::vmexit::{VmExitReason, VmExitInfo};

/// Virtual machine identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmId(pub u32);

/// Virtual machine state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmState {
    Created,
    Running,
    Halted,
    Destroyed,
}

/// A virtual machine instance.
pub struct VirtualMachine {
    pub id: VmId,
    pub vmcs: SoftVmcs,
    pub ept: EptContext,
    pub state: VmState,
    pub memory_pages: usize,
    pub host_frames: Vec<PhysAddr>,
    pub exit_count: u64,
    /// Last I/O exit data (port, data byte) for testing
    pub last_io_port: u16,
    pub last_io_data: u8,
}

/// VM operation errors.
#[derive(Debug)]
pub enum VmError {
    AllocationFailed,
    TableFull,
    InvalidId,
    InvalidState,
    EptError,
    LoadFailed,
}

/// Maximum concurrent VMs.
pub const MAX_VMS: usize = 4;

/// VM table: fixed array of VM slots.
pub struct VmTable {
    vms: [Option<VirtualMachine>; MAX_VMS],
    next_id: u32,
}

impl VmTable {
    pub const fn new() -> Self {
        Self {
            vms: [const { None }; MAX_VMS],
            next_id: 1,
        }
    }

    /// Create a new VM with the specified number of guest memory pages.
    pub fn create(&mut self, memory_pages: usize) -> Result<VmId, VmError> {
        // Find a free slot
        let slot = self.vms.iter().position(|s| s.is_none())
            .ok_or(VmError::TableFull)?;

        // Allocate guest physical memory frames
        let mut host_frames = Vec::new();
        for _ in 0..memory_pages {
            let frame = frame::allocate_frame().ok_or(VmError::AllocationFailed)?;
            // Zero the frame
            let virt = frame.0 + hhdm_offset();
            unsafe { core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE); }
            host_frames.push(frame);
        }

        // Create EPT and map guest pages
        let mut ept = EptContext::create().map_err(|_| VmError::EptError)?;
        for (i, &host_phys) in host_frames.iter().enumerate() {
            let guest_phys = (i * PAGE_SIZE) as u64;
            ept.map_page(guest_phys, host_phys, EptFlags::RWX_WB)
                .map_err(|_| VmError::EptError)?;
        }

        let id = VmId(self.next_id);
        self.next_id += 1;

        let mut vmcs = SoftVmcs::new();
        vmcs.eptp = ept.eptp();

        self.vms[slot] = Some(VirtualMachine {
            id,
            vmcs,
            ept,
            state: VmState::Created,
            memory_pages,
            host_frames,
            exit_count: 0,
            last_io_port: 0,
            last_io_data: 0,
        });

        Ok(id)
    }

    pub fn get(&self, id: VmId) -> Option<&VirtualMachine> {
        self.vms.iter().find_map(|s| {
            s.as_ref().filter(|vm| vm.id == id)
        })
    }

    pub fn get_mut(&mut self, id: VmId) -> Option<&mut VirtualMachine> {
        self.vms.iter_mut().find_map(|s| {
            s.as_mut().filter(|vm| vm.id == id)
        })
    }

    /// Destroy a VM, freeing all resources.
    pub fn destroy(&mut self, id: VmId) -> bool {
        for slot in self.vms.iter_mut() {
            if let Some(vm) = slot {
                if vm.id == id {
                    // Free guest data frames
                    for phys in vm.host_frames.drain(..) {
                        frame::deallocate_frame(phys);
                    }
                    // Free EPT table frames
                    vm.ept.destroy();
                    vm.state = VmState::Destroyed;
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// Count active VMs.
    pub fn count(&self) -> usize {
        self.vms.iter().filter(|s| s.is_some()).count()
    }
}

/// Global VM table.
pub static VM_TABLE: Mutex<VmTable> = Mutex::new(VmTable::new());

impl VirtualMachine {
    /// Load flat binary code into guest physical address 0.
    pub fn load_code(&mut self, code: &[u8]) -> Result<(), VmError> {
        if code.is_empty() {
            return Err(VmError::LoadFailed);
        }

        // Calculate how many pages the code spans
        let pages_needed = (code.len() + PAGE_SIZE - 1) / PAGE_SIZE;
        if pages_needed > self.memory_pages {
            return Err(VmError::LoadFailed);
        }

        // Copy code into guest physical memory via HHDM
        let mut offset = 0;
        for (page_idx, &host_phys) in self.host_frames.iter().enumerate() {
            if page_idx >= pages_needed {
                break;
            }
            let virt = host_phys.0 + hhdm_offset();
            let remaining = code.len() - offset;
            let to_copy = core::cmp::min(remaining, PAGE_SIZE);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    code[offset..].as_ptr(),
                    virt as *mut u8,
                    to_copy,
                );
            }
            offset += to_copy;
        }

        // Set guest RIP to 0 (start of loaded code)
        self.vmcs.guest.rip = 0;
        self.state = VmState::Created;
        Ok(())
    }

    /// Run the guest until it halts or exceeds step limit.
    pub fn run(&mut self, max_steps: u64) -> VmExitInfo {
        self.state = VmState::Running;
        self.exit_count = 0;

        let exit_info = emulate::run_guest(&mut self.vmcs, &self.ept, max_steps);

        // Track exit count (each non-Continue step counts)
        self.exit_count = 1;

        // Capture I/O data if it was an I/O exit
        if exit_info.reason == VmExitReason::IoInstruction {
            self.last_io_port = (exit_info.qualification >> 16) as u16;
            // Data was in AL at time of OUT
        }

        match exit_info.reason {
            VmExitReason::Hlt => self.state = VmState::Halted,
            _ => {}
        }

        exit_info
    }

    /// Read a value from guest physical memory.
    pub fn read_guest_phys_u8(&self, guest_addr: u64) -> Option<u8> {
        let host_phys = self.ept.translate(guest_addr)?;
        let host_virt = host_phys.0 + hhdm_offset();
        Some(unsafe { *(host_virt as *const u8) })
    }

    /// Read a u32 from guest physical memory (little-endian).
    pub fn read_guest_phys_u32(&self, guest_addr: u64) -> Option<u32> {
        let b0 = self.read_guest_phys_u8(guest_addr)? as u32;
        let b1 = self.read_guest_phys_u8(guest_addr + 1)? as u32;
        let b2 = self.read_guest_phys_u8(guest_addr + 2)? as u32;
        let b3 = self.read_guest_phys_u8(guest_addr + 3)? as u32;
        Some(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    /// Write a byte to guest physical memory.
    pub fn write_guest_phys_u8(&self, guest_addr: u64, value: u8) -> bool {
        emulate::write_guest_byte(&self.ept, guest_addr, value)
    }

    /// Write a u32 to guest physical memory (little-endian).
    pub fn write_guest_phys_u32(&self, guest_addr: u64, value: u32) -> bool {
        self.write_guest_phys_u8(guest_addr, value as u8)
            && self.write_guest_phys_u8(guest_addr + 1, (value >> 8) as u8)
            && self.write_guest_phys_u8(guest_addr + 2, (value >> 16) as u8)
            && self.write_guest_phys_u8(guest_addr + 3, (value >> 24) as u8)
    }
}
