//! Interrupt Descriptor Table — 256 gate descriptors for exceptions, IRQs, and syscalls.
//!
//! Interrupt stubs are generated via global_asm!. Each stub pushes a dummy error
//! code (if the CPU doesn't push one), the vector number, then jumps to `isr_common`
//! which saves all GPRs and calls the Rust `interrupt_dispatch` function.
//!
//! Context switch support: after interrupt_dispatch returns, isr_common checks
//! CONTEXT_SWITCH_RSP. If non-zero, RSP is switched to the new context before
//! restoring GPRs and IRETQ — enabling preemptive multitasking.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use core::sync::atomic::{AtomicU64, Ordering};
use fabric_types::ProcessState;
use crate::serial_println;
use super::gdt;
use super::context::SavedContext;


/// IDT gate descriptor (16 bytes).
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,    // Handler address bits 0-15
    selector: u16,      // Code segment selector
    ist: u8,            // IST index (bits 0-2)
    type_attr: u8,      // Type and attributes
    offset_mid: u16,    // Handler address bits 16-31
    offset_high: u32,   // Handler address bits 32-63
    _reserved: u32,
}

impl IdtEntry {
    const fn empty() -> Self {
        Self {
            offset_low: 0, selector: 0, ist: 0, type_attr: 0,
            offset_mid: 0, offset_high: 0, _reserved: 0,
        }
    }

    /// Create an interrupt gate (DPL=0, present, 64-bit interrupt gate type = 0x8E).
    fn new(handler: u64, selector: u16, ist_index: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: ist_index & 0x7,
            type_attr: 0x8E, // Present | DPL=0 | Interrupt Gate
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            _reserved: 0,
        }
    }

    /// Check if this entry is present.
    pub fn is_present(&self) -> bool {
        self.type_attr & 0x80 != 0
    }

    /// Get the handler address.
    pub fn handler_addr(&self) -> u64 {
        (self.offset_low as u64)
            | ((self.offset_mid as u64) << 16)
            | ((self.offset_high as u64) << 32)
    }
}

/// IDT pointer for LIDT.
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

/// The IDT — 256 entries.
static mut IDT: [IdtEntry; 256] = [IdtEntry::empty(); 256];

/// Global interrupt tick counter (incremented by timer handler).
pub static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Context switch RSP — when non-zero, isr_common switches RSP to this value
/// before popping GPRs and IRETQ. Set by timer handler during context switch.
#[no_mangle]
static mut CONTEXT_SWITCH_RSP: u64 = 0;

/// Saved RSP for the idle/kernel context (when no scheduler process is current).
/// Used to return to the idle loop after all scheduled processes complete.
static mut IDLE_RSP: u64 = 0;

/// Saved kernel CR3 for returning to idle context.
static mut KERNEL_CR3: u64 = 0;

// --- Interrupt stub declarations ---
// These are defined in global_asm! below.

extern "C" {
    fn isr_stub_0();  fn isr_stub_1();  fn isr_stub_2();  fn isr_stub_3();
    fn isr_stub_4();  fn isr_stub_5();  fn isr_stub_6();  fn isr_stub_7();
    fn isr_stub_8();  fn isr_stub_9();  fn isr_stub_10(); fn isr_stub_11();
    fn isr_stub_12(); fn isr_stub_13(); fn isr_stub_14(); fn isr_stub_15();
    fn isr_stub_16(); fn isr_stub_17(); fn isr_stub_18(); fn isr_stub_19();
    fn isr_stub_20(); fn isr_stub_21(); fn isr_stub_22(); fn isr_stub_23();
    fn isr_stub_24(); fn isr_stub_25(); fn isr_stub_26(); fn isr_stub_27();
    fn isr_stub_28(); fn isr_stub_29(); fn isr_stub_30(); fn isr_stub_31();
    fn isr_stub_32(); // Timer
    fn isr_stub_33(); fn isr_stub_34(); fn isr_stub_35(); fn isr_stub_36();
    fn isr_stub_37(); fn isr_stub_38(); fn isr_stub_39(); fn isr_stub_40();
    fn isr_stub_41(); fn isr_stub_42(); fn isr_stub_43(); fn isr_stub_44();
    fn isr_stub_45(); fn isr_stub_46(); fn isr_stub_47();
    fn isr_stub_255(); // Spurious
    fn isr_stub_default(); // Catch-all for unassigned vectors
}

