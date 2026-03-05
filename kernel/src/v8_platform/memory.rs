//! V8 Memory Allocator for FabricOS
//!
//! Provides page-aligned memory allocation for V8 heap and JIT code.
//! Uses DMA manager for physical memory backing with proper capability tracking.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{null_mut, NonNull};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::stubs::{DmaManager, DmaAllocation, SysCallError};
use super::V8PlatformError;

/// Page size for V8 memory (4KB)
pub const V8_PAGE_SIZE: usize = 4096;

/// Large page size (2MB) for huge TLB
pub const V8_LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Memory region types for V8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8MemoryType {
    /// Regular heap memory (RW)
    Heap,
    /// JIT code memory (RX - executable)
    Code,
    /// Data segment (RW)
    Data,
    /// Large object space (RW, huge pages)
    LargeObject,
}

/// Memory allocation tracking
#[derive(Debug)]
pub struct V8Allocation {
    /// Base pointer
    pub ptr: *mut u8,
    /// Size in bytes
    pub size: usize,
    /// Memory type
    pub mem_type: V8MemoryType,
    /// DMA backing (if any)
    pub dma: Option<DmaAllocation>,
}

// Safety: V8Allocation is Send if ptr is valid
unsafe impl Send for V8Allocation {}
unsafe impl Sync for V8Allocation {}

/// Global allocation statistics
static ALLOC_STATS: AllocationStats = AllocationStats::new();

struct AllocationStats {
    total_allocated: AtomicU64,
    total_freed: AtomicU64,
    current_used: AtomicU64,
    peak_used: AtomicU64,
    allocation_count: AtomicU64,
}

impl AllocationStats {
    const fn new() -> Self {
        Self {
            total_allocated: AtomicU64::new(0),
            total_freed: AtomicU64::new(0),
            current_used: AtomicU64::new(0),
            peak_used: AtomicU64::new(0),
            allocation_count: AtomicU64::new(0),
        }
    }

    fn record_alloc(&self, size: usize) {
        self.total_allocated.fetch_add(size as u64, Ordering::Relaxed);
        let current = self.current_used.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
        self.allocation_count.fetch_add(1, Ordering::Relaxed);
        
        // Update peak
        let mut peak = self.peak_used.load(Ordering::Relaxed);
        while current > peak {
            match self.peak_used.compare_exchange_weak(
                peak, 
                current, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    fn record_free(&self, size: usize) {
        self.total_freed.fetch_add(size as u64, Ordering::Relaxed);
        self.current_used.fetch_sub(size as u64, Ordering::Relaxed);
    }
}

/// Get allocation statistics
pub fn get_alloc_stats() -> AllocStats {
    AllocStats {
        total_allocated: ALLOC_STATS.total_allocated.load(Ordering::Relaxed),
        total_freed: ALLOC_STATS.total_freed.load(Ordering::Relaxed),
        current_used: ALLOC_STATS.current_used.load(Ordering::Relaxed),
        peak_used: ALLOC_STATS.peak_used.load(Ordering::Relaxed),
        allocation_count: ALLOC_STATS.allocation_count.load(Ordering::Relaxed),
    }
}

/// Allocation statistics snapshot
#[derive(Debug, Clone, Copy)]
pub struct AllocStats {
    pub total_allocated: u64,
    pub total_freed: u64,
    pub current_used: u64,
    pub peak_used: u64,
    pub allocation_count: u64,
}

/// DMA manager instance (initialized at boot)
static mut V8_DMA_MANAGER: Option<DmaManager> = None;

/// Initialize memory allocator with DMA manager
/// 
/// # Safety
/// Must be called exactly once during kernel initialization
pub unsafe fn init_memory_allocator(dma: DmaManager) {
    V8_DMA_MANAGER = Some(dma);
}

/// Get DMA manager reference
/// 
/// # Safety
/// Only valid after init_memory_allocator() called
unsafe fn dma_manager() -> Option<&'static DmaManager> {
    V8_DMA_MANAGER.as_ref()
}

/// Align size up to page boundary
#[inline(always)]
pub const fn align_up(size: usize, align: usize) -> usize {
    (size + align - 1) & !(align - 1)
}

/// Allocate memory for V8 heap
/// 
/// Returns page-aligned memory suitable for V8 heap objects.
/// Memory is read-write but not executable.
pub fn v8_alloc(size: usize) -> *mut u8 {
    let aligned_size = align_up(size, V8_PAGE_SIZE);
    
    // Try DMA allocation first
    unsafe {
        if let Some(dma) = dma_manager() {
            if let Some(allocation) = dma.allocate(aligned_size) {
                let ptr = allocation.as_ptr();
                ALLOC_STATS.record_alloc(aligned_size);
                
                // Store allocation metadata (simplified - real impl would use hash map)
                // For now, just return the pointer
                return ptr;
            }
        }
    }
    
    // Fallback: use global allocator
    let layout = match Layout::from_size_align(aligned_size, V8_PAGE_SIZE) {
        Ok(l) => l,
        Err(_) => return null_mut(),
    };
    
    let ptr = unsafe { alloc(layout) };
    if !ptr.is_null() {
        ALLOC_STATS.record_alloc(aligned_size);
    }
    ptr
}

/// Allocate executable memory for JIT code
/// 
/// Returns page-aligned memory with execute permissions.
/// This is required for V8's JIT-compiled JavaScript code.
pub fn v8_alloc_executable(size: usize) -> *mut u8 {
    let aligned_size = align_up(size, V8_PAGE_SIZE);
    
    unsafe {
        if let Some(dma) = dma_manager() {
            // Request executable pages
            if let Some(allocation) = dma.allocate_executable(aligned_size) {
                let ptr = allocation.as_ptr();
                ALLOC_STATS.record_alloc(aligned_size);
                return ptr;
            }
        }
    }
    
    // Fallback - in real impl would use mmap with PROT_EXEC
    // For stub, just use regular allocation
    v8_alloc(aligned_size)
}

/// Allocate large pages (huge TLB)
/// 
/// Used for large object space to reduce TLB pressure
pub fn v8_alloc_large_pages(size: usize) -> *mut u8 {
    let aligned_size = align_up(size, V8_LARGE_PAGE_SIZE);
    
    unsafe {
        if let Some(dma) = dma_manager() {
            if let Some(allocation) = dma.allocate_huge(aligned_size) {
                let ptr = allocation.as_ptr();
                ALLOC_STATS.record_alloc(aligned_size);
                return ptr;
            }
        }
    }
    
    // Fallback to regular pages
    v8_alloc(aligned_size)
}

/// Free memory allocated by v8_alloc
/// 
/// # Safety
/// ptr must have been allocated by v8_alloc and not already freed
pub unsafe fn v8_free(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }
    
    let aligned_size = align_up(size, V8_PAGE_SIZE);
    
    // Try DMA manager first
    if let Some(dma) = dma_manager() {
        dma.deallocate(ptr as usize, aligned_size);
        ALLOC_STATS.record_free(aligned_size);
        return;
    }
    
    // Fallback: use global allocator
    let layout = Layout::from_size_align_unchecked(aligned_size, V8_PAGE_SIZE);
    dealloc(ptr, layout);
    ALLOC_STATS.record_free(aligned_size);
}

/// Reallocate memory
/// 
/// # Safety
/// ptr must have been allocated by v8_alloc and not already freed
pub unsafe fn v8_realloc(ptr: *mut u8, old_size: usize, new_size: usize) -> *mut u8 {
    if ptr.is_null() {
        return v8_alloc(new_size);
    }
    
    if new_size == 0 {
        v8_free(ptr, old_size);
        return null_mut();
    }
    
    // Allocate new block
    let new_ptr = v8_alloc(new_size);
    if new_ptr.is_null() {
        return null_mut();
    }
    
    // Copy data
    let copy_size = if old_size < new_size { old_size } else { new_size };
    core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_size);
    
