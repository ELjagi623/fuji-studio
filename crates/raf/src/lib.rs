//! Minimal Fujifilm RAF reader: header fields + embedded preview JPEG.
//!
//! RAF header layout (big-endian):
//! 0x00 "FUJIFILMCCD-RAW " (16) | 0x10 format version (4) | 0x14 camera id (8)
//! 0x1C camera model (32, NUL padded) | 0x3C dir version (4)
//! 0x54 JPEG offset u32 | 0x58 JPEG length u32
//! 0x5C CFA header offset | 0x60 length | 0x64 CFA offset | 0x68 length

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 16] = b"FUJIFILMCCD-RAW ";
const HEADER_LEN: usize = 0x6C;

#[derive(Debug, Clone)]
pub struct RafHeader {
    pub format_version: String,
    pub camera_id: String,
    pub model: String,
    pub jpeg_offset: u32,
    pub jpeg_length: u32,
    pub cfa_header_offset: u32,
    pub cfa_header_length: u32,
    pub cfa_offset: u32,
    pub cfa_length: u32,
}

fn be32(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn ascii(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

pub fn parse_header(b: &[u8]) -> Result<RafHeader> {
    if b.len() < HEADER_LEN {
        bail!("file too short for RAF header");
    }
    if &b[..16] != MAGIC {
        bail!("not a Fujifilm RAF (bad magic)");
    }
    Ok(RafHeader {
        format_version: ascii(&b[0x10..0x14]),
        camera_id: ascii(&b[0x14..0x1C]),
        model: ascii(&b[0x1C..0x3C]),
        jpeg_offset: be32(b, 0x54),
        jpeg_length: be32(b, 0x58),
        cfa_header_offset: be32(b, 0x5C),
        cfa_header_length: be32(b, 0x60),
        cfa_offset: be32(b, 0x64),
        cfa_length: be32(b, 0x68),
    })
}

pub fn read_header(path: &Path) -> Result<RafHeader> {
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = [0u8; HEADER_LEN];
    f.read_exact(&mut buf)?;
    parse_header(&buf)
}

/// Read only the embedded preview JPEG (fast thumbnail, no RAW decode).
pub fn read_embedded_jpeg(path: &Path) -> Result<Vec<u8>> {
    let h = read_header(path)?;
    if h.jpeg_length == 0 {
        bail!("RAF has no embedded JPEG");
    }
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(h.jpeg_offset as u64))?;
    let mut jpg = vec![0u8; h.jpeg_length as usize];
    f.read_exact(&mut jpg)?;
    if jpg.len() < 2 || jpg[0] != 0xFF || jpg[1] != 0xD8 {
        bail!("embedded preview is not a JPEG");
    }
    Ok(jpg)
}

pub fn has_raf_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("raf"))
        .unwrap_or(false)
}

/// List RAF files in a folder (optionally recursive), sorted by path.
/// Only files with a .RAF extension *and* a valid RAF magic are returned.
pub fn scan_dir(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(rd) => rd,
            Err(e) if d != dir => {
                eprintln!("skip {}: {e}", d.display());
                continue;
            }
            Err(e) => return Err(e).with_context(|| format!("read_dir {}", d.display())),
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let ft = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                if recursive {
                    stack.push(p);
                }
            } else if has_raf_extension(&p) && read_header(&p).is_ok() {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folder names with spaces / Chinese, nested folders, and the `._X.RAF` AppleDouble
    /// junk macOS writes next to files on exFAT/FAT cards and drives.
    #[test]
    fn scan_handles_unicode_paths_and_appledouble_junk() {
        let mut raf = vec![0u8; HEADER_LEN];
        raf[..16].copy_from_slice(MAGIC);
        raf[0x1C..0x23].copy_from_slice(b"X-Pro3\0");
        let root = std::env::temp_dir().join(format!("raf-scan-{}", std::process::id())).join("照片 test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("子文件夹 2")).unwrap();
        std::fs::write(root.join("DSCF0001.RAF"), &raf).unwrap();
        std::fs::write(root.join("子文件夹 2").join("DSCF0002.raf"), &raf).unwrap();
        std::fs::write(root.join("._DSCF0001.RAF"), [0u8, 5, 22, 7, 0, 2, 0, 0, 0]).unwrap(); // AppleDouble header
        std::fs::write(root.join(".DS_Store"), b"x").unwrap();
        let flat = scan_dir(&root, false).unwrap();
        assert_eq!(flat, vec![root.join("DSCF0001.RAF")]);
        let deep = scan_dir(&root, true).unwrap();
        assert_eq!(deep.len(), 2);
        assert!(deep.iter().all(|p| !p.file_name().unwrap().to_string_lossy().starts_with("._")));
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_bad_magic() {
        let b = vec![0u8; HEADER_LEN];
        assert!(parse_header(&b).is_err());
    }

    #[test]
    fn parses_synthetic_header() {
        let mut b = vec![0u8; HEADER_LEN];
        b[..16].copy_from_slice(MAGIC);
        b[0x10..0x14].copy_from_slice(b"0201");
        b[0x1C..0x23].copy_from_slice(b"X-Pro3\0");
        b[0x54..0x58].copy_from_slice(&0x100u32.to_be_bytes());
        b[0x58..0x5C].copy_from_slice(&0x2000u32.to_be_bytes());
        let h = parse_header(&b).unwrap();
        assert_eq!(h.model, "X-Pro3");
        assert_eq!(h.jpeg_offset, 0x100);
        assert_eq!(h.jpeg_length, 0x2000);
    }
}
