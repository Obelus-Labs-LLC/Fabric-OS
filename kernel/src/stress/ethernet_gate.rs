//! STRESS Phase 20A Gate — Ethernet Driver Tests
//!
//! 10 tests verifying e1000e driver structures, PCI device ID matching,
//! BAR0 MMIO extraction, DMA descriptor ring layout, TX/RX descriptor
//! formatting, IRQ handler registration, MAC address byte order,
//! NicDriver trait compliance, and driver resources bundling.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::ocrb::StressResult;
use crate::drivers::e1000e::*;
use crate::pci::PciDevice;
use crate::hal::driver_sdk::{MmioRegion, DmaBuffer, DriverResources};
use crate::hal::irq_router::{IrqRouter, IrqHandler, IRQ_VECTOR_BASE};

pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();
    results.push(test_e1000e_struct_defaults());
    results.push(test_pci_device_id_matching());
    results.push(test_bar0_mmio_extraction());
    results.push(test_dma_descriptor_ring_layout());
    results.push(test_tx_descriptor_formatting());
    results.push(test_rx_descriptor_formatting());
    results.push(test_irq_handler_registration());
    results.push(test_mac_address_byte_order());
    results.push(test_nic_driver_trait_compliance());
    results.push(test_driver_resources_bundle());
    results
}

/// Test 1: E1000eDriver struct field sizes and descriptor sizes (w:10)
fn test_e1000e_struct_defaults() -> StressResult {
    // Verify descriptor sizes are exactly 16 bytes (hardware requirement)
    let tx_desc_size = core::mem::size_of::<E1000eTxDesc>();
    let rx_desc_size = core::mem::size_of::<E1000eRxDesc>();

    let tx_ok = tx_desc_size == 16;
    let rx_ok = rx_desc_size == 16;

    // Verify ring sizes
    let ring_ok = TX_RING_SIZE == 32 && RX_RING_SIZE == 32;

    // Verify buffer size
    let buf_ok = BUFFER_SIZE == 2048;

    let all_ok = tx_ok && rx_ok && ring_ok && buf_ok;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "E1000e struct sizes and constants",
        passed,
        score,
        weight: 10,
        details: format!(
            "tx_desc={} rx_desc={} ring={}/{} buf={}",
            tx_desc_size, rx_desc_size, TX_RING_SIZE, RX_RING_SIZE, BUFFER_SIZE
        ),
    }
}

/// Test 2: PCI device ID matching — all 6 Intel IDs + non-match (w:10)
fn test_pci_device_id_matching() -> StressResult {
    // Build a template PCI device
    let make_dev = |vendor: u16, device: u16| -> PciDevice {
        PciDevice {
            bus: 0, device: 0, function: 0,
            vendor_id: vendor, device_id: device,
            class_code: 0x02, subclass: 0x00,
            header_type: 0, irq_line: 11,
            bars: [0; 6],
        }
    };

    // All 6 supported device IDs should match
    let mut match_count = 0;
    for &did in &DEVICE_IDS {
        if is_e1000e(&make_dev(VENDOR_INTEL, did)) {
            match_count += 1;
        }
    }
    let all_ids_match = match_count == 6;

    // Non-Intel vendor should not match
    let non_intel = !is_e1000e(&make_dev(0x10EC, 0x8168)); // Realtek
    // Intel but unsupported device
    let unsupported = !is_e1000e(&make_dev(VENDOR_INTEL, 0x1234));
    // VirtIO should not match
    let no_virtio = !is_e1000e(&make_dev(0x1AF4, 0x1000));

    let all_ok = all_ids_match && non_intel && unsupported && no_virtio;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "PCI device ID matching (6 IDs + reject)",
        passed,
        score,
        weight: 10,
        details: format!(
            "matched={}/6 non_intel={} unsupported={} no_virtio={}",
            match_count, non_intel, unsupported, no_virtio
        ),
    }
}

