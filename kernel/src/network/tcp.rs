//! TCP protocol — Transmission Control Protocol (loopback-simplified).
//!
//! 11 states, no retransmission (lossless loopback), no congestion control.
//! Synchronous handshake via deliver_pending(). ISN = global atomic counter.
//!
//! // TODO(Phase 10): TCP retransmission timers, proper ISN randomization,
//! // congestion control (Reno/CUBIC), Nagle algorithm, silly window syndrome.

#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, Ordering};
use super::addr::{Ipv4Addr, SocketAddr, Protocol};
use super::checksum::pseudo_header_checksum;
use super::ip::Ipv4Header;
use super::socket::{SocketId, SocketState, SocketError, SocketTable, MAX_BACKLOG};

/// Global ISN counter — incremented for each new connection.
static ISN_COUNTER: AtomicU32 = AtomicU32::new(1000);

/// Generate the next initial sequence number.
pub fn next_isn() -> u32 {
    ISN_COUNTER.fetch_add(1000, Ordering::Relaxed)
}

/// TCP flags.
pub const FIN: u8 = 0x01;
pub const SYN: u8 = 0x02;
pub const RST: u8 = 0x04;
pub const PSH: u8 = 0x08;
pub const ACK: u8 = 0x10;

/// TCP header — 20 bytes (no options).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset_flags: u16, // data offset (4 bits) + reserved (3) + flags (9)
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

impl TcpHeader {
    pub const SIZE: usize = 20;

    pub fn new(src_port: u16, dst_port: u16, seq: u32, ack: u32, flags: u8, window: u16) -> Self {
        // Data offset = 5 (20 bytes / 4), shifted to top 4 bits of the u16
        let data_offset_flags = (5u16 << 12) | (flags as u16);
        Self {
            src_port,
            dst_port,
            seq_num: seq,
            ack_num: ack,
            data_offset_flags,
            window,
            checksum: 0,
            urgent_ptr: 0,
        }
    }

    pub fn flags(&self) -> u8 {
        (self.data_offset_flags & 0x3F) as u8
    }

    pub fn has_flag(&self, flag: u8) -> bool {
        self.flags() & flag != 0
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let sp = self.src_port.to_be_bytes();
        let dp = self.dst_port.to_be_bytes();
        let sq = self.seq_num.to_be_bytes();
        let ak = self.ack_num.to_be_bytes();
        let df = self.data_offset_flags.to_be_bytes();
        let wn = self.window.to_be_bytes();
        let cs = self.checksum.to_be_bytes();
        let up = self.urgent_ptr.to_be_bytes();
        [
            sp[0], sp[1], dp[0], dp[1],
            sq[0], sq[1], sq[2], sq[3],
            ak[0], ak[1], ak[2], ak[3],
            df[0], df[1], wn[0], wn[1],
            cs[0], cs[1], up[0], up[1],
        ]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            seq_num: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ack_num: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            data_offset_flags: u16::from_be_bytes([data[12], data[13]]),
            window: u16::from_be_bytes([data[14], data[15]]),
            checksum: u16::from_be_bytes([data[16], data[17]]),
            urgent_ptr: u16::from_be_bytes([data[18], data[19]]),
        })
    }
}

/// Build a TCP segment (IP + TCP + data). Returns total length.
pub fn build_tcp_packet(
    src: SocketAddr,
    dst: SocketAddr,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    data: &[u8],
    buf: &mut [u8],
) -> usize {
    let tcp_len = TcpHeader::SIZE + data.len();
    let total = Ipv4Header::SIZE + tcp_len;

    if buf.len() < total {
        return 0;
    }

    // IP header
    let ip = Ipv4Header::new(src.addr, dst.addr, 6, tcp_len as u16); // 6 = TCP
    buf[..Ipv4Header::SIZE].copy_from_slice(&ip.to_bytes());

    // TCP header
    let mut tcp = TcpHeader::new(src.port, dst.port, seq, ack, flags, window);
    let tcp_start = Ipv4Header::SIZE;
    let tcp_bytes = tcp.to_bytes();
    buf[tcp_start..tcp_start + TcpHeader::SIZE].copy_from_slice(&tcp_bytes);

    // Data
    if !data.is_empty() {
        buf[tcp_start + TcpHeader::SIZE..total].copy_from_slice(data);
    }

    // Compute TCP checksum over pseudo-header + TCP segment
    let chk = pseudo_header_checksum(
        &src.addr.0,
        &dst.addr.0,
        6,
        tcp_len as u16,
        &buf[tcp_start..total],
    );
    tcp.checksum = chk;
    let tcp_bytes = tcp.to_bytes();
    buf[tcp_start..tcp_start + TcpHeader::SIZE].copy_from_slice(&tcp_bytes);

    total
}

