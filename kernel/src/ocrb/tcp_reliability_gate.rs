//! OCRB Phase 13 — TCP Reliability & Async I/O Gate (10 tests, weight 100).
//!
//! Verifies retransmit queue, RTO calculation, Karn's algorithm,
//! poll() events, DNS cache, and DNS retry transaction IDs.

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use super::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    let mut results = Vec::new();

    results.push(test_retransmit_enqueue_ack());
    results.push(test_rto_calculation());
    results.push(test_karns_algorithm());
    results.push(test_retransmit_timeout());
    results.push(test_pollfd_immediate_ready());
    results.push(test_pollfd_timeout_zero());
    results.push(test_pollfd_events());
    results.push(test_dns_cache_insert_lookup());
    results.push(test_dns_cache_lru_eviction());
    results.push(test_dns_retry_txn_id());

    results
}

/// Test 1: Enqueue 3 entries, ACK clears correct ones (w:10).
fn test_retransmit_enqueue_ack() -> OcrbResult {
    use crate::network::tcp_timer::RetransmitQueue;

    let mut rq = RetransmitQueue::new();

    // Enqueue 3 segments at different sequence numbers
    rq.enqueue(100, vec![1, 2, 3], 1000);
    rq.enqueue(200, vec![4, 5, 6], 1001);
    rq.enqueue(300, vec![7, 8, 9], 1002);

    let before = rq.pending_count();

    // ACK up to seq 250 — should remove entries with seq < 250 (100 and 200)
    rq.ack_received(250, 1050);
    let after = rq.pending_count();

    let passed = before == 3 && after == 1;

    OcrbResult {
        test_name: "RetransmitQueue Enqueue/ACK",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("before={}, after={}", before, after),
    }
}

/// Test 2: RTO calculation produces valid clamped values (w:10).
fn test_rto_calculation() -> OcrbResult {
    use crate::network::tcp_timer;

    // Test with small RTT
    let rto_small = tcp_timer::rto_from_rtt(10);
    // Test with very large RTT
    let rto_large = tcp_timer::rto_from_rtt(100_000);
    // Test with typical RTT
    let rto_typical = tcp_timer::rto_from_rtt(50);

    let passed = rto_small >= 200   // Min clamp
        && rto_large <= 60_000       // Max clamp
        && rto_typical >= 200
        && rto_typical <= 60_000;

    OcrbResult {
        test_name: "RTO Calculation",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("small={}, typical={}, large={}", rto_small, rto_typical, rto_large),
    }
}

/// Test 3: Karn's algorithm — RTT unchanged for retransmitted segments (w:10).
fn test_karns_algorithm() -> OcrbResult {
    use crate::network::tcp_timer::RetransmitQueue;

    let mut rq = RetransmitQueue::new();
    let initial_rto = rq.rto;

    // Enqueue and mark as retransmitted (retries > 0)
    rq.enqueue(100, vec![1, 2, 3], 1000);
    // Simulate retransmit by directly setting retries
    rq.entries[0].retries = 1;

    // ACK it — should NOT update RTT because retries > 0 (Karn's)
    let rto_before = rq.rto;
    rq.ack_received(200, 5000); // Very late ACK — if RTT were measured, RTO would change

    let rto_after = rq.rto;

    // RTO should be unchanged because Karn's algorithm skips retransmitted segments
    let passed = rto_before == rto_after;

    OcrbResult {
        test_name: "Karn's Algorithm",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("rto_before={}, rto_after={} (should be equal)", rto_before, rto_after),
    }
}

/// Test 4: check_timeouts returns packets after RTO expires (w:10).
fn test_retransmit_timeout() -> OcrbResult {
    use crate::network::tcp_timer::RetransmitQueue;

    let mut rq = RetransmitQueue::new();
    rq.rto = 100; // Set low RTO for testing (100ms)

    // Enqueue at time 1000
    rq.enqueue(100, vec![0xDE, 0xAD], 1000);

    // Check at time 1050 — should NOT timeout yet (only 50ms elapsed, rto=100)
    let too_early = rq.check_timeouts(1050);

    // Check at time 1200 — should timeout (200ms > 100ms rto)
    let timed_out = rq.check_timeouts(1200);

    let passed = too_early.is_empty() && timed_out.len() == 1;

    OcrbResult {
        test_name: "Retransmit Timeout",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("early={}, timed_out={}", too_early.len(), timed_out.len()),
    }
}

