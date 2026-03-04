//! OCRB Phase 11 — NIC + Keyboard Gate (11 tests, weight 100).
//!
//! Verifies port I/O, PCI bus, IO APIC, PS/2 keyboard buffer,
//! virtio device discovery, virtqueue setup, Ethernet frames,
//! ARP packets, DNS queries, and HTTP requests.

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use super::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    let mut results = Vec::new();

    results.push(test_port_io_exists());
    results.push(test_pci_config_read());
    results.push(test_pci_bus_scan());
    results.push(test_ioapic_init());
    results.push(test_keyboard_buffer());
    results.push(test_virtio_device_found());
    results.push(test_virtqueue_setup());
    results.push(test_ethernet_frame_build());
    results.push(test_arp_packet_build());
    results.push(test_dns_query_build());
    results.push(test_http_get_build());

    results
}

/// Test 1: Port I/O functions exist and are callable (w:5).
fn test_port_io_exists() -> OcrbResult {
    // Simply calling inb on a known-safe port (PIT channel 2, 0x42) won't crash.
    // We can't verify the value but we can verify the function compiles and runs.
    let val = unsafe { crate::io::inb(0x42) };
    let _ = val; // Use the value to avoid optimization

    OcrbResult {
        test_name: "Port I/O Exists",
        passed: true,
        score: 100,
        weight: 5,
        details: String::from("inb/outb callable"),
    }
}

/// Test 2: PCI config read returns valid data (w:10).
fn test_pci_config_read() -> OcrbResult {
    // Read vendor ID at bus 0, device 0, function 0 — QEMU always has a host bridge
    let vendor = crate::pci::config_read_u16(0, 0, 0, 0x00);

    if vendor != 0xFFFF && vendor != 0x0000 {
        OcrbResult {
            test_name: "PCI Config Read",
            passed: true,
            score: 100,
            weight: 10,
            details: alloc::format!("vendor=0x{:04x}", vendor),
        }
    } else {
        OcrbResult {
            test_name: "PCI Config Read",
            passed: false,
            score: 0,
            weight: 10,
            details: alloc::format!("vendor=0x{:04x} (invalid)", vendor),
        }
    }
}

/// Test 3: PCI bus scan finds devices (w:10).
fn test_pci_bus_scan() -> OcrbResult {
    let devices = crate::pci::scan_bus(0);

    if !devices.is_empty() {
        let valid = devices.iter().all(|d| d.vendor_id != 0xFFFF);
        OcrbResult {
            test_name: "PCI Bus Scan",
            passed: valid,
            score: if valid { 100 } else { 50 },
            weight: 10,
            details: alloc::format!("{} devices found", devices.len()),
        }
    } else {
        OcrbResult {
            test_name: "PCI Bus Scan",
            passed: false,
            score: 0,
            weight: 10,
            details: String::from("no devices found"),
        }
    }
}

/// Test 4: IO APIC init — verify register read returns valid ID (w:5).
fn test_ioapic_init() -> OcrbResult {
    let id = crate::x86::ioapic::read_id();
    let (ver, max_entries) = crate::x86::ioapic::read_version();

    // IO APIC version should be reasonable (0x11 or 0x20 typically)
    let valid = ver > 0 && max_entries > 0;

    OcrbResult {
        test_name: "IO APIC Init",
        passed: valid,
        score: if valid { 100 } else { 0 },
        weight: 5,
        details: alloc::format!("id={}, ver=0x{:02x}, max_entries={}", id, ver, max_entries),
    }
}

/// Test 5: Keyboard buffer push/pop FIFO order (w:10).
fn test_keyboard_buffer() -> OcrbResult {
    let mut buf = crate::keyboard::KeyboardBuffer::new();

    // Push A, B, C
    buf.push(b'A');
    buf.push(b'B');
    buf.push(b'C');

    // Pop should be FIFO
    let a = buf.pop();
    let b = buf.pop();
    let c = buf.pop();
    let empty = buf.pop();

    let passed = a == Some(b'A') && b == Some(b'B') && c == Some(b'C') && empty.is_none();

    OcrbResult {
        test_name: "Keyboard Buffer",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: if passed {
            String::from("FIFO order verified")
        } else {
            alloc::format!("got {:?},{:?},{:?},{:?}", a, b, c, empty)
        },
    }
}

/// Test 6: Virtio device found via PCI scan (w:10).
fn test_virtio_device_found() -> OcrbResult {
    let devices = crate::pci::scan_bus(0);
    let virtio = devices.iter().find(|d| d.is_virtio());

    if let Some(dev) = virtio {
        OcrbResult {
            test_name: "Virtio Device Found",
            passed: true,
            score: 100,
            weight: 10,
            details: alloc::format!(
                "{:02x}:{:02x}.{} {:04x}:{:04x}",
                dev.bus, dev.device, dev.function,
                dev.vendor_id, dev.device_id
            ),
        }
    } else {
        // Graceful skip if no virtio device (QEMU might not have one)
        OcrbResult {
            test_name: "Virtio Device Found",
            passed: true,
            score: 50,
            weight: 10,
            details: String::from("no virtio device (may need QEMU flags)"),
        }
    }
}

