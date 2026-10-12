//! Streaming PCM decoders. HCA decoding is delegated to the `cridecoder` crate (MIT).

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};

use crate::Error;

/// A source of interleaved `f32` PCM frames.
pub trait Decoder: Send + fmt::Debug {
    fn channels(&self) -> u16;
    fn sample_rate(&self) -> u32;
    /// Total frames (after encoder delay/padding are trimmed).
    fn total_frames(&self) -> u64;
    /// Append the next decoded block of interleaved frames to `out`; `false` at end of stream.
    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error>;
    /// Continue decoding at `frame` (clamped to the end).
    fn seek(&mut self, frame: u64) -> Result<(), Error>;
}

/// `Read + Seek` view of the byte range `[start, start + len)` of `inner`
/// (one AWB entry, so the archive is never read whole).
#[derive(Debug)]
pub struct Slice<R> {
    inner: R,
    start: u64,
    len: u64,
    pos: u64,
}

impl<R: Seek> Slice<R> {
    pub fn new(mut inner: R, start: u64, len: u64) -> io::Result<Self> {
        inner.seek(SeekFrom::Start(start))?;
        Ok(Slice {
            inner,
            start,
            len,
            pos: 0,
        })
    }
}

impl<R: Read> Read for Slice<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = usize::try_from(self.len - self.pos).unwrap_or(usize::MAX);
        let want = buf.len().min(left);
        let n = self.inner.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Slice<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before slice start"))?;
        let target = target.min(self.len);
        self.inner.seek(SeekFrom::Start(self.start + target))?;
        self.pos = target;
        Ok(target)
    }
}

/// HCA stream decoder. Seeking decodes one priming frame before the target (as vgmstream
/// does) so the IMDCT overlap is valid at the first output sample.
pub struct HcaStream<R: Read + Seek> {
    inner: cridecoder::HcaDecoder<R>,
    channels: u16,
    sample_rate: u32,
    total: u64,
    frames_per_block: u64,
    /// Frame index of the next output frame.
    pos: u64,
    /// Decoded frames still to drop after a seek.
    skip: u64,
}

impl<R: Read + Seek> fmt::Debug for HcaStream<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HcaStream")
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .field("total", &self.total)
            .field("pos", &self.pos)
            .finish()
    }
}

fn hca_err(e: cridecoder::HcaDecoderError) -> Error {
    Error::Hca(e.to_string())
}

impl<R: Read + Seek> HcaStream<R> {
    /// Open an HCA file that starts at the reader's offset 0.
    pub fn new(reader: R) -> Result<Self, Error> {
        let inner = cridecoder::HcaDecoder::from_reader(reader).map_err(hca_err)?;
        let i = inner.info();
        if i.encryption_enabled {
            // WACCA's HCAs are all ciph type 0; keyed audio is out of scope.
            log::warn!("HCA is encrypted; output will be noise");
        }
        let frames_per_block = i.samples_per_block as u64;
        let total = (u64::from(i.block_count) * frames_per_block)
            .saturating_sub(i.encoder_delay.into())
            .saturating_sub(i.encoder_padding.into());
        let channels = u16::try_from(i.channel_count).map_err(|_| Error::Hca("channels".into()))?;
        log::debug!(
            "HCA v{:x}: {} ch, {} Hz, {total} frames",
            i.version,
            channels,
            i.sampling_rate
        );
        Ok(HcaStream {
            channels,
            sample_rate: i.sampling_rate,
            total,
            frames_per_block,
            inner,
            pos: 0,
            skip: 0,
        })
    }
}

impl<R: Read + Seek + Send> Decoder for HcaStream<R> {
    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_frames(&self) -> u64 {
        self.total
    }

    fn decode(&mut self, out: &mut Vec<f32>) -> Result<bool, Error> {
        let ch = usize::from(self.channels);
        while self.pos < self.total {
            let (pcm, n) = match self.inner.decode_frame() {
                Ok(v) => v,
                Err(cridecoder::HcaDecoderError::Eof) => break,
                Err(e) => return Err(hca_err(e)),
            };
            let skip = self.skip.min(n as u64);
            self.skip -= skip;
            let take = (n as u64 - skip).min(self.total - self.pos);
            let (from, to) = (skip as usize * ch, (skip + take) as usize * ch);
            let frames = pcm
                .get(from..to)
                .ok_or_else(|| Error::Hca("decoder returned a short frame".into()))?;
            out.extend_from_slice(frames);
            self.pos += take;
            if take > 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn seek(&mut self, frame: u64) -> Result<(), Error> {
        let frame = frame.min(self.total);
        let prime = frame.min(self.frames_per_block);
        let start = u32::try_from(frame - prime).map_err(|_| Error::Hca("seek".into()))?;
        // reset() also clears the crate's sample counter, which seek() alone does not.
        self.inner.reset();
        self.inner.seek(start);
        self.skip = prime;
        self.pos = frame;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn slice_reads_and_seeks_within_range() {
        let data: Vec<u8> = (0..20).collect();
        let mut s = Slice::new(Cursor::new(data), 5, 10).unwrap();
        let mut buf = [0; 4];
        s.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5, 6, 7, 8]);
        assert_eq!(s.seek(SeekFrom::End(-2)).unwrap(), 8);
        let mut rest = Vec::new();
        s.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, [13, 14]);
        assert_eq!(s.seek(SeekFrom::Start(100)).unwrap(), 10);
        assert!(s.seek(SeekFrom::Current(-11)).is_err());
    }

    /// 0.5 s stereo 48 kHz sine encoded with the crate's own encoder.
    fn sine_hca() -> Vec<u8> {
        let pcm: Vec<f32> = (0..24000)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5;
                [v, -v]
            })
            .collect();
        let config = cridecoder::HcaEncoderConfig::new(48000, 2);
        let mut enc = cridecoder::HcaEncoder::new(config).unwrap();
        let mut out = Cursor::new(Vec::new());
        enc.encode(&pcm, &mut out).unwrap();
        out.into_inner()
    }

    fn decode_all(s: &mut impl Decoder) -> Vec<f32> {
        let mut pcm = Vec::new();
        while s.decode(&mut pcm).unwrap() {}
        pcm
    }

    #[test]
    fn decodes_exact_length_from_a_slice() {
        let mut file = vec![0xEE; 7];
        file.extend(sine_hca());
        let len = file.len() as u64 - 7;
        let mut s = HcaStream::new(Slice::new(Cursor::new(file), 7, len).unwrap()).unwrap();
        assert_eq!(
            (s.channels(), s.sample_rate(), s.total_frames()),
            (2, 48000, 24000)
        );
        let pcm = decode_all(&mut s);
        assert_eq!(pcm.len(), 24000 * 2);
        let peak = pcm.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.3, "peak {peak}");
    }

    #[test]
    fn seek_matches_continuous_decode() {
        let mut s = HcaStream::new(Cursor::new(sine_hca())).unwrap();
        let full = decode_all(&mut s);
        for target in [0u64, 100, 5000, 23999] {
            s.seek(target).unwrap();
            let tail = decode_all(&mut s);
            assert_eq!(tail.len(), (24000 - target as usize) * 2, "target {target}");
            let want = &full[target as usize * 2..];
            let err = tail
                .iter()
                .zip(want)
                .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(err < 1e-5, "target {target}: max diff {err}");
        }
        s.seek(1 << 40).unwrap();
        assert!(decode_all(&mut s).is_empty());
    }

    #[test]
    fn rejects_non_hca() {
        let err = HcaStream::new(Cursor::new(vec![0u8; 64])).unwrap_err();
        assert!(matches!(err, Error::Hca(_)));
    }
}
