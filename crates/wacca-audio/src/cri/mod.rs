//! CRI container formats: `@UTF` tables, AFS2 archives and the ACB cue map.

pub mod acb;
pub mod afs2;
pub mod utf;

use crate::Error;

/// Fixed-size slice at `offset`, or a `Truncated` error naming `what`.
pub(crate) fn bytes<const N: usize>(
    buf: &[u8],
    offset: usize,
    what: &'static str,
) -> Result<[u8; N], Error> {
    offset
        .checked_add(N)
        .and_then(|end| buf.get(offset..end))
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::Truncated {
            what,
            offset,
            need: N,
            have: buf.len(),
        })
}

/// `len` bytes at `offset`, or a `Truncated` error naming `what`.
pub(crate) fn slice<'a>(
    buf: &'a [u8],
    offset: usize,
    len: usize,
    what: &'static str,
) -> Result<&'a [u8], Error> {
    offset
        .checked_add(len)
        .and_then(|end| buf.get(offset..end))
        .ok_or(Error::Truncated {
            what,
            offset,
            need: len,
            have: buf.len(),
        })
}
