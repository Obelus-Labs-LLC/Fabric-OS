//! Network Stack — Phase 9 of Fabric OS.
//!
//! Provides a loopback-based network stack with socket API, IPv4,
//! UDP, and TCP (simplified for lossless loopback). Real NIC support
//! deferred to Phase 10+.
//!
//! // LOCK ORDER: SOCKETS then LOOPBACK — NEVER held simultaneously.
//! //   Send:    lock SOCKETS (read) -> drop -> lock LOOPBACK (enqueue) -> drop
//! //   Deliver: lock LOOPBACK (dequeue) -> drop -> lock SOCKETS (write RX) -> drop

#![allow(dead_code)]

pub mod addr;
pub mod buffer;
pub mod checksum;
pub mod socket;
pub mod ip;
pub mod udp;
pub mod tcp;
pub mod loopback;
pub mod ops;

use spin::Mutex;
use crate::serial_println;

pub use socket::SocketTable;
pub use loopback::Loopback;

/// Global socket table — 256 slots.
pub static SOCKETS: Mutex<SocketTable> = Mutex::new(SocketTable::new());

/// Global loopback interface — 64-slot packet queue.
pub static LOOPBACK: Mutex<Loopback> = Mutex::new(Loopback::new());

/// Initialize the network subsystem.
pub fn init() {
    serial_println!("[NET] Network subsystem initializing...");
    serial_println!("[NET] Loopback interface: 127.0.0.1 (MTU=1500, queue=64)");
    serial_println!("[NET] Socket table: 256 slots, 4KB rx/tx buffers");
    serial_println!("[NET] Protocols: UDP, TCP (loopback-simplified)");
    // TODO(Phase 10): TCP retransmission timers, proper ISN randomization,
    // congestion control (Reno/CUBIC), Nagle algorithm, silly window syndrome.
    serial_println!("[NET] Network subsystem initialized");
}

/// Clean up all sockets owned by a process. Called during terminate().
/// Must be called BEFORE address space free.
pub fn cleanup_process_sockets(pid: fabric_types::ProcessId) {
    let mut table = SOCKETS.lock();
    table.cleanup_by_owner(pid);
}
