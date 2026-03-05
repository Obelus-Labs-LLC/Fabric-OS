//! STRESS Phase 19 Gate — Driver Framework Tests
//!
//! 10 tests verifying MMIO/PIO region abstractions, DMA buffer management,
//! IRQ routing with shared handlers, PCI device matching, and driver
//! resource bundling.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::ocrb::StressResult;
use crate::hal::driver_sdk::{MmioRegion, PioRegion, DmaBuffer, DriverResources};
use crate::hal::dma::DmaManager;
use crate::hal::irq_router::{IrqRouter, IrqHandler, MAX_SHARED, IRQ_VECTOR_BASE};
use crate::hal::pci_bind::{PciDeviceId, PciDriverTable, PciBdf};
use crate::pci::PciDevice;

pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();
    results.push(test_mmio_region());
    results.push(test_pio_region());
    results.push(test_dma_alloc_free());
    results.push(test_dma_process_cleanup());
    results.push(test_irq_register_dispatch());
    results.push(test_irq_shared_handlers());
    results.push(test_irq_overflow());
    results.push(test_pci_device_id_matching());
    results.push(test_pci_driver_table());
    results.push(test_driver_resources_bundle());
    results
}

/// Test 1: MMIO region bounds checking (w:10)
fn test_mmio_region() -> StressResult {
    // Create a small MMIO region — we test bounds logic only,
    // not actual hardware access (no real MMIO in STRESS tests)
    let region = MmioRegion::new(0x1000_0000, 256);

    // Bounds checking
    let within_8 = region.check_bounds(0, 1);
    let within_32 = region.check_bounds(252, 4);
    let oob_8 = !region.check_bounds(256, 1);
    let oob_32 = !region.check_bounds(253, 4);
    let zero_ok = region.check_bounds(0, 0);

    let all_ok = within_8 && within_32 && oob_8 && oob_32 && zero_ok;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "MMIO region bounds checking",
        passed,
        score,
        weight: 10,
        details: format!(
            "within_8={} within_32={} oob_8={} oob_32={} zero={}",
            within_8, within_32, oob_8, oob_32, zero_ok
        ),
    }
}

/// Test 2: PIO region port range validation (w:10)
fn test_pio_region() -> StressResult {
    let region = PioRegion::new(0xC000, 32);

    // Bounds checking
    let within_8 = region.check_bounds(0, 1);
    let within_16 = region.check_bounds(30, 2);
    let within_32 = region.check_bounds(28, 4);
    let oob_8 = !region.check_bounds(32, 1);
    let oob_32 = !region.check_bounds(29, 4);

    let all_ok = within_8 && within_16 && within_32 && oob_8 && oob_32;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "PIO region port range validation",
        passed,
        score,
        weight: 10,
        details: format!(
            "within_8={} within_16={} within_32={} oob_8={} oob_32={}",
            within_8, within_16, within_32, oob_8, oob_32
        ),
    }
}

/// Test 3: DMA alloc/free roundtrip (w:10)
fn test_dma_alloc_free() -> StressResult {
    let mut mgr = DmaManager::new();

    // Allocate a 4K buffer (order 0)
    let buf = mgr.alloc(4096, 1);
    let alloc_ok = buf.is_some();
    let count_after_alloc = mgr.active_count();

    let mut phys_aligned = false;
    let mut size_ok = false;
    let mut phys_val = 0usize;

    if let Some(b) = buf {
        phys_val = b.phys;
        phys_aligned = b.phys % 4096 == 0;
        size_ok = b.size >= 4096;

        // Free it
        let freed = mgr.free(b.phys);
        let count_after_free = mgr.active_count();

        let all_ok = alloc_ok && phys_aligned && size_ok && freed
            && count_after_alloc == 1 && count_after_free == 0;

        let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

        StressResult {
            test_name: "DMA alloc/free roundtrip",
            passed,
            score,
            weight: 10,
            details: format!(
                "alloc={} phys=0x{:x} aligned={} size={} freed={}",
                alloc_ok, phys_val, phys_aligned, size_ok, freed
            ),
        }
    } else {
        StressResult {
            test_name: "DMA alloc/free roundtrip",
            passed: false,
            score: 0,
            weight: 10,
            details: format!("allocation failed"),
        }
    }
}

/// Test 4: DMA per-process cleanup (w:10)
fn test_dma_process_cleanup() -> StressResult {
    let mut mgr = DmaManager::new();
    let pid: u32 = 5;

    // Allocate 3 buffers for process 5
    let b1 = mgr.alloc(4096, pid);
    let b2 = mgr.alloc(8192, pid);
    let b3 = mgr.alloc(4096, pid);

    let alloc_ok = b1.is_some() && b2.is_some() && b3.is_some();
    let count_before = mgr.active_count();

    // Also allocate one for a different process
    let b4 = mgr.alloc(4096, 10);
    let other_ok = b4.is_some();

    // Free all for process 5
    let freed = mgr.free_all_for_process(pid);
    let count_after = mgr.active_count();

    // Process 10's buffer should still be there
    let all_ok = alloc_ok && other_ok && freed == 3
        && count_before == 3 && count_after == 1;

    // Clean up remaining
    if let Some(b) = b4 {
        mgr.free(b.phys);
    }

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "DMA per-process cleanup",
        passed,
        score,
        weight: 10,
        details: format!(
            "allocated=3 freed={} remaining={} (expected 1)",
            freed, count_after
        ),
    }
}