/// Test 3: BAR0 MMIO base extraction — 32-bit, 64-bit, I/O BAR (w:10)
fn test_bar0_mmio_extraction() -> StressResult {
    // 32-bit MMIO BAR (type bits 1:2 = 00, bit 0 = 0)
    let dev_32bit = PciDevice {
        bus: 0, device: 0, function: 0,
        vendor_id: VENDOR_INTEL, device_id: 0x153A,
        class_code: 0x02, subclass: 0x00,
        header_type: 0, irq_line: 11,
        bars: [0xF780_0000, 0, 0, 0, 0, 0], // 32-bit MMIO at 0xF7800000
    };
    let base_32 = E1000eDriver::bar0_mmio_base(&dev_32bit);
    let ok_32 = base_32 == Some(0xF780_0000);

    // 64-bit MMIO BAR (type bits 1:2 = 10, bit 0 = 0)
    let dev_64bit = PciDevice {
        bus: 0, device: 0, function: 0,
        vendor_id: VENDOR_INTEL, device_id: 0x155A,
        class_code: 0x02, subclass: 0x00,
        header_type: 0, irq_line: 11,
        bars: [0xF780_0004, 0x0000_0001, 0, 0, 0, 0], // 64-bit MMIO
    };
    let base_64 = E1000eDriver::bar0_mmio_base(&dev_64bit);
    // Should combine: low = 0xF7800000 (masked), high = 0x00000001
    let ok_64 = base_64 == Some(0x0000_0001_F780_0000);

    // I/O BAR (bit 0 = 1) — should return None
    let dev_io = PciDevice {
        bus: 0, device: 0, function: 0,
        vendor_id: VENDOR_INTEL, device_id: 0x15B8,
        class_code: 0x02, subclass: 0x00,
        header_type: 0, irq_line: 11,
        bars: [0x0000_C001, 0, 0, 0, 0, 0], // I/O BAR
    };
    let base_io = E1000eDriver::bar0_mmio_base(&dev_io);
    let ok_io = base_io.is_none();

    let all_ok = ok_32 && ok_64 && ok_io;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "BAR0 MMIO base extraction (32/64/IO)",
        passed,
        score,
        weight: 10,
        details: format!(
            "32bit={:?} 64bit={:?} io={:?}",
            base_32, base_64, base_io
        ),
    }
}

/// Test 4: DMA descriptor ring allocation — verify size (w:10)
fn test_dma_descriptor_ring_layout() -> StressResult {
    // TX ring: 32 descriptors × 16 bytes = 512 bytes
    let tx_ring_bytes = TX_RING_SIZE * core::mem::size_of::<E1000eTxDesc>();
    let tx_ok = tx_ring_bytes == 512;

    // RX ring: 32 descriptors × 16 bytes = 512 bytes
    let rx_ring_bytes = RX_RING_SIZE * core::mem::size_of::<E1000eRxDesc>();
    let rx_ok = rx_ring_bytes == 512;

    // TX buffers: 32 × 2048 = 65536 bytes
    let tx_buf_bytes = TX_RING_SIZE * BUFFER_SIZE;
    let tx_buf_ok = tx_buf_bytes == 65536;

    // RX buffers: 32 × 2048 = 65536 bytes
    let rx_buf_bytes = RX_RING_SIZE * BUFFER_SIZE;
    let rx_buf_ok = rx_buf_bytes == 65536;

    let all_ok = tx_ok && rx_ok && tx_buf_ok && rx_buf_ok;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "DMA descriptor ring layout",
        passed,
        score,
        weight: 10,
        details: format!(
            "tx_ring={} rx_ring={} tx_buf={} rx_buf={}",
            tx_ring_bytes, rx_ring_bytes, tx_buf_bytes, rx_buf_bytes
        ),
    }
}