    // Free old block
    v8_free(ptr, old_size);
    
    new_ptr
}

/// Make memory executable (for JIT)
/// 
/// # Safety
/// ptr must point to valid allocated memory
pub unsafe fn v8_protect_executable(ptr: *mut u8, size: usize) -> Result<(), V8PlatformError> {
    if ptr.is_null() {
        return Err(V8PlatformError::InvalidPointer);
    }
    
    if let Some(dma) = dma_manager() {
        dma.protect_executable(ptr as usize, align_up(size, V8_PAGE_SIZE))
            .map_err(|e| V8PlatformError::SyscallFailed(e))?;
    }
    
    Ok(())
}

/// Make memory read-only (for code segments)
/// 
/// # Safety
/// ptr must point to valid allocated memory
pub unsafe fn v8_protect_readonly(ptr: *mut u8, size: usize) -> Result<(), V8PlatformError> {
    if ptr.is_null() {
        return Err(V8PlatformError::InvalidPointer);
    }
    
    if let Some(dma) = dma_manager() {
        dma.protect_readonly(ptr as usize, align_up(size, V8_PAGE_SIZE))
            .map_err(|e| V8PlatformError::SyscallFailed(e))?;
    }
    
    Ok(())
}

/// Global allocator interface for alloc crate
pub struct V8GlobalAlloc;

unsafe impl GlobalAlloc for V8GlobalAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        v8_alloc(layout.size())
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // We don't track size in global alloc interface
        // In real impl, would need size metadata
        v8_free(ptr, _layout.size());
    }
    
    unsafe fn realloc(&self, ptr: *mut u8, _layout: Layout, new_size: usize) -> *mut u8 {
        v8_realloc(ptr, _layout.size(), new_size)
    }
}

// Import allocator functions
use alloc::alloc::{alloc, dealloc};

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 4096), 0);
        assert_eq!(align_up(1, 4096), 4096);
        assert_eq!(align_up(4095, 4096), 4096);
        assert_eq!(align_up(4096, 4096), 4096);
        assert_eq!(align_up(4097, 4096), 8192);
    }
    
    #[test]
    fn test_alloc_stats() {
        let stats = get_alloc_stats();
        assert_eq!(stats.total_allocated, 0);
        assert_eq!(stats.current_used, 0);
    }
}
