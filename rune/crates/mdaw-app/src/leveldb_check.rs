//! Phase 10 review finding 8: checks a copied Chromium LevelDB before `rusty-leveldb` reads it.
//!
//! `rusty-leveldb` 4.0.2 asserts (panics) on some corrupt tables, and its log reader drops a
//! record whose checksum fails without saying so (a flipped byte in the write-ahead log read as
//! an empty profile). Every file is therefore verified first, with LevelDB's own format rules:
//!
//! - **Logs** (`*.log`, `MANIFEST-*`): 32 KiB blocks of records `crc32c (masked) | length u16 |
//!   type u8 | data`; every record's checksum, type and length must be right. A record cut off
//!   at the end of the file is **transient** (the file was being written while it was copied).
//! - **Tables** (`*.ldb`, `*.sst`): the 48-byte footer (magic `0xdb4775248b80fb57`), the
//!   metaindex and index blocks, and every data block the index names: in bounds, checksum
//!   (`crc32c` of the block and its compression byte, masked) right, a known compression
//!   (none / snappy), index entries well formed.

use std::path::Path;

/// What is wrong with a database folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// A file is damaged (the import can't trust what it would read).
    Corrupt(String),
    /// A file ends mid-record (copied while the Electron app wrote it): try again.
    Transient(String),
}

const BLOCK: usize = 32 * 1024;
const HEADER: usize = 7;
const TABLE_MAGIC: u64 = 0xdb47_7524_8b80_fb57;

fn mask(crc: u32) -> u32 {
    crc.rotate_right(15).wrapping_add(0xa282_ead8)
}

fn crc_with_type(t: u8, data: &[u8]) -> u32 {
    crc32c::crc32c_append(crc32c::crc32c(&[t]), data)
}

/// Checks a log-format file (`*.log`, `MANIFEST-*`).
pub fn check_log(name: &str, b: &[u8]) -> Result<(), CheckError> {
    let corrupt = |what: &str, at: usize| CheckError::Corrupt(format!("{name}: {what} at {at}"));
    let mut pos = 0;
    while pos < b.len() {
        let left = BLOCK - pos % BLOCK;
        if left < HEADER {
            // A block's trailer: zero padding.
            if b[pos..(pos + left).min(b.len())].iter().any(|&x| x != 0) {
                return Err(corrupt("non-zero block trailer", pos));
            }
            pos += left;
            continue;
        }
        if pos + HEADER > b.len() {
            return Err(CheckError::Transient(format!(
                "{name}: a record header is cut off at {pos}"
            )));
        }
        let stored = u32::from_le_bytes(b[pos..pos + 4].try_into().expect("4 bytes"));
        let len = u16::from_le_bytes([b[pos + 4], b[pos + 5]]) as usize;
        let t = b[pos + 6];
        if t == 0 && len == 0 && stored == 0 {
            // Zero-filled space (preallocated): the rest of the block must be zero too.
            let end = (pos + left).min(b.len());
            if b[pos..end].iter().any(|&x| x != 0) {
                return Err(corrupt("garbage after zero padding", pos));
            }
            pos = end;
            continue;
        }
        if !(1..=4).contains(&t) {
            return Err(corrupt("unknown record type", pos));
        }
        if HEADER + len > left {
            return Err(corrupt("record longer than its block", pos));
        }
        if pos + HEADER + len > b.len() {
            return Err(CheckError::Transient(format!(
                "{name}: the last record is cut off at {pos}"
            )));
        }
        let data = &b[pos + HEADER..pos + HEADER + len];
        if mask(crc_with_type(t, data)) != stored {
            return Err(corrupt("checksum mismatch", pos));
        }
        pos += HEADER + len;
    }
    Ok(())
}

fn varint(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*pos)?;
        *pos += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// The checked (and decompressed) contents of the block at `off` / `size`.
fn block(name: &str, b: &[u8], off: u64, size: u64) -> Result<Vec<u8>, CheckError> {
    let corrupt = |what: &str| CheckError::Corrupt(format!("{name}: {what} (block at {off})"));
    let (off, size) = (off as usize, size as usize);
    let end = off
        .checked_add(size)
        .and_then(|e| e.checked_add(5))
        .filter(|e| *e <= b.len())
        .ok_or_else(|| corrupt("block out of bounds"))?;
    let data = &b[off..off + size];
    let kind = b[off + size];
    let stored = u32::from_le_bytes(b[end - 4..end].try_into().expect("4 bytes"));
    if mask(crc_with_type_tail(data, kind)) != stored {
        return Err(corrupt("block checksum mismatch"));
    }
    match kind {
        0 => Ok(data.to_vec()),
        1 => snap::raw::Decoder::new()
            .decompress_vec(data)
            .map_err(|_| corrupt("bad snappy data")),
        _ => Err(corrupt("unknown compression")),
    }
}

