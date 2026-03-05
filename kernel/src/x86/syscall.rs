//! SYSCALL/SYSRET — fast user→kernel transition via MSRs.
//!
//! Configures IA32_EFER, IA32_STAR, IA32_LSTAR, and IA32_FMASK for the
//! SYSCALL instruction. The entry stub builds an interrupt-compatible
//! SavedContext frame so context switch and IRETQ work uniformly.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use crate::serial_println;
use super::gdt;
use super::context::SavedContext;
use fabric_types::HandleId;

// MSR addresses
const IA32_EFER:  u32 = 0xC000_0080;
const IA32_STAR:  u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_CSTAR: u32 = 0xC000_0083; // Unused (compat mode)
const IA32_FMASK: u32 = 0xC000_0084;

// EFER bits
const EFER_SCE: u64 = 1 << 0; // System Call Enable

// RFLAGS mask: clear IF (bit 9) on SYSCALL entry to disable interrupts
const FMASK_VALUE: u64 = 0x200;

/// Per-CPU scratch area for syscall entry.
/// [0] = user RSP save slot, [1] = kernel RSP for current process.
#[no_mangle]
static mut SYSCALL_SCRATCH: [u64; 2] = [0; 2];

/// Read a model-specific register.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack)
    );
    (high as u64) << 32 | low as u64
}

/// Write a model-specific register.
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack)
    );
}

/// Initialize SYSCALL/SYSRET MSRs.
pub fn init() {
    unsafe {
        // Enable SYSCALL/SYSRET in EFER
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | EFER_SCE);

        // STAR: bits 47:32 = kernel CS (for SYSCALL), bits 63:48 = base for SYSRET
        // SYSCALL: CS = STAR[47:32], SS = STAR[47:32]+8
        // SYSRET:  CS = STAR[63:48]+16 | RPL3, SS = STAR[63:48]+8 | RPL3
        let star = ((gdt::KERNEL_CS as u64) << 32) | ((0x10u64) << 48);
        wrmsr(IA32_STAR, star);

        // LSTAR: syscall entry point address
        extern "C" { fn syscall_entry(); }
        wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);

        // FMASK: clear IF on SYSCALL entry
        wrmsr(IA32_FMASK, FMASK_VALUE);

        // CSTAR: unused (32-bit compat mode)
        wrmsr(IA32_CSTAR, 0);
    }

    serial_println!("[SYSCALL] MSRs configured (EFER.SCE=1, LSTAR set, FMASK=0x{:x})", FMASK_VALUE);
}

/// Update the kernel stack pointer for syscall entry (called on context switch).
pub fn set_kernel_rsp(kernel_stack_top: u64) {
    unsafe {
        SYSCALL_SCRATCH[1] = kernel_stack_top;
    }
}

/// Read EFER MSR (for OCRB testing).
pub fn read_efer() -> u64 {
    unsafe { rdmsr(IA32_EFER) }
}

/// Read STAR MSR (for OCRB testing).
pub fn read_star() -> u64 {
    unsafe { rdmsr(IA32_STAR) }
}

/// Read LSTAR MSR (for OCRB testing).
pub fn read_lstar() -> u64 {
    unsafe { rdmsr(IA32_LSTAR) }
}

/// Read FMASK MSR (for OCRB testing).
pub fn read_fmask() -> u64 {
    unsafe { rdmsr(IA32_FMASK) }
}

