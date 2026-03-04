//! Socket API operations.
//!
//! High-level socket operations: create, bind, connect, listen, accept,
//! send, recv, shutdown, close. These are called from syscall handlers.

#![allow(dead_code)]

use fabric_types::ProcessId;
use super::addr::{Ipv4Addr, SocketAddr, SocketType, Protocol, AddressFamily};
use super::socket::{SocketId, SocketState, SocketError, MAX_BACKLOG};
use super::ip::Ipv4Header;
use super::tcp::{self, TcpHeader, SYN, ACK, FIN};
use super::udp;
use super::loopback::LOOPBACK_MTU;
use super::nic_dispatch;
use super::SOCKETS;
use super::LOOPBACK;

/// Create a new socket. Returns SocketId.
pub fn socket_create(
    sock_type: SocketType,
    protocol: Protocol,
    owner: ProcessId,
) -> Result<SocketId, SocketError> {
    let mut table = SOCKETS.lock();
    table.alloc(sock_type, protocol, owner)
}

/// Bind a socket to a local address.
pub fn socket_bind(id: SocketId, addr: SocketAddr) -> Result<(), SocketError> {
    let mut table = SOCKETS.lock();

    // Check if port is already in use
    let protocol = match table.get(id) {
        Some(s) => s.protocol,
        None => return Err(SocketError::NotFound),
    };

    if addr.port != 0 && table.port_in_use(addr.port, protocol) {
        // Check it's not us
        let our_port = table.get(id).map(|s| s.local_addr.port).unwrap_or(0);
        if our_port != addr.port {
            return Err(SocketError::AddressInUse);
        }
    }

    let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;
    if sock.state != SocketState::Closed {
        return Err(SocketError::InvalidState);
    }

    sock.local_addr = addr;
    sock.state = SocketState::Bound;
    Ok(())
}

/// Connect a socket to a remote address (TCP or UDP).
pub fn socket_connect(id: SocketId, remote: SocketAddr) -> Result<(), SocketError> {
    // Phase 1: Set up connection state and build SYN packet
    let (protocol, local, pkt_buf, pkt_len) = {
        let mut table = SOCKETS.lock();
        let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;

        // For UDP, connect just sets the remote address
        if sock.protocol == Protocol::Udp {
            sock.remote_addr = remote;
            if sock.state == SocketState::Closed {
                sock.state = SocketState::Bound;
            }
            return Ok(());
        }

        // TCP: send SYN
        if sock.state != SocketState::Closed && sock.state != SocketState::Bound {
            return Err(SocketError::InvalidState);
        }

        // Auto-bind if not bound — compute ephemeral port before mutable borrow
        let needs_port = sock.local_addr.port == 0;
        let needs_addr = sock.local_addr.addr.is_unspecified();
        let ephemeral = if needs_port {
            allocate_ephemeral_port(&table)
        } else {
            0
        };
        // Re-borrow sock after immutable borrow of table
        let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;
        // Use GUEST_IP for NIC connections, LOOPBACK for loopback
        let bind_addr = if nic_dispatch::is_loopback(&remote.addr.0) {
            Ipv4Addr::LOOPBACK
        } else {
            Ipv4Addr(nic_dispatch::GUEST_IP)
        };
        if needs_port {
            sock.local_addr = SocketAddr::new(bind_addr, ephemeral);
        }
        if needs_addr {
            sock.local_addr.addr = bind_addr;
        }

        sock.remote_addr = remote;
        let isn = tcp::next_isn();
        sock.send_seq = isn;
        sock.state = SocketState::SynSent;

        let local = sock.local_addr;
        let protocol = sock.protocol;

        let mut buf = [0u8; LOOPBACK_MTU];
        let len = tcp::build_tcp_packet(
            local, remote, isn, 0, SYN, 4096, &[], &mut buf,
        );

        (protocol, local, buf, len)
    };
    // SOCKETS dropped here

    // Phase 2: Transmit SYN packet (routes to LOOPBACK or NIC)
    if pkt_len > 0 {
        crate::serial_println!("[TCP] SYN sent to {}.{}.{}.{}:{} (local port {}, pkt_len={})",
            remote.addr.0[0], remote.addr.0[1], remote.addr.0[2], remote.addr.0[3],
            remote.port, local.port, pkt_len);
        nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
    }

    // Phase 3: Deliver pending packets (handshake)
    // For NIC connections, need more iterations to allow for wire latency
    let is_nic = !nic_dispatch::is_loopback(&remote.addr.0);
    let max_iters = if is_nic { 100_000 } else { 10 };

    for i in 0..max_iters {
        deliver_one();

        // Check if we're established
        let table = SOCKETS.lock();
        if let Some(sock) = table.get(id) {
            if sock.state == SocketState::Established {
                crate::serial_println!("[TCP] Established after {} iterations", i);
                return Ok(());
            }
        }

        if is_nic {
            core::hint::spin_loop();
        }
    }

    // If we get here, handshake didn't complete
    crate::serial_println!("[TCP] Connect timeout after {} iterations", max_iters);
    Err(SocketError::ConnectionRefused)
}

