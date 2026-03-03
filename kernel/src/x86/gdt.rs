//! Global Descriptor Table — flat segments for x86_64 long mode.
//!
//! GDT layout:
//!   Index 0 (0x00): Null
//!   Index 1 (0x08): Kernel Code 64-bit, DPL=0
//!   Index 2 (0x10): Kernel Data, DPL=0
//!   Index 3 (0x18): User Data, DPL=3
//!   Index 4 (0x20): User Code 64-bit, DPL=3
//!   Index 5-6 (0x28): TSS (16-byte system segment descriptor)
//!
//! User Data before User Code is required for SYSRET compatibility.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use crate::serial_println;

/// Segment selectors.
pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16   = 0x18 | 3; // 0x1B
pub const USER_CS: u16   = 0x20 | 3; // 0x23
pub const TSS_SEL: u16   = 0x28;

/// Number of GDT entries (5 normal + 2 for TSS = 7 entries, but TSS is double-width).
const GDT_ENTRIES: usize = 7;

/// GDT descriptor for LGDT.
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// The GDT itself — 7 u64 entries.
/// Entry 5-6 forms the 16-byte TSS descriptor (filled in by tss::init).
static mut GDT: [u64; GDT_ENTRIES] = [0; GDT_ENTRIES];

/// Build a 64-bit code segment descriptor.
const fn code_segment(dpl: u8) -> u64 {
    let mut desc: u64 = 0;
    // Limit 0-15 and 16-19 (ignored in long mode, set to 0xFFFFF for compat)
    desc |= 0xFFFF; // limit bits 0-15
    // Access byte: Present(7) | DPL(5-6) | S=1(4) | Type=0xA(0-3) (execute/read)
    let access = 0x80 | ((dpl as u64 & 3) << 5) | 0x10 | 0x0A;
    desc |= access << 40;
    // Flags nibble: Granularity(3)=1 | Long mode(1)=1 | Size(2)=0 | Limit hi(0)=0xF
    // Bits 48-55: Limit 16-19 (0xF) + flags
    desc |= 0x0F << 48; // Limit bits 16-19
    desc |= 1u64 << 53; // L=1 (long mode)
    desc |= 1u64 << 55; // G=1 (granularity)
    desc
}

/// Build a data segment descriptor.
const fn data_segment(dpl: u8) -> u64 {
    let mut desc: u64 = 0;
    desc |= 0xFFFF; // limit bits 0-15
    // Access byte: Present(7) | DPL(5-6) | S=1(4) | Type=0x2(0-3) (read/write)
    let access = 0x80 | ((dpl as u64 & 3) << 5) | 0x10 | 0x02;
    desc |= access << 40;
    // Flags: G=1, DB=1 (32-bit operand size, but ignored for data in long mode)
    desc |= 0x0F << 48; // Limit bits 16-19
    desc |= 1u64 << 54; // D/B = 1
    desc |= 1u64 << 55; // G = 1
    desc
}

/// Initialize the GDT with flat segments and load it.
pub fn init() {
    unsafe {
        GDT[0] = 0; // Null descriptor
        GDT[1] = code_segment(0); // Kernel Code
        GDT[2] = data_segment(0); // Kernel Data
        GDT[3] = data_segment(3); // User Data
        GDT[4] = code_segment(3); // User Code
        GDT[5] = 0; // TSS low — filled by tss::init()
        GDT[6] = 0; // TSS high — filled by tss::init()
    }

    load_gdt();
    reload_segments();

    serial_println!("[GDT] Loaded (null + kcode + kdata + udata + ucode + TSS)");
}

/// Write TSS descriptor into GDT entries 5-6.
/// Called by tss::init() after TSS is set up.
pub fn set_tss_entry(tss_addr: u64, tss_size: u16) {
    let limit = (tss_size - 1) as u64;
    let base = tss_addr;

    // Low 8 bytes (entry 5):
    let mut low: u64 = 0;
    low |= limit & 0xFFFF;                         // Limit 0-15
    low |= (base & 0xFFFF) << 16;                  // Base 0-15
    low |= ((base >> 16) & 0xFF) << 32;            // Base 16-23
    // Access byte: Present | DPL=0 | Type=0x9 (64-bit TSS available)
    low |= 0x89u64 << 40;
    low |= ((limit >> 16) & 0xF) << 48;            // Limit 16-19
    low |= ((base >> 24) & 0xFF) << 56;            // Base 24-31

    // High 8 bytes (entry 6):
    let high: u64 = base >> 32; // Base 32-63

    unsafe {
        GDT[5] = low;
        GDT[6] = high;
    }
}

/// Load the GDT via LGDT instruction.
fn load_gdt() {
    let gdt_ptr = GdtPointer {
        limit: (core::mem::size_of::<[u64; GDT_ENTRIES]>() - 1) as u16,
        base: unsafe { GDT.as_ptr() as u64 },
    };

    unsafe {
        core::arch::asm!(
            "lgdt [{}]",
            in(reg) &gdt_ptr as *const GdtPointer,
            options(nostack)
        );
    }
}

/// Reload segment registers after GDT change.
/// CS: far return trick. DS/ES/SS: direct mov.
fn reload_segments() {
    unsafe {
        // Reload CS via far return
        core::arch::asm!(
            "push {kcs}",      // Push new CS
            "lea {tmp}, [rip + 2f]", // Push return address
            "push {tmp}",
            "retfq",           // Far return → loads CS
            "2:",
            kcs = const KERNEL_CS as u64,
            tmp = out(reg) _,
            options(nostack),
        );

        // Reload data segments
        core::arch::asm!(
            "mov ds, {kds:x}",
            "mov es, {kds:x}",
            "mov ss, {kds:x}",
            "xor {null:e}, {null:e}",
            "mov fs, {null:x}",
            "mov gs, {null:x}",
            kds = in(reg) KERNEL_DS as u64,
            null = out(reg) _,
            options(nostack),
        );
    }
}

/// Load the TSS into TR register. Called after tss::init() sets up GDT entries 5-6.
pub fn load_tss() {
    unsafe {
        core::arch::asm!(
            "ltr {sel:x}",
            sel = in(reg) TSS_SEL as u64,
            options(nostack, nomem),
        );
    }
    serial_println!("[GDT] TSS loaded (selector 0x{:02X})", TSS_SEL);
}

/// Get a reference to the raw GDT entries (for OCRB testing).
pub fn raw_entries() -> &'static [u64; GDT_ENTRIES] {
    unsafe { &GDT }
}
