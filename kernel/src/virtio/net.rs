//! Virtio-Net Device Driver — legacy I/O port mode.
//!
//! Implements a minimal virtio network device driver using the legacy
//! PCI I/O port interface. Supports send/receive via split virtqueues.

#![allow(dead_code)]

use crate::sync::OrderedMutex;
use crate::io::{inb, outb, inw, outw, inl, outl};
use crate::memory::{frame, PhysAddr, PAGE_SIZE};
use crate::pci::PciDevice;
use crate::serial_println;
use super::{Virtqueue, VirtqDesc, VIRTQ_DESC_F_WRITE};

/// Virtio PCI legacy register offsets.
const VIRTIO_PCI_HOST_FEATURES: u16 = 0;
const VIRTIO_PCI_GUEST_FEATURES: u16 = 4;
const VIRTIO_PCI_QUEUE_PFN: u16 = 8;
const VIRTIO_PCI_QUEUE_SIZE: u16 = 12;
const VIRTIO_PCI_QUEUE_SEL: u16 = 14;
const VIRTIO_PCI_QUEUE_NOTIFY: u16 = 16;
const VIRTIO_PCI_STATUS: u16 = 18;
const VIRTIO_PCI_ISR: u16 = 19;
const VIRTIO_PCI_MAC: u16 = 20; // 6 bytes of MAC address

/// Virtio device status bits.
const VIRTIO_STATUS_ACK: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
const VIRTIO_STATUS_FAILED: u8 = 128;

/// Virtio-net feature bits.
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

/// Number of descriptors per queue.
const QUEUE_SIZE: u16 = 256;

/// Maximum packet size (Ethernet MTU + headers).
const MAX_PACKET_SIZE: usize = 1514 + 10; // ETH frame + virtio-net header

/// Virtio-net header (10 bytes for legacy, prepended to every packet).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
}

impl VirtioNetHeader {
    pub const fn empty() -> Self {
        Self {
            flags: 0,
            gso_type: 0,
            hdr_len: 0,
            gso_size: 0,
            csum_start: 0,
            csum_offset: 0,
        }
    }
}

/// Virtio-net device state.
pub struct VirtioNet {
    pub io_base: u16,
    pub mac: [u8; 6],
    pub rx_queue: Virtqueue,
    pub tx_queue: Virtqueue,
    /// Pre-allocated RX buffer physical addresses.
    rx_buffers: [u64; QUEUE_SIZE as usize],
    /// Pre-allocated RX buffer virtual addresses.
    rx_buffers_virt: [u64; QUEUE_SIZE as usize],
}

/// Global virtio-net device instance.
pub static NIC: OrderedMutex<Option<VirtioNet>, { crate::sync::levels::HAL }> =
    OrderedMutex::new(None);

impl VirtioNet {
    /// Initialize the virtio-net device from a PCI device descriptor.
    pub fn init(pci_dev: &PciDevice) -> Option<Self> {
        let io_base = pci_dev.bar0_io_base()?;
        serial_println!("[VIRTIO] virtio-net at I/O base 0x{:04x}", io_base);

        // 1. Reset device
        unsafe { outb(io_base + VIRTIO_PCI_STATUS, 0); }

        // 2. Acknowledge + driver
        unsafe {
            outb(io_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACK);
            outb(io_base + VIRTIO_PCI_STATUS, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER);
        }

        // 3. Feature negotiation
        let host_features = unsafe { inl(io_base + VIRTIO_PCI_HOST_FEATURES) };
        let guest_features = host_features & VIRTIO_NET_F_MAC; // only request MAC
        unsafe { outl(io_base + VIRTIO_PCI_GUEST_FEATURES, guest_features); }

        serial_println!("[VIRTIO] Host features: 0x{:08x}, negotiated: 0x{:08x}",
            host_features, guest_features);

        // 4. Read MAC address
        let mut mac = [0u8; 6];
        for i in 0..6 {
            mac[i] = unsafe { inb(io_base + VIRTIO_PCI_MAC + i as u16) };
        }
        serial_println!("[VIRTIO] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

        // 5. Set up RX queue (index 0)
        unsafe { outw(io_base + VIRTIO_PCI_QUEUE_SEL, 0); }
        let rx_size = unsafe { inw(io_base + VIRTIO_PCI_QUEUE_SIZE) };
        let rx_queue_size = rx_size.min(QUEUE_SIZE);
        serial_println!("[VIRTIO] RX queue size: {} (using {})", rx_size, rx_queue_size);

        let rx_queue = Virtqueue::new(rx_queue_size, io_base, 0)?;

        // 6. Set up TX queue (index 1)
        unsafe { outw(io_base + VIRTIO_PCI_QUEUE_SEL, 1); }
        let tx_size = unsafe { inw(io_base + VIRTIO_PCI_QUEUE_SIZE) };
        let tx_queue_size = tx_size.min(QUEUE_SIZE);
        serial_println!("[VIRTIO] TX queue size: {} (using {})", tx_size, tx_queue_size);

        let tx_queue = Virtqueue::new(tx_queue_size, io_base, 1)?;

        // 7. Allocate RX buffers and populate RX queue
        let mut rx_buffers = [0u64; QUEUE_SIZE as usize];
        let mut rx_buffers_virt = [0u64; QUEUE_SIZE as usize];
        let mut rx_queue_mut = rx_queue;

        for i in 0..rx_queue_size as usize {
            // Allocate a page for each RX buffer
            let phys = frame::allocate_frame()?;
            let virt = phys.to_virt().as_u64();

            // Zero the buffer
            unsafe { core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE); }

            rx_buffers[i] = phys.as_u64();
            rx_buffers_virt[i] = virt;

            // Set up descriptor: device writes into this buffer
            let desc_idx = rx_queue_mut.alloc_desc()?;
            unsafe {
                let d = &mut *rx_queue_mut.desc.add(desc_idx as usize);
                d.addr = phys.as_u64();
                d.len = MAX_PACKET_SIZE as u32;
                d.flags = VIRTQ_DESC_F_WRITE;
                d.next = 0;
            }

            // Add to available ring
            rx_queue_mut.submit(desc_idx);
        }

        // 8. Notify device about RX buffers
        unsafe { outw(io_base + VIRTIO_PCI_QUEUE_NOTIFY, 0); }

        // 9. Set DRIVER_OK
        unsafe {
            outb(io_base + VIRTIO_PCI_STATUS,
                VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK);
        }

        serial_println!("[VIRTIO] virtio-net initialized, DRIVER_OK set");

        Some(Self {
            io_base,
            mac,
            rx_queue: rx_queue_mut,
            tx_queue,
            rx_buffers,
            rx_buffers_virt,
        })
    }