/// Initialize the IDT with all 256 entries and load via LIDT.
pub fn init() {
    let stubs: [unsafe extern "C" fn(); 49] = [
        isr_stub_0,  isr_stub_1,  isr_stub_2,  isr_stub_3,
        isr_stub_4,  isr_stub_5,  isr_stub_6,  isr_stub_7,
        isr_stub_8,  isr_stub_9,  isr_stub_10, isr_stub_11,
        isr_stub_12, isr_stub_13, isr_stub_14, isr_stub_15,
        isr_stub_16, isr_stub_17, isr_stub_18, isr_stub_19,
        isr_stub_20, isr_stub_21, isr_stub_22, isr_stub_23,
        isr_stub_24, isr_stub_25, isr_stub_26, isr_stub_27,
        isr_stub_28, isr_stub_29, isr_stub_30, isr_stub_31,
        isr_stub_32, isr_stub_33, isr_stub_34, isr_stub_35,
        isr_stub_36, isr_stub_37, isr_stub_38, isr_stub_39,
        isr_stub_40, isr_stub_41, isr_stub_42, isr_stub_43,
        isr_stub_44, isr_stub_45, isr_stub_46, isr_stub_47,
        isr_stub_255,
    ];

    unsafe {
        // Vectors 0-47: individual stubs
        for i in 0..48 {
            let ist = if i == 8 { 1 } else { 0 }; // IST1 for Double Fault
            IDT[i] = IdtEntry::new(stubs[i] as *const () as u64, gdt::KERNEL_CS, ist);
        }

        // Vector 255: spurious interrupt
        IDT[255] = IdtEntry::new(stubs[48] as *const () as u64, gdt::KERNEL_CS, 0);

        // Vectors 48-254: default handler
        let default_handler = isr_stub_default as *const () as u64;
        for i in 48..255 {
            IDT[i] = IdtEntry::new(default_handler, gdt::KERNEL_CS, 0);
        }

        // Save kernel CR3 for idle context restoration
        core::ptr::write_volatile(core::ptr::addr_of_mut!(KERNEL_CR3), super::context::read_cr3());
    }

    load_idt();
    serial_println!("[IDT] Loaded (256 entries, IST1 on vector 8)");
}

/// Load the IDT via LIDT.
fn load_idt() {
    let idt_ptr = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: unsafe { IDT.as_ptr() as u64 },
    };

    unsafe {
        core::arch::asm!(
            "lidt [{}]",
            in(reg) &idt_ptr as *const IdtPointer,
            options(nostack)
        );
    }
}

/// Get reference to IDT entries (for OCRB testing).
pub fn raw_entries() -> &'static [IdtEntry; 256] {
    unsafe { &IDT }
}

/// Get current tick count.
pub fn tick_count() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

// ============================================================================
// Rust interrupt dispatch — called from isr_common with RSP pointing to SavedContext
// ============================================================================

