//! Minimal 32-bit float WAV writer (RIFF, `WAVE_FORMAT_IEEE_FLOAT`). The header is written with zero sizes and
//! patched in [`WavWriter::finish`].

use std::io::{self, Seek, SeekFrom, Write};

pub const HEADER_LEN: usize = 44;
const FORMAT_IEEE_FLOAT: u16 = 3;

/// Canonical 44-byte header for `frames` frames of interleaved f32.
pub fn header(sample_rate: u32, channels: u16, frames: u32) -> [u8; HEADER_LEN] {
    let block = u32::from(channels) * 4;
    let data_len = frames.saturating_mul(block);
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&data_len.saturating_add(36).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&FORMAT_IEEE_FLOAT.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * block).to_le_bytes());
    h[32..34].copy_from_slice(&(block as u16).to_le_bytes());
    h[34..36].copy_from_slice(&32u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

#[derive(Debug)]
pub struct WavWriter<W: Write + Seek> {
    out: W,
    sample_rate: u32,
    channels: u16,
    samples: u64,
}

impl<W: Write + Seek> WavWriter<W> {
    pub fn new(mut out: W, sample_rate: u32, channels: u16) -> io::Result<Self> {
        out.write_all(&header(sample_rate, channels, 0))?;
        Ok(Self {
            out,
            sample_rate,
            channels,
            samples: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        samples
            .iter()
            .for_each(|s| bytes.extend_from_slice(&s.to_le_bytes()));
        self.out.write_all(&bytes)?;
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// Patches the sizes; returns the frame count and the inner writer.
    pub fn finish(mut self) -> io::Result<(u64, W)> {
        let frames = self.samples / u64::from(self.channels.max(1));
        let frames32 = u32::try_from(frames).unwrap_or(u32::MAX);
        self.out.seek(SeekFrom::Start(0))?;
        self.out
            .write_all(&header(self.sample_rate, self.channels, frames32))?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok((frames, self.out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn header_bytes_48k_stereo() {
        let h = header(48_000, 2, 3);
        let expected: [u8; HEADER_LEN] = [
            b'R', b'I', b'F', b'F', 60, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ',
            16, 0, 0, 0, 3, 0, 2, 0, 0x80, 0xBB, 0, 0, 0x00, 0xDC, 0x05, 0, 8, 0, 32, 0, b'd',
            b'a', b't', b'a', 24, 0, 0, 0,
        ];
        assert_eq!(h, expected);
    }

    #[test]
    fn writer_patches_sizes() {
        let mut w = WavWriter::new(Cursor::new(Vec::new()), 48_000, 2).unwrap();
        w.write(&[0.5, -0.5, 1.0, 0.0]).unwrap();
        let (frames, out) = w.finish().unwrap();
        let bytes = out.into_inner();
        assert_eq!(frames, 2);
        assert_eq!(bytes.len(), HEADER_LEN + 16);
        assert_eq!(&bytes[..HEADER_LEN], &header(48_000, 2, 2));
        assert_eq!(&bytes[HEADER_LEN..HEADER_LEN + 4], &0.5f32.to_le_bytes());
    }
}