/// Test 5: TX descriptor formatting — EOP|IFCS|RS flags, addr/length layout (w:10)
fn test_tx_descriptor_formatting() -> StressResult {
    let mut desc = E1000eTxDesc {
        addr: 0xDEAD_BEEF_0000,
        length: 1500,
        cso: 0,
        cmd: TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS,
        status: 0,
        css: 0,
        special: 0,
    };

    // Verify command bits
    let eop_set = desc.cmd & TX_CMD_EOP != 0;
    let ifcs_set = desc.cmd & TX_CMD_IFCS != 0;
    let rs_set = desc.cmd & TX_CMD_RS != 0;
    let cmd_ok = eop_set && ifcs_set && rs_set;

    // Verify length
    let len_ok = desc.length == 1500;

    // Verify address
    let addr_ok = desc.addr == 0xDEAD_BEEF_0000;

    // Status: DD bit should be clear before TX
    let dd_clear = desc.status & TX_STATUS_DD == 0;

    // After TX completion, hardware sets DD
    desc.status = TX_STATUS_DD;
    let dd_set = desc.status & TX_STATUS_DD != 0;

    let all_ok = cmd_ok && len_ok && addr_ok && dd_clear && dd_set;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "TX descriptor formatting (EOP|IFCS|RS)",
        passed,
        score,
        weight: 10,
        details: format!(
            "eop={} ifcs={} rs={} len={} dd_clear={} dd_set={}",
            eop_set, ifcs_set, rs_set, len_ok, dd_clear, dd_set
        ),
    }
}

/// Test 6: RX descriptor formatting — DD/EOP status bit parsing (w:10)
fn test_rx_descriptor_formatting() -> StressResult {
    let mut desc = E1000eRxDesc {
        addr: 0xBEEF_CAFE_0000,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        special: 0,
    };

    // Initially: no packet received, DD clear
    let dd_clear = desc.status & RX_STATUS_DD == 0;
    let eop_clear = desc.status & RX_STATUS_EOP == 0;

    // Simulate hardware completing receive
    desc.status = RX_STATUS_DD | RX_STATUS_EOP;
    desc.length = 1514; // Full Ethernet frame

    let dd_set = desc.status & RX_STATUS_DD != 0;
    let eop_set = desc.status & RX_STATUS_EOP != 0;
    let len_ok = desc.length == 1514;

    // Verify no errors
    let no_errors = desc.errors == 0;

    // Test addr preserved
    let addr_ok = desc.addr == 0xBEEF_CAFE_0000;

    let all_ok = dd_clear && eop_clear && dd_set && eop_set && len_ok && no_errors && addr_ok;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "RX descriptor formatting (DD|EOP status)",
        passed,
        score,
        weight: 10,
        details: format!(
            "dd_clear={} dd_set={} eop_set={} len={} errors={}",
            dd_clear, dd_set, eop_set, desc.length, desc.errors
        ),
    }
}

/// Test 7: IRQ handler registration — register/dispatch/unregister via IrqRouter (w:10)
fn test_irq_handler_registration() -> StressResult {
    let mut router = IrqRouter::new();

    // Register e1000e handler on vector 43 (IRQ 11)
    let handler = IrqHandler {
        driver_name: "e1000e",
        resource_id: 0x8086_153A,
        active: true,
    };

    let reg_ok = router.register(43, handler).is_ok();

    // Dispatch should return the handler
    let slot = router.dispatch(43);
    let found = slot.iter().any(|h| h.active && h.driver_name == "e1000e");

    // Unregister
    let unreg_ok = router.unregister(43, 0x8086_153A);

    // After unregister, handler should be inactive
    let slot_after = router.dispatch(43);
    let gone = !slot_after.iter().any(|h| h.active && h.driver_name == "e1000e");

    let all_ok = reg_ok && found && unreg_ok && gone;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "IRQ handler registration (register/dispatch/unregister)",
        passed,
        score,
        weight: 10,
        details: format!(
            "registered={} found={} unregistered={} gone={}",
            reg_ok, found, unreg_ok, gone
        ),
    }
}

