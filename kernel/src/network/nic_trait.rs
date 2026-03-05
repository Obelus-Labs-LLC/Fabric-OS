//! NIC Driver Trait — abstract interface for network interface controllers.
//!
//! Both VirtIO-Net and real hardware Ethernet drivers (e1000e, RTL8169, etc.)
//! implement this trait. The network stack (nic_dispatch, arp) uses ACTIVE_NIC
//! instead of referencing a specific driver directly.

#![allow(dead_code)]

extern crate alloc;
use alloc::boxed::Box;
use crate::sync::OrderedMutex;

/// Trait for network interface controller drivers.
///
/// All NIC drivers must implement this trait to be used by the network stack.
/// The trait is object-safe so it can be used with `Box<dyn NicDriver>`.
pub trait NicDriver: Send {
    /// Human-readable driver name (e.g., "virtio-net", "e1000e").
    fn name(&self) -> &'static str;

    /// Return the 6-byte MAC address of this NIC.
    fn mac_address(&self) -> [u8; 6];

    /// Send a raw Ethernet frame (dst_mac + src_mac + ethertype + payload).
    /// Returns true if the frame was successfully queued for transmission.
    fn send_packet(&mut self, data: &[u8]) -> bool;

    /// Poll the receive queue for one completed frame.
    /// Returns Some((virt_addr, length)) if a frame is available.
    /// The caller must copy the data immediately and call recycle_rx().
    fn poll_rx(&mut self) -> Option<(usize, usize)>;

    /// Recycle the most recently polled RX buffer back to the hardware.
    /// Must be called after poll_rx() returns Some, before the next poll.
    fn recycle_rx(&mut self);

    /// Handle an interrupt from this NIC.
    /// Returns true if the interrupt was from this device.
    fn handle_irq(&mut self) -> bool;
}

/// The active NIC driver instance. Replaces the old `virtio::net::NIC` global.
///
/// Lock ordering: SOCKETS → ACTIVE_NIC is safe.
/// In IRQ context, use try_lock() to avoid deadlock.
pub static ACTIVE_NIC: OrderedMutex<Option<Box<dyn NicDriver>>, { crate::sync::levels::HAL }> =
    OrderedMutex::new(None);

/// Register a NIC driver as the active network interface.
pub fn register_nic(nic: Box<dyn NicDriver>) {
    *ACTIVE_NIC.lock() = Some(nic);
}

/// Get the MAC address of the active NIC, if any.
pub fn get_mac() -> Option<[u8; 6]> {
    ACTIVE_NIC.lock().as_ref().map(|n| n.mac_address())
}

/// Check if an active NIC is registered.
pub fn has_nic() -> bool {
    ACTIVE_NIC.lock().is_some()
}
