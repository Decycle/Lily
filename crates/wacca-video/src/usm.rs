//! CRI Sofdec2 `.usm` demuxer (06-visuals §1 "Chunk layout").
//!
//! A USM file is a sequence of big-endian chunks:
//! `sig[4] size u32 | u8 0, u8 payload_off, u16 pad | u8 chno, u16, u8 type | u32 frame_time, u32 frame_rate | 8 bytes`
//! with the payload at `[8 + payload_off, 8 + size - pad)`. Concatenating the `@SFV` data payloads gives the
//! MPEG-1 elementary stream; every data chunk holds exactly one picture (verified on the game's files).

use std::io::{ErrorKind, Read};

use crate::Error;

/// Bytes before `payload_off` is counted (signature + size).
const PREFIX: usize = 8;
/// Fixed part of the chunk header: prefix + the 0x18-byte header that `payload_off` usually points past.
const HEADER: usize = PREFIX + 0x18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// `CRID`: file directory (@UTF table).
    Crid,
    /// `@SFV`: video.
    Video,
    /// `@SFA`: audio (the game's movies have none).
    Audio,
    /// Anything else (`@SBT` subtitles, `@CUE`, `@ALP` alpha, ...): skipped.
    Other([u8; 4]),
}

impl Stream {
    fn from_sig(sig: [u8; 4]) -> Self {
        match &sig {
            b"CRID" => Self::Crid,
            b"@SFV" => Self::Video,
            b"@SFA" => Self::Audio,
            _ => Self::Other(sig),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataType {
    Data,
    /// @UTF header table.
    Header,
    /// `#HEADER END` / `#METADATA END` / `#CONTENTS END` markers.
    End,
    /// Seek / metadata table.
    Seek,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHeader {
    pub stream: Stream,
    pub channel: u8,
    pub data_type: DataType,
    /// Presentation time in units of `frame_rate`-dependent ticks (≈ 1/3000 s, informational only).
    pub frame_time: u32,
    /// Frames per 100 s (6000 = 60.00 fps).
    pub frame_rate: u32,
    /// Bytes from the start of the chunk to the payload.
    pub payload_start: usize,
    pub payload_len: usize,
    /// Bytes from the start of this chunk to the next one.
    pub total_len: usize,
}

/// Parse the fixed header at the start of `b` (needs at least 0x20 bytes).
pub fn parse_header(b: &[u8]) -> Result<ChunkHeader, Error> {
    let h: &[u8; HEADER] = b
        .get(..HEADER)
        .and_then(|h| h.try_into().ok())
        .ok_or(Error::Usm("truncated chunk header"))?;
    let be32 = |o: usize| u32::from_be_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]);
    let size = be32(4) as usize;
    let payload_off = usize::from(h[9]);
    let pad = usize::from(u16::from_be_bytes([h[10], h[11]]));
    let payload_len = size
        .checked_sub(payload_off + pad)
        .ok_or(Error::Usm("chunk payload offset/padding exceed its size"))?;
    if PREFIX + payload_off < HEADER {
        return Err(Error::Usm("chunk payload overlaps its header"));
    }
    let data_type = match h[15] & 3 {
        0 => DataType::Data,
        1 => DataType::Header,
        2 => DataType::End,
        _ => DataType::Seek,
    };
    Ok(ChunkHeader {
        stream: Stream::from_sig([h[0], h[1], h[2], h[3]]),
        channel: h[12],
        data_type,
        frame_time: be32(16),
        frame_rate: be32(20),
        payload_start: PREFIX + payload_off,
        payload_len,
        total_len: PREFIX + size,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub header: ChunkHeader,
    pub payload: Vec<u8>,
}

/// Reads chunks one by one from a byte stream (the file is streamed, never loaded whole).
#[derive(Debug)]
pub struct ChunkReader<R> {
    inner: R,
}

impl<R: Read> ChunkReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// The next chunk, `None` at a clean end of stream.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>, Error> {
        let mut head = [0u8; HEADER];
        match self.inner.read_exact(&mut head[..1]) {
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            r => r?,
        }
        self.inner.read_exact(&mut head[1..])?;
        let header = parse_header(&head)?;
        let mut rest = vec![0; header.total_len.saturating_sub(HEADER)];
        self.inner.read_exact(&mut rest)?;
        let start = header.payload_start - HEADER;
        let payload = rest
            .get(start..start + header.payload_len)
            .ok_or(Error::Usm("chunk payload past its end"))?
            .to_vec();
        Ok(Some(Chunk { header, payload }))
    }

    /// The next `@SFV` data payload (one picture), skipping every other chunk.
    pub fn next_picture(&mut self) -> Result<Option<Chunk>, Error> {
        while let Some(c) = self.next_chunk()? {
            if c.header.stream == Stream::Video && c.header.data_type == DataType::Data {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }
}

/// MPEG-1 sequence header fields (ISO 11172-2 §2.4.2.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SequenceHeader {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Finds and parses the first sequence header (`00 00 01 B3`) in `es`.
pub fn sequence_header(es: &[u8]) -> Option<SequenceHeader> {
    let at = es.windows(4).position(|w| w == [0, 0, 1, 0xB3])?;
    let b = es.get(at + 4..at + 8)?;
    let width = (u32::from(b[0]) << 4) | (u32::from(b[1]) >> 4);
    let height = ((u32::from(b[1]) & 0xF) << 8) | u32::from(b[2]);
    let fps = match b[3] & 0xF {
        1 => 24000.0 / 1001.0,
        2 => 24.0,
        3 => 25.0,
        4 => 30000.0 / 1001.0,
        5 => 30.0,
        6 => 50.0,
        7 => 60000.0 / 1001.0,
        8 => 60.0,
        _ => return None,
    };
    Some(SequenceHeader { width, height, fps })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A chunk with the game's layout: payload_off 0x18, `pad` zero bytes of padding.
    pub(crate) fn chunk(
        sig: &[u8; 4],
        data_type: u8,
        frame_time: u32,
        payload: &[u8],
        pad: usize,
    ) -> Vec<u8> {
        let size = 0x18 + payload.len() + pad;
        let mut c = Vec::new();
        c.extend_from_slice(sig);
        c.extend_from_slice(&(size as u32).to_be_bytes());
        c.extend_from_slice(&[0, 0x18]);
        c.extend_from_slice(&(pad as u16).to_be_bytes());
        c.extend_from_slice(&[0, 0, 0, data_type]);
        c.extend_from_slice(&frame_time.to_be_bytes());
        c.extend_from_slice(&6000u32.to_be_bytes());
        c.extend_from_slice(&[0; 8]);
        c.extend_from_slice(payload);
        c.extend(std::iter::repeat_n(0, pad));
        c
    }

    /// Sequence header for 1080x1080, aspect 1, 60 fps (frame_rate_code 8).
    pub(crate) const SEQ_1080: [u8; 8] = [0, 0, 1, 0xB3, 0x43, 0x84, 0x38, 0x18];

    #[test]
    fn parses_header_fields() {
        let c = chunk(b"@SFV", 0, 49, b"abc", 5);
        let h = parse_header(&c).unwrap();
        assert_eq!(h.stream, Stream::Video);
        assert_eq!(h.data_type, DataType::Data);
        assert_eq!((h.frame_time, h.frame_rate), (49, 6000));
        assert_eq!(
            (h.payload_start, h.payload_len, h.total_len),
            (0x20, 3, c.len())
        );
        assert_eq!(
            parse_header(&chunk(b"@SBT", 1, 0, b"", 0)).unwrap().stream,
            Stream::Other(*b"@SBT")
        );
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(parse_header(&[0; 10]).is_err());
        let mut c = chunk(b"@SFV", 0, 0, b"abc", 0);
        c[11] = 200; // padding larger than the chunk
        assert!(parse_header(&c).is_err());
        let mut c = chunk(b"@SFV", 0, 0, b"abc", 0);
        c[9] = 4; // payload inside the header
        assert!(parse_header(&c).is_err());
    }

    #[test]
    fn reads_only_video_pictures() {
        let mut file = chunk(b"CRID", 1, 0, b"@UTF dir", 4);
        file.extend(chunk(b"@SFV", 1, 0, b"@UTF hdr", 0));
        file.extend(chunk(b"@SFA", 0, 0, b"audio", 0));
        file.extend(chunk(b"@SFV", 0, 0, b"pic0", 3));
        file.extend(chunk(b"@SBT", 0, 0, b"sub", 0));
        file.extend(chunk(b"@SFV", 2, 0, b"#CONTENTS END", 0));
        file.extend(chunk(b"@SFV", 0, 49, b"pic1", 0));
        let mut r = ChunkReader::new(file.as_slice());
        let p0 = r.next_picture().unwrap().unwrap();
        assert_eq!(p0.payload, b"pic0");
        let p1 = r.next_picture().unwrap().unwrap();
        assert_eq!(
            (p1.payload.as_slice(), p1.header.frame_time),
            (&b"pic1"[..], 49)
        );
        assert!(r.next_picture().unwrap().is_none());
    }

    #[test]
    fn truncated_chunk_is_an_error() {
        let c = chunk(b"@SFV", 0, 0, b"picture", 0);
        let mut r = ChunkReader::new(&c[..c.len() - 2]);
        assert!(r.next_chunk().is_err());
        assert!(ChunkReader::new(&[][..]).next_chunk().unwrap().is_none());
    }

    #[test]
    fn parses_sequence_header() {
        let mut es = vec![0xFF, 0x00];
        es.extend_from_slice(&SEQ_1080);
        let s = sequence_header(&es).unwrap();
        assert_eq!((s.width, s.height, s.fps), (1080, 1080, 60.0));
        assert!(sequence_header(&SEQ_1080[..6]).is_none());
        assert!(sequence_header(b"no header").is_none());
    }
}