/// Dead loop for terminated processes — HLT with interrupts enabled.
/// Timer will preempt and switch to the next process.
#[no_mangle]
extern "C" fn syscall_dead_loop() {
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

/// Main syscall dispatch — called from assembly with RDI = pointer to SavedContext.
/// RAX = syscall number. Args in RDI (frame.rdi), RSI, RDX, R10, R8, R9.
#[no_mangle]
extern "C" fn syscall_dispatch(frame: *mut SavedContext) {
    let frame = unsafe { &mut *frame };
    let syscall_num = frame.rax;

    match syscall_num {
        // SYS_EXIT: rdi = exit code
        0 => {
            let exit_code = frame.rdi;
            serial_println!("[SYSCALL] sys_exit({})", exit_code);

            // Terminate the current process
            if let Some(mut sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    sched.dequeue(pid);
                    if let Some(mut table) = crate::process::TABLE.try_lock() {
                        if let Some(pcb) = table.get_mut(pid) {
                            pcb.state = fabric_types::ProcessState::Terminated;
                            pcb.exit_reason = Some(crate::process::ExitReason::Normal);
                            pcb.exit_code = exit_code;
                        }
                    }
                }
            }

            // Modify saved context to return to kernel-mode dead loop.
            // Timer will preempt and switch to next context (idle or another process).
            frame.rip = syscall_dead_loop as *const () as u64;
            frame.cs = gdt::KERNEL_CS as u64;
            frame.ss = gdt::KERNEL_DS as u64;
            frame.rflags = 0x202; // IF=1 so timer can preempt
            frame.rsp = unsafe { SYSCALL_SCRATCH[1] }; // kernel stack top
        },

        // SYS_YIELD: voluntarily yield time slice
        1 => {
            // Zero the time slice so next timer tick triggers switch
            if let Some(sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    if let Some(mut table) = crate::process::TABLE.try_lock() {
                        if let Some(pcb) = table.get_mut(pid) {
                            pcb.time_slice_remaining = 0;
                        }
                    }
                }
            }
            frame.rax = 0;
        },

        // SYS_WRITE: rdi = fd, rsi = buf_ptr, rdx = len
        2 => {
            let fd = frame.rdi;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;

            // Validate user pointer
            if buf_ptr >= 0x0000_8000_0000_0000 || len >= 4096 {
                frame.rax = u64::MAX;
                return;
            }

            let slice = unsafe {
                core::slice::from_raw_parts(buf_ptr as *const u8, len as usize)
            };

            // fd 1 (stdout) and fd 2 (stderr) → serial output
            if fd == 1 || fd == 2 {
                for &byte in slice {
                    crate::serial::write_byte(byte);
                }
                frame.rax = len;
            } else {
                // VFS write path: resolve fd → open file → write
                frame.rax = syscall_vfs_write(fd, slice);
            }
        },

        // SYS_GETPID: return current process ID
        3 => {
            if let Some(sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    frame.rax = pid.0 as u64;
                } else {
                    frame.rax = 0;
                }
            } else {
                frame.rax = 0;
            }
        },

        // SYS_OPEN: rdi = path_ptr, rsi = path_len, rdx = flags
        4 => {
            let path_ptr = frame.rdi;
            let path_len = frame.rsi;
            let flags = frame.rdx as u32;

            if path_ptr >= 0x0000_8000_0000_0000 || path_len > 256 {
                frame.rax = u64::MAX;
                return;
            }

            let path = unsafe {
                core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize)
            };

            frame.rax = syscall_open(path, flags);
        },

        // SYS_READ: rdi = fd, rsi = buf_ptr, rdx = len
        5 => {
            let fd = frame.rdi;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;

            if buf_ptr >= 0x0000_8000_0000_0000 || len >= 4096 {
                frame.rax = u64::MAX;
                return;
            }

            let buf = unsafe {
                core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize)
            };

            frame.rax = syscall_read(fd, buf);
        },

        // SYS_CLOSE: rdi = fd
        6 => {
            let fd = frame.rdi;
            frame.rax = syscall_close(fd);
        },

        // SYS_STAT: rdi = path_ptr, rsi = path_len, rdx = stat_buf
        7 => {
            let path_ptr = frame.rdi;
            let path_len = frame.rsi;
            let stat_buf = frame.rdx;

            if path_ptr >= 0x0000_8000_0000_0000 || path_len > 256
                || stat_buf >= 0x0000_8000_0000_0000
            {
                frame.rax = u64::MAX;
                return;
            }

            let path = unsafe {
                core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize)
            };

            frame.rax = syscall_stat(path, stat_buf);
        },

        // SYS_FSTAT: rdi = fd, rsi = stat_buf
        8 => {
            let fd = frame.rdi;
            let stat_buf = frame.rsi;

            if stat_buf >= 0x0000_8000_0000_0000 {
                frame.rax = u64::MAX;
                return;
            }

            frame.rax = syscall_fstat(fd, stat_buf);
        },

        // SYS_GETDENTS: rdi = fd, rsi = buf_ptr, rdx = len
        9 => {
            let fd = frame.rdi;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;

            if buf_ptr >= 0x0000_8000_0000_0000 || len >= 65536 {
                frame.rax = u64::MAX;
                return;
            }

            let buf = unsafe {
                core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize)
            };

            frame.rax = syscall_getdents(fd, buf);
        },

        // SYS_SOCKET: rdi = type (1=stream,2=dgram), rsi = protocol (6=tcp,17=udp)
        10 => {
            frame.rax = syscall_socket(frame.rdi, frame.rsi);
        },

        // SYS_BIND: rdi = fd, rsi = addr (u32 IPv4 big-endian), rdx = port
        11 => {
            frame.rax = syscall_bind(frame.rdi, frame.rsi as u32, frame.rdx as u16);
        },

        // SYS_LISTEN: rdi = fd
        12 => {
            frame.rax = syscall_listen(frame.rdi);
        },

        // SYS_ACCEPT: rdi = fd
        13 => {
            frame.rax = syscall_accept(frame.rdi);
        },

        // SYS_CONNECT: rdi = fd, rsi = addr (u32 IPv4 big-endian), rdx = port
        14 => {
            frame.rax = syscall_connect(frame.rdi, frame.rsi as u32, frame.rdx as u16);
        },

        // SYS_SEND: rdi = fd, rsi = buf_ptr, rdx = len
        15 => {
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            if buf_ptr >= 0x0000_8000_0000_0000 || len > 65536 {
                frame.rax = u64::MAX;
                return;
            }
            let data = unsafe {
                core::slice::from_raw_parts(buf_ptr as *const u8, len as usize)
            };
            frame.rax = syscall_send(frame.rdi, data);
        },

        // SYS_RECV: rdi = fd, rsi = buf_ptr, rdx = len
        16 => {
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            if buf_ptr >= 0x0000_8000_0000_0000 || len > 65536 {
                frame.rax = u64::MAX;
                return;
            }
            let buf = unsafe {
                core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize)
            };
            frame.rax = syscall_recv(frame.rdi, buf);
        },

        // SYS_SHUTDOWN: rdi = fd
        17 => {
            frame.rax = syscall_shutdown(frame.rdi);
        },

        // SYS_DISPLAY_ALLOC_SURFACE: rdi = width, rsi = height
        18 => {
            frame.rax = syscall_display_alloc_surface(frame.rdi as u32, frame.rsi as u32);
        },

        // SYS_DISPLAY_BLIT: rdi = surface_id, rsi = buf_ptr, rdx = len
        19 => {
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            if buf_ptr >= 0x0000_8000_0000_0000 {
                frame.rax = u64::MAX;
                return;
            }
            frame.rax = syscall_display_blit(frame.rdi as u32, buf_ptr, len as usize);
        },

        // SYS_DISPLAY_PRESENT: rdi = surface_id
        20 => {
            frame.rax = syscall_display_present(frame.rdi as u32);
        },

        // SYS_KB_READ: read a key from keyboard buffer
        21 => {
            let mut kb = crate::keyboard::KEYBOARD_BUFFER.lock();
            frame.rax = match kb.pop() {
                Some(ch) => ch as u64,
                None => 0,
            };
        },

        // SYS_DNS_RESOLVE: rdi = name_ptr, rsi = name_len
        22 => {
            let name_ptr = frame.rdi;
            let name_len = frame.rsi;
            if name_ptr >= 0x0000_8000_0000_0000 || name_len > 256 {
                frame.rax = u64::MAX;
            } else {
                let name_slice = unsafe {
                    core::slice::from_raw_parts(name_ptr as *const u8, name_len as usize)
                };
                if let Ok(hostname) = core::str::from_utf8(name_slice) {
                    match crate::network::dns::dns_resolve(hostname) {
                        Some(ip) => {
                            frame.rax = ((ip[0] as u64) << 24)
                                | ((ip[1] as u64) << 16)
                                | ((ip[2] as u64) << 8)
                                | (ip[3] as u64);
                        }
                        None => frame.rax = u64::MAX,
                    }
                } else {
                    frame.rax = u64::MAX;
                }
            }
        },

        // SYS_SENDTO: rdi = fd, rsi = buf_ptr, rdx = len, r10 = dest_ip (u32), r8 = dest_port
        23 => {
            let fd = frame.rdi;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            let dest_ip = frame.r10 as u32;
            let dest_port = frame.r8 as u16;

            if buf_ptr >= 0x0000_8000_0000_0000 || len > 65536 {
                frame.rax = u64::MAX;
            } else {
                let data = unsafe {
                    core::slice::from_raw_parts(buf_ptr as *const u8, len as usize)
                };

                let sock_id = match resolve_socket_fd(fd) {
                    Some(id) => id,
                    None => { frame.rax = u64::MAX; return; }
                };

                use crate::network::addr::{Ipv4Addr, SocketAddr};
                let dst = SocketAddr::new(Ipv4Addr::from_u32(dest_ip), dest_port);

                match crate::network::ops::socket_sendto(sock_id, data, dst) {
                    Ok(n) => frame.rax = n as u64,
                    Err(_) => frame.rax = u64::MAX,
                }
            }
        },

        // SYS_TLS_CONNECT: rdi = socket fd, rsi = hostname_ptr, rdx = hostname_len
        25 => {
            let fd = frame.rdi;
            let name_ptr = frame.rsi;
            let name_len = frame.rdx;
            if name_ptr >= 0x0000_8000_0000_0000 || name_len > 256 {
                frame.rax = u64::MAX;
            } else {
                let name_slice = unsafe {
                    core::slice::from_raw_parts(name_ptr as *const u8, name_len as usize)
                };
                if let Ok(hostname) = core::str::from_utf8(name_slice) {
                    let sock_id = match resolve_socket_fd(fd) {
                        Some(id) => id,
                        None => { frame.rax = u64::MAX; return; }
                    };
                    match crate::network::tls::tls_connect(sock_id, hostname) {
                        Ok(session_idx) => frame.rax = session_idx as u64,
                        Err(_) => frame.rax = u64::MAX,
                    }
                } else {
                    frame.rax = u64::MAX;
                }
            }
        },

        // SYS_TLS_SEND: rdi = session_idx, rsi = buf_ptr, rdx = len
        26 => {
            let session_idx = frame.rdi as usize;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            if buf_ptr >= 0x0000_8000_0000_0000 || len > 65536 {
                frame.rax = u64::MAX;
            } else {
                let data = unsafe {
                    core::slice::from_raw_parts(buf_ptr as *const u8, len as usize)
                };
                match crate::network::tls::tls_send(session_idx, data) {
                    Ok(n) => frame.rax = n as u64,
                    Err(_) => frame.rax = u64::MAX,
                }
            }
        },

        // SYS_TLS_RECV: rdi = session_idx, rsi = buf_ptr, rdx = len
        27 => {
            let session_idx = frame.rdi as usize;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;
            if buf_ptr >= 0x0000_8000_0000_0000 || len > 65536 {
                frame.rax = u64::MAX;
            } else {
                let buf = unsafe {
                    core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize)
                };
                match crate::network::tls::tls_recv(session_idx, buf) {
                    Ok(n) => frame.rax = n as u64,
                    Err(_) => frame.rax = u64::MAX,
                }
            }
        },

        // SYS_TLS_CLOSE: rdi = session_idx
        28 => {
            let session_idx = frame.rdi as usize;
            match crate::network::tls::tls_close(session_idx) {
                Ok(()) => frame.rax = 0,
                Err(_) => frame.rax = u64::MAX,
            }
        },

        // SYS_POLL: rdi = fds_ptr, rsi = nfds, rdx = timeout_ms
        24 => {
            let fds_ptr = frame.rdi;
            let nfds = frame.rsi as usize;
            let timeout_ms = frame.rdx as i64;

            if fds_ptr >= 0x0000_8000_0000_0000 || nfds > 16 {
                frame.rax = u64::MAX;
            } else {
                use crate::network::ops::PollFd;
                let user_fds = unsafe {
                    core::slice::from_raw_parts_mut(
                        fds_ptr as *mut PollFd,
                        nfds,
                    )
                };

                // Copy to kernel and resolve handle slot → SocketId
                let mut kernel_fds = [PollFd { fd: 0, events: 0, revents: 0 }; 16];
                for (i, pfd) in user_fds.iter().enumerate() {
                    kernel_fds[i].events = pfd.events;
                    kernel_fds[i].revents = 0;
                    if let Some(sock_id) = resolve_socket_fd(pfd.fd as u64) {
                        kernel_fds[i].fd = sock_id.0;
                    } else {
                        kernel_fds[i].fd = u32::MAX; // Invalid — socket_poll will skip
                    }
                }

                let result = crate::network::ops::socket_poll(&mut kernel_fds[..nfds], timeout_ms);

                // Copy revents back to userspace
                for (i, pfd) in user_fds.iter_mut().enumerate() {
                    pfd.revents = kernel_fds[i].revents;
                }

                frame.rax = result as u64;
            }
        },

        // SYS_WM_CREATE: rdi = x, rsi = y, rdx = width, r10 = height, r8 = title_ptr, r9 = title_len
        29 => {
            let x = frame.rdi as i32;
            let y = frame.rsi as i32;
            let width = frame.rdx as u32;
            let height = frame.r10 as u32;
            let title_ptr = frame.r8;
            let title_len = frame.r9 as usize;

            // Validate dimensions
            if width < 1 || width > 2048 || height < 1 || height > 2048 {
                frame.rax = u64::MAX;
                return;
            }

            // Validate title pointer
            if title_ptr >= 0x0000_8000_0000_0000 || title_len > 256 {
                frame.rax = u64::MAX;
                return;
            }

            let title = if title_len > 0 {
                let title_slice = unsafe {
                    core::slice::from_raw_parts(title_ptr as *const u8, title_len)
                };
                core::str::from_utf8(title_slice)
                    .unwrap_or("Untitled")
                    .into()
            } else {
                alloc::string::String::from("Untitled")
            };

            let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            match wt.create(owner, title, x, y, width, height) {
                Some(wid) => {
                    frame.rax = wid.0 as u64;
                    drop(wt);
                    crate::wm::compositor::compose_and_present();
                }
                None => frame.rax = u64::MAX,
            }
        },

        // SYS_WM_DESTROY: rdi = window_id
        30 => {
            let wid = crate::wm::WindowId(frame.rdi as u32);
            let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            // Verify ownership
            let authorized = wt.get(wid)
                .map(|w| w.owner_pid == owner || owner == fabric_types::ProcessId::KERNEL)
                .unwrap_or(false);

            if authorized && wt.destroy(wid) {
                frame.rax = 0;
                drop(wt);
                crate::wm::compositor::compose_and_present();
            } else {
                frame.rax = u64::MAX;
            }
        },

        // SYS_WM_BLIT: rdi = window_id, rsi = buf_ptr, rdx = buf_len
        31 => {
            let wid = crate::wm::WindowId(frame.rdi as u32);
            let buf_ptr = frame.rsi;
            let buf_len = frame.rdx as usize;

            if buf_ptr >= 0x0000_8000_0000_0000 {
                frame.rax = u64::MAX;
                return;
            }

            let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            if let Some(win) = wt.get_mut(wid) {
                if win.owner_pid != owner && owner != fabric_types::ProcessId::KERNEL {
                    frame.rax = u64::MAX;
                    return;
                }

                let expected = (win.width as usize) * (win.height as usize) * 4;
                if buf_len != expected {
                    frame.rax = u64::MAX;
                    return;
                }

                let src = unsafe {
                    core::slice::from_raw_parts(buf_ptr as *const u32, expected / 4)
                };

                // Copy pixels to window surface
                win.surface.buffer.copy_from_slice(src);
                win.surface.dirty = true;
                frame.rax = 0;
                drop(wt);
                crate::wm::compositor::compose_and_present();
            } else {
                frame.rax = u64::MAX;
            }
        },

        // SYS_WM_MOVE_RESIZE: rdi = window_id, rsi = x, rdx = y, r10 = width, r8 = height
        32 => {
            let wid = crate::wm::WindowId(frame.rdi as u32);
            let new_x = frame.rsi as i32;
            let new_y = frame.rdx as i32;
            let new_w = frame.r10 as u32;
            let new_h = frame.r8 as u32;

            // Validate dimensions
            if new_w < 1 || new_w > 2048 || new_h < 1 || new_h > 2048 {
                frame.rax = u64::MAX;
                return;
            }

            let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            if let Some(win) = wt.get_mut(wid) {
                if win.owner_pid != owner && owner != fabric_types::ProcessId::KERNEL {
                    frame.rax = u64::MAX;
                    return;
                }

                win.x = new_x;
                win.y = new_y;

                // Reallocate surface if dimensions changed
                if new_w != win.width || new_h != win.height {
                    if let Some(new_surface) = crate::display::compositor::Surface::new(new_w, new_h) {
                        win.surface = new_surface;
                        win.width = new_w;
                        win.height = new_h;
                    } else {
                        frame.rax = u64::MAX;
                        return;
                    }
                }

                frame.rax = 0;
                drop(wt);
                crate::wm::compositor::compose_and_present();
            } else {
                frame.rax = u64::MAX;
            }
        },

        // SYS_WM_FOCUS: rdi = window_id
        33 => {
            let wid = crate::wm::WindowId(frame.rdi as u32);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            if wt.get(wid).is_some() {
                wt.set_focus(wid);
                frame.rax = 0;
                drop(wt);
                crate::wm::compositor::compose_and_present();
            } else {
                frame.rax = u64::MAX;
            }
        },

        // SYS_WM_EVENT: rdi = window_id, rsi = event_buf_ptr, rdx = buf_len
        34 => {
            let wid = crate::wm::WindowId(frame.rdi as u32);
            let evt_buf = frame.rsi;
            let buf_len = frame.rdx as usize;

            if evt_buf >= 0x0000_8000_0000_0000 {
                frame.rax = 0; // no event
                return;
            }

            let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

            let mut wt = crate::wm::WINDOW_TABLE.lock();
            if let Some(win) = wt.get_mut(wid) {
                if win.owner_pid != owner && owner != fabric_types::ProcessId::KERNEL {
                    frame.rax = 0;
                    return;
                }

                if buf_len < crate::wm::event::SERIALIZED_SIZE {
                    frame.rax = 0;
                    return;
                }

                if let Some(event) = win.event_queue.pop() {
                    let bytes = event.to_bytes();
                    let dst = unsafe {
                        core::slice::from_raw_parts_mut(
                            evt_buf as *mut u8,
                            crate::wm::event::SERIALIZED_SIZE,
                        )
                    };
                    dst.copy_from_slice(&bytes);
                    frame.rax = crate::wm::event::SERIALIZED_SIZE as u64;
                } else {
                    frame.rax = 0; // no events pending
                }
            } else {
                frame.rax = 0;
            }
        },

        _ => {
            serial_println!("[SYSCALL] Unknown syscall {}", syscall_num);
            frame.rax = u64::MAX; // Error
        },
    }
}

