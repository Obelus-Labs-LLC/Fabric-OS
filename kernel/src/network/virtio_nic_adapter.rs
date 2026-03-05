//! VirtIO NIC Adapter — wraps VirtioNet as a NicDriver implementor.
//!
//! This adapter allows the existing VirtIO network driver to be used
//! through the NicDriver trait without modifying virtio/net.rs.

#![allow(dead_code)]

use crate::virtio::net::VirtioNet;
use super::nic_trait::NicDriver;

/// Adapter that wraps a VirtioNet instance as a NicDriver.
pub struct VirtioNicAdapter {
    inner: VirtioNet,
    /// Descriptor index from the most recent poll_rx(), used by recycle_rx().
    last_rx_desc: u16,
    /// Whether we have an un-recycled RX buffer.
    has_pending_rx: bool,
}

impl VirtioNicAdapter {
    /// Wrap an initialized VirtioNet in the adapter.
    pub fn new(nic: VirtioNet) -> Self {
        Self {
            inner: nic,
            last_rx_desc: 0,
            has_pending_rx: false,
        }
    }
}

impl NicDriver for VirtioNicAdapter {
    fn name(&self) -> &'static str {
        "virtio-net"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.inner.mac
    }

    fn send_packet(&mut self, data: &[u8]) -> bool {
        self.inner.send_packet(data)
    }

    fn poll_rx(&mut self) -> Option<(usize, usize)> {
        match self.inner.poll_rx() {
            Some((ptr, len)) => {
                // Track the descriptor index for recycle_rx()
                self.last_rx_desc = self.inner.rx_queue.last_used_idx.wrapping_sub(1);
                self.has_pending_rx = true;
                Some((ptr as usize, len))
            }
            None => None,
        }
    }

    fn recycle_rx(&mut self) {
        if self.has_pending_rx {
            self.inner.recycle_rx(self.last_rx_desc);
            self.has_pending_rx = false;
        }
    }

    fn handle_irq(&mut self) -> bool {
        self.inner.handle_irq()
    }
}
