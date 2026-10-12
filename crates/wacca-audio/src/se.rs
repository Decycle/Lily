//! The sound-effect bank `Sound/Se/MER_SE.uexp`: one ACB whose waveforms (HCA and ADX) all live
//! in the in-memory AWB of its `AwbFile` column (`docs/research/03-audio-and-jackets.md` §1.5),
//! and the hit sounds of an SE set (§5).

use std::io::Cursor;
use std::path::Path;

use crate::cri::acb::{Acb, Source, Waveform};
use crate::cri::afs2;
use crate::{Decoder, Error, HcaStream, Sfx, adx};

/// UE4 asset that carries the SE ACB.
pub const ACB_FILE: &str = "MER_SE.uexp";
const ENCODE_ADX: u8 = 0;
const ENCODE_HCA: u8 = 2;

/// Opened SE bank. Holds the memory AWB (≈ 13 MB) so any cue can be decoded on demand.
#[derive(Debug, Clone)]
pub struct SeBank {
    acb: Acb,
    awb: Vec<u8>,
    index: afs2::Index,
}

impl SeBank {
    /// Open `<dir>/MER_SE.uexp` (`dir` = `<assets>/Sound/Se`).
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let path = dir.as_ref().join(ACB_FILE);
        let bytes = std::fs::read(&path).map_err(Error::file(&path))?;
        Self::parse(&bytes)
    }

    /// Parse an ACB that carries a memory AWB.
    pub fn parse(buf: &[u8]) -> Result<Self, Error> {
        let (acb, awb) = Acb::parse_with_awb(buf)?;
        let awb = awb.ok_or_else(|| Error::MissingColumn {
            table: "Header".into(),
            column: "AwbFile".into(),
        })?;
        let index = afs2::Index::parse(awb)?;
        log::debug!(
            "SE bank {}: {} memory waveforms",
            acb.name,
            index.entries().len()
        );
        Ok(SeBank {
            acb,
            awb: awb.to_vec(),
            index,
        })
    }

    pub fn acb(&self) -> &Acb {
        &self.acb
    }

    /// Decode one waveform completely. Loop points outside the decoded length are dropped.
    pub fn decode(&self, w: &Waveform) -> Result<Sfx, Error> {
        let unsupported = |reason| Error::UnsupportedWaveform {
            index: w.index,
            reason,
        };
        let Source::Memory { id } = w.source else {
            return Err(unsupported("stream AWB"));
        };
        let entry = self.index.find(id).ok_or_else(|| Error::AwbIdNotFound {
            awb: "<memory AwbFile>".into(),
            id,
        })?;
        let bytes = usize::try_from(entry.start)
            .ok()
            .zip(usize::try_from(entry.end).ok())
            .and_then(|(s, e)| self.awb.get(s..e))
            .ok_or(Error::Truncated {
                what: "memory AWB entry",
                offset: entry.start as usize,
                need: entry.len() as usize,
                have: self.awb.len(),
            })?;
        let (rate, channels, mut samples) = match w.encode_type {
            ENCODE_HCA => {
                let mut s = HcaStream::new(Cursor::new(bytes))?;
                let mut pcm = Vec::new();
                while s.decode(&mut pcm)? {}
                (s.sample_rate(), s.channels(), pcm)
            }
            ENCODE_ADX => {
                let a = adx::decode(bytes)?;
                (a.sample_rate, a.channels, a.samples)
            }
            _ => return Err(unsupported("encode type")),
        };
        // Looped HCAs decode a few hundred frames past their loop end (the `loop` chunk's end
        // padding); the ACB's NumSamples (= loop end) is the playable length.
        let len = w.num_samples as usize * usize::from(channels);
        if samples.len() > len {
            log::debug!(
                "waveform {}: trimming {} decoded samples to NumSamples",
                w.index,
                samples.len() - len
            );
            samples.truncate(len);
        }
        let mut sfx = Sfx {
            rate,
            channels,
            samples: samples.into(),
            loop_range: None,
        };
        if let Some((start, end)) = w.loop_range {
            let (start, end) = (start as usize, end as usize);
            if start < end && end <= sfx.frames() {
                sfx.loop_range = Some((start, end));
            } else {
                log::warn!(
                    "waveform {}: loop {start}..{end} outside {} frames; ignored",
                    w.index,
                    sfx.frames()
                );
            }
        }
        Ok(sfx)
    }

    /// Decode every waveform the cue plays, in track order (they start together; e.g. a hold
    /// cue is a looping sustain plus a one-shot attack).
    pub fn decode_cue(&self, cue: &str) -> Result<Vec<Sfx>, Error> {
        let tracks = self.acb.tracks(cue)?;
        if tracks.is_empty() {
            return Err(Error::CueSilent(cue.to_string()));
        }
        tracks.iter().map(|t| self.decode(&t.waveform)).collect()
    }
}