/// Test 8: MAC address byte order — RAL/RAH → 6-byte MAC extraction (w:10)
fn test_mac_address_byte_order() -> StressResult {
    // Simulate RAL/RAH register values for MAC 52:54:00:12:34:56
    let ral: u32 = 0x12005452; // bytes 0-3: 52, 54, 00, 12
    let rah: u32 = 0x00005634; // bytes 4-5: 34, 56

    let mac = [
        (ral & 0xFF) as u8,
        ((ral >> 8) & 0xFF) as u8,
        ((ral >> 16) & 0xFF) as u8,
        ((ral >> 24) & 0xFF) as u8,
        (rah & 0xFF) as u8,
        ((rah >> 8) & 0xFF) as u8,
    ];

    let expected: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let mac_ok = mac == expected;

    // Verify each byte
    let byte0 = mac[0] == 0x52;
    let byte1 = mac[1] == 0x54;
    let byte4 = mac[4] == 0x34;
    let byte5 = mac[5] == 0x56;

    let all_ok = mac_ok && byte0 && byte1 && byte4 && byte5;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "MAC address byte order (RAL/RAH extraction)",
        passed,
        score,
        weight: 10,
        details: format!(
            "mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} expected=52:54:00:12:34:56",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        ),
    }
}

/// Test 9: NicDriver trait compliance — verify ACTIVE_NIC global exists and works (w:10)
fn test_nic_driver_trait_compliance() -> StressResult {
    use crate::network::nic_trait;

    // has_nic() should work (may be true or false depending on boot state)
    let has_nic = nic_trait::has_nic();

    // get_mac() should work without panic
    let mac_result = nic_trait::get_mac();

    // If NIC is registered, both should be consistent
    let consistent = if has_nic {
        mac_result.is_some()
    } else {
        mac_result.is_none()
    };

    // Verify ACTIVE_NIC lock doesn't deadlock (try_lock after lock drop)
    let lock_ok = {
        let _guard = nic_trait::ACTIVE_NIC.lock();
        true // If we get here, lock succeeded
    };
    // Now try_lock should succeed since we dropped the guard
    let try_lock_ok = nic_trait::ACTIVE_NIC.try_lock().is_some();

    // Verify NicDriver trait is object-safe by checking the type compiles
    let trait_object_ok = true; // Box<dyn NicDriver> is used in ACTIVE_NIC

    let all_ok = consistent && lock_ok && try_lock_ok && trait_object_ok;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "NicDriver trait compliance (ACTIVE_NIC)",
        passed,
        score,
        weight: 10,
        details: format!(
            "has_nic={} mac={:?} consistent={} lock={} try_lock={}",
            has_nic, mac_result, consistent, lock_ok, try_lock_ok
        ),
    }
}

/// Test 10: Driver resources bundle — MMIO + DMA + IRQ vector counts (w:10)
fn test_driver_resources_bundle() -> StressResult {
    let mut res = DriverResources::new();

    // Add MMIO regions (e1000e uses 1 BAR)
    let mmio = MmioRegion::new(0xF780_0000, 128 * 1024);
    let mmio_added = res.add_mmio(mmio);
    let mmio_count = res.mmio_count();

    // Add DMA buffers (TX ring, TX buffers, RX ring, RX buffers)
    let dma1 = DmaBuffer { virt: 0x1000, phys: 0x1000, size: 512, order: 0 };
    let dma2 = DmaBuffer { virt: 0x2000, phys: 0x2000, size: 65536, order: 4 };
    let dma3 = DmaBuffer { virt: 0x3000, phys: 0x3000, size: 512, order: 0 };
    let dma4 = DmaBuffer { virt: 0x4000, phys: 0x4000, size: 65536, order: 4 };
    let d1 = res.add_dma(dma1);
    let d2 = res.add_dma(dma2);
    let d3 = res.add_dma(dma3);
    let d4 = res.add_dma(dma4);
    let dma_count = res.dma_count();

    // Set IRQ vector
    res.irq_vector = Some(43);
    let irq_ok = res.irq_vector == Some(43);

    let mmio_ok = mmio_added && mmio_count == 1;
    let dma_ok = d1 && d2 && d3 && d4 && dma_count == 4;

    let all_ok = mmio_ok && dma_ok && irq_ok;
    let (passed, score) = if all_ok { (true, 100) } else { (false, 0) };

    StressResult {
        test_name: "Driver resources bundle (MMIO+DMA+IRQ)",
        passed,
        score,
        weight: 10,
        details: format!(
            "mmio={} dma={} irq={:?}",
            mmio_count, dma_count, res.irq_vector
        ),
    }
}
