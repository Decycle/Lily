//! CRI ADX (standard 4-bit ADPCM, encoding type 3), decoded fully into memory.
//!
//! Header (big-endian): u16 `0x8000`, u16 copyright offset (data starts 4 bytes after it),
//! u8 encoding type, u8 frame size, u8 bit depth, u8 channels, u32 rate, u32 total frames,
//! u16 highpass cutoff (Hz), u8 version, u8 flags (8/9 = encrypted).
//! Each frame is a u16 scale and `frame_size - 2` bytes of signed nibbles (high nibble first);
//! channels take turns frame by frame. References: MultimediaWiki "CRI ADX ADPCM",
//! vgmstream `adx_decoder.c`, FFmpeg `adxdec.c`.
//!
//! WACCA's 20 ADX waveforms (in MER_SE) are all type 3, version 4, unencrypted
//! (`docs/research/03-audio-and-jackets.md` §1.5), so other types are rejected.

use crate::Error;
use crate::cri::{bytes, slice};

const MAGIC: u16 = 0x8000;
const TYPE_STANDARD: u8 = 3;

/// Decoded ADX: interleaved samples in -1..1.
#[derive(Debug, Clone, PartialEq)]
pub struct Adx {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

fn err(msg: impl Into<String>) -> Error {
    Error::Adx(msg.into())
}

/// Fixed-point (×4096) prediction coefficients for a highpass cutoff, as in the reference
/// decoders: `c = (a − √((a+b)(a−b))) / b` with `a = √2 − cos(2π·cutoff/rate)`, `b = √2 − 1`.
pub fn coefficients(cutoff: u16, rate: u32) -> (i32, i32) {
    let z = (std::f64::consts::TAU * f64::from(cutoff) / f64::from(rate)).cos();
    let a = std::f64::consts::SQRT_2 - z;
    let b = std::f64::consts::SQRT_2 - 1.0;
    let c = (a - ((a + b) * (a - b)).sqrt()) / b;
    ((c * 8192.0) as i32, (c * c * -4096.0) as i32)
}

/// Decode a complete ADX file.
pub fn decode(buf: &[u8]) -> Result<Adx, Error> {
    let u16_at = |o| bytes::<2>(buf, o, "ADX header").map(u16::from_be_bytes);
    let u32_at = |o| bytes::<4>(buf, o, "ADX header").map(u32::from_be_bytes);
    if u16_at(0)? != MAGIC {
        return Err(Error::BadMagic {
            what: "ADX",
            expected: "\\x80\\x00",
            offset: 0,
        });
    }
    let data_at = usize::from(u16_at(2)?) + 4;
    let [kind, frame_size, bits, channels] = bytes::<4>(buf, 4, "ADX header")?;
    let rate = u32_at(8)?;
    let total = u32_at(12)? as usize;
    let cutoff = u16_at(16)?;
    let [_version, flags] = bytes::<2>(buf, 18, "ADX header")?;
    if kind != TYPE_STANDARD {
        return Err(err(format!("encoding type {kind} not supported")));
    }
    if matches!(flags, 8 | 9) {
        return Err(err("encrypted ADX not supported"));
    }
    if bits != 4 || frame_size <= 2 || channels == 0 || rate == 0 {
        return Err(err(format!(
            "bad header: {bits} bits, frame {frame_size}, {channels} ch, {rate} Hz"
        )));
    }
    let (ch, frame_size) = (usize::from(channels), usize::from(frame_size));
    let per_frame = (frame_size - 2) * 2;
    // Check the length up front so a bad frame count cannot cause a huge allocation.
    let need = total.div_ceil(per_frame) * ch * frame_size;
    slice(buf, data_at, need, "ADX data")?;
    let (c1, c2) = coefficients(cutoff, rate);
    let mut hist = vec![(0i32, 0i32); ch];
    let mut samples = vec![0f32; total * ch];
    for (f, out) in samples.chunks_mut(per_frame * ch).enumerate() {
        for (c, (h1, h2)) in hist.iter_mut().enumerate() {
            let at = data_at + (f * ch + c) * frame_size;
            let frame = slice(buf, at, frame_size, "ADX frame")?;
            let scale = i32::from(u16::from_be_bytes([frame[0], frame[1]]));
            let nibbles = frame[2..].iter().flat_map(|&b| [b >> 4, b & 0xF]);
            for (i, n) in nibbles.enumerate().take(out.len() / ch) {
                let d = i32::from(((n << 4) as i8) >> 4);
                let s = (d * scale + ((c1 * *h1 + c2 * *h2) >> 12)).clamp(-32768, 32767);
                (*h2, *h1) = (*h1, s);
                out[i * ch + c] = s as f32 / 32768.0;
            }
        }
    }
    log::debug!("ADX: {ch} ch, {rate} Hz, {total} frames, cutoff {cutoff} Hz");
    Ok(Adx {
        channels: channels.into(),
        sample_rate: rate,
        samples,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const FRAME: usize = 18;

    /// ADX header for `channels`/`rate`/`total`; data starts at 0x20.
    fn header(channels: u8, rate: u32, total: u32, cutoff: u16) -> Vec<u8> {
        let mut b = vec![0x80, 0x00, 0x00, 0x1C, 3, FRAME as u8, 4, channels];
        b.extend(rate.to_be_bytes());
        b.extend(total.to_be_bytes());
        b.extend(cutoff.to_be_bytes());
        b.extend([4, 0]);
        b.resize(0x1A, 0);
        b.extend(b"(c)CRI");
        b
    }

    /// Minimal encoder: per frame, a scale from the largest open-loop residual, then greedy
    /// quantisation against the decoder's own reconstruction.
    pub(crate) fn encode(pcm: &[i16], channels: u8, rate: u32, cutoff: u16) -> Vec<u8> {
        let ch = usize::from(channels);
        let total = pcm.len() / ch;
        let mut out = header(channels, rate, total as u32, cutoff);
        let (c1, c2) = coefficients(cutoff, rate);
        let mut hist = vec![(0i32, 0i32); ch];
        for f in 0..total.div_ceil(32) {
            for (c, (h1, h2)) in hist.iter_mut().enumerate() {
                let x: Vec<i32> = (0..32)
                    .map(|i| pcm.get((f * 32 + i) * ch + c).map_or(0, |&v| i32::from(v)))
                    .collect();
                let (mut p1, mut p2) = (*h1, *h2);
                let mut peak = 1;
                for &v in &x {
                    peak = peak.max((v - ((c1 * p1 + c2 * p2) >> 12)).abs());
                    (p2, p1) = (p1, v);
                }
                let scale = (peak + 6) / 7;
                out.extend((scale as u16).to_be_bytes());
                let mut nib = Vec::new();
                for &v in &x {
                    let pred = (c1 * *h1 + c2 * *h2) >> 12;
                    let d = ((v - pred) as f32 / scale as f32).round().clamp(-8.0, 7.0) as i32;
                    let s = (d * scale + pred).clamp(-32768, 32767);
                    (*h2, *h1) = (*h1, s);
                    nib.push((d & 0xF) as u8);
                }
                out.extend(nib.chunks(2).map(|p| (p[0] << 4) | p[1]));
            }
        }
        out
    }

    #[test]
    fn coefficients_match_reference_values() {
        // 500 Hz at 44.1 kHz is the usual CRI default; reference decoders give 7334 / -3283.
        assert_eq!(coefficients(500, 44100), (7334, -3283));
        // Cutoff 0: c = 1, i.e. linear extrapolation 2·h1 − h2.
        assert_eq!(coefficients(0, 48000), (8192, -4096));
    }

    #[test]
    fn decodes_hand_made_frame() {
        // Cutoff 0 → s = d·scale + 2·h1 − h2. Nibbles 1, 0, 0, -1 with scale 10:
        // 10, 20, 30, 30.
        let mut adx = header(1, 48000, 4, 0);
        adx.extend(10u16.to_be_bytes());
        adx.extend([0x10, 0x0F]);
        adx.extend([0; 14]);
        let got = decode(&adx).unwrap();
        assert_eq!((got.channels, got.sample_rate), (1, 48000));
        let ints: Vec<i32> = got.samples.iter().map(|v| (v * 32768.0) as i32).collect();
        assert_eq!(ints, [10, 20, 30, 30]);
    }

    #[test]
    fn round_trips_a_stereo_sine() {
        let total = 1000;
        let pcm: Vec<i16> = (0..total)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 12000.0;
                [v as i16, (-v * 0.5) as i16]
            })
            .collect();
        let got = decode(&encode(&pcm, 2, 44100, 500)).unwrap();
        assert_eq!((got.channels, got.sample_rate), (2, 44100));
        assert_eq!(got.samples.len(), pcm.len());
        let err = got
            .samples
            .iter()
            .zip(&pcm)
            .fold(0f32, |m, (a, &b)| m.max((a - f32::from(b) / 32768.0).abs()));
        assert!(err < 0.01, "max error {err}");
    }

    #[test]
    fn rejects_bad_magic_encryption_other_types_and_truncation() {
        assert!(matches!(decode(&[0; 32]), Err(Error::BadMagic { .. })));
        let good = encode(&[0; 64], 1, 44100, 500);
        let mut enc = good.clone();
        enc[0x13] = 9;
        assert!(matches!(decode(&enc), Err(Error::Adx(_))));
        let mut ahx = good.clone();
        ahx[4] = 0x10;
        assert!(matches!(decode(&ahx), Err(Error::Adx(_))));
        assert!(matches!(
            decode(&good[..good.len() - 1]),
            Err(Error::Truncated { .. })
        ));
    }
}
