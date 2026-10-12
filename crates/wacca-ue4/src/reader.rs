//! Little-endian byte cursor with UE4 `FString` support.

use crate::Ue4Error;

/// Longest `FString` we accept (in characters); guards against garbage lengths.
const MAX_STRING_LEN: usize = 1 << 20;

#[derive(Debug)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn at(data: &'a [u8], pos: usize) -> Result<Self, Ue4Error> {
        let mut r = Self::new(data);
        r.seek(pos)?;
        Ok(r)
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    pub(crate) fn seek(&mut self, pos: usize) -> Result<(), Ue4Error> {
        if pos > self.data.len() {
            return Err(self.eof(pos - self.pos));
        }
        self.pos = pos;
        Ok(())
    }

    fn eof(&self, need: usize) -> Ue4Error {
        Ue4Error::Eof {
            offset: self.pos,
            need,
            len: self.data.len(),
        }
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], Ue4Error> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or_else(|| self.eof(n))?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Ue4Error> {
        let mut out = [0; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Ue4Error> {
        Ok(self.array::<1>()?[0])
    }
    pub(crate) fn i8(&mut self) -> Result<i8, Ue4Error> {
        self.array().map(i8::from_le_bytes)
    }
    pub(crate) fn i16(&mut self) -> Result<i16, Ue4Error> {
        self.array().map(i16::from_le_bytes)
    }
    pub(crate) fn u16(&mut self) -> Result<u16, Ue4Error> {
        self.array().map(u16::from_le_bytes)
    }
    pub(crate) fn i32(&mut self) -> Result<i32, Ue4Error> {
        self.array().map(i32::from_le_bytes)
    }
    pub(crate) fn u32(&mut self) -> Result<u32, Ue4Error> {
        self.array().map(u32::from_le_bytes)
    }
    pub(crate) fn i64(&mut self) -> Result<i64, Ue4Error> {
        self.array().map(i64::from_le_bytes)
    }
    pub(crate) fn u64(&mut self) -> Result<u64, Ue4Error> {
        self.array().map(u64::from_le_bytes)
    }
    pub(crate) fn f32(&mut self) -> Result<f32, Ue4Error> {
        self.array().map(f32::from_le_bytes)
    }
    pub(crate) fn f64(&mut self) -> Result<f64, Ue4Error> {
        self.array().map(f64::from_le_bytes)
    }

    /// An `int32` element count; negative counts are an error.
    pub(crate) fn count(&mut self) -> Result<usize, Ue4Error> {
        let offset = self.pos;
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| Ue4Error::BadCount { count: n, offset })
    }

    /// `FString`: `i32` length incl. NUL; positive = Latin-1 bytes, negative = UTF-16LE units.
    pub(crate) fn fstring(&mut self) -> Result<String, Ue4Error> {
        let offset = self.pos;
        let len = self.i32()?;
        let n = len.unsigned_abs() as usize;
        if n > MAX_STRING_LEN {
            return Err(Ue4Error::BadStringLength { len, offset });
        }
        if len >= 0 {
            let b = self.bytes(n)?;
            let b = b.strip_suffix(&[0]).unwrap_or(b);
            Ok(b.iter().map(|&c| char::from(c)).collect())
        } else {
            let b = self.bytes(n * 2)?;
            let units: Vec<u16> = b
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_le_bytes(c))
                .collect();
            let units = units.strip_suffix(&[0]).unwrap_or(&units);
            Ok(String::from_utf16_lossy(units))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_numbers() {
        let data = [1, 0, 0, 0, 0xff, 0xff, 0, 0, 0x80, 0x3f];
        let mut r = Reader::new(&data);
        assert_eq!(r.i32().unwrap(), 1);
        assert_eq!(r.i16().unwrap(), -1);
        assert_eq!(r.f32().unwrap(), 1.0);
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn eof_is_an_error() {
        let mut r = Reader::new(&[1, 2]);
        assert!(matches!(r.i32(), Err(Ue4Error::Eof { need: 4, .. })));
        assert!(r.seek(3).is_err());
    }

    #[test]
    fn reads_latin1_and_utf16_strings() {
        let mut data = vec![4, 0, 0, 0, b'a', 0xe9, b'c', 0];
        data.extend([0xfd, 0xff, 0xff, 0xff, 0x42, 0x30, 0x44, 0x30, 0, 0]); // -3 → "あい"
        data.extend([0, 0, 0, 0]);
        let mut r = Reader::new(&data);
        assert_eq!(r.fstring().unwrap(), "aéc");
        assert_eq!(r.fstring().unwrap(), "あい");
        assert_eq!(r.fstring().unwrap(), "");
    }

    #[test]
    fn rejects_absurd_string_and_negative_count() {
        let mut r = Reader::new(&[0, 0, 0, 0x10]);
        assert!(matches!(r.fstring(), Err(Ue4Error::BadStringLength { .. })));
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff]);
        assert!(matches!(
            r.count(),
            Err(Ue4Error::BadCount { count: -1, .. })
        ));
    }
}