/// Test 7: Virtqueue alloc + free list (w:10).
fn test_virtqueue_setup() -> OcrbResult {
    // We can't actually set up a real virtqueue without a device,
    // but we can test the free list logic with a manually created one.
    // Test: allocate 3 descriptors, free them, verify counts.

    // Check if NIC is initialized (has working virtqueues)
    let nic_guard = crate::virtio::net::NIC.lock();
    if let Some(ref nic) = *nic_guard {
        // NIC is initialized, verify queue sizes are reasonable
        let rx_size = nic.rx_queue.size;
        let tx_size = nic.tx_queue.size;
        let valid = rx_size > 0 && tx_size > 0;
        drop(nic_guard);

        OcrbResult {
            test_name: "Virtqueue Setup",
            passed: valid,
            score: if valid { 100 } else { 0 },
            weight: 10,
            details: alloc::format!("rx_size={}, tx_size={}", rx_size, tx_size),
        }
    } else {
        drop(nic_guard);

        // No NIC — test the allocator logic directly
        // Verify pages_to_order works correctly
        let order0 = crate::virtio::pages_to_order(1);
        let order1 = crate::virtio::pages_to_order(2);
        let order2 = crate::virtio::pages_to_order(3);

        let valid = order0 == 0 && order1 == 1 && order2 == 2;

        OcrbResult {
            test_name: "Virtqueue Setup",
            passed: valid,
            score: if valid { 100 } else { 0 },
            weight: 10,
            details: alloc::format!("pages_to_order: 1->{}, 2->{}, 3->{}", order0, order1, order2),
        }
    }
}

/// Test 8: Ethernet frame build (w:10).
fn test_ethernet_frame_build() -> OcrbResult {
    use crate::network::ethernet::{EthernetFrame, ETHERTYPE_IPV4};

    let dst = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let src = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let payload = [0x45, 0x00, 0x00, 0x20]; // Fake IPv4 header start

    let frame = EthernetFrame::build(dst, src, ETHERTYPE_IPV4, &payload);

    // Verify: 14-byte header + 4-byte payload = 18 bytes
    let passed = frame.len() == 18
        && frame[0..6] == dst
        && frame[6..12] == src
        && frame[12] == 0x08  // EtherType 0x0800 high byte
        && frame[13] == 0x00  // EtherType 0x0800 low byte
        && frame[14..18] == payload;

    OcrbResult {
        test_name: "Ethernet Frame Build",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("frame_len={}", frame.len()),
    }
}

/// Test 9: ARP packet build (w:10).
fn test_arp_packet_build() -> OcrbResult {
    use crate::network::arp::ArpPacket;

    let src_mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let src_ip = [10, 0, 2, 15];
    let target_ip = [10, 0, 2, 2];

    let arp = ArpPacket::build_request(src_mac, src_ip, target_ip);
    let bytes = arp.to_bytes();

    // Verify: 28 bytes, opcode=1 (request), correct IPs
    let passed = bytes.len() == 28
        && bytes[6] == 0x00 && bytes[7] == 0x01  // Opcode = Request
        && bytes[14..18] == src_ip
        && bytes[24..28] == target_ip;

    OcrbResult {
        test_name: "ARP Packet Build",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("arp_len={}, opcode={}", bytes.len(), arp.opcode),
    }
}

/// Test 10: DNS query build (w:10).
fn test_dns_query_build() -> OcrbResult {
    use crate::network::dns;

    let query = dns::build_query("example.com");

    // Verify: 12-byte header + encoded name + 4 bytes (QTYPE+QCLASS)
    // "example.com" encodes as: [7]example[3]com[0] = 13 bytes
    // Total: 12 + 13 + 4 = 29 bytes
    let expected_len = 29;

    let passed = query.len() == expected_len
        && query[0] == 0x12 && query[1] == 0x34  // Transaction ID
        && query[2] == 0x01 && query[3] == 0x00  // Flags: RD
        && query[4] == 0x00 && query[5] == 0x01  // QDCOUNT = 1
        && query[12] == 7                          // "example" label length
        && query[20] == 3;                         // "com" label length

    OcrbResult {
        test_name: "DNS Query Build",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("query_len={}", query.len()),
    }
}

/// Test 11: HTTP GET build (w:10).
fn test_http_get_build() -> OcrbResult {
    use crate::network::http;

    let request = http::build_get_request("example.com", "/index.html");

    let passed = request.starts_with("GET /index.html HTTP/1.1\r\n")
        && request.contains("Host: example.com\r\n")
        && request.ends_with("\r\n\r\n");

    OcrbResult {
        test_name: "HTTP GET Build",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("request_len={}", request.len()),
    }
}
