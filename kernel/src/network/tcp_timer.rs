//! TCP Retransmission Timers — Jacobson/Karels RTO + retransmit queue.
//!
//! Each TCP socket has a RetransmitQueue holding unacknowledged segments.
//! `check_all_retransmits()` is called from `deliver_one()` to resend
//! timed-out packets. Karn's algorithm: RTT is not updated for
//! retransmitted segments.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use crate::serial_println;
use crate::x86::idt::tick_count;

/// Minimum RTO in milliseconds.
const RTO_MIN_MS: u32 = 200;
/// Maximum RTO in milliseconds.
const RTO_MAX_MS: u32 = 60_000;
/// Maximum retransmit attempts before RST.
const MAX_RETRIES: u8 = 5;

/// A single unacknowledged segment awaiting ACK.
pub struct RetransmitEntry {
    /// Starting sequence number of this segment.
    pub seq: u32,
    /// Full IP packet bytes (ready to re-send via transmit_ip).
    pub data: Vec<u8>,
    /// Tick count when originally sent.
    pub timestamp: u64,
    /// Number of retransmissions so far.
    pub retries: u8,
}

/// Per-socket retransmit state: queue + RTT estimator.
pub struct RetransmitQueue {
    /// Unacknowledged segments.
    pub entries: Vec<RetransmitEntry>,
    /// Smoothed RTT estimate in ms (scaled x8 for integer math).
    srtt_x8: u32,
    /// RTT variance in ms (scaled x4 for integer math).
    rttvar_x4: u32,
    /// Current retransmit timeout in ms.
    pub rto: u32,
}

impl RetransmitQueue {
    /// Create a new retransmit queue with default RTO.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            srtt_x8: 1000 * 8,    // Initial SRTT = 1000ms
            rttvar_x4: 500 * 4,   // Initial RTTVAR = 500ms
            rto: 1000,            // Initial RTO = 1s
        }
    }

    /// Add a sent segment to the retransmit queue.
    pub fn enqueue(&mut self, seq: u32, packet: Vec<u8>, now: u64) {
        self.entries.push(RetransmitEntry {
            seq,
            data: packet,
            timestamp: now,
            retries: 0,
        });
    }

    /// Process an incoming ACK: remove all entries with seq < ack_num,
    /// update RTT/RTO using Jacobson/Karels (Karn's: skip retransmitted).
    pub fn ack_received(&mut self, ack_num: u32, now: u64) {
        let mut rtt_sample = None;

        self.entries.retain(|entry| {
            // Keep entries whose sequence is >= ack_num (still unacked)
            if wrapping_lt(entry.seq, ack_num) {
                // This segment was ACKed
                // Only measure RTT for non-retransmitted segments (Karn's algorithm)
                if entry.retries == 0 {
                    let rtt_ms = now.saturating_sub(entry.timestamp) as u32;
                    rtt_sample = Some(rtt_ms);
                }
                false // remove from queue
            } else {
                true // keep
            }
        });

        // Update RTO from RTT sample (Jacobson/Karels algorithm)
        if let Some(rtt) = rtt_sample {
            self.update_rto(rtt);
        }
    }

    /// Check for timed-out entries. Returns packets that need retransmission.
    /// Doubles RTO for each retry (exponential backoff).
    pub fn check_timeouts(&mut self, now: u64) -> Vec<Vec<u8>> {
        let mut to_resend = Vec::new();

        for entry in &mut self.entries {
            let elapsed = now.saturating_sub(entry.timestamp) as u32;
            // Effective RTO doubles with each retry
            let effective_rto = self.rto.saturating_mul(1u32 << entry.retries.min(5));
            let effective_rto = effective_rto.min(RTO_MAX_MS);

            if elapsed >= effective_rto {
                if entry.retries < MAX_RETRIES {
                    entry.retries += 1;
                    entry.timestamp = now; // Reset timer
                    to_resend.push(entry.data.clone());
                }
                // If retries >= MAX_RETRIES, we'll handle RST in the caller
            }
        }

        to_resend
    }

    /// Check if any entry has exceeded max retries.
    pub fn has_max_retries(&self) -> bool {
        self.entries.iter().any(|e| e.retries >= MAX_RETRIES)
    }

    /// Clear all entries (on connection close).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Update RTO using Jacobson/Karels algorithm.
    /// SRTT = 7/8 * SRTT + 1/8 * R
    /// RTTVAR = 3/4 * RTTVAR + 1/4 * |SRTT - R|
    /// RTO = SRTT + 4 * RTTVAR, clamped to [RTO_MIN, RTO_MAX]
    fn update_rto(&mut self, rtt_ms: u32) {
        let r_x8 = rtt_ms * 8;
        let srtt = self.srtt_x8 / 8;

        // |SRTT - R| for variance calculation
        let diff = if srtt > rtt_ms {
            srtt - rtt_ms
        } else {
            rtt_ms - srtt
        };

        // RTTVAR = 3/4 * RTTVAR + 1/4 * |SRTT - R|
        self.rttvar_x4 = (self.rttvar_x4 * 3 + diff * 4) / 4;

        // SRTT = 7/8 * SRTT + 1/8 * R
        self.srtt_x8 = (self.srtt_x8 * 7 + r_x8) / 8;

        // RTO = SRTT + 4 * RTTVAR
        let new_rto = (self.srtt_x8 / 8) + self.rttvar_x4;
        self.rto = new_rto.clamp(RTO_MIN_MS, RTO_MAX_MS);
    }

    /// Get current smoothed RTT in ms.
    pub fn srtt_ms(&self) -> u32 {
        self.srtt_x8 / 8
    }

    /// Get current RTT variance in ms.
    pub fn rttvar_ms(&self) -> u32 {
        self.rttvar_x4 / 4
    }

    /// Number of pending entries.
    pub fn pending_count(&self) -> usize {
        self.entries.len()
    }
}