// ============================================================================
// VFS syscall helpers — resolve fd through HandleTable and dispatch to VFS
// ============================================================================

/// Resolve a file descriptor to an OpenFileId using the current process's HandleTable.
fn resolve_fd(fd: u64) -> Option<crate::vfs::open_file::OpenFileId> {
    let handle = HandleId::pack(fd as u8, 0);

    let sched = crate::process::SCHEDULER.try_lock()?;
    let pid = sched.current()?;
    drop(sched);

    let table = crate::process::TABLE.try_lock()?;
    let pcb = table.get(pid)?;

    // Try to resolve — we need to match the generation stored in the handle table
    // For stdio handles (0,1,2) allocated at spawn, generation is 0
    // We try generation 0 first, then scan active entries
    if let Ok(cap_id) = pcb.handle_table.resolve(handle) {
        return Some(crate::vfs::open_file::OpenFileId::new(cap_id as u32));
    }

    // Fallback: check if slot is active and get its value directly
    // The handle table uses generation counters, so we need the right generation
    None
}

/// SYS_OPEN helper
fn syscall_open(path: &[u8], flags: u32) -> u64 {
    use crate::vfs::open_file::OpenFlags;
    use crate::vfs::ops;

    let open_flags = OpenFlags::from_raw(flags);
    match ops::vfs_open(path, open_flags) {
        Ok(open_file_id) => {
            // Allocate a handle in the current process
            if let Some(sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    drop(sched);
                    if let Some(mut table) = crate::process::TABLE.try_lock() {
                        if let Some(pcb) = table.get_mut(pid) {
                            match pcb.handle_table.alloc(open_file_id.0 as u64) {
                                Ok(handle) => return handle.slot() as u64,
                                Err(_) => {
                                    // Clean up the open file
                                    let _ = ops::vfs_close(open_file_id);
                                    return u64::MAX;
                                }
                            }
                        }
                    }
                }
            }
            u64::MAX
        }
        Err(_) => u64::MAX,
    }
}

