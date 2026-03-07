# FabricOS Roadmap v5.0

## Overview
AI-coordinated microkernel with capability-based security.

## Completed Phases

| Phase | Component | Status | Description |
|:---|:---|:---|:---|
| 0-5B | Microkernel Core | ✅ Complete | Memory, IPC, Capabilities, Scheduler, Interrupts |
| 6-10 | Boot & HAL | ✅ Complete | UEFI boot, ACPI, PCI, virtio |
| 11-14 | Network Stack | ✅ Complete | Ethernet, TCP/IP, DNS, sockets |
| 15 | TLS 1.3 | ✅ Complete | Secure connections, certificates |
| 16 | Window Manager | ✅ Complete | Overlapping windows, z-ordering, taskbar |
| L13.5 | V8 Platform Interface | ✅ Complete | `#[no_std]` platform layer (D2-D7) |
| 21a | USB XHCI Bringup | ✅ Complete | Host controller initialization |
| TD-021 | TRB Cycle Bit Fix | ✅ Complete | Hardware race condition fix |
| 21b | USB Root Hub | ✅ Complete | Root hub emulation, hub device interface |
| TD-008 | BTreeMap → Fixed Slab | ✅ Complete | Alloc-free FixedMap in capability store |
| BUGFIX | Heap Address Collision | ✅ Complete | Fixed QEMU 8.2.2 + OVMF heap mapping conflict |
| TD-003 | Lock Ordering Enforcement | ✅ Complete | OrderedMutex with debug-only ordering checks (643cf47) |
| TD-010 | Buddy Allocator Safety | ✅ Complete | FreeBlockPtr typed wrapper, canary, bounded traversal |

## Immediate Sprint (This Week)

| Phase | Component | Status | Description |
|:---|:---|:---|:---|
| 21c | USB Device Enumeration | 📋 Planned | HID keyboard, control transfers (hardware blocked) |
| TD-022 | HID Boot Protocol Test | 📋 Planned | 8-byte descriptor validation (needs Dell hardware) |

## Short-Term (Next 4 Weeks)

| Phase | Component | Status | Description |
|:---|:---|:---|:---|
| 22 | NVMe Driver | 📋 Planned | SSD driver with AHCI fallback |
| 23 | GPU Modesetting | 📋 Planned | Intel i915 framebuffer |
| L13.6 | V8 Cross-Compile | 🚧 In Progress | Build V8 for x86_64-unknown-none |
| L13.7 | V8 Link Test | 🚧 In Progress | Verify V8 + platform interface |

## Medium-Term (Months 2-3)

| Phase | Component | Status | Description |
|:---|:---|:---|:---|
| 24 | Intel WiFi | 📋 Planned | iwlwifi driver, firmware management |
| 25 | AI Marketplace | 📋 Planned | Third-party agent SDK |
| 26 | Servo Decision | 📋 Planned | Final engine architecture choice |
| TD-001 | Real ML Models | 📋 Planned | Replace XOR gradient placeholder |
| TD-005 | Production Models | 📋 Planned | Functional Council Tier 2/3 |
| TD-012 | FabricFS | 📋 Planned | Persistent content-addressable storage |

## Long-Term (Months 4-6)

| Phase | Component | Status | Description |
|:---|:---|:---|:---|
| 27 | ARM64/RISC-V Ports | 📋 Planned | Hardware diversity |
| 28 | Enterprise Features | 📋 Planned | Fleet management |
| 29 | Formal Verification | 📋 Planned | Kani/seL4-level proofs |
| 30 | AI Council Training | 📋 Planned | Weight hash verification |
| 31 | Mesh Networking | 📋 Planned | Chauffeur P2P GA |
| 32 | Security Certification | 📋 Planned | Common Criteria |

## New Features (Post-Debt)

| Phase | Component | Description |
|:---|:---|:---|
| 33 | Kernel Live Patching | Update drivers without reboot |
| 34 | eBPF-like Tracing | Safe kernel instrumentation |
| 35 | Confidential Computing | AMD SEV/Intel TDX |
| 36 | Quantum-Safe Crypto | CRYSTALS default |

---

*Last updated: 2026-03-06*
*Version: 5.0*
