//! AFS2 (`.awb`) archive index.
//!
//! Header (little-endian): `"AFS2"`, u8 version, u8 offset_size, u16 id_size, u32 count,
//! u16 align, u16 subkey, then `count` ids and `count + 1` offsets. Offsets are absolute in the
//! archive; entry `i` spans `[align_up(offset[i]), offset[i + 1])`.
//! Reference: vgmstream `awb.c`, `tools/research/cri_utf.py`.
//!
//! Only the header is ever read, so the 2.8 GB `MER_BGM.awb` is never loaded into memory.

use std::io::{Read, Seek, SeekFrom};

use super::{bytes, slice};
use crate::Error;

const MAGIC: &[u8; 4] = b"AFS2";
const HEADER_LEN: usize = 16;

/// One archive entry: its id and byte range in the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub start: u64,
    pub end: u64,
}

impl Entry {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
}

/// Parsed AFS2 header: entry ids and byte ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub version: u8,
    pub align: u16,
    pub subkey: u16,
    entries: Vec<Entry>,
}

/// Field sizes from the fixed header.
struct Layout {
    offset_size: usize,
    id_size: usize,
    count: usize,
}

impl Layout {
    fn parse(buf: &[u8]) -> Result<Self, Error> {
        if bytes::<4>(buf, 0, "AFS2 header")? != *MAGIC {
            return Err(Error::BadMagic {
                what: "AFS2",
                expected: "AFS2",
                offset: 0,
            });
        }
        let offset_size = bytes::<1>(buf, 5, "AFS2 header")?[0];
        let id_size = u16::from_le_bytes(bytes(buf, 6, "AFS2 header")?);
        let count = u32::from_le_bytes(bytes(buf, 8, "AFS2 header")?) as usize;
        if !matches!(offset_size, 2 | 4 | 8) {
            return Err(Error::UnsupportedFieldSize(offset_size.into()));
        }
        if !matches!(id_size, 2 | 4) {
            return Err(Error::UnsupportedFieldSize(id_size));
        }
        Ok(Layout {
            offset_size: offset_size.into(),
            id_size: id_size.into(),
            count,
        })
    }

    fn total_len(&self) -> usize {
        HEADER_LEN + self.count * self.id_size + (self.count + 1) * self.offset_size
    }
}

/// Little-endian unsigned integer of 2, 4 or 8 bytes.
fn uint_le(buf: &[u8], offset: usize, size: usize) -> Result<u64, Error> {
    let s = slice(buf, offset, size, "AFS2 table")?;
    Ok(s.iter().rev().fold(0, |acc, &b| (acc << 8) | u64::from(b)))
}

fn align_up(v: u64, align: u16) -> u64 {
    let a = u64::from(align.max(1));
    v.div_ceil(a) * a
}

impl Index {
    /// Parse a complete AFS2 header (e.g. the copy in the ACB's `StreamAwbAfs2Header`).
    pub fn parse(buf: &[u8]) -> Result<Self, Error> {
        let l = Layout::parse(buf)?;
        let version = bytes::<1>(buf, 4, "AFS2 header")?[0];
        let align = u16::from_le_bytes(bytes(buf, 12, "AFS2 header")?);
        let subkey = u16::from_le_bytes(bytes(buf, 14, "AFS2 header")?);
        let ids_at = HEADER_LEN;
        let offs_at = ids_at + l.count * l.id_size;
        let mut entries = Vec::with_capacity(l.count.min(buf.len()));
        for i in 0..l.count {
            let id = uint_le(buf, ids_at + i * l.id_size, l.id_size)? as u32;
            let raw_start = uint_le(buf, offs_at + i * l.offset_size, l.offset_size)?;
            let end = uint_le(buf, offs_at + (i + 1) * l.offset_size, l.offset_size)?;
            let start = align_up(raw_start, align).min(end);
            entries.push(Entry { id, start, end });
        }
        Ok(Index {
            version,
            align,
            subkey,
            entries,
        })
    }

    /// Read only the header of an archive starting at the reader's offset 0.
    pub fn read<R: Read + Seek>(reader: &mut R) -> Result<Self, Error> {
        let mut buf = vec![0; HEADER_LEN];
        reader.seek(SeekFrom::Start(0))?;
        reader.read_exact(&mut buf)?;
        let total = Layout::parse(&buf)?.total_len();
        buf.resize(total, 0);
        reader.read_exact(&mut buf[HEADER_LEN..])?;
        Self::parse(&buf)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with archive id `id`.
    pub fn find(&self, id: u32) -> Option<Entry> {
        self.entries.iter().copied().find(|e| e.id == id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// AFS2 header with 2-byte ids, 4-byte offsets; entry data follows the given offsets.
    pub(crate) fn header(ids: &[u16], offsets: &[u32], align: u16) -> Vec<u8> {
        let mut b = b"AFS2".to_vec();
        b.extend([2, 4]);
        b.extend(2u16.to_le_bytes());
        b.extend((ids.len() as u32).to_le_bytes());
        b.extend(align.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        ids.iter().for_each(|i| b.extend(i.to_le_bytes()));
        offsets.iter().for_each(|o| b.extend(o.to_le_bytes()));
        b
    }

    #[test]
    fn parses_ids_and_aligned_ranges() {
        let h = header(&[5, 9], &[0x22, 0x50, 0x61], 32);
        let idx = Index::parse(&h).unwrap();
        assert_eq!(idx.version, 2);
        assert_eq!(idx.align, 32);
        assert_eq!(
            idx.entries(),
            &[
                Entry {
                    id: 5,
                    start: 0x40,
                    end: 0x50
                },
                Entry {
                    id: 9,
                    start: 0x60,
                    end: 0x61
                }
            ]
        );
        assert_eq!(idx.find(9).unwrap().len(), 1);
        assert_eq!(idx.find(1), None);
    }

    #[test]
    fn reads_only_the_header_from_a_reader() {
        let mut file = header(&[1], &[0x20, 0x30], 16);
        file.resize(0x30, 0xAB);
        let idx = Index::read(&mut Cursor::new(file)).unwrap();
        assert_eq!(idx.find(1).unwrap().start, 0x20);
    }

    #[test]
    fn rejects_bad_magic_sizes_and_truncation() {
        assert!(matches!(
            Index::parse(b"AFS3\x02\x04\x02\0\0\0\0\0\0\0\0\0"),
            Err(Error::BadMagic { .. })
        ));
        let mut h = header(&[1], &[0x20, 0x30], 16);
        h[5] = 3;
        assert!(matches!(
            Index::parse(&h),
            Err(Error::UnsupportedFieldSize(3))
        ));
        let h = header(&[1], &[0x20, 0x30], 16);
        assert!(matches!(
            Index::parse(&h[..h.len() - 1]),
            Err(Error::Truncated { .. })
        ));
    }
}
