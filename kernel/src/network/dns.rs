//! DNS — Minimal DNS stub resolver for A-record queries.
//!
//! Builds DNS query packets and parses responses.
//! Uses QEMU user-mode DNS proxy at 10.0.2.3:53.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU16, Ordering};
use crate::serial_println;

/// DNS server (QEMU user-mode DNS proxy).
pub const DNS_SERVER_IP: [u8; 4] = [10, 0, 2, 3];
pub const DNS_SERVER_PORT: u16 = 53;

/// DNS header (12 bytes).
#[derive(Clone, Debug)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qdcount: u16,
    pub ancount: u16,
    pub nscount: u16,
    pub arcount: u16,
}

impl DnsHeader {
    /// Standard recursive query header.
    pub fn query(id: u16) -> Self {
        Self {
            id,
            flags: 0x0100, // RD (Recursion Desired)
            qdcount: 1,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }

    /// Serialize to 12 bytes (big-endian).
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0] = (self.id >> 8) as u8;
        buf[1] = self.id as u8;
        buf[2] = (self.flags >> 8) as u8;
        buf[3] = self.flags as u8;
        buf[4] = (self.qdcount >> 8) as u8;
        buf[5] = self.qdcount as u8;
        buf[6] = (self.ancount >> 8) as u8;
        buf[7] = self.ancount as u8;
        buf[8] = (self.nscount >> 8) as u8;
        buf[9] = self.nscount as u8;
        buf[10] = (self.arcount >> 8) as u8;
        buf[11] = self.arcount as u8;
        buf
    }

    /// Parse from 12 bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        Some(Self {
            id: (data[0] as u16) << 8 | data[1] as u16,
            flags: (data[2] as u16) << 8 | data[3] as u16,
            qdcount: (data[4] as u16) << 8 | data[5] as u16,
            ancount: (data[6] as u16) << 8 | data[7] as u16,
            nscount: (data[8] as u16) << 8 | data[9] as u16,
            arcount: (data[10] as u16) << 8 | data[11] as u16,
        })
    }
}

/// DNS record types.
const DNS_TYPE_A: u16 = 1;     // IPv4 address
const DNS_CLASS_IN: u16 = 1;   // Internet

/// Encode a hostname into DNS wire format (length-prefixed labels).
///
/// e.g., "example.com" -> [7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 3, 'c', 'o', 'm', 0]
fn encode_hostname(hostname: &str) -> Vec<u8> {
    let mut encoded = Vec::new();
    for label in hostname.split('.') {
        if label.is_empty() {
            continue;
        }
        encoded.push(label.len() as u8);
        encoded.extend_from_slice(label.as_bytes());
    }
    encoded.push(0); // Root label terminator
    encoded
}

/// Build a DNS A-record query for the given hostname.
///
/// Returns the complete DNS packet bytes ready to send over UDP.
pub fn build_query(hostname: &str) -> Vec<u8> {
    let header = DnsHeader::query(0x1234); // Fixed ID for simplicity
    let name = encode_hostname(hostname);

    let mut packet = Vec::with_capacity(12 + name.len() + 4);
    packet.extend_from_slice(&header.to_bytes());
    packet.extend_from_slice(&name);

    // QTYPE = A (1)
    packet.push(0);
    packet.push(DNS_TYPE_A as u8);
    // QCLASS = IN (1)
    packet.push(0);
    packet.push(DNS_CLASS_IN as u8);

    packet
}

/// Parse a DNS response and extract the first A record (IPv4 address).
///
/// Returns the IPv4 address as [u8; 4], or None if no A record found.
pub fn parse_response(data: &[u8]) -> Option<[u8; 4]> {
    let header = DnsHeader::parse(data)?;

    // Check response flag (bit 15 of flags = QR)
    if header.flags & 0x8000 == 0 {
        return None; // Not a response
    }

    // Check for errors (RCODE in bits 0-3)
    if header.flags & 0x000F != 0 {
        serial_println!("[DNS] Response error: RCODE={}", header.flags & 0x000F);
        return None;
    }

    if header.ancount == 0 {
        return None;
    }

    // Skip header (12 bytes)
    let mut offset = 12;

    // Skip question section (qdcount questions)
    for _ in 0..header.qdcount {
        // Skip QNAME (label format, terminated by 0 or pointer)
        offset = skip_dns_name(data, offset)?;
        // Skip QTYPE (2) + QCLASS (2)
        offset += 4;
        if offset > data.len() {
            return None;
        }
    }

    // Parse answer section — look for first A record
    for _ in 0..header.ancount {
        if offset >= data.len() {
            return None;
        }

        // Skip NAME (may be a pointer)
        offset = skip_dns_name(data, offset)?;

        if offset + 10 > data.len() {
            return None;
        }

        let rtype = (data[offset] as u16) << 8 | data[offset + 1] as u16;
        let _rclass = (data[offset + 2] as u16) << 8 | data[offset + 3] as u16;
        let _ttl = (data[offset + 4] as u32) << 24
            | (data[offset + 5] as u32) << 16
            | (data[offset + 6] as u32) << 8
            | data[offset + 7] as u32;
        let rdlength = (data[offset + 8] as u16) << 8 | data[offset + 9] as u16;
        offset += 10;

        if rtype == DNS_TYPE_A && rdlength == 4 {
            if offset + 4 <= data.len() {
                let ip = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]];
                return Some(ip);
            }
        }

        offset += rdlength as usize;
    }

    None
}