/// Test 5: IRQ router register/dispatch (w:10)
fn test_irq_register_dispatch() -> StressResult {
    let mut router = IrqRouter::new();

    let handler = IrqHandler {
        driver_name: "test-driver",
        resource_id: 100,
        active: true,
    };

    // Register on vector 35
    let reg_ok = router.register(35, handler).is_ok();
    let count = router.handler_count(35);

    // Dispatch and check
    let handlers = router.dispatch(35);
    let found = handlers.iter().any(|h| h.active && h.resource_id == 100);

    // Unregister
    let unreg = router.unregister(35, 100);
    let count_after = router.handler_count(35);

    let all_ok = reg_ok && count == 1 && found && unreg && count_after == 0;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "IRQ router register/dispatch",
        passed,
        score,
        weight: 10,
        details: format!(
            "reg={} count={} found={} unreg={} after={}",
            reg_ok, count, found, unreg, count_after
        ),
    }
}

/// Test 6: IRQ shared handlers — 4 on same vector (w:10)
fn test_irq_shared_handlers() -> StressResult {
    let mut router = IrqRouter::new();

    // Register 4 handlers on vector 40
    let mut reg_count = 0;
    for i in 0..4u32 {
        let handler = IrqHandler {
            driver_name: "shared",
            resource_id: 200 + i,
            active: true,
        };
        if router.register(40, handler).is_ok() {
            reg_count += 1;
        }
    }

    let count = router.handler_count(40);

    // Dispatch — all 4 should be active
    let handlers = router.dispatch(40);
    let active_count = handlers.iter().filter(|h| h.active).count();

    let all_ok = reg_count == 4 && count == 4 && active_count == 4;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "IRQ shared handlers (4 on vector)",
        passed,
        score,
        weight: 10,
        details: format!(
            "registered={} count={} active={}",
            reg_count, count, active_count
        ),
    }
}

/// Test 7: IRQ router overflow — 5th handler rejected (w:10)
fn test_irq_overflow() -> StressResult {
    let mut router = IrqRouter::new();

    // Fill vector 42 with 4 handlers
    for i in 0..4u32 {
        let handler = IrqHandler {
            driver_name: "overflow",
            resource_id: 300 + i,
            active: true,
        };
        let _ = router.register(42, handler);
    }

    // 5th should fail
    let handler5 = IrqHandler {
        driver_name: "overflow5",
        resource_id: 304,
        active: true,
    };
    let overflow_err = router.register(42, handler5).is_err();

    // Out-of-range vector should also fail
    let oob_handler = IrqHandler {
        driver_name: "oob",
        resource_id: 999,
        active: true,
    };
    let oob_err = router.register(48, oob_handler).is_err(); // vector 48 is out of range

    let all_ok = overflow_err && oob_err;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "IRQ router overflow rejection",
        passed,
        score,
        weight: 10,
        details: format!(
            "5th_rejected={} oob_rejected={}",
            overflow_err, oob_err
        ),
    }
}

/// Test 8: PCI device ID wildcard matching (w:10)
fn test_pci_device_id_matching() -> StressResult {
    // Create a mock PCI device
    let dev = PciDevice {
        bus: 0,
        device: 3,
        function: 0,
        vendor_id: 0x8086,
        device_id: 0x1234,
        class_code: 0x02,
        subclass: 0x00,
        header_type: 0,
        irq_line: 11,
        bars: [0; 6],
    };

    // Exact match
    let exact = PciDeviceId::new(0x8086, 0x1234, 0x02, 0x00);
    let exact_ok = exact.matches(&dev);

    // Vendor wildcard
    let vendor_wild = PciDeviceId::new(0xFFFF, 0x1234, 0x02, 0x00);
    let vendor_ok = vendor_wild.matches(&dev);

    // Device wildcard
    let device_wild = PciDeviceId::new(0x8086, 0xFFFF, 0x02, 0x00);
    let device_ok = device_wild.matches(&dev);

    // Class wildcard
    let class_wild = PciDeviceId::new(0x8086, 0x1234, 0xFF, 0xFF);
    let class_ok = class_wild.matches(&dev);

    // All wildcards
    let all_wild = PciDeviceId::new(0xFFFF, 0xFFFF, 0xFF, 0xFF);
    let all_ok_match = all_wild.matches(&dev);

    // Non-matching vendor
    let wrong_vendor = PciDeviceId::new(0x1AF4, 0x1234, 0x02, 0x00);
    let wrong_ok = !wrong_vendor.matches(&dev);

    // Non-matching class
    let wrong_class = PciDeviceId::new(0x8086, 0x1234, 0x03, 0x00);
    let wrong_class_ok = !wrong_class.matches(&dev);

    let all_ok = exact_ok && vendor_ok && device_ok && class_ok
        && all_ok_match && wrong_ok && wrong_class_ok;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "PCI device ID wildcard matching",
        passed,
        score,
        weight: 10,
        details: format!(
            "exact={} vendor_wild={} dev_wild={} class_wild={} all_wild={} wrong_ven={} wrong_cls={}",
            exact_ok, vendor_ok, device_ok, class_ok, all_ok_match, wrong_ok, wrong_class_ok
        ),
    }
}

