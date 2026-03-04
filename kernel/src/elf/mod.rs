//! ELF64 loader — parse and load ELF binaries into per-process address spaces.
//!
//! Phase 7C: Loads PT_LOAD segments from ELF64 executables, mapping pages via
//! `AddressSpace::map_user_page()`. Returns the entry point address.
//!
//! Includes embedded test binaries for OCRB verification.

#![allow(dead_code)]

use crate::address_space::AddressSpace;
use crate::memory::{VirtAddr, PAGE_SIZE};
use crate::memory::frame;
use crate::memory::page_table::PageTableFlags;

// --- ELF64 structures ---

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;
const PT_LOAD: u32 = 1;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// ELF64 file header (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// ELF64 program header (56 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// ELF loading errors.
#[derive(Debug)]
pub enum ElfError {
    TooSmall,
    BadMagic,
    Not64Bit,
    NotLittleEndian,
    WrongArch,
    NotExecutable,
    InvalidPhdr,
    OutOfMemory,
    MapFailed,
}

/// Parse and validate an ELF64 header.
pub fn parse_header(data: &[u8]) -> Result<&Elf64Header, ElfError> {
    if data.len() < core::mem::size_of::<Elf64Header>() {
        return Err(ElfError::TooSmall);
    }

    let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if header.e_ident[4] != ELFCLASS64 {
        return Err(ElfError::Not64Bit);
    }
    if header.e_ident[5] != ELFDATA2LSB {
        return Err(ElfError::NotLittleEndian);
    }
    if header.e_type != ET_EXEC {
        return Err(ElfError::NotExecutable);
    }
    if header.e_machine != EM_X86_64 {
        return Err(ElfError::WrongArch);
    }

    Ok(header)
}

/// Get program headers from parsed ELF data.
pub fn program_headers<'a>(data: &'a [u8], header: &Elf64Header) -> Result<&'a [Elf64Phdr], ElfError> {
    let ph_offset = header.e_phoff as usize;
    let ph_size = header.e_phentsize as usize;
    let ph_count = header.e_phnum as usize;

    if ph_size < core::mem::size_of::<Elf64Phdr>() {
        return Err(ElfError::InvalidPhdr);
    }
    let end = ph_offset + ph_count * ph_size;
    if end > data.len() {
        return Err(ElfError::InvalidPhdr);
    }

    let ptr = unsafe { data.as_ptr().add(ph_offset) as *const Elf64Phdr };
    Ok(unsafe { core::slice::from_raw_parts(ptr, ph_count) })
}

/// Load an ELF64 binary into a per-process address space.
///
/// For each PT_LOAD segment: allocates frames, maps user pages, copies data.
/// Returns the entry point virtual address.
pub fn load_elf(
    data: &[u8],
    address_space: &mut AddressSpace,
) -> Result<u64, ElfError> {
    let header = parse_header(data)?;
    let phdrs = program_headers(data, header)?;

    for phdr in phdrs {
        if phdr.p_type != PT_LOAD {
            continue;
        }
        if phdr.p_memsz == 0 {
            continue;
        }

        // Calculate page-aligned range
        let vaddr_start = phdr.p_vaddr & !0xFFF;
        let vaddr_end = (phdr.p_vaddr + phdr.p_memsz + 0xFFF) & !0xFFF;

        // Build flags from ELF segment flags
        let mut flags = PageTableFlags::empty();
        if phdr.p_flags & PF_W != 0 {
            flags = flags | PageTableFlags::WRITABLE;
        }
        if phdr.p_flags & PF_X == 0 {
            flags = flags | PageTableFlags::NO_EXECUTE;
        }

        // Map each page
        let mut page_va = vaddr_start;
        while page_va < vaddr_end {
            let phys_frame = frame::allocate_frame()
                .ok_or(ElfError::OutOfMemory)?;

            // Zero the frame
            let frame_virt = phys_frame.to_virt().as_u64() as *mut u8;
            unsafe { core::ptr::write_bytes(frame_virt, 0, PAGE_SIZE); }

            // Calculate overlap between this page and the file data
            let seg_file_start = phdr.p_offset;
            let _seg_file_end = phdr.p_offset + phdr.p_filesz;
            let seg_vaddr_start = phdr.p_vaddr;

            // Virtual range of this page
            let page_end = page_va + PAGE_SIZE as u64;

            // Intersection of [page_va, page_end) with [seg_vaddr_start, seg_vaddr_start + filesz)
            let copy_vstart = page_va.max(seg_vaddr_start);
            let copy_vend = page_end.min(seg_vaddr_start + phdr.p_filesz);

            if copy_vstart < copy_vend {
                let copy_len = (copy_vend - copy_vstart) as usize;
                let dst_offset = (copy_vstart - page_va) as usize;
                let src_offset = (seg_file_start + (copy_vstart - seg_vaddr_start)) as usize;

                if src_offset + copy_len <= data.len() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.as_ptr().add(src_offset),
                            frame_virt.add(dst_offset),
                            copy_len,
                        );
                    }
                }
            }

            address_space.map_user_page(
                VirtAddr::new(page_va),
                phys_frame,
                flags,
            ).map_err(|_| ElfError::MapFailed)?;

            page_va += PAGE_SIZE as u64;
        }
    }

    Ok(header.e_entry)
}