    /// Send a raw Ethernet frame (with virtio-net header prepended).
    pub fn send_packet(&mut self, data: &[u8]) -> bool {
        // Need a descriptor for the packet
        let desc_idx = match self.tx_queue.alloc_desc() {
            Some(idx) => idx,
            None => return false,
        };

        // Allocate a temporary buffer for header + data
        let total_len = core::mem::size_of::<VirtioNetHeader>() + data.len();
        let phys = match frame::allocate_frame() {
            Some(p) => p,
            None => {
                self.tx_queue.free_desc(desc_idx);
                return false;
            }
        };

        let virt = phys.to_virt().as_u64() as *mut u8;

        // Write virtio-net header (all zeros = no offloading)
        unsafe {
            core::ptr::write_bytes(virt, 0, core::mem::size_of::<VirtioNetHeader>());
            // Copy packet data after header
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                virt.add(core::mem::size_of::<VirtioNetHeader>()),
                data.len(),
            );
        }

        // Set up descriptor
        unsafe {
            let d = &mut *self.tx_queue.desc.add(desc_idx as usize);
            d.addr = phys.as_u64();
            d.len = total_len as u32;
            d.flags = 0; // device reads from this buffer
            d.next = 0;
        }

        // Submit and notify
        self.tx_queue.submit(desc_idx);
        unsafe { outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 1); }

        true
    }

    /// Poll the RX queue for received packets.
    /// Returns the virtual address and length of the received data (including virtio-net header).
    pub fn poll_rx(&mut self) -> Option<(*const u8, usize)> {
        let (desc_idx, len) = self.rx_queue.poll_used()?;

        let virt = self.rx_buffers_virt[desc_idx as usize];
        if virt == 0 {
            return None;
        }

        // Skip the virtio-net header (10 bytes) to get the raw Ethernet frame
        let hdr_size = core::mem::size_of::<VirtioNetHeader>();
        let data_ptr = (virt as *const u8).wrapping_add(hdr_size);
        let data_len = (len as usize).saturating_sub(hdr_size);

        Some((data_ptr, data_len))
    }

    /// Re-submit an RX descriptor after processing.
    pub fn recycle_rx(&mut self, desc_idx: u16) {
        // Re-add to available ring
        unsafe {
            let d = &mut *self.rx_queue.desc.add(desc_idx as usize);
            d.len = MAX_PACKET_SIZE as u32;
            d.flags = VIRTQ_DESC_F_WRITE;
        }
        self.rx_queue.submit(desc_idx);
        unsafe { outw(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY, 0); }
    }

    /// Handle a virtio-net interrupt (read ISR to acknowledge).
    pub fn handle_irq(&mut self) -> bool {
        let isr = unsafe { inb(self.io_base + VIRTIO_PCI_ISR) };
        isr & 1 != 0 // bit 0 = used buffer notification
    }
}

/// Interrupt handler for virtio-net (called from IDT vector 43).
///
/// Only acknowledges the IRQ — does NOT process packets here.
/// Packet processing happens in deliver_one() → nic_receive_one(),
/// called from socket_connect/socket_poll/deliver_pending.
/// Processing packets in the IRQ handler would deadlock on SOCKETS
/// if the mainline code holds SOCKETS when the IRQ fires.
pub fn virtio_net_irq_handler() {
    // Use try_lock to avoid deadlock if mainline code (transmit_ip)
    // holds NIC when this IRQ fires.
    if let Some(mut guard) = NIC.try_lock() {
        if let Some(ref mut nic) = *guard {
            nic.handle_irq();
        }
    }
    crate::x86::apic::send_eoi();
}