/// Main interrupt dispatch function.
/// Called from assembly with RDI = pointer to SavedContext on the stack.
#[no_mangle]
extern "C" fn interrupt_dispatch(frame: *mut SavedContext) {
    let frame = unsafe { &mut *frame };
    let vector = frame.vector;

    match vector {
        // Exceptions 0-31
        0 => exception_handler("Divide Error (#DE)", frame),
        1 => {}, // Debug — ignore
        2 => exception_handler("NMI", frame),
        3 => {}, // Breakpoint — ignore for now
        4 => exception_handler("Overflow (#OF)", frame),
        5 => exception_handler("Bound Range (#BR)", frame),
        6 => exception_handler("Invalid Opcode (#UD)", frame),
        7 => exception_handler("Device Not Available (#NM)", frame),
        8 => double_fault_handler(frame),
        9 => exception_handler("Coprocessor Segment Overrun", frame),
        10 => exception_handler("Invalid TSS (#TS)", frame),
        11 => exception_handler("Segment Not Present (#NP)", frame),
        12 => exception_handler("Stack-Segment Fault (#SS)", frame),
        13 => gpf_handler(frame),
        14 => page_fault_handler(frame),
        16 => exception_handler("x87 FP Exception (#MF)", frame),
        17 => exception_handler("Alignment Check (#AC)", frame),
        18 => exception_handler("Machine Check (#MC)", frame),
        19 => exception_handler("SIMD FP Exception (#XM)", frame),
        20 => exception_handler("Virtualization Exception (#VE)", frame),
        21 => exception_handler("Control Protection (#CP)", frame),

        // Timer interrupt (vector 32)
        32 => timer_handler(frame),

        // Keyboard interrupt (vector 33, IRQ1)
        33 => {
            crate::keyboard::keyboard_irq_handler();
        },

        // Virtio-net interrupt (vector 43, IRQ11)
        43 => {
            crate::virtio::net::virtio_net_irq_handler();
        },

        // Spurious interrupt (vector 255)
        255 => {
            // Spurious — do NOT send EOI
        },

        // Default: unexpected interrupt
        _ => {
            serial_println!("[IDT] Unexpected interrupt vector {}", vector);
            super::apic::send_eoi();
        },
    }
}

fn exception_handler(name: &str, frame: &SavedContext) {
    serial_println!("[EXCEPTION] {} at RIP=0x{:016x}", name, frame.rip);
    serial_println!("  Error code: 0x{:x}, CS: 0x{:x}, RFLAGS: 0x{:x}",
        frame.error_code, frame.cs, frame.rflags);
    serial_println!("  RSP: 0x{:016x}, SS: 0x{:x}", frame.rsp, frame.ss);
    serial_println!("  RAX=0x{:016x} RBX=0x{:016x} RCX=0x{:016x} RDX=0x{:016x}",
        frame.rax, frame.rbx, frame.rcx, frame.rdx);
    panic!("Unrecoverable exception: {}", name);
}

fn double_fault_handler(frame: &SavedContext) -> ! {
    serial_println!("[EXCEPTION] DOUBLE FAULT at RIP=0x{:016x}", frame.rip);
    serial_println!("  Error code: 0x{:x}", frame.error_code);
    panic!("Double fault — kernel halted");
}

fn gpf_handler(frame: &SavedContext) {
    serial_println!("[EXCEPTION] General Protection Fault (#GP) at RIP=0x{:016x}", frame.rip);
    serial_println!("  Error code: 0x{:x}, CS: 0x{:x}, SS: 0x{:x}",
        frame.error_code, frame.cs, frame.ss);
    serial_println!("  RSP: 0x{:016x}, RFLAGS: 0x{:016x}", frame.rsp, frame.rflags);
    panic!("General Protection Fault");
}

fn page_fault_handler(frame: &SavedContext) {
    // CR2 holds the faulting address
    let cr2: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
    }
    serial_println!("[EXCEPTION] Page Fault (#PF) at RIP=0x{:016x}", frame.rip);
    serial_println!("  Faulting address (CR2): 0x{:016x}", cr2);
    serial_println!("  Error code: 0x{:x} [{}{}{}{}]",
        frame.error_code,
        if frame.error_code & 1 != 0 { "P " } else { "NP " },
        if frame.error_code & 2 != 0 { "W " } else { "R " },
        if frame.error_code & 4 != 0 { "U " } else { "S " },
        if frame.error_code & 16 != 0 { "I" } else { "D" },
    );
    panic!("Page fault");
}

// ============================================================================
// Timer handler with preemptive context switch
// ============================================================================