// =============================================================================
// Embedded test binaries
// =============================================================================

/// Raw x86_64 machine code: mov rax, 0 (SYS_EXIT); mov rdi, 42; syscall
/// 16 bytes — can be loaded at any page-aligned address.
pub const TEST_CODE_EXIT42: &[u8] = &[
    0x48, 0xc7, 0xc0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
    0x48, 0xc7, 0xc7, 0x2a, 0x00, 0x00, 0x00, // mov rdi, 42
    0x0f, 0x05,                                 // syscall
];

/// Infinite loop for preemptive scheduling tests (2 bytes).
pub const TEST_CODE_LOOP: &[u8] = &[
    0xeb, 0xfe, // jmp $ (infinite loop)
];

/// Delay loop then sys_exit(0) — for preemptive scheduling tests (23 bytes).
/// Runs 0x200000 (~2M) iterations of DEC+JNZ (~4ms at 1GHz), then calls SYS_EXIT(0).
/// The delay ensures the process accumulates timer ticks before exiting.
pub const TEST_CODE_DELAY_EXIT: &[u8] = &[
    0x48, 0xc7, 0xc1, 0x00, 0x00, 0x20, 0x00, // mov rcx, 0x200000
    0x48, 0xff, 0xc9,                           // dec rcx
    0x75, 0xfb,                                 // jnz -5 (back to dec)
    0x48, 0xc7, 0xc0, 0x00, 0x00, 0x00, 0x00,  // mov rax, 0 (SYS_EXIT)
    0x31, 0xff,                                 // xor edi, edi (exit_code=0)
    0x0f, 0x05,                                 // syscall
];

/// Minimal ELF64 binary wrapping TEST_CODE_EXIT42.
/// Entry point: 0x400078 (code at file offset 120 = ELF header + 1 phdr).
/// Single PT_LOAD segment maps entire file at 0x400000.
pub const TEST_ELF_EXIT42: &[u8] = &[
    // ELF header (64 bytes)
    0x7f, 0x45, 0x4c, 0x46, // e_ident[0..4]: magic
    0x02,                     // e_ident[4]: ELFCLASS64
    0x01,                     // e_ident[5]: ELFDATA2LSB
    0x01,                     // e_ident[6]: EV_CURRENT
    0x00,                     // e_ident[7]: ELFOSABI_NONE
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_ident[8..16]: padding
    0x02, 0x00,               // e_type: ET_EXEC
    0x3e, 0x00,               // e_machine: EM_X86_64
    0x01, 0x00, 0x00, 0x00,   // e_version: EV_CURRENT
    0x78, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // e_entry: 0x400078
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_phoff: 64
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // e_shoff: 0
    0x00, 0x00, 0x00, 0x00,   // e_flags: 0
    0x40, 0x00,               // e_ehsize: 64
    0x38, 0x00,               // e_phentsize: 56
    0x01, 0x00,               // e_phnum: 1
    0x00, 0x00,               // e_shentsize: 0
    0x00, 0x00,               // e_shnum: 0
    0x00, 0x00,               // e_shstrndx: 0
    // Program header (56 bytes, at offset 64)
    0x01, 0x00, 0x00, 0x00,   // p_type: PT_LOAD
    0x05, 0x00, 0x00, 0x00,   // p_flags: PF_R | PF_X
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_offset: 0
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // p_vaddr: 0x400000
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, // p_paddr: 0x400000
    0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_filesz: 136
    0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_memsz: 136
    0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // p_align: 0x1000
    // Code (16 bytes, at offset 120)
    0x48, 0xc7, 0xc0, 0x00, 0x00, 0x00, 0x00,       // mov rax, 0 (SYS_EXIT)
    0x48, 0xc7, 0xc7, 0x2a, 0x00, 0x00, 0x00,       // mov rdi, 42
    0x0f, 0x05,                                        // syscall
];

/// User stack constants.
pub const USER_STACK_PAGES: u64 = 4;
pub const USER_STACK_BASE: u64 = 0x0000_7FFF_FFFE_C000;
pub const USER_STACK_TOP: u64 = USER_STACK_BASE + USER_STACK_PAGES * PAGE_SIZE as u64;