/// SYS_READ helper
fn syscall_read(fd: u64, buf: &mut [u8]) -> u64 {
    use crate::vfs::ops;

    match resolve_fd(fd) {
        Some(open_file_id) => {
            match ops::vfs_read(open_file_id, buf) {
                Ok(n) => n as u64,
                Err(_) => u64::MAX,
            }
        }
        None => u64::MAX,
    }
}

/// SYS_WRITE VFS path helper (for fds other than 1/2)
fn syscall_vfs_write(fd: u64, data: &[u8]) -> u64 {
    use crate::vfs::ops;

    match resolve_fd(fd) {
        Some(open_file_id) => {
            // Check if this is a serial inode
            {
                let open_files = crate::vfs::OPEN_FILES.lock();
                if let Some(of) = open_files.get(open_file_id) {
                    if crate::vfs::stdio::is_serial_inode(of.inode_id) {
                        drop(open_files);
                        for &byte in data {
                            crate::serial::write_byte(byte);
                        }
                        return data.len() as u64;
                    }
                }
            }
            match ops::vfs_write(open_file_id, data) {
                Ok(n) => n as u64,
                Err(_) => u64::MAX,
            }
        }
        None => u64::MAX,
    }
}

/// SYS_CLOSE helper
fn syscall_close(fd: u64) -> u64 {
    use crate::vfs::ops;

    // First resolve the fd to get the open file id
    let open_file_id = match resolve_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    // Release the handle from the process's handle table
    if let Some(sched) = crate::process::SCHEDULER.try_lock() {
        if let Some(pid) = sched.current() {
            drop(sched);
            if let Some(mut table) = crate::process::TABLE.try_lock() {
                if let Some(pcb) = table.get_mut(pid) {
                    let handle = HandleId::pack(fd as u8, 0);
                    let _ = pcb.handle_table.release(handle);
                }
            }
        }
    }

    // Close the open file
    match ops::vfs_close(open_file_id) {
        Ok(()) => 0,
        Err(_) => u64::MAX,
    }
}