fn timer_handler(frame: &mut SavedContext) {
    // Increment global tick counter
    let tick = TICK_COUNT.fetch_add(1, Ordering::Relaxed);

    // Try to acquire scheduler and table locks.
    // If either is held (e.g., spawn() in progress), skip context switch this tick.
    let mut sched = match crate::process::SCHEDULER.try_lock() {
        Some(s) => s,
        None => {
            super::apic::send_eoi();
            return;
        }
    };
    let mut table = match crate::process::TABLE.try_lock() {
        Some(t) => t,
        None => {
            super::apic::send_eoi();
            return;
        }
    };

    let frame_rsp = frame as *mut SavedContext as u64;

    // Advance scheduler tick
    sched.tick();

    // Get the currently running process
    let current_pid = sched.current();

    if let Some(pid) = current_pid {
        let pcb_state = table.get(pid).map(|p| p.state);

        if pcb_state == Some(ProcessState::Terminated) {
            // Current process terminated (e.g., via sys_exit dead loop).
            // Don't save its context. Pick next runnable process or return to idle.
            try_switch_next(&mut sched, &mut table);
        } else {
            // Consume a tick from current process's time slice
            let has_remaining = sched.consume_tick(&mut table);

            if !has_remaining {
                // Time slice expired — context switch
                // Save current RSP (points to SavedContext on current kernel stack)
                if let Some(pcb) = table.get_mut(pid) {
                    if pcb.kernel_stack_base.is_some() {
                        pcb.saved_rsp = frame_rsp;
                    }
                }

                // Pick next runnable process
                try_switch_next(&mut sched, &mut table);
            }
        }
    } else {
        // No current process. Save idle RSP only once — prevents dead_loop
        // from overwriting the original idle/test context.
        unsafe {
            if read_idle_rsp() == 0 {
                write_idle_rsp(frame_rsp);
            }
        }
        // Try to schedule a runnable process
        try_switch_next(&mut sched, &mut table);
    }

    drop(table);
    drop(sched);
    super::apic::send_eoi();
}

/// Try to find and switch to a runnable process (one with a kernel stack).
/// Skips processes without a kernel stack (e.g., Butler) by deferring their
/// re-enqueue until after the loop. This prevents a high-priority non-runnable
/// process from starving lower-priority runnable processes.
/// If no runnable process is found and IDLE_RSP is saved, switches to idle.

fn try_switch_next(
    sched: &mut crate::process::scheduler::Scheduler,
    table: &mut crate::process::ProcessTable,
) {
    // Collect PIDs that can't run on hardware (no kernel stack) so we can
    // re-enqueue them AFTER the loop. Without this, a high-priority process
    // without a kernel stack (e.g., Butler at Critical) would be picked,
    // rejected, re-enqueued, and picked again — starving lower-priority
    // processes that DO have kernel stacks.
    let mut skipped: [(fabric_types::ProcessId, u8); 8] =
        [(fabric_types::ProcessId::KERNEL, 0); 8];
    let mut skipped_count = 0usize;

    for _i in 0..8u32 {
        match sched.schedule_next(table) {
            Some(pid) => {
                if switch_to_process(pid, table) {
                    // Context switch set up successfully.
                    // Re-enqueue any skipped processes before returning.
                    for j in 0..skipped_count {
                        let (spid, sprio) = skipped[j];
                        sched.enqueue(spid, sprio);
                    }
                    return;
                }
                // Process can't run on hardware (no kernel stack).
                // Remove from current and set Ready, but DON'T re-enqueue yet.
                let prio = table.get(pid).map(|p| p.effective_priority).unwrap_or(0);
                sched.dequeue(pid);
                if let Some(pcb) = table.get_mut(pid) {
                    pcb.state = ProcessState::Ready;
                }
                if skipped_count < 8 {
                    skipped[skipped_count] = (pid, prio);
                    skipped_count += 1;
                }
            }
            None => break,
        }
    }

    // Re-enqueue all skipped (non-runnable) processes
    for j in 0..skipped_count {
        let (spid, sprio) = skipped[j];
        sched.enqueue(spid, sprio);
    }

    // No runnable process found — switch to idle if we have a saved context
    unsafe {
        if read_idle_rsp() != 0 {
            switch_to_idle();
        }
    }
}

