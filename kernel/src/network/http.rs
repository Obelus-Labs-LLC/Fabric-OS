//! HTTP Client — minimal HTTP/1.1 GET request builder.
//!
//! Constructs HTTP request strings for fetching web pages.
//! Actual transport (TCP connect + send/recv) is deferred to Phase 12
//! when the existing TCP state machine is wired to the real NIC.

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// Default HTTP port.
pub const HTTP_PORT: u16 = 80;

/// Build an HTTP/1.1 GET request string.
///
/// # Example
/// ```
/// let req = build_get_request("example.com", "/index.html");
/// // "GET /index.html HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
/// ```
pub fn build_get_request(host: &str, path: &str) -> String {
    let mut request = String::with_capacity(256);
    request.push_str("GET ");
    request.push_str(path);
    request.push_str(" HTTP/1.1\r\n");
    request.push_str("Host: ");
    request.push_str(host);
    request.push_str("\r\n");
    request.push_str("Connection: close\r\n");
    request.push_str("User-Agent: FabricOS/0.7.0\r\n");
    request.push_str("\r\n");
    request
}

/// Parse an HTTP response status line.
///
/// Returns (status_code, reason_phrase) if parseable.
pub fn parse_status_line(data: &[u8]) -> Option<(u16, &[u8])> {
    // Look for "HTTP/1.x NNN reason\r\n"
    if data.len() < 12 {
        return None;
    }

    // Check "HTTP/1."
    if &data[0..7] != b"HTTP/1." {
        return None;
    }

    // Skip version digit and space
    if data[8] != b' ' {
        return None;
    }

    // Parse 3-digit status code
    let code_bytes = &data[9..12];
    let code = parse_digits(code_bytes)?;

    // Find end of status line
    let reason_start = if data.len() > 13 && data[12] == b' ' { 13 } else { 12 };
    let mut reason_end = reason_start;
    while reason_end < data.len() {
        if data[reason_end] == b'\r' || data[reason_end] == b'\n' {
            break;
        }
        reason_end += 1;
    }

    Some((code, &data[reason_start..reason_end]))
}

/// Find the body in an HTTP response (after \r\n\r\n).
pub fn find_body(data: &[u8]) -> Option<&[u8]> {
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(&data[i + 4..]);
        }
    }
    None
}

/// Parse ASCII digits to u16.
fn parse_digits(data: &[u8]) -> Option<u16> {
    let mut val: u16 = 0;
    for &b in data {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val * 10 + (b - b'0') as u16;
    }
    Some(val)
}

/// Build raw HTTP GET request bytes (for direct TCP send).
pub fn build_get_request_bytes(host: &str, path: &str) -> Vec<u8> {
    build_get_request(host, path).into_bytes()
}