/// SYS_STAT helper
fn syscall_stat(path: &[u8], stat_buf_addr: u64) -> u64 {
    use crate::vfs::ops;

    match ops::vfs_stat(path) {
        Ok(stat) => {
            // Copy stat to user buffer
            let stat_ptr = stat_buf_addr as *mut ops::StatBuf;
            unsafe { *stat_ptr = stat; }
            0
        }
        Err(_) => u64::MAX,
    }
}

/// SYS_FSTAT helper
fn syscall_fstat(fd: u64, stat_buf_addr: u64) -> u64 {
    use crate::vfs::ops;

    match resolve_fd(fd) {
        Some(open_file_id) => {
            match ops::vfs_fstat(open_file_id) {
                Ok(stat) => {
                    let stat_ptr = stat_buf_addr as *mut ops::StatBuf;
                    unsafe { *stat_ptr = stat; }
                    0
                }
                Err(_) => u64::MAX,
            }
        }
        None => u64::MAX,
    }
}

/// SYS_GETDENTS helper
fn syscall_getdents(fd: u64, buf: &mut [u8]) -> u64 {
    use crate::vfs::ops;

    match resolve_fd(fd) {
        Some(open_file_id) => {
            match ops::vfs_readdir(open_file_id, buf) {
                Ok(n) => n as u64,
                Err(_) => u64::MAX,
            }
        }
        None => u64::MAX,
    }
}