/// Process a received TCP segment. Runs the TCP state machine.
/// Called from deliver path with SOCKETS lock held.
pub fn tcp_receive_packet(
    ip_hdr: &Ipv4Header,
    tcp_data: &[u8],
    sockets: &mut SocketTable,
) {
    let tcp_hdr = match TcpHeader::from_bytes(tcp_data) {
        Some(h) => h,
        None => return,
    };

    let src_addr = SocketAddr::new(Ipv4Addr(ip_hdr.src_addr), tcp_hdr.src_port);
    let dst_addr = SocketAddr::new(Ipv4Addr(ip_hdr.dst_addr), tcp_hdr.dst_port);
    let flags = tcp_hdr.flags();

    let payload_start = TcpHeader::SIZE;
    let payload = if payload_start < tcp_data.len() {
        &tcp_data[payload_start..]
    } else {
        &[]
    };

    // Try to find an existing connection first (4-tuple match)
    if let Some(sock_id) = sockets.find_tcp_connection(&dst_addr, &src_addr) {
        tcp_state_machine(sock_id, &tcp_hdr, payload, &src_addr, sockets);
        return;
    }

    // If SYN, look for a listener
    if flags & SYN != 0 && flags & ACK == 0 {
        if let Some(listener_id) = sockets.find_listener(tcp_hdr.dst_port) {
            handle_syn(listener_id, &tcp_hdr, &src_addr, &dst_addr, sockets);
        }
    }
}

/// Handle incoming SYN on a listening socket — create a new connection socket.
fn handle_syn(
    listener_id: SocketId,
    tcp_hdr: &TcpHeader,
    src_addr: &SocketAddr,
    dst_addr: &SocketAddr,
    sockets: &mut SocketTable,
) {
    let owner = match sockets.get(listener_id) {
        Some(s) => s.owner,
        None => return,
    };

    // Allocate a new socket for this connection
    let conn_id = match sockets.alloc(
        super::addr::SocketType::Stream,
        super::addr::Protocol::Tcp,
        owner,
    ) {
        Ok(id) => id,
        Err(_) => return,
    };

    let isn = next_isn();

    if let Some(conn) = sockets.get_mut(conn_id) {
        conn.local_addr = *dst_addr;
        conn.remote_addr = *src_addr;
        conn.state = SocketState::SynReceived;
        conn.recv_seq = tcp_hdr.seq_num.wrapping_add(1); // SYN consumes one seq
        conn.send_seq = isn;
        conn.send_ack = tcp_hdr.seq_num.wrapping_add(1);
        conn.send_window = tcp_hdr.window;
    }

    // Add to listener's backlog
    if let Some(listener) = sockets.get_mut(listener_id) {
        if listener.backlog_count < MAX_BACKLOG {
            listener.backlog[listener.backlog_count] = conn_id;
            listener.backlog_count += 1;
        }
    }

    // Send SYN+ACK
    let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
    let pkt_len = build_tcp_packet(
        *dst_addr,
        *src_addr,
        isn,
        tcp_hdr.seq_num.wrapping_add(1),
        SYN | ACK,
        4096,
        &[],
        &mut pkt_buf,
    );

    if pkt_len > 0 {
        super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
    }
}