/// Skip a DNS name (handling both labels and compression pointers).
fn skip_dns_name(data: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        if offset >= data.len() {
            return None;
        }
        let len = data[offset];
        if len == 0 {
            // Root label — end of name
            return Some(offset + 1);
        }
        if len & 0xC0 == 0xC0 {
            // Compression pointer (2 bytes)
            return Some(offset + 2);
        }
        // Regular label
        offset += 1 + len as usize;
    }
}

// ============================================================================
// DNS Cache — 32-entry LRU with TTL
// ============================================================================

/// Maximum cache entries.
const DNS_CACHE_SIZE: usize = 32;

/// A single DNS cache entry.
pub struct DnsCacheEntry {
    pub hostname: [u8; 64],
    pub hostname_len: usize,
    pub ip: [u8; 4],
    pub expiry_tick: u64,
    pub last_access: u64,
    pub valid: bool,
}

impl DnsCacheEntry {
    const fn empty() -> Self {
        Self {
            hostname: [0u8; 64],
            hostname_len: 0,
            ip: [0; 4],
            expiry_tick: 0,
            last_access: 0,
            valid: false,
        }
    }
}

/// DNS cache with LRU eviction.
pub struct DnsCache {
    pub entries: [DnsCacheEntry; DNS_CACHE_SIZE],
    access_seq: u64,
}

impl DnsCache {
    pub const fn new() -> Self {
        const EMPTY: DnsCacheEntry = DnsCacheEntry::empty();
        Self {
            entries: [EMPTY; DNS_CACHE_SIZE],
            access_seq: 0,
        }
    }

    /// Get the next monotonic access sequence number.
    fn next_seq(&mut self) -> u64 {
        self.access_seq += 1;
        self.access_seq
    }

    /// Look up a hostname in the cache. Returns IP if found and not expired.
    pub fn lookup(&mut self, hostname: &str) -> Option<[u8; 4]> {
        let now = crate::x86::idt::tick_count();
        let seq = self.next_seq();
        let hb = hostname.as_bytes();

        for entry in &mut self.entries {
            if entry.valid
                && entry.hostname_len == hb.len()
                && &entry.hostname[..entry.hostname_len] == hb
            {
                if now < entry.expiry_tick {
                    entry.last_access = seq;
                    return Some(entry.ip);
                } else {
                    // Expired
                    entry.valid = false;
                    return None;
                }
            }
        }
        None
    }

    /// Insert a hostname→IP mapping with TTL in seconds.
    pub fn insert(&mut self, hostname: &str, ip: [u8; 4], ttl_secs: u32) {
        let now = crate::x86::idt::tick_count();
        let seq = self.next_seq();
        let hb = hostname.as_bytes();
        let hl = hb.len().min(64);

        // First check if we can replace an existing entry for the same hostname
        for entry in &mut self.entries {
            if entry.valid
                && entry.hostname_len == hl
                && &entry.hostname[..hl] == &hb[..hl]
            {
                entry.ip = ip;
                entry.expiry_tick = now + (ttl_secs as u64) * 1000;
                entry.last_access = seq;
                return;
            }
        }

        // Find an empty slot or evict LRU
        let slot = self.find_slot();
        let entry = &mut self.entries[slot];
        entry.hostname[..hl].copy_from_slice(&hb[..hl]);
        entry.hostname_len = hl;
        entry.ip = ip;
        entry.expiry_tick = now + (ttl_secs as u64) * 1000;
        entry.last_access = seq;
        entry.valid = true;
    }

    /// Find an empty slot or the LRU entry to evict.
    fn find_slot(&self) -> usize {
        // First try empty slots
        for (i, entry) in self.entries.iter().enumerate() {
            if !entry.valid {
                return i;
            }
        }
        // Evict LRU
        let mut min_access = u64::MAX;
        let mut min_idx = 0;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.last_access < min_access {
                min_access = entry.last_access;
                min_idx = i;
            }
        }
        min_idx
    }
}