/// Listen on a bound socket (TCP only).
pub fn socket_listen(id: SocketId) -> Result<(), SocketError> {
    let mut table = SOCKETS.lock();
    let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;

    if sock.protocol != Protocol::Tcp {
        return Err(SocketError::InvalidArgument);
    }
    if sock.state != SocketState::Bound {
        return Err(SocketError::NotBound);
    }

    sock.state = SocketState::Listen;
    sock.backlog_count = 0;
    Ok(())
}

/// Accept a connection from a listening socket. Returns the new connection SocketId.
pub fn socket_accept(listener_id: SocketId) -> Result<SocketId, SocketError> {
    let mut table = SOCKETS.lock();
    let listener = table.get_mut(listener_id).ok_or(SocketError::NotFound)?;

    if listener.state != SocketState::Listen {
        return Err(SocketError::InvalidState);
    }

    if listener.backlog_count == 0 {
        return Err(SocketError::WouldBlock);
    }

    // Pop the first connection from backlog
    let conn_id = listener.backlog[0];
    for i in 1..listener.backlog_count {
        listener.backlog[i - 1] = listener.backlog[i];
    }
    listener.backlog_count -= 1;

    // Deliver pending to complete the handshake if needed
    drop(table);

    for _ in 0..10 {
        deliver_one();
        let table = SOCKETS.lock();
        if let Some(conn) = table.get(conn_id) {
            if conn.state == SocketState::Established {
                return Ok(conn_id);
            }
        }
    }

    Ok(conn_id)
}

/// Send data on a connected socket (TCP or UDP).
pub fn socket_send(id: SocketId, data: &[u8]) -> Result<usize, SocketError> {
    // Phase 1: Build packet under SOCKETS lock
    let (pkt_buf, pkt_len) = {
        let mut table = SOCKETS.lock();
        let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;

        match sock.protocol {
            Protocol::Udp => {
                if sock.remote_addr.is_unspecified() {
                    return Err(SocketError::NotConnected);
                }
                let src = sock.local_addr;
                let dst = sock.remote_addr;
                let mut buf = [0u8; LOOPBACK_MTU];
                let len = udp::build_udp_packet(src, dst, data, &mut buf);
                (buf, len)
            }
            Protocol::Tcp => {
                if sock.state != SocketState::Established {
                    return Err(SocketError::NotConnected);
                }
                let src = sock.local_addr;
                let dst = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                sock.send_seq = seq.wrapping_add(data.len() as u32);

                let mut buf = [0u8; LOOPBACK_MTU];
                let len = tcp::build_tcp_packet(
                    src, dst, seq, ack, ACK, 4096, data, &mut buf,
                );
                (buf, len)
            }
        }
    };
    // SOCKETS dropped here

    // Phase 2: Transmit the packet (routes to LOOPBACK or NIC)
    if pkt_len > 0 {
        nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);

        // Phase 3: Deliver pending
        deliver_one();
    }

    Ok(data.len())
}

/// Send data to a specific address (UDP only, for unconnected sockets).
pub fn socket_sendto(
    id: SocketId,
    data: &[u8],
    dst: SocketAddr,
) -> Result<usize, SocketError> {
    let (pkt_buf, pkt_len) = {
        let table = SOCKETS.lock();
        let sock = table.get(id).ok_or(SocketError::NotFound)?;

        if sock.protocol != Protocol::Udp {
            return Err(SocketError::InvalidArgument);
        }

        let src = sock.local_addr;
        let mut buf = [0u8; LOOPBACK_MTU];
        let len = udp::build_udp_packet(src, dst, data, &mut buf);
        (buf, len)
    };

    if pkt_len > 0 {
        nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
        deliver_one();
    }

    Ok(data.len())
}