/// `crc32c(data ++ [kind])` (a table block's checksum covers its compression byte last).
fn crc_with_type_tail(data: &[u8], kind: u8) -> u32 {
    crc32c::crc32c_append(crc32c::crc32c(data), &[kind])
}

/// The values of a block's entries (`shared | non_shared | value_len | key delta | value`,
/// restart array at the end).
fn entries(name: &str, blk: &[u8]) -> Result<Vec<Vec<u8>>, CheckError> {
    let corrupt = |what: &str| CheckError::Corrupt(format!("{name}: {what}"));
    if blk.len() < 4 {
        return Err(corrupt("block too short"));
    }
    let n = u32::from_le_bytes(blk[blk.len() - 4..].try_into().expect("4 bytes")) as usize;
    let restarts = n
        .checked_mul(4)
        .and_then(|r| r.checked_add(4))
        .filter(|r| *r <= blk.len())
        .ok_or_else(|| corrupt("bad restart count"))?;
    let limit = blk.len() - restarts;
    let mut pos = 0;
    let mut out = Vec::new();
    let mut last_key_len = 0usize;
    while pos < limit {
        let (Some(shared), Some(non_shared), Some(vlen)) = (
            varint(blk, &mut pos),
            varint(blk, &mut pos),
            varint(blk, &mut pos),
        ) else {
            return Err(corrupt("bad entry header"));
        };
        if shared as usize > last_key_len {
            return Err(corrupt("shared key prefix longer than the previous key"));
        }
        let end = pos
            .checked_add(non_shared as usize)
            .and_then(|e| e.checked_add(vlen as usize))
            .filter(|e| *e <= limit)
            .ok_or_else(|| corrupt("entry out of bounds"))?;
        last_key_len = shared as usize + non_shared as usize;
        out.push(blk[pos + non_shared as usize..end].to_vec());
        pos = end;
    }
    Ok(out)
}

/// Checks a table file (`*.ldb`, `*.sst`).
pub fn check_table(name: &str, b: &[u8]) -> Result<(), CheckError> {
    let corrupt = |what: &str| CheckError::Corrupt(format!("{name}: {what}"));
    if b.len() < 48 {
        return Err(CheckError::Transient(format!(
            "{name}: shorter than a footer"
        )));
    }
    let footer = &b[b.len() - 48..];
    if u64::from_le_bytes(footer[40..48].try_into().expect("8 bytes")) != TABLE_MAGIC {
        return Err(corrupt("bad table magic"));
    }
    let mut pos = 0;
    let handles: Option<[u64; 4]> = (|| {
        Some([
            varint(footer, &mut pos)?,
            varint(footer, &mut pos)?,
            varint(footer, &mut pos)?,
            varint(footer, &mut pos)?,
        ])
    })();
    let [meta_off, meta_size, index_off, index_size] =
        handles.ok_or_else(|| corrupt("bad footer"))?;
    block(name, b, meta_off, meta_size)?;
    let index = block(name, b, index_off, index_size)?;
    for h in entries(name, &index)? {
        let mut p = 0;
        let (Some(off), Some(size)) = (varint(&h, &mut p), varint(&h, &mut p)) else {
            return Err(corrupt("bad index entry"));
        };
        let data = block(name, b, off, size)?;
        entries(name, &data)?;
    }
    Ok(())
}

/// Checks every log and table in `dir`. Ok(true) when at least one holds data.
pub fn check_dir(dir: &Path) -> Result<bool, CheckError> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| CheckError::Transient(format!("{}: {e}", dir.display())))?;
    let mut data = false;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_log = name.ends_with(".log") || name.starts_with("MANIFEST-");
        let is_table = name.ends_with(".ldb") || name.ends_with(".sst");
        if !is_log && !is_table {
            continue;
        }
        let b = std::fs::read(e.path())
            .map_err(|err| CheckError::Transient(format!("{name}: {err}")))?;
        if is_log {
            check_log(&name, &b)?;
            if name.ends_with(".log") && b.iter().any(|&x| x != 0) {
                data = true;
            }
        } else {
            check_table(&name, &b)?;
            data = true;
        }
    }
    Ok(data)
}