/// Test 9: PCI driver table register (w:10)
fn test_pci_driver_table() -> StressResult {
    let mut table = PciDriverTable::new();

    // Register a driver
    static SUPPORTED: [PciDeviceId; 1] = [
        PciDeviceId::new(0x8086, 0x1234, 0xFF, 0xFF),
    ];

    let reg = table.register("test-nic", &SUPPORTED, 0x0001);
    let reg_ok = reg.is_ok();
    let idx = reg.unwrap_or(99);
    let count = table.driver_count();
    let name = table.driver_name(idx);
    let name_ok = name == Some("test-nic");

    // Bind against a matching device
    let devices = [PciDevice {
        bus: 0,
        device: 5,
        function: 0,
        vendor_id: 0x8086,
        device_id: 0x1234,
        class_code: 0x02,
        subclass: 0x00,
        header_type: 0,
        irq_line: 10,
        bars: [0; 6],
    }];

    let bound = table.bind_all(&devices);
    let bound_ok = bound == 1;
    let is_bound = table.is_bound(PciBdf::new(0, 5, 0));

    // Unbind
    let unbind_bdf = table.unbind(idx);
    let unbind_ok = unbind_bdf == Some(PciBdf::new(0, 5, 0));
    let bound_after = table.bound_count();

    // Unregister
    let unreg = table.unregister(idx);
    let count_after = table.driver_count();

    let all_ok = reg_ok && count == 1 && name_ok && bound_ok && is_bound
        && unbind_ok && bound_after == 0 && unreg && count_after == 0;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "PCI driver table register/bind",
        passed,
        score,
        weight: 10,
        details: format!(
            "reg={} count={} name={} bound={} is_bound={} unbind={} unreg={}",
            reg_ok, count, name_ok, bound_ok, is_bound, unbind_ok, unreg
        ),
    }
}

/// Test 10: Driver resources bundle (w:10)
fn test_driver_resources_bundle() -> StressResult {
    let mut res = DriverResources::new();

    // Add MMIO regions
    let mmio1 = res.add_mmio(MmioRegion::new(0xFE00_0000, 0x1000));
    let mmio2 = res.add_mmio(MmioRegion::new(0xFE01_0000, 0x2000));
    let mmio_count = res.mmio_count();

    // Add PIO region
    let pio1 = res.add_pio(PioRegion::new(0xC000, 32));
    let pio_count = res.pio_count();

    // Add DMA buffer (mock — doesn't actually allocate)
    let dma_buf = DmaBuffer { virt: 0x1000, phys: 0x1000, size: 4096, order: 0 };
    let dma1 = res.add_dma(dma_buf);
    let dma_count = res.dma_count();

    // Set IRQ
    res.irq_vector = Some(35);

    let all_ok = mmio1 && mmio2 && mmio_count == 2
        && pio1 && pio_count == 1
        && dma1 && dma_count == 1
        && res.irq_vector == Some(35);

    // Test overflow: add 5 more MMIO (should fill 4 more, then fail)
    let mmio3 = res.add_mmio(MmioRegion::new(0xFE02_0000, 0x1000));
    let mmio4 = res.add_mmio(MmioRegion::new(0xFE03_0000, 0x1000));
    let mmio5 = res.add_mmio(MmioRegion::new(0xFE04_0000, 0x1000));
    let mmio6 = res.add_mmio(MmioRegion::new(0xFE05_0000, 0x1000));
    let mmio7 = res.add_mmio(MmioRegion::new(0xFE06_0000, 0x1000)); // should fail
    let overflow_ok = mmio3 && mmio4 && mmio5 && mmio6 && !mmio7;

    let all_ok = all_ok && overflow_ok && res.mmio_count() == 6;

    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "Driver resources bundle",
        passed,
        score,
        weight: 10,
        details: format!(
            "mmio={} pio={} dma={} irq={:?} overflow_ok={}",
            res.mmio_count(), res.pio_count(), res.dma_count(),
            res.irq_vector, overflow_ok
        ),
    }
}