/// Receive data from a socket.
/// Returns the number of bytes read.
pub fn socket_recv(id: SocketId, buf: &mut [u8]) -> Result<usize, SocketError> {
    // First try to deliver any pending packets
    deliver_one();

    let mut table = SOCKETS.lock();
    let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;

    if sock.protocol == Protocol::Udp {
        // UDP recv: read the framing [src_port:2][src_ip:4][len:2][data:N]
        if sock.rx.is_empty() {
            return Err(SocketError::WouldBlock);
        }
        let mut hdr = [0u8; 8]; // 2 + 4 + 2
        let read = sock.rx.read(&mut hdr);
        if read < 8 {
            return Err(SocketError::WouldBlock);
        }
        let data_len = u16::from_be_bytes([hdr[6], hdr[7]]) as usize;
        let to_read = data_len.min(buf.len());
        let n = sock.rx.read(&mut buf[..to_read]);
        // Discard any remaining data that didn't fit
        if data_len > to_read {
            sock.rx.discard(data_len - to_read);
        }
        Ok(n)
    } else {
        // TCP recv: read directly from RX buffer
        if sock.rx.is_empty() {
            // Check if connection is closing
            if sock.state == SocketState::CloseWait
                || sock.state == SocketState::Closed
                || sock.state == SocketState::TimeWait
            {
                return Ok(0); // EOF
            }
            return Err(SocketError::WouldBlock);
        }
        let n = sock.rx.read(buf);
        Ok(n)
    }
}

/// Shutdown a socket (initiate close for TCP).
pub fn socket_shutdown(id: SocketId) -> Result<(), SocketError> {
    // Phase 1: Build FIN packet under SOCKETS lock
    let (pkt_buf, pkt_len) = {
        let mut table = SOCKETS.lock();
        let sock = table.get_mut(id).ok_or(SocketError::NotFound)?;

        if sock.protocol == Protocol::Udp {
            sock.state = SocketState::Closed;
            return Ok(());
        }

        // TCP: send FIN
        match sock.state {
            SocketState::Established => {
                let src = sock.local_addr;
                let dst = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;
                sock.send_seq = seq.wrapping_add(1); // FIN consumes one seq
                sock.state = SocketState::FinWait1;

                let mut buf = [0u8; LOOPBACK_MTU];
                let len = tcp::build_tcp_packet(
                    src, dst, seq, ack, FIN | ACK, 4096, &[], &mut buf,
                );
                (buf, len)
            }
            SocketState::CloseWait => {
                let src = sock.local_addr;
                let dst = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;
                sock.send_seq = seq.wrapping_add(1);
                sock.state = SocketState::LastAck;

                let mut buf = [0u8; LOOPBACK_MTU];
                let len = tcp::build_tcp_packet(
                    src, dst, seq, ack, FIN | ACK, 4096, &[], &mut buf,
                );
                (buf, len)
            }
            _ => return Err(SocketError::InvalidState),
        }
    };
    // SOCKETS dropped here

    // Phase 2: Transmit FIN
    if pkt_len > 0 {
        nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);

        // Phase 3: Deliver FIN + process ACK/FIN responses
        for _ in 0..10 {
            deliver_one();
        }
    }

    Ok(())
}

/// Close a socket — release resources.
pub fn socket_close(id: SocketId) -> Result<(), SocketError> {
    // If TCP and established, shutdown first
    {
        let table = SOCKETS.lock();
        if let Some(sock) = table.get(id) {
            if sock.protocol == Protocol::Tcp
                && (sock.state == SocketState::Established
                    || sock.state == SocketState::CloseWait)
            {
                drop(table);
                let _ = socket_shutdown(id);
            }
        }
    }

    let mut table = SOCKETS.lock();
    table.release(id)
}

/// Public wrapper for deliver_one (used by TLS handshake).
pub fn deliver_one_public() {
    deliver_one();
}