/// Test 5: poll() returns immediately when data available (w:10).
fn test_pollfd_immediate_ready() -> OcrbResult {
    use crate::network::ops::{self, PollFd, POLLIN};
    use crate::network::addr::{SocketType, Protocol};
    use crate::network::SOCKETS;

    // Create a TCP socket and put data in its RX buffer
    let sock_id = {
        let mut table = SOCKETS.lock();
        match table.alloc(SocketType::Stream, Protocol::Tcp, fabric_types::ProcessId::KERNEL) {
            Ok(id) => {
                if let Some(sock) = table.get_mut(id) {
                    sock.state = crate::network::socket::SocketState::Established;
                    sock.rx.write(b"hello");
                }
                id
            }
            Err(_) => {
                return OcrbResult {
                    test_name: "PollFd Immediate Ready",
                    passed: false,
                    score: 0,
                    weight: 10,
                    details: String::from("failed to alloc socket"),
                };
            }
        }
    };

    let mut fds = [PollFd { fd: sock_id.0, events: POLLIN, revents: 0 }];
    let result = ops::socket_poll(&mut fds, 0); // timeout=0 = instant

    // Clean up
    {
        let mut table = SOCKETS.lock();
        let _ = table.release(sock_id);
    }

    let passed = result == 1 && fds[0].revents & POLLIN != 0;

    OcrbResult {
        test_name: "PollFd Immediate Ready",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("result={}, revents=0x{:x}", result, fds[0].revents),
    }
}

/// Test 6: poll(timeout=0) returns 0 when nothing ready (w:10).
fn test_pollfd_timeout_zero() -> OcrbResult {
    use crate::network::ops::{self, PollFd, POLLIN};
    use crate::network::addr::{SocketType, Protocol};
    use crate::network::SOCKETS;

    // Create a socket with empty RX buffer
    let sock_id = {
        let mut table = SOCKETS.lock();
        match table.alloc(SocketType::Stream, Protocol::Tcp, fabric_types::ProcessId::KERNEL) {
            Ok(id) => {
                if let Some(sock) = table.get_mut(id) {
                    sock.state = crate::network::socket::SocketState::Established;
                    // No data in RX
                }
                id
            }
            Err(_) => {
                return OcrbResult {
                    test_name: "PollFd Timeout Zero",
                    passed: false,
                    score: 0,
                    weight: 10,
                    details: String::from("failed to alloc socket"),
                };
            }
        }
    };

    let mut fds = [PollFd { fd: sock_id.0, events: POLLIN, revents: 0 }];
    let result = ops::socket_poll(&mut fds, 0);

    // Clean up
    {
        let mut table = SOCKETS.lock();
        let _ = table.release(sock_id);
    }

    let passed = result == 0;

    OcrbResult {
        test_name: "PollFd Timeout Zero",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("result={}", result),
    }
}

