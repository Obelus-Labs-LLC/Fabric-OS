//! CPIO newc format parser for initramfs loading.
//!
//! The CPIO newc (SVR4) format uses 110-byte ASCII headers with
//! 8-hex-digit fields. Files are 4-byte aligned. The archive ends
//! with a trailer entry named "TRAILER!!!".

#![allow(dead_code)]

use alloc::vec::Vec;

/// Magic bytes for CPIO newc format.
const CPIO_MAGIC: &[u8; 6] = b"070701";

/// A parsed CPIO archive entry.
pub struct CpioEntry<'a> {
    /// File/directory name (without leading "./" stripped).
    pub name: &'a [u8],
    /// File data (empty for directories).
    pub data: &'a [u8],
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// File mode from CPIO header.
    pub mode: u32,
}

/// CPIO parsing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpioError {
    /// Archive is too short for a header.
    TooShort,
    /// Invalid magic number.
    BadMagic,
    /// Invalid hex digit in header field.
    BadHex,
    /// Entry extends beyond archive bounds.
    OutOfBounds,
}

/// Parse 8 ASCII hex digits from a byte slice into a u32.
fn parse_hex8(bytes: &[u8]) -> Result<u32, CpioError> {
    if bytes.len() < 8 {
        return Err(CpioError::BadHex);
    }
    let mut value: u32 = 0;
    for &b in &bytes[..8] {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(CpioError::BadHex),
        };
        value = (value << 4) | digit as u32;
    }
    Ok(value)
}

/// Align a position up to the next 4-byte boundary.
fn align4(pos: usize) -> usize {
    (pos + 3) & !3
}

/// Parse a CPIO newc archive into a vector of entries.
///
/// Each entry references slices of the original archive data (zero-copy).
/// Skips the "." entry and the "TRAILER!!!" terminator.
pub fn parse_cpio(archive: &[u8]) -> Result<Vec<CpioEntry<'_>>, CpioError> {
    let mut entries = Vec::new();
    let mut pos = 0;

    loop {
        // Check minimum header size
        if pos + 110 > archive.len() {
            if entries.is_empty() {
                return Err(CpioError::TooShort);
            }
            break; // Truncated archive, return what we have
        }

        // Verify magic
        if &archive[pos..pos + 6] != CPIO_MAGIC {
            return Err(CpioError::BadMagic);
        }

        // Parse header fields (offsets from CPIO newc spec)
        // 0-5: magic (6)
        // 6-13: ino (8)
        // 14-21: mode (8)
        // 22-29: uid (8)
        // 30-37: gid (8)
        // 38-45: nlink (8)
        // 46-53: mtime (8)
        // 54-61: filesize (8)
        // 62-69: devmajor (8)
        // 70-77: devminor (8)
        // 78-85: rdevmajor (8)
        // 86-93: rdevminor (8)
        // 94-101: namesize (8)
        // 102-109: check (8)

        let mode = parse_hex8(&archive[pos + 14..pos + 22])?;
        let filesize = parse_hex8(&archive[pos + 54..pos + 62])? as usize;
        let namesize = parse_hex8(&archive[pos + 94..pos + 102])? as usize;

        // Name starts right after the 110-byte header
        let name_start = pos + 110;
        let name_end = name_start + namesize;

        if name_end > archive.len() {
            return Err(CpioError::OutOfBounds);
        }

        // Name is NUL-terminated; strip the NUL
        let name = if namesize > 0 && archive[name_end - 1] == 0 {
            &archive[name_start..name_end - 1]
        } else {
            &archive[name_start..name_end]
        };

        // Check for trailer
        if name == b"TRAILER!!!" {
            break;
        }

        // Data starts after name (aligned to 4 bytes)
        let data_start = align4(name_end);
        let data_end = data_start + filesize;

        if data_end > archive.len() {
            return Err(CpioError::OutOfBounds);
        }

        let data = &archive[data_start..data_end];

        // Determine if directory (S_IFDIR = 0o040000)
        let is_directory = (mode & 0o170000) == 0o040000;

        // Skip "." root entry
        if name != b"." {
            entries.push(CpioEntry {
                name,
                data,
                is_directory,
                mode,
            });
        }

        // Advance to next entry (data aligned to 4 bytes)
        pos = align4(data_end);
    }

    Ok(entries)
}

/// Build a minimal CPIO newc archive from entries.
/// Useful for testing. Each entry is (name, data, is_dir).
pub fn build_test_cpio(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut archive = Vec::new();
    let mut ino: u32 = 1;

    for &(name, data, is_dir) in entries {
        let mode: u32 = if is_dir { 0o040755 } else { 0o100644 };
        let namesize = name.len() + 1; // Include NUL terminator
        let filesize = if is_dir { 0 } else { data.len() };

        // Write 110-byte header
        let header = alloc::format!(
            "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
            ino,        // ino
            mode,       // mode
            0u32,       // uid
            0u32,       // gid
            1u32,       // nlink
            0u32,       // mtime
            filesize,   // filesize
            0u32,       // devmajor
            0u32,       // devminor
            0u32,       // rdevmajor
            0u32,       // rdevminor
            namesize,   // namesize
            0u32,       // check
        );
        archive.extend_from_slice(header.as_bytes());

        // Write name + NUL
        archive.extend_from_slice(name.as_bytes());
        archive.push(0);

        // Pad to 4-byte alignment
        while archive.len() % 4 != 0 {
            archive.push(0);
        }

        // Write data
        if !is_dir {
            archive.extend_from_slice(data);
            // Pad to 4-byte alignment
            while archive.len() % 4 != 0 {
                archive.push(0);
            }
        }

        ino += 1;
    }

    // Write trailer
    let trailer_name = b"TRAILER!!!";
    let namesize = trailer_name.len() + 1;
    let trailer_header = alloc::format!(
        "070701{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
        0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32, namesize, 0u32,
    );
    archive.extend_from_slice(trailer_header.as_bytes());
    archive.extend_from_slice(trailer_name);
    archive.push(0);
    while archive.len() % 4 != 0 {
        archive.push(0);
    }

    archive
}