/// Deliver one pending packet from either loopback or NIC.
/// Dequeues from LOOPBACK, then also polls NIC RX.
fn deliver_one() {
    // Phase 1: Try LOOPBACK first
    let packet = {
        let mut lo = LOOPBACK.lock();
        lo.dequeue()
    };
    // LOOPBACK dropped here

    if let Some((pkt_buf, pkt_len)) = packet {
        // Phase 2: Parse IP header and dispatch
        let ip_hdr = match Ipv4Header::from_bytes(&pkt_buf[..pkt_len]) {
            Some(h) => h,
            None => return,
        };

        let ip_payload = &pkt_buf[Ipv4Header::SIZE..pkt_len];

        // Phase 3: Lock SOCKETS and deliver to protocol handler
        let mut table = SOCKETS.lock();

        match ip_hdr.protocol {
            17 => udp::udp_receive_packet(&ip_hdr, ip_payload, &mut table),
            6 => tcp::tcp_receive_packet(&ip_hdr, ip_payload, &mut table),
            _ => {} // Unknown protocol, drop
        }
        return;
    }

    // Phase 4: Try NIC RX if loopback was empty
    nic_dispatch::nic_receive_one();

    // Phase 5: Check retransmit timers
    super::tcp_timer::check_all_retransmits();
}

/// Deliver all pending loopback packets (up to queue size).
pub fn deliver_pending() {
    for _ in 0..super::loopback::LOOPBACK_QUEUE_SIZE {
        let has_more = {
            let lo = LOOPBACK.lock();
            !lo.is_empty()
        };
        if !has_more {
            break;
        }
        deliver_one();
    }
}

// ============================================================================
// poll() support
// ============================================================================

/// Poll event flags (POSIX-compatible).
pub const POLLIN: u16 = 1;
pub const POLLOUT: u16 = 4;
pub const POLLERR: u16 = 8;
pub const POLLHUP: u16 = 16;

/// Poll file descriptor — C-compatible layout.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PollFd {
    pub fd: u32,
    pub events: u16,
    pub revents: u16,
}

/// Poll multiple socket file descriptors for events.
///
/// Returns the number of fds with non-zero revents, 0 on timeout, -1 on error.
/// timeout_ms: -1 = infinite, 0 = instant, >0 = milliseconds.
pub fn socket_poll(fds: &mut [PollFd], timeout_ms: i64) -> i32 {
    use crate::x86::idt::tick_count;

    let start = tick_count();

    loop {
        // Process pending packets
        deliver_one();

        // Check all fds
        let mut ready_count: i32 = 0;
        {
            let table = SOCKETS.lock();
            for pfd in fds.iter_mut() {
                let sock_id = super::socket::SocketId(pfd.fd);
                pfd.revents = 0;

                if let Some(sock) = table.get(sock_id) {
                    // POLLIN: data available or connection closed (EOF)
                    if pfd.events & POLLIN != 0 {
                        if !sock.rx.is_empty()
                            || sock.state == SocketState::CloseWait
                            || sock.state == SocketState::Closed
                            || sock.state == SocketState::TimeWait
                        {
                            pfd.revents |= POLLIN;
                        }
                    }

                    // POLLOUT: can send data
                    if pfd.events & POLLOUT != 0 {
                        if sock.state == SocketState::Established {
                            pfd.revents |= POLLOUT;
                        }
                    }

                    // POLLERR: error condition
                    if sock.active && sock.state == SocketState::Closed {
                        pfd.revents |= POLLERR;
                    }

                    // POLLHUP: hangup
                    if sock.state == SocketState::Closed
                        || sock.state == SocketState::CloseWait
                    {
                        pfd.revents |= POLLHUP;
                    }

                    if pfd.revents != 0 {
                        ready_count += 1;
                    }
                } else {
                    // Invalid fd
                    pfd.revents = POLLERR;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return ready_count;
        }

        // Timeout check
        if timeout_ms == 0 {
            return 0;
        }

        if timeout_ms > 0 {
            let elapsed = tick_count().saturating_sub(start) as i64;
            if elapsed >= timeout_ms {
                return 0;
            }
        }

        core::hint::spin_loop();
    }
}

/// Allocate an ephemeral port (simple linear scan from 49152).
fn allocate_ephemeral_port(table: &super::socket::SocketTable) -> u16 {
    // Use a simple counter-based approach
    static PORT_COUNTER: core::sync::atomic::AtomicU16 =
        core::sync::atomic::AtomicU16::new(49152);

    for _ in 0..1000 {
        let port = PORT_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let port = if port >= 65535 { 49152 } else { port };
        if !table.port_in_use(port, Protocol::Tcp) && !table.port_in_use(port, Protocol::Udp) {
            return port;
        }
    }
    49152 // Fallback
}
