//! Butler state externalization — persistent supervision state across restarts.
//!
//! Butler (PID 1) supervision data is serialized to a dedicated physical page
//! so it persists across Butler restarts. The block uses a magic number and
//! CRC16 checksum for integrity verification.

#![allow(dead_code)]

use crate::memory::{PhysAddr, PAGE_SIZE};
use crate::memory::frame;
use crate::serial_println;
use crate::sync::OrderedMutex;

/// Maximum children tracked in the externalized state.
pub const MAX_CHILDREN: usize = 64;

/// Magic number for valid state block.
const BUTLER_STATE_MAGIC: u32 = 0xBE01_FA8C;

/// Current state block version.
const BUTLER_STATE_VERSION: u16 = 1;

/// The externalized Butler supervision state (fits in one 4K page).
#[repr(C)]
pub struct ButlerStateBlock {
    /// Magic number for validity check.
    pub magic: u32,
    /// Block version.
    pub version: u16,
    /// CRC16 checksum of payload (bytes 8..4096).
    pub checksum: u16,
    /// Number of active children.
    pub child_count: u16,
    /// Reserved padding.
    pub _pad: [u8; 6],
    /// Per-child restart counts.
    pub restart_counts: [u16; MAX_CHILDREN],
    /// Per-child supervision strategy (u8 enum discriminant).
    pub strategies: [u8; MAX_CHILDREN],
    /// Per-child last crash tick.
    pub last_crash_tick: [u64; MAX_CHILDREN],
    /// Whether break-glass was active at last checkpoint.
    pub break_glass_active: bool,
    /// Tick of last checkpoint.
    pub last_checkpoint_tick: u64,
}

impl ButlerStateBlock {
    /// Create a fresh (empty) state block.
    pub fn fresh() -> Self {
        Self {
            magic: BUTLER_STATE_MAGIC,
            version: BUTLER_STATE_VERSION,
            checksum: 0,
            child_count: 0,
            _pad: [0; 6],
            restart_counts: [0; MAX_CHILDREN],
            strategies: [0; MAX_CHILDREN],
            last_crash_tick: [0; MAX_CHILDREN],
            break_glass_active: false,
            last_checkpoint_tick: 0,
        }
    }

    /// Check if this block has a valid magic number.
    pub fn is_valid(&self) -> bool {
        self.magic == BUTLER_STATE_MAGIC && self.version == BUTLER_STATE_VERSION
    }

    /// Compute CRC16 of the payload (simple additive checksum).
    pub fn compute_checksum(&self) -> u16 {
        let bytes = unsafe {
            let ptr = self as *const Self as *const u8;
            // Checksum covers bytes 8 onward (skip magic+version+checksum)
            // Use struct size, not PAGE_SIZE, so checksum is valid on copies too
            core::slice::from_raw_parts(ptr.add(8), core::mem::size_of::<Self>() - 8)
        };
        let mut sum: u32 = 0;
        for &b in bytes {
            sum = sum.wrapping_add(b as u32);
        }
        (sum & 0xFFFF) as u16
    }

    /// Update the checksum field.
    pub fn update_checksum(&mut self) {
        self.checksum = self.compute_checksum();
    }

    /// Verify the checksum is correct.
    pub fn verify_checksum(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// Record a child restart.
    pub fn record_restart(&mut self, child_index: usize, tick: u64) {
        if child_index < MAX_CHILDREN {
            self.restart_counts[child_index] = self.restart_counts[child_index].saturating_add(1);
            self.last_crash_tick[child_index] = tick;
        }
    }

    /// Set the supervision strategy for a child.
    pub fn set_strategy(&mut self, child_index: usize, strategy: u8) {
        if child_index < MAX_CHILDREN {
            self.strategies[child_index] = strategy;
        }
    }

    /// Get total restarts for a child.
    pub fn total_restarts(&self, child_index: usize) -> u16 {
        if child_index < MAX_CHILDREN {
            self.restart_counts[child_index]
        } else {
            0
        }
    }
}

/// Global Butler state manager.
pub struct ButlerStateManager {
    /// Physical address of the dedicated state page.
    state_phys: Option<PhysAddr>,
    /// Whether the state has been initialized.
    initialized: bool,
}

impl ButlerStateManager {
    pub const fn new() -> Self {
        Self {
            state_phys: None,
            initialized: false,
        }
    }

    /// Allocate a dedicated physical page and initialize the state block.
    pub fn init(&mut self) -> bool {
        if let Some(phys) = frame::allocate_frame() {
            // Zero the page
            let virt = phys.to_virt();
            unsafe {
                let ptr = virt.as_u64() as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }

            // Write fresh state
            let block = unsafe { &mut *(virt.as_u64() as *mut ButlerStateBlock) };
            *block = ButlerStateBlock::fresh();
            block.update_checksum();

            self.state_phys = Some(phys);
            self.initialized = true;
            true
        } else {
            false
        }
    }

    /// Load the state block (returns a copy for reading).
    pub fn load(&self) -> Option<ButlerStateBlock> {
        let phys = self.state_phys?;
        let virt = phys.to_virt();
        let block = unsafe { &*(virt.as_u64() as *const ButlerStateBlock) };

        if block.is_valid() && block.verify_checksum() {
            // Safe to copy since ButlerStateBlock is all plain data
            Some(unsafe { core::ptr::read(block) })
        } else {
            None
        }
    }

    /// Save the state block (writes to the dedicated page).
    pub fn save(&self, block: &mut ButlerStateBlock) -> bool {
        if let Some(phys) = self.state_phys {
            block.update_checksum();
            let virt = phys.to_virt();
            unsafe {
                let dst = virt.as_u64() as *mut ButlerStateBlock;
                core::ptr::write(dst, core::ptr::read(block));
            }
            true
        } else {
            false
        }
    }

    /// Whether the manager is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// Global Butler state manager instance.
pub static BUTLER_STATE: OrderedMutex<ButlerStateManager, { crate::sync::levels::TABLE }> =
    OrderedMutex::new(ButlerStateManager::new());

/// Initialize the Butler state subsystem.
pub fn init() {
    let mut mgr = BUTLER_STATE.lock();
    if mgr.init() {
        serial_println!("[BUTLER] State externalization initialized (1 dedicated page)");
    } else {
        serial_println!("[BUTLER] WARNING: Could not allocate state page");
    }
}