/// Cue names of one `SESetTable` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeSet {
    pub id: u32,
    pub marvelous: &'static str,
    pub good: &'static str,
    pub slide_marvelous: &'static str,
    pub slide_good: &'static str,
    pub snap_marvelous: &'static str,
    pub snap_good: &'static str,
    pub hold_marvelous: &'static str,
    pub hold_good: &'static str,
    pub re_hold: &'static str,
    pub chain: &'static str,
}

/// Row 205005 of `Table/SESetTable` (`docs/research/tables/SESetTable.json`, doc §5).
// UNVERIFIED: that 205005 is the "Default (WACCA)" set the game starts with. Both 205005 and
// 105001 have bIsInitItem = true; the display names were matched to ids by row order only.
pub const DEFAULT_SE_SET: SeSet = SeSet {
    id: 205005,
    marvelous: "MER_SE_SYS_002",
    good: "MER_SE_SYS_001",
    slide_marvelous: "MER_SE_SYS_009",
    slide_good: "MER_SE_SYS_008",
    snap_marvelous: "MER_SE_SYS_111",
    snap_good: "MER_SE_SYS_110",
    hold_marvelous: "MER_SE_SYS_010",
    hold_good: "MER_SE_SYS_003",
    re_hold: "MER_SE_SYS_004",
    chain: "MER_SE_SYS_109",
};

/// Decoded sounds of an [`SeSet`], resampled to the output rate. Each field holds every track
/// of the cue; play them all together. Hold cues contain a looping sustain track, which plays
/// until [`crate::Player::stop_sfx`] is called on its voice.
#[derive(Debug, Clone)]
pub struct HitSounds {
    pub marvelous: Vec<Sfx>,
    pub good: Vec<Sfx>,
    pub slide_marvelous: Vec<Sfx>,
    pub slide_good: Vec<Sfx>,
    pub snap_marvelous: Vec<Sfx>,
    pub snap_good: Vec<Sfx>,
    pub hold_marvelous: Vec<Sfx>,
    pub hold_good: Vec<Sfx>,
    pub re_hold: Vec<Sfx>,
    pub chain: Vec<Sfx>,
}