/// Request a context switch to the given process.
/// Returns true if CONTEXT_SWITCH_RSP was set (process has kernel stack).
/// Returns false if the process can't be switched to.
fn switch_to_process(pid: fabric_types::ProcessId, table: &crate::process::ProcessTable) -> bool {
    if let Some(pcb) = table.get(pid) {
        let has_kstack = pcb.kernel_stack_base.is_some();
        let saved = pcb.saved_rsp;
        if has_kstack && saved != 0 {
            // Update TSS RSP0 for Ring 3→0 transitions
            super::tss::set_rsp0(pcb.kernel_stack_top);
            // Update SYSCALL scratch kernel RSP
            super::syscall::set_kernel_rsp(pcb.kernel_stack_top);
            // Switch CR3 if process has its own address space
            if let Some(ref addr_space) = pcb.address_space {
                let new_cr3 = addr_space.cr3().as_u64();
                let old_cr3 = super::context::read_cr3();
                if new_cr3 != old_cr3 {
                    super::context::write_cr3(new_cr3);
                }
            }
            // Request stack switch — isr_common will pick this up.
            // Must use volatile write: this variable is read ONLY by assembly
            // (isr_common), so the compiler could otherwise eliminate the store.
            unsafe { write_context_switch_rsp(saved); }
            return true;
        }
    }
    false
}

/// Return to the idle/kernel context (no scheduled process).
fn switch_to_idle() {
    unsafe {
        let idle = read_idle_rsp();
        if idle != 0 {
            // Restore kernel CR3
            let current_cr3 = super::context::read_cr3();
            let kernel_cr3 = read_kernel_cr3();
            if current_cr3 != kernel_cr3 {
                super::context::write_cr3(kernel_cr3);
            }
            write_context_switch_rsp(idle);
            write_idle_rsp(0);
        }
    }
}

// ==========================================================================
// Volatile accessors for static mut variables read/written by assembly.
//
// CONTEXT_SWITCH_RSP, IDLE_RSP, and KERNEL_CR3 are written by Rust code but
// read only by global_asm! (isr_common). Without volatile access, the compiler
// may eliminate these writes as "dead stores" since no Rust code reads them.
// ==========================================================================

#[inline(always)]
unsafe fn write_context_switch_rsp(val: u64) {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(CONTEXT_SWITCH_RSP), val);
}

#[inline(always)]
unsafe fn read_idle_rsp() -> u64 {
    core::ptr::read_volatile(core::ptr::addr_of!(IDLE_RSP))
}

#[inline(always)]
unsafe fn write_idle_rsp(val: u64) {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(IDLE_RSP), val);
}

#[inline(always)]
unsafe fn read_kernel_cr3() -> u64 {
    core::ptr::read_volatile(core::ptr::addr_of!(KERNEL_CR3))
}

// ============================================================================
// Interrupt stubs — generated via global_asm!
// ============================================================================

// Macro for stubs WITHOUT CPU-pushed error code
macro_rules! isr_no_err {
    ($name:ident, $vec:literal) => {
        core::arch::global_asm!(
            concat!(".global ", stringify!($name)),
            concat!(stringify!($name), ":"),
            "push 0",                      // dummy error code
            concat!("push ", $vec),         // vector number
            "jmp isr_common",
        );
    };
}

// Macro for stubs WITH CPU-pushed error code
macro_rules! isr_err {
    ($name:ident, $vec:literal) => {
        core::arch::global_asm!(
            concat!(".global ", stringify!($name)),
            concat!(stringify!($name), ":"),
            // Error code already pushed by CPU
            concat!("push ", $vec),         // vector number
            "jmp isr_common",
        );
    };
}