// ============================================================================
// Socket syscall helpers — SOCKET_FD_FLAG discriminates socket vs file fds
// ============================================================================

/// Bit 63 flag to distinguish socket fds from file fds in HandleTable.
const SOCKET_FD_FLAG: u64 = 1u64 << 63;

/// SYS_SOCKET helper
fn syscall_socket(sock_type: u64, protocol: u64) -> u64 {
    use crate::network::addr::{SocketType, Protocol};
    use crate::network::ops;

    let st = match sock_type {
        1 => SocketType::Stream,
        2 => SocketType::Datagram,
        _ => return u64::MAX,
    };
    let proto = match protocol {
        6 => Protocol::Tcp,
        17 => Protocol::Udp,
        _ => return u64::MAX,
    };

    let owner = get_current_pid().unwrap_or(fabric_types::ProcessId::KERNEL);

    match ops::socket_create(st, proto, owner) {
        Ok(sock_id) => {
            // Allocate a handle in the current process, storing socket id with flag
            let value = (sock_id.0 as u64) | SOCKET_FD_FLAG;
            match alloc_handle(value) {
                Some(fd) => fd as u64,
                None => u64::MAX,
            }
        }
        Err(_) => u64::MAX,
    }
}

/// SYS_BIND helper
fn syscall_bind(fd: u64, addr_u32: u32, port: u16) -> u64 {
    use crate::network::addr::{Ipv4Addr, SocketAddr};
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    let addr = SocketAddr::new(Ipv4Addr::from_u32(addr_u32), port);
    match ops::socket_bind(sock_id, addr) {
        Ok(()) => 0,
        Err(_) => u64::MAX,
    }
}