impl HitSounds {
    /// Decode the cues of `set` and resample them to `rate` (the [`crate::Player`] rate).
    pub fn load(bank: &SeBank, set: &SeSet, rate: u32) -> Result<Self, Error> {
        let load = |cue| -> Result<Vec<Sfx>, Error> {
            Ok(bank
                .decode_cue(cue)?
                .iter()
                .map(|s| s.resampled(rate))
                .collect())
        };
        Ok(HitSounds {
            marvelous: load(set.marvelous)?,
            good: load(set.good)?,
            slide_marvelous: load(set.slide_marvelous)?,
            slide_good: load(set.slide_good)?,
            snap_marvelous: load(set.snap_marvelous)?,
            snap_good: load(set.snap_good)?,
            hold_marvelous: load(set.hold_marvelous)?,
            hold_good: load(set.hold_good)?,
            re_hold: load(set.re_hold)?,
            chain: load(set.chain)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adx::tests::encode as encode_adx;
    use crate::cri::afs2::tests::header as afs2_header;
    use crate::cri::utf::build::{Col, V, table};

    /// ACB with a memory AWB: cue "adx" → ADX waveform (id 4); cue "hold" → sequence of a
    /// looping ADX sustain (id 5) and a one-shot (id 4); cue "stream" → a stream waveform.
    fn fixture() -> Vec<u8> {
        let one_shot = encode_adx(&[1000; 40], 1, 44100, 500);
        let sustain = encode_adx(&[-2000; 256], 2, 48000, 500);
        // 16-byte header + 2 ids × 2 + 3 offsets × 4 = 32 bytes, so entry 4 starts at 32.
        let a = 32;
        let b = a + one_shot.len() as u32;
        let c = b.next_multiple_of(32);
        let mut awb = afs2_header(&[4, 5], &[a, b, c + sustain.len() as u32], 32);
        awb.resize(a as usize, 0);
        awb.extend(&one_shot);
        awb.resize(c as usize, 0);
        awb.extend(&sustain);

        let cue = table(
            "Cue",
            &[Col::Row("ReferenceType"), Col::Row("ReferenceIndex")],
            &[
                vec![V::U8(1), V::U16(0)],
                vec![V::U8(3), V::U16(0)],
                vec![V::U8(1), V::U16(2)],
            ],
        );
        let cue_name = table(
            "CueName",
            &[Col::Row("CueName"), Col::Row("CueIndex")],
            &[
                vec![V::Str("adx"), V::U16(0)],
                vec![V::Str("hold"), V::U16(1)],
                vec![V::Str("stream"), V::U16(2)],
            ],
        );
        let waveform = table(
            "Waveform",
            &[
                Col::Row("EncodeType"),
                Col::Row("Streaming"),
                Col::Row("NumChannels"),
                Col::Row("SamplingRate"),
                Col::Row("NumSamples"),
                Col::Const("StreamAwbPortNo", V::U16(0)),
                Col::Row("StreamAwbId"),
                Col::Row("MemoryAwbId"),
                Col::Row("ExtensionData"),
            ],
            &[
                vec![
                    V::U8(0),
                    V::U8(0),
                    V::U8(1),
                    V::U16(44100),
                    V::U32(30),
                    V::U16(0xFFFF),
                    V::U16(4),
                    V::U16(0xFFFF),
                ],
                vec![
                    V::U8(0),
                    V::U8(0),
                    V::U8(2),
                    V::U16(48000),
                    V::U32(128),
                    V::U16(0xFFFF),
                    V::U16(5),
                    V::U16(0),
                ],
                vec![
                    V::U8(2),
                    V::U8(1),
                    V::U8(2),
                    V::U16(48000),
                    V::U32(10),
                    V::U16(3),
                    V::U16(0xFFFF),
                    V::U16(0xFFFF),
                ],
            ],
        );
        let ext = table(
            "WaveformExtensionData",
            &[Col::Row("LoopStart"), Col::Row("LoopEnd")],
            &[vec![V::U32(32), V::U32(128)]],
        );
        let pair = |a: u16, b: u16| [a.to_be_bytes(), b.to_be_bytes()].concat();
        let synth = table(
            "Synth",
            &[Col::Row("ReferenceItems")],
            &[vec![V::Data(pair(1, 1))], vec![V::Data(pair(1, 0))]],
        );
        let sequence = table(
            "Sequence",
            &[Col::Row("TrackIndex")],
            &[vec![V::Data(pair(0, 1))]],
        );
        let track = table(
            "Track",
            &[
                Col::Row("EventIndex"),
                Col::Const("CommandIndex", V::U16(0xFFFF)),
            ],
            &[vec![V::U16(0)], vec![V::U16(1)]],
        );
        let note_on = |synth: u16| [vec![0x07, 0xD0, 4], pair(2, synth)].concat();
        let track_event = table(
            "TrackEvent",
            &[Col::Row("Command")],
            &[vec![V::Data(note_on(0))], vec![V::Data(note_on(1))]],
        );
        table(
            "Header",
            &[
                Col::Row("Name"),
                Col::Row("CueTable"),
                Col::Row("CueNameTable"),
                Col::Row("WaveformTable"),
                Col::Row("WaveformExtensionDataTable"),
                Col::Row("SynthTable"),
                Col::Row("SequenceTable"),
                Col::Row("TrackTable"),
                Col::Row("TrackEventTable"),
                Col::Row("AwbFile"),
                Col::Row("StreamAwbHash"),
            ],
            &[vec![
                V::Str("SE"),
                V::Data(cue),
                V::Data(cue_name),
                V::Data(waveform),
                V::Data(ext),
                V::Data(synth),
                V::Data(sequence),
                V::Data(track),
                V::Data(track_event),
                V::Data(awb),
                // Not a table (as in MER_SE): treated as absent.
                V::Data(vec![0; 16]),
            ]],
        )
    }

    #[test]
    fn decodes_memory_awb_cues_with_loops() {
        let bank = SeBank::parse(&fixture()).unwrap();
        let adx = bank.decode_cue("adx").unwrap();
        assert_eq!(adx.len(), 1);
        assert_eq!(
            (adx[0].rate, adx[0].channels, adx[0].frames()),
            (44100, 1, 30) // 40 encoded, trimmed to NumSamples
        );
        assert!((adx[0].samples[29] - 1000.0 / 32768.0).abs() < 5e-3);
        assert_eq!(adx[0].loop_range, None);

        let hold = bank.decode_cue("hold").unwrap();
        assert_eq!(hold.len(), 2);
        assert_eq!((hold[0].channels, hold[0].frames()), (2, 128));
        assert_eq!(hold[0].loop_range, Some((32, 128)));
        assert_eq!(hold[1], adx[0]);
    }

    #[test]
    fn reports_unsupported_and_missing_cues() {
        let bank = SeBank::parse(&fixture()).unwrap();
        assert!(matches!(
            bank.decode_cue("stream"),
            Err(Error::UnsupportedWaveform { .. })
        ));
        assert!(matches!(
            bank.decode_cue("nope"),
            Err(Error::CueNotFound(_))
        ));
        let no_awb = crate::cri::acb::tests::fixture();
        assert!(matches!(
            SeBank::parse(&no_awb),
            Err(Error::MissingColumn { .. })
        ));
    }

    #[test]
    fn hit_sounds_need_every_cue_of_the_set() {
        let bank = SeBank::parse(&fixture()).unwrap();
        let set = SeSet {
            id: 1,
            marvelous: "adx",
            good: "adx",
            slide_marvelous: "adx",
            slide_good: "adx",
            snap_marvelous: "adx",
            snap_good: "adx",
            hold_marvelous: "hold",
            hold_good: "hold",
            re_hold: "hold",
            chain: "adx",
        };
        let hits = HitSounds::load(&bank, &set, 48000).unwrap();
        assert_eq!(hits.marvelous[0].rate, 48000);
        assert_eq!(hits.hold_good.len(), 2);
        let broken = SeSet {
            chain: "missing",
            ..set
        };
        assert!(HitSounds::load(&bank, &broken, 48000).is_err());
    }
}