// Exceptions 0-31
isr_no_err!(isr_stub_0,  "0");   // #DE Divide Error
isr_no_err!(isr_stub_1,  "1");   // #DB Debug
isr_no_err!(isr_stub_2,  "2");   // NMI
isr_no_err!(isr_stub_3,  "3");   // #BP Breakpoint
isr_no_err!(isr_stub_4,  "4");   // #OF Overflow
isr_no_err!(isr_stub_5,  "5");   // #BR Bound Range
isr_no_err!(isr_stub_6,  "6");   // #UD Invalid Opcode
isr_no_err!(isr_stub_7,  "7");   // #NM Device Not Available
isr_err!(isr_stub_8,     "8");   // #DF Double Fault (error code = 0)
isr_no_err!(isr_stub_9,  "9");   // Coprocessor Segment Overrun
isr_err!(isr_stub_10,    "10");  // #TS Invalid TSS
isr_err!(isr_stub_11,    "11");  // #NP Segment Not Present
isr_err!(isr_stub_12,    "12");  // #SS Stack-Segment Fault
isr_err!(isr_stub_13,    "13");  // #GP General Protection
isr_err!(isr_stub_14,    "14");  // #PF Page Fault
isr_no_err!(isr_stub_15, "15");  // Reserved
isr_no_err!(isr_stub_16, "16");  // #MF x87 FP
isr_err!(isr_stub_17,    "17");  // #AC Alignment Check
isr_no_err!(isr_stub_18, "18");  // #MC Machine Check
isr_no_err!(isr_stub_19, "19");  // #XM SIMD FP
isr_no_err!(isr_stub_20, "20");  // #VE Virtualization
isr_err!(isr_stub_21,    "21");  // #CP Control Protection
isr_no_err!(isr_stub_22, "22");
isr_no_err!(isr_stub_23, "23");
isr_no_err!(isr_stub_24, "24");
isr_no_err!(isr_stub_25, "25");
isr_no_err!(isr_stub_26, "26");
isr_no_err!(isr_stub_27, "27");
isr_no_err!(isr_stub_28, "28");
isr_err!(isr_stub_29,    "29");  // #VC VMM Communication
isr_err!(isr_stub_30,    "30");  // #SX Security Exception
isr_no_err!(isr_stub_31, "31");

// IRQs 32-47
isr_no_err!(isr_stub_32, "32");  // Timer (IRQ0)
isr_no_err!(isr_stub_33, "33");
isr_no_err!(isr_stub_34, "34");
isr_no_err!(isr_stub_35, "35");
isr_no_err!(isr_stub_36, "36");
isr_no_err!(isr_stub_37, "37");
isr_no_err!(isr_stub_38, "38");
isr_no_err!(isr_stub_39, "39");
isr_no_err!(isr_stub_40, "40");
isr_no_err!(isr_stub_41, "41");
isr_no_err!(isr_stub_42, "42");
isr_no_err!(isr_stub_43, "43");
isr_no_err!(isr_stub_44, "44");
isr_no_err!(isr_stub_45, "45");
isr_no_err!(isr_stub_46, "46");
isr_no_err!(isr_stub_47, "47");

// Spurious (vector 255)
isr_no_err!(isr_stub_255, "255");

// Default handler for unassigned vectors
core::arch::global_asm!(
    ".global isr_stub_default",
    "isr_stub_default:",
    "push 0",           // dummy error code
    "push 0xFF",        // vector = 255 (treat as spurious)
    "jmp isr_common",
);

// Common interrupt handler — saves all GPRs, calls Rust dispatch, restores, IRETQ.
// Supports context switch: after dispatch, checks CONTEXT_SWITCH_RSP and
// switches stack if non-zero.
core::arch::global_asm!(
    ".global isr_common",
    "isr_common:",
    // Save all general-purpose registers (matches SavedContext layout)
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

    // Call Rust handler: RDI = pointer to SavedContext on stack
    "mov rdi, rsp",
    "call interrupt_dispatch",

    // --- Context switch support ---
    // Check if timer_handler requested a context switch
    "mov rax, [rip + CONTEXT_SWITCH_RSP]",
    "test rax, rax",
    "jz 2f",
    // Non-zero: switch RSP to the new context
    "mov rsp, rax",
    // Clear the flag
    "xor rax, rax",
    "mov [rip + CONTEXT_SWITCH_RSP], rax",
    "2:",

    // Restore all general-purpose registers
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

    // Skip vector and error code
    "add rsp, 16",

    // Return from interrupt
    "iretq",
);
