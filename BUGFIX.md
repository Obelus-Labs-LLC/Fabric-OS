# FabricOS Bug Fixes

This document records significant bug fixes and their resolutions.

## 2026-03-06: QEMU 8.2.2 + OVMF Heap Address Collision

### Symptom
Kernel panic during boot:
```
heap: map failed: AlreadyMapped
```

### Environment
- **Hardware**: Dell Inspiron 5558 (build slave)
- **QEMU**: 8.2.2
- **Firmware**: OVMF (UEFI)
- **Host OS**: Ubuntu 24.04

### Root Cause
`HEAP_START` at `0xFFFF_FFFF_8040_0000` (64MB offset from kernel base) collided with either:
- Kernel image mapping in higher half
- UEFI/OVMF reserved memory regions
- QEMU-specific memory layout quirks

The collision only manifested in this specific QEMU + OVMF combination. Other test environments ( bare metal, different QEMU versions) worked correctly.

### Fix
Changed `HEAP_START` in `kernel/src/memory/heap.rs`:

```rust
// Before (64MB offset - collided)
pub const HEAP_START: u64 = 0xFFFF_FFFF_8040_0000;

// After (160MB offset - safe)
pub const HEAP_START: u64 = 0xFFFF_FFFF_A000_0000;
```

### Verification
- [x] Kernel boots fully through Phase 0 initialization
- [x] Phase 1 stress gates pass with SRI 100/100
- [x] Heap allocation works correctly
- [x] No regression in other test environments

### Prevention
Consider making heap start address configurable at build time or detect collisions at runtime with a clearer error message.

### References
- Commit: `8de8e5d`
- File: `kernel/src/memory/heap.rs`