/// Compute RTO from a single RTT sample (for testing/standalone use).
pub fn rto_from_rtt(rtt_ms: u32) -> u32 {
    // Simple: RTO = RTT + 4 * (RTT/2) = 3 * RTT, clamped
    let rto = rtt_ms.saturating_add(rtt_ms * 2);
    rto.clamp(RTO_MIN_MS, RTO_MAX_MS)
}

/// Check all active TCP sockets for retransmit timeouts.
/// Called from `deliver_one()` in ops.rs.
pub fn check_all_retransmits() {
    use super::SOCKETS;
    use super::socket::MAX_SOCKETS;
    use super::nic_dispatch;

    let now = tick_count();

    // Phase 1: Collect packets to retransmit under SOCKETS lock
    let mut resend_packets: Vec<Vec<u8>> = Vec::new();
    let mut rst_sockets: Vec<usize> = Vec::new();

    {
        let mut table = SOCKETS.lock();
        for slot in 0..MAX_SOCKETS {
            let sock = &mut table.sockets[slot];
            if !sock.active {
                continue;
            }
            if let Some(ref mut rq) = sock.retransmit {
                if rq.entries.is_empty() {
                    continue;
                }
                let mut packets = rq.check_timeouts(now);
                resend_packets.append(&mut packets);

                if rq.has_max_retries() {
                    rst_sockets.push(slot);
                }
            }
        }

        // Close sockets that exceeded max retries
        for slot in &rst_sockets {
            let sock = &mut table.sockets[*slot];
            sock.state = super::socket::SocketState::Closed;
            sock.active = false;
            if let Some(ref mut rq) = sock.retransmit {
                rq.clear();
            }
            serial_println!("[TCP] Connection in slot {} RST (max retries exceeded)", slot);
        }
    }
    // SOCKETS dropped here

    // Phase 2: Retransmit packets without holding SOCKETS
    for packet in &resend_packets {
        nic_dispatch::transmit_ip(packet);
    }
}

/// Wrapping sequence number comparison: returns true if a < b in the
/// TCP sequence space (handles wraparound).
fn wrapping_lt(a: u32, b: u32) -> bool {
    // a < b if (b - a) is in the range (0, 2^31)
    let diff = b.wrapping_sub(a);
    diff > 0 && diff < 0x8000_0000
}