/// SYS_LISTEN helper
fn syscall_listen(fd: u64) -> u64 {
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    match ops::socket_listen(sock_id) {
        Ok(()) => 0,
        Err(_) => u64::MAX,
    }
}

/// SYS_ACCEPT helper
fn syscall_accept(fd: u64) -> u64 {
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    match ops::socket_accept(sock_id) {
        Ok(new_id) => {
            // Allocate a handle for the new connection socket
            let value = (new_id.0 as u64) | SOCKET_FD_FLAG;
            match alloc_handle(value) {
                Some(fd) => fd as u64,
                None => u64::MAX,
            }
        }
        Err(_) => u64::MAX,
    }
}

/// SYS_CONNECT helper
fn syscall_connect(fd: u64, addr_u32: u32, port: u16) -> u64 {
    use crate::network::addr::{Ipv4Addr, SocketAddr};
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    let remote = SocketAddr::new(Ipv4Addr::from_u32(addr_u32), port);
    match ops::socket_connect(sock_id, remote) {
        Ok(()) => 0,
        Err(_) => u64::MAX,
    }
}

/// SYS_SEND helper
fn syscall_send(fd: u64, data: &[u8]) -> u64 {
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    match ops::socket_send(sock_id, data) {
        Ok(n) => n as u64,
        Err(_) => u64::MAX,
    }
}

/// SYS_RECV helper
fn syscall_recv(fd: u64, buf: &mut [u8]) -> u64 {
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    match ops::socket_recv(sock_id, buf) {
        Ok(n) => n as u64,
        Err(_) => u64::MAX,
    }
}

/// SYS_SHUTDOWN helper
fn syscall_shutdown(fd: u64) -> u64 {
    use crate::network::ops;

    let sock_id = match resolve_socket_fd(fd) {
        Some(id) => id,
        None => return u64::MAX,
    };

    match ops::socket_shutdown(sock_id) {
        Ok(()) => 0,
        Err(_) => u64::MAX,
    }
}

/// Resolve a socket fd to a SocketId. Checks SOCKET_FD_FLAG bit 63.
fn resolve_socket_fd(fd: u64) -> Option<crate::network::socket::SocketId> {
    let handle = HandleId::pack(fd as u8, 0);

    let sched = crate::process::SCHEDULER.try_lock()?;
    let pid = sched.current()?;
    drop(sched);

    let table = crate::process::TABLE.try_lock()?;
    let pcb = table.get(pid)?;

    if let Ok(value) = pcb.handle_table.resolve(handle) {
        if value & SOCKET_FD_FLAG != 0 {
            let raw = (value & !SOCKET_FD_FLAG) as u32;
            return Some(crate::network::socket::SocketId(raw));
        }
    }
    None
}

/// Get current process PID.
fn get_current_pid() -> Option<fabric_types::ProcessId> {
    let sched = crate::process::SCHEDULER.try_lock()?;
    sched.current()
}

/// Allocate a handle in the current process's handle table.
fn alloc_handle(value: u64) -> Option<u8> {
    let sched = crate::process::SCHEDULER.try_lock()?;
    let pid = sched.current()?;
    drop(sched);

    let mut table = crate::process::TABLE.try_lock()?;
    let pcb = table.get_mut(pid)?;
    match pcb.handle_table.alloc(value) {
        Ok(handle) => Some(handle.slot()),
        Err(_) => None,
    }
}

// ============================================================================
// Display syscall helpers — alloc surface, blit pixels, present to screen
// ============================================================================