/// TCP state machine — process a received segment on an existing connection.
fn tcp_state_machine(
    sock_id: SocketId,
    tcp_hdr: &TcpHeader,
    payload: &[u8],
    remote: &SocketAddr,
    sockets: &mut SocketTable,
) {
    let sock = match sockets.get_mut(sock_id) {
        Some(s) => s,
        None => return,
    };

    let flags = tcp_hdr.flags();
    let state = sock.state;

    match state {
        SocketState::SynSent => {
            // Expecting SYN+ACK
            if flags & SYN != 0 && flags & ACK != 0 {
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(1);
                sock.send_ack = tcp_hdr.seq_num.wrapping_add(1);
                sock.send_window = tcp_hdr.window;
                sock.state = SocketState::Established;
                // Clear SYN from retransmit queue
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }

                // Send ACK
                let local = sock.local_addr;
                let remote = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            }
        }

        SocketState::SynReceived => {
            // Expecting ACK to complete handshake
            if flags & ACK != 0 {
                sock.send_seq = sock.send_seq.wrapping_add(1); // SYN consumed one seq
                sock.state = SocketState::Established;
                // Clear SYN+ACK from retransmit queue
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }
            }
        }

        SocketState::Established => {
            if flags & FIN != 0 {
                // Peer wants to close
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(1);
                sock.state = SocketState::CloseWait;

                // Send ACK for FIN
                let local = sock.local_addr;
                let remote_addr = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote_addr, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            } else if flags & ACK != 0 && !payload.is_empty() {
                // Data segment
                sock.rx.write(payload);
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(payload.len() as u32);

                // Send ACK
                let local = sock.local_addr;
                let remote_addr = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote_addr, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            }
            // Pure ACK with no data — update send_ack and clear retransmit
            if flags & ACK != 0 && payload.is_empty() && flags & FIN == 0 {
                sock.send_ack = tcp_hdr.ack_num;
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }
            }
        }

        SocketState::FinWait1 => {
            if flags & ACK != 0 && flags & FIN != 0 {
                // Simultaneous close: FIN+ACK
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(1);
                sock.state = SocketState::TimeWait;
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }

                let local = sock.local_addr;
                let remote_addr = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote_addr, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            } else if flags & ACK != 0 {
                sock.state = SocketState::FinWait2;
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }
            } else if flags & FIN != 0 {
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(1);
                sock.state = SocketState::Closing;

                let local = sock.local_addr;
                let remote_addr = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote_addr, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            }
        }

        SocketState::FinWait2 => {
            if flags & FIN != 0 {
                sock.recv_seq = tcp_hdr.seq_num.wrapping_add(1);
                sock.state = SocketState::TimeWait;

                let local = sock.local_addr;
                let remote_addr = sock.remote_addr;
                let seq = sock.send_seq;
                let ack = sock.recv_seq;

                let mut pkt_buf = [0u8; super::loopback::LOOPBACK_MTU];
                let pkt_len = build_tcp_packet(
                    local, remote_addr, seq, ack, ACK, 4096, &[], &mut pkt_buf,
                );
                if pkt_len > 0 {
                    super::nic_dispatch::transmit_ip(&pkt_buf[..pkt_len]);
                }
            }
        }

        SocketState::CloseWait => {
            // Waiting for application to close — nothing to do on receive
        }

        SocketState::LastAck => {
            if flags & ACK != 0 {
                sock.state = SocketState::Closed;
                sock.active = false;
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                    rq.clear();
                }
            }
        }

        SocketState::Closing => {
            if flags & ACK != 0 {
                sock.state = SocketState::TimeWait;
                if let Some(ref mut rq) = sock.retransmit {
                    rq.ack_received(tcp_hdr.ack_num, crate::x86::idt::tick_count());
                }
            }
        }

        SocketState::TimeWait => {
            // In real TCP, we'd wait 2*MSL. On loopback, transition to Closed.
            sock.state = SocketState::Closed;
            sock.active = false;
            if let Some(ref mut rq) = sock.retransmit {
                rq.clear();
            }
        }

        _ => {} // Closed, Bound, Listen — shouldn't receive data segments
    }
}