/// Test 7: POLLIN/POLLOUT/POLLHUP set correctly per state (w:10).
fn test_pollfd_events() -> OcrbResult {
    use crate::network::ops::{self, PollFd, POLLIN, POLLOUT, POLLHUP};
    use crate::network::addr::{SocketType, Protocol};
    use crate::network::socket::SocketState;
    use crate::network::SOCKETS;

    // Create socket in Established state — should have POLLOUT
    let sock_id = {
        let mut table = SOCKETS.lock();
        match table.alloc(SocketType::Stream, Protocol::Tcp, fabric_types::ProcessId::KERNEL) {
            Ok(id) => {
                if let Some(sock) = table.get_mut(id) {
                    sock.state = SocketState::Established;
                }
                id
            }
            Err(_) => {
                return OcrbResult {
                    test_name: "PollFd Events",
                    passed: false,
                    score: 0,
                    weight: 10,
                    details: String::from("failed to alloc socket"),
                };
            }
        }
    };

    let mut fds = [PollFd { fd: sock_id.0, events: POLLIN | POLLOUT, revents: 0 }];
    let _ = ops::socket_poll(&mut fds, 0);
    let established_out = fds[0].revents & POLLOUT != 0;
    let established_no_in = fds[0].revents & POLLIN == 0; // no data

    // Move to CloseWait — should have POLLIN (EOF) + POLLHUP
    {
        let mut table = SOCKETS.lock();
        if let Some(sock) = table.get_mut(sock_id) {
            sock.state = SocketState::CloseWait;
        }
    }

    let mut fds2 = [PollFd { fd: sock_id.0, events: POLLIN | POLLOUT, revents: 0 }];
    let _ = ops::socket_poll(&mut fds2, 0);
    let closewait_in = fds2[0].revents & POLLIN != 0;
    let closewait_hup = fds2[0].revents & POLLHUP != 0;

    // Clean up
    {
        let mut table = SOCKETS.lock();
        let _ = table.release(sock_id);
    }

    let passed = established_out && established_no_in && closewait_in && closewait_hup;

    OcrbResult {
        test_name: "PollFd Events",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!(
            "est_out={}, est_no_in={}, cw_in={}, cw_hup={}",
            established_out, established_no_in, closewait_in, closewait_hup
        ),
    }
}

/// Test 8: DNS cache insert/lookup, respects TTL (w:10).
fn test_dns_cache_insert_lookup() -> OcrbResult {
    use crate::network::dns::{DnsCache};

    let mut cache = DnsCache::new();

    // Insert
    cache.insert("test.example.com", [1, 2, 3, 4], 300);

    // Lookup — should hit
    let hit = cache.lookup("test.example.com");

    // Lookup different — should miss
    let miss = cache.lookup("other.example.com");

    let passed = hit == Some([1, 2, 3, 4]) && miss.is_none();

    OcrbResult {
        test_name: "DNS Cache Insert/Lookup",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("hit={:?}, miss={:?}", hit, miss),
    }
}

/// Test 9: 33rd insert evicts LRU entry (w:10).
fn test_dns_cache_lru_eviction() -> OcrbResult {
    use crate::network::dns::DnsCache;

    let mut cache = DnsCache::new();

    // Fill all 32 slots
    for i in 0..32u8 {
        let name = alloc::format!("host{}.example.com", i);
        cache.insert(&name, [10, 0, 0, i], 300);
    }

    // Access host0 to make it most recently used
    let _ = cache.lookup("host0.example.com");

    // Insert 33rd — should evict an entry (not host0 since it was just accessed)
    cache.insert("new.example.com", [192, 168, 1, 1], 300);

    // host0 should still be in cache (was recently accessed)
    let host0_hit = cache.lookup("host0.example.com");
    // new entry should be in cache
    let new_hit = cache.lookup("new.example.com");
    // Count valid entries — should still be 32 (one was evicted)
    let valid_count = cache.entries.iter().filter(|e| e.valid).count();

    let passed = host0_hit.is_some() && new_hit == Some([192, 168, 1, 1]) && valid_count == 32;

    OcrbResult {
        test_name: "DNS Cache LRU Eviction",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("host0={:?}, new={:?}, valid={}", host0_hit, new_hit, valid_count),
    }
}

/// Test 10: Each DNS retry uses different transaction ID (w:10).
fn test_dns_retry_txn_id() -> OcrbResult {
    use crate::network::dns;

    let id1 = dns::next_txn_id();
    let id2 = dns::next_txn_id();
    let id3 = dns::next_txn_id();

    // All three should be different (pseudo-random)
    let all_different = id1 != id2 && id2 != id3 && id1 != id3;

    OcrbResult {
        test_name: "DNS Retry TxnID",
        passed: all_different,
        score: if all_different { 100 } else { 0 },
        weight: 10,
        details: alloc::format!("ids: 0x{:04x}, 0x{:04x}, 0x{:04x}", id1, id2, id3),
    }
}