/// SYS_DISPLAY_ALLOC_SURFACE: allocate a new surface of given dimensions.
/// Returns surface_id (u64) or u64::MAX on failure.
fn syscall_display_alloc_surface(width: u32, height: u32) -> u64 {
    use crate::display::{SURFACE_TABLE, SurfaceId};

    // Sanity check dimensions (max 4096x4096 = 64MB)
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return u64::MAX;
    }

    if let Some(mut table) = SURFACE_TABLE.try_lock() {
        match table.alloc(width, height) {
            Some(id) => {
                serial_println!("[SYSCALL] display_alloc_surface({}x{}) -> {}", width, height, id.0);
                id.0 as u64
            }
            None => u64::MAX,
        }
    } else {
        u64::MAX
    }
}

/// SYS_DISPLAY_BLIT: copy pixel data from userspace buffer into surface.
/// buf_ptr points to packed u32 pixels (width * height * 4 bytes).
/// Returns 0 on success, u64::MAX on error.
/// One-shot blit log counter.
static BLIT_LOG_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn syscall_display_blit(surface_id: u32, buf_ptr: u64, len: usize) -> u64 {
    use crate::display::{SURFACE_TABLE, SurfaceId};

    if let Some(mut table) = SURFACE_TABLE.try_lock() {
        let id = SurfaceId(surface_id);
        if let Some(surface) = table.get_mut(id) {
            let expected = surface.buffer.len() * 4;
            if len != expected {
                if BLIT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
                    BLIT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    serial_println!("[SYSCALL] display_blit: size mismatch expected={} got={}", expected, len);
                }
                return u64::MAX;
            }
            // Copy from user buffer into surface
            let src = unsafe {
                core::slice::from_raw_parts(buf_ptr as *const u32, surface.buffer.len())
            };
            surface.buffer.copy_from_slice(src);
            surface.dirty = true;
            if BLIT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
                BLIT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                serial_println!("[SYSCALL] display_blit(surface={}, buf=0x{:x}, len={}) -> ok", surface_id, buf_ptr, len);
            }
            return 0;
        }
    }
    u64::MAX
}

/// SYS_DISPLAY_PRESENT: blit surface to hardware framebuffer.
/// Returns 0 on success, u64::MAX on error.
/// One-shot present log counter.
static PRESENT_LOG_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn syscall_display_present(surface_id: u32) -> u64 {
    use crate::display::{DISPLAY, SURFACE_TABLE, SurfaceId};
    use crate::display::compositor;

    if let Some(table) = SURFACE_TABLE.try_lock() {
        let id = SurfaceId(surface_id);
        if let Some(surface) = table.get(id) {
            if let Some(display) = DISPLAY.try_lock() {
                if let Some(ref ds) = *display {
                    compositor::present(surface, &ds.fb);
                    if PRESENT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
                        PRESENT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                        serial_println!("[SYSCALL] display_present(surface={}) -> ok (fb={}x{})", surface_id, ds.fb.width, ds.fb.height);
                    }
                    return 0;
                } else if PRESENT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
                    PRESENT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    serial_println!("[SYSCALL] display_present: DISPLAY is None");
                }
            } else if PRESENT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
                PRESENT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                serial_println!("[SYSCALL] display_present: DISPLAY lock contention");
            }
        } else if PRESENT_LOG_COUNT.load(core::sync::atomic::Ordering::Relaxed) < 3 {
            PRESENT_LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            serial_println!("[SYSCALL] display_present: surface {} not found", surface_id);
        }
    }
    u64::MAX
}

// ============================================================================
// SYSCALL entry stub — saves user state, switches to kernel stack, calls dispatch
// ============================================================================
core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // At this point:
    //   RCX = user RIP (saved by CPU)
    //   R11 = user RFLAGS (saved by CPU)
    //   RSP = user RSP (NOT switched — CPU does NOT switch RSP on SYSCALL)
    //   CS/SS = kernel segments (set by STAR MSR)

    // Save user RSP and load kernel RSP from scratch area
    "mov [rip + SYSCALL_SCRATCH], rsp",      // Save user RSP
    "mov rsp, [rip + SYSCALL_SCRATCH + 8]",  // Load kernel RSP

    // Build interrupt-compatible frame (SavedContext layout)
    // CPU-pushed part (we do it manually since SYSCALL doesn't push)
    "push 0x1B",                              // User SS (USER_DS = 0x1B)
    "push [rip + SYSCALL_SCRATCH]",           // User RSP
    "push r11",                               // User RFLAGS
    "push 0x23",                              // User CS (USER_CS = 0x23)
    "push rcx",                               // User RIP

    // Stub-pushed part
    "push 0",                                 // Error code = 0
    "push 256",                               // Vector = 256 (syscall marker)

    // Save all GPRs (same order as isr_common)
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",

    // Enable interrupts in kernel (FMASK cleared IF)
    "sti",

    // Call Rust dispatch: RDI = pointer to SavedContext
    "mov rdi, rsp",
    "call syscall_dispatch",

    // Disable interrupts for return path
    "cli",

    // Restore all GPRs
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",

    // Skip vector + error code
    "add rsp, 16",

    // Return to userspace via IRETQ (simpler than SYSRET, same SavedContext format)
    "iretq",
);