/// Global DNS cache.
pub static DNS_CACHE: Mutex<DnsCache> = Mutex::new(DnsCache::new());

/// Transaction ID counter for pseudo-random IDs.
static TXN_COUNTER: AtomicU16 = AtomicU16::new(0x1234);

/// Generate a pseudo-random transaction ID.
pub fn next_txn_id() -> u16 {
    let tick = crate::x86::idt::tick_count() as u16;
    let counter = TXN_COUNTER.fetch_add(0x4321, Ordering::Relaxed);
    tick ^ counter
}

/// Resolve a hostname to an IPv4 address via real DNS over the NIC.
///
/// Builds a DNS A-record query, wraps it in UDP/IP, sends via NIC,
/// polls for response, and parses the A record from the reply.
///
/// Uses raw UDP packet (bypasses socket layer) to avoid lock issues.
/// Response is captured by nic_dispatch into DNS_RESPONSE buffer.
pub fn dns_resolve(hostname: &str) -> Option<[u8; 4]> {
    use super::udp;
    use super::addr::{Ipv4Addr, SocketAddr};
    use super::nic_dispatch;

    // Check cache first
    {
        let mut cache = DNS_CACHE.lock();
        if let Some(ip) = cache.lookup(hostname) {
            serial_println!(
                "[DNS] Cache hit: '{}' -> {}.{}.{}.{}",
                hostname, ip[0], ip[1], ip[2], ip[3]
            );
            return Some(ip);
        }
    }

    serial_println!("[DNS] Query for '{}' -> {}.{}.{}.{}:{}",
        hostname,
        DNS_SERVER_IP[0], DNS_SERVER_IP[1], DNS_SERVER_IP[2], DNS_SERVER_IP[3],
        DNS_SERVER_PORT);

    // Retry configuration: 3 attempts with increasing poll windows
    let poll_iterations: [u32; 3] = [50_000, 100_000, 200_000];

    for attempt in 0..3u8 {
        let txn_id = next_txn_id();

        // Build query with this attempt's transaction ID
        let header = DnsHeader::query(txn_id);
        let name = encode_hostname(hostname);
        let mut query = Vec::with_capacity(12 + name.len() + 4);
        query.extend_from_slice(&header.to_bytes());
        query.extend_from_slice(&name);
        query.push(0); query.push(DNS_TYPE_A as u8);
        query.push(0); query.push(DNS_CLASS_IN as u8);

        // Clear DNS response buffer
        {
            let mut dns_buf = nic_dispatch::DNS_RESPONSE.lock();
            dns_buf.clear();
        }

        // Build raw UDP/IP packet
        let src = SocketAddr::new(Ipv4Addr(nic_dispatch::GUEST_IP), 12345);
        let dst = SocketAddr::new(Ipv4Addr(DNS_SERVER_IP), DNS_SERVER_PORT);

        let mut udp_buf = [0u8; 1500];
        let udp_len = udp::build_udp_packet(src, dst, &query, &mut udp_buf);

        if udp_len == 0 {
            serial_println!("[DNS] Failed to build UDP packet");
            return None;
        }

        // Send via NIC
        nic_dispatch::transmit_ip(&udp_buf[..udp_len]);

        if attempt > 0 {
            serial_println!("[DNS] Retry {} for '{}' (txn=0x{:04x})", attempt + 1, hostname, txn_id);
        }

        // Poll for DNS response
        let max_iters = poll_iterations[attempt as usize];
        for _ in 0..max_iters {
            nic_dispatch::nic_receive_one();

            let dns_buf = nic_dispatch::DNS_RESPONSE.lock();
            if dns_buf.ready {
                // Verify transaction ID matches
                if dns_buf.len >= 2 {
                    let resp_id = (dns_buf.data[0] as u16) << 8 | dns_buf.data[1] as u16;
                    if resp_id != txn_id {
                        drop(dns_buf);
                        continue; // Wrong transaction, keep polling
                    }
                }

                let result = parse_response(&dns_buf.data[..dns_buf.len]);
                if let Some(ip) = result {
                    serial_println!(
                        "[DNS] Resolved '{}' -> {}.{}.{}.{}",
                        hostname, ip[0], ip[1], ip[2], ip[3]
                    );
                    // Insert into cache (default TTL 300s)
                    drop(dns_buf);
                    DNS_CACHE.lock().insert(hostname, ip, 300);
                    return Some(ip);
                } else {
                    serial_println!("[DNS] No A record in response for '{}'", hostname);
                    return None;
                }
            }
            drop(dns_buf);

            core::hint::spin_loop();
        }
    }

    serial_println!("[DNS] Timeout resolving '{}' (3 attempts)", hostname);
    None
}
