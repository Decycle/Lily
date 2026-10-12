//! ACB cue sheet: resolves cue names to waveforms and their AWB entries.
//!
//! Chain (see `docs/research/03-audio-and-jackets.md` §1.2):
//! `CueName → Cue (ReferenceType, ReferenceIndex) → Sequence.TrackIndex[] → Track.EventIndex →
//! TrackEvent.Command noteOn (0x07D0: ref type, ref index) → Synth.ReferenceItems → Waveform`.
//! Track bus sends come from `Track.CommandIndex → TrackCommand.Command` code 0x006F.
//!
//! The ACB lives inside a UE4 `.uexp` as bulk data; [`Acb::parse`] finds the first `@UTF`.

use std::collections::BTreeMap;

use super::afs2;
use super::utf::{self, Table};
use crate::Error;

const REF_WAVEFORM: u64 = 1;
const REF_SYNTH: u64 = 2;
const REF_SEQUENCE: u64 = 3;
const CMD_NOTE_ON: u16 = 0x07D0;
const CMD_BUS_SEND: u16 = 0x006F;
const NONE_U16: u64 = 0xFFFF;
const MAX_DEPTH: u32 = 8;
/// Bus of the speaker mix; the other track of each song goes to `BGM_HEADPHONE`.
pub const SPEAKER_BUS: &str = "BGM_SPEAKER";

/// Where a waveform's encoded bytes live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Entry `id` of the stream AWB on `port` (see [`Acb::stream_awb`]).
    Stream { port: u16, id: u32 },
    /// Entry `id` of the ACB's in-memory AWB (`AwbFile`, used by MER_SE; not handled yet).
    Memory { id: u32 },
}

/// One waveform row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    pub index: u16,
    pub source: Source,
    /// 2 = HCA, 0 = ADX.
    pub encode_type: u8,
    pub channels: u8,
    pub sample_rate: u32,
    pub num_samples: u32,
}

/// A track's send to a mixer bus (level 0..=1).
#[derive(Debug, Clone, PartialEq)]
pub struct BusSend {
    pub bus: String,
    pub level: f32,
}

/// One waveform a cue plays, with the bus sends of the track that triggers it.
#[derive(Debug, Clone, PartialEq)]
pub struct CueTrack {
    pub waveform: Waveform,
    pub sends: Vec<BusSend>,
}

impl CueTrack {
    fn sends_to(&self, bus: &str) -> bool {
        self.sends.iter().any(|s| s.bus == bus && s.level > 0.0)
    }
}

/// A stream AWB referenced by the ACB, with the copy of its AFS2 header embedded in the ACB.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamAwb {
    /// File stem, e.g. `MER_BGM_V3_01` (the file is `<name>.awb`).
    pub name: String,
    pub index: Option<afs2::Index>,
}

/// A resolved cue sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Acb {
    pub name: String,
    cues: BTreeMap<String, Vec<CueTrack>>,
    stream_awbs: Vec<StreamAwb>,
}

/// TLV command list: u16 code, u8 size, payload. A truncated tail is ignored.
pub fn commands(blob: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut rest = blob;
    std::iter::from_fn(move || {
        let (&[c0, c1, size], tail) = rest.split_first_chunk::<3>()?;
        let payload = tail.get(..usize::from(size))?;
        rest = &tail[payload.len()..];
        Some((u16::from_be_bytes([c0, c1]), payload))
    })
}

/// Big-endian (u16, u16) pairs.
fn u16_pairs(blob: &[u8]) -> impl Iterator<Item = (u64, usize)> + '_ {
    blob.as_chunks::<4>().0.iter().map(|&[a, b, c, d]| {
        (
            u64::from(u16::from_be_bytes([a, b])),
            usize::from(u16::from_be_bytes([c, d])),
        )
    })
}

/// The nested tables the resolver walks.
struct Tables<'a> {
    cue: Table<'a>,
    cue_name: Table<'a>,
    waveform: Table<'a>,
    synth: Option<Table<'a>>,
    sequence: Option<Table<'a>>,
    track: Option<Table<'a>>,
    track_event: Option<Table<'a>>,
    track_command: Option<Table<'a>>,
    strings: Option<Table<'a>>,
}

fn missing(name: &str) -> Error {
    Error::MissingColumn {
        table: "Header".into(),
        column: name.into(),
    }
}

fn required<'a>(t: Option<Table<'a>>, name: &str) -> Result<Table<'a>, Error> {
    t.ok_or_else(|| missing(name))
}

fn need<'t, 'a>(t: &'t Option<Table<'a>>, name: &str) -> Result<&'t Table<'a>, Error> {
    t.as_ref().ok_or_else(|| missing(name))
}

impl Tables<'_> {
    /// Waveform indexes (with the triggering track) reachable from a reference.
    fn walk(
        &self,
        ref_type: u64,
        index: usize,
        track: Option<usize>,
        depth: u32,
        out: &mut Vec<(usize, Option<usize>)>,
    ) -> Result<(), Error> {
        if depth > MAX_DEPTH {
            log::warn!("ACB reference chain deeper than {MAX_DEPTH}; ignoring");
            return Ok(());
        }
        match ref_type {
            REF_WAVEFORM => out.push((index, track)),
            REF_SYNTH => {
                let synth = need(&self.synth, "SynthTable")?;
                for (t, i) in u16_pairs(synth.data(index, "ReferenceItems")?) {
                    self.walk(t, i, track, depth + 1, out)?;
                }
            }
            REF_SEQUENCE => {
                let seq = need(&self.sequence, "SequenceTable")?;
                let tracks = need(&self.track, "TrackTable")?;
                let events = need(&self.track_event, "TrackEventTable")?;
                for &t in seq.data(index, "TrackIndex")?.as_chunks::<2>().0 {
                    let t = usize::from(u16::from_be_bytes(t));
                    let ev = tracks.uint(t, "EventIndex")?;
                    if ev == NONE_U16 {
                        continue;
                    }
                    for (code, payload) in commands(events.data(ev as usize, "Command")?) {
                        if code == CMD_NOTE_ON {
                            for (rt, ri) in u16_pairs(payload).take(1) {
                                self.walk(rt, ri, Some(t), depth + 1, out)?;
                            }
                        }
                    }
                }
            }
            other => log::warn!("ACB reference type {other} not supported; ignoring"),
        }
        Ok(())
    }

    fn bus_sends(&self, track: usize) -> Result<Vec<BusSend>, Error> {
        let (Some(tracks), Some(cmds), Some(strings)) =
            (&self.track, &self.track_command, &self.strings)
        else {
            return Ok(Vec::new());
        };
        let ci = tracks.uint(track, "CommandIndex")?;
        if ci == NONE_U16 {
            return Ok(Vec::new());
        }
        let mut sends = Vec::new();
        for (code, payload) in commands(cmds.data(ci as usize, "Command")?) {
            if code == CMD_BUS_SEND
                && let Some((bus, level)) = u16_pairs(payload).next()
            {
                sends.push(BusSend {
                    bus: strings.str(bus as usize, "StringValue")?.to_string(),
                    level: level as f32 / 10000.0,
                });
            }
        }
        Ok(sends)
    }

    fn waveform(&self, index: usize) -> Result<Waveform, Error> {
        let w = &self.waveform;
        let streaming = w.uint(index, "Streaming")? != 0;
        let source = if streaming {
            let port = match w.uint(index, "StreamAwbPortNo") {
                Ok(NONE_U16) | Err(Error::MissingColumn { .. }) => 0,
                Ok(p) => p as u16,
                Err(e) => return Err(e),
            };
            Source::Stream {
                port,
                id: w.uint(index, "StreamAwbId")? as u32,
            }
        } else {
            Source::Memory {
                id: w.uint(index, "MemoryAwbId")? as u32,
            }
        };
        Ok(Waveform {
            index: index as u16,
            source,
            encode_type: w.uint(index, "EncodeType")? as u8,
            channels: w.uint(index, "NumChannels")? as u8,
            sample_rate: w.uint(index, "SamplingRate")? as u32,
            num_samples: w.uint(index, "NumSamples")? as u32,
        })
    }

    fn cue_tracks(&self, cue_index: usize) -> Result<Vec<CueTrack>, Error> {
        let rt = self.cue.uint(cue_index, "ReferenceType")?;
        let ri = self.cue.uint(cue_index, "ReferenceIndex")? as usize;
        let mut refs = Vec::new();
        self.walk(rt, ri, None, 0, &mut refs)?;
        refs.into_iter()
            .map(|(w, track)| {
                Ok(CueTrack {
                    waveform: self.waveform(w)?,
                    sends: match track {
                        Some(t) => self.bus_sends(t)?,
                        None => Vec::new(),
                    },
                })
            })
            .collect()
    }
}

impl Acb {
    /// Parse the ACB at the first `@UTF` in `buf` (a raw `.acb` or a UE4 `.uexp`).
    pub fn parse(buf: &[u8]) -> Result<Self, Error> {
        let at = utf::find(buf).ok_or(Error::BadMagic {
            what: "ACB",
            expected: "@UTF",
            offset: 0,
        })?;
        let header = Table::parse(&buf[at..])?;
        let sub = |name: &str| match header.sub_table(0, name) {
            Err(Error::MissingColumn { .. }) => Ok(None),
            r => r,
        };
        let t = Tables {
            cue: required(sub("CueTable")?, "CueTable")?,
            cue_name: required(sub("CueNameTable")?, "CueNameTable")?,
            waveform: required(sub("WaveformTable")?, "WaveformTable")?,
            synth: sub("SynthTable")?,
            sequence: sub("SequenceTable")?,
            track: sub("TrackTable")?,
            track_event: sub("TrackEventTable")?,
            track_command: sub("TrackCommandTable")?,
            strings: sub("StringValueTable")?,
        };

        let mut cues = BTreeMap::new();
        for row in 0..t.cue_name.rows.len() {
            let name = t.cue_name.str(row, "CueName")?.to_string();
            let cue_index = t.cue_name.uint(row, "CueIndex")? as usize;
            let tracks = t.cue_tracks(cue_index).unwrap_or_else(|e| {
                log::warn!("ACB cue {name:?}: {e}; treating as silent");
                Vec::new()
            });
            cues.insert(name, tracks);
        }

        let mut stream_awbs = Vec::new();
        if let Some(hash) = sub("StreamAwbHash")? {
            let headers = sub("StreamAwbAfs2Header")?;
            for port in 0..hash.rows.len() {
                let index = match &headers {
                    Some(h) => match afs2::Index::parse(h.data(port, "Header")?) {
                        Ok(i) => Some(i),
                        Err(e) => {
                            log::warn!("ACB embedded AFS2 header {port}: {e}");
                            None
                        }
                    },
                    None => None,
                };
                stream_awbs.push(StreamAwb {
                    name: hash.str(port, "Name")?.to_string(),
                    index,
                });
            }
        }
        let name = header.str(0, "Name").unwrap_or_default().to_string();
        log::debug!(
            "ACB {name}: {} cues, {} stream AWBs",
            cues.len(),
            stream_awbs.len()
        );
        Ok(Acb {
            name,
            cues,
            stream_awbs,
        })
    }

    /// All cue names, sorted.
    pub fn cue_names(&self) -> impl Iterator<Item = &str> {
        self.cues.keys().map(String::as_str)
    }

    /// Every waveform the cue plays (empty for intentionally silent cues such as `blank`).
    pub fn tracks(&self, cue: &str) -> Result<&[CueTrack], Error> {
        self.cues
            .get(cue)
            .map(Vec::as_slice)
            .ok_or_else(|| Error::CueNotFound(cue.to_string()))
    }

    /// The track to play on speakers: the one sending to `BGM_SPEAKER`, else the first.
    pub fn speaker_track(&self, cue: &str) -> Result<&CueTrack, Error> {
        let tracks = self.tracks(cue)?;
        tracks
            .iter()
            .find(|t| t.sends_to(SPEAKER_BUS))
            .or_else(|| tracks.first())
            .ok_or_else(|| Error::CueSilent(cue.to_string()))
    }

    /// Stream AWB on `port`.
    pub fn stream_awb(&self, port: u16) -> Result<&StreamAwb, Error> {
        self.stream_awbs
            .get(usize::from(port))
            .ok_or(Error::UnknownAwbPort(port))
    }

    pub fn stream_awbs(&self) -> &[StreamAwb] {
        &self.stream_awbs
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cri::afs2::tests::header as afs2_header;
    use crate::cri::utf::build::{Col, V, table};

    fn cmd(code: u16, payload: &[u8]) -> Vec<u8> {
        let mut v = code.to_be_bytes().to_vec();
        v.push(payload.len() as u8);
        v.extend(payload);
        v
    }

    fn pair(a: u16, b: u16) -> Vec<u8> {
        [a.to_be_bytes(), b.to_be_bytes()].concat()
    }

    /// Cue "song": sequence of 2 tracks (headphone first, speaker second);
    /// cue "blank": sequence with no tracks; cue "direct": waveform reference; cue "bad": broken synth index.
    pub(crate) fn fixture() -> Vec<u8> {
        let cue = table(
            "Cue",
            &[Col::Row("ReferenceType"), Col::Row("ReferenceIndex")],
            &[
                vec![V::U8(3), V::U16(0)],
                vec![V::U8(3), V::U16(1)],
                vec![V::U8(1), V::U16(1)],
                vec![V::U8(2), V::U16(9)],
            ],
        );
        let cue_name = table(
            "CueName",
            &[Col::Row("CueName"), Col::Row("CueIndex")],
            &[
                vec![V::Str("song"), V::U16(0)],
                vec![V::Str("blank"), V::U16(1)],
                vec![V::Str("direct"), V::U16(2)],
                vec![V::Str("bad"), V::U16(3)],
            ],
        );
        let waveform = table(
            "Waveform",
            &[
                Col::Const("EncodeType", V::U8(2)),
                Col::Const("Streaming", V::U8(1)),
                Col::Row("NumChannels"),
                Col::Const("SamplingRate", V::U16(48000)),
                Col::Row("NumSamples"),
                Col::Row("StreamAwbPortNo"),
                Col::Row("StreamAwbId"),
                Col::Const("MemoryAwbId", V::U16(0xFFFF)),
            ],
            &[
                vec![V::U8(2), V::U32(100), V::U16(0), V::U16(10)],
                vec![V::U8(2), V::U32(100), V::U16(1), V::U16(11)],
            ],
        );
        let synth = table(
            "Synth",
            &[Col::Row("ReferenceItems")],
            &[vec![V::Data(pair(1, 0))], vec![V::Data(pair(1, 1))]],
        );
        let sequence = table(
            "Sequence",
            &[Col::Row("TrackIndex")],
            &[vec![V::Data(pair(0, 1))], vec![V::Data(vec![])]],
        );
        let track = table(
            "Track",
            &[Col::Row("EventIndex"), Col::Row("CommandIndex")],
            &[vec![V::U16(0), V::U16(0)], vec![V::U16(1), V::U16(1)]],
        );
        let note_on =
            |synth: u16| [cmd(0x07D1, &[0; 2]), cmd(CMD_NOTE_ON, &pair(2, synth))].concat();
        let track_event = table(
            "TrackEvent",
            &[Col::Row("Command")],
            &[vec![V::Data(note_on(0))], vec![V::Data(note_on(1))]],
        );
        let send = |a: u16, b: u16| {
            [
                cmd(CMD_BUS_SEND, &pair(0, a)),
                cmd(CMD_BUS_SEND, &pair(1, b)),
            ]
            .concat()
        };
        let track_command = table(
            "TrackCommand",
            &[Col::Row("Command")],
            &[vec![V::Data(send(0, 10000))], vec![V::Data(send(10000, 0))]],
        );
        let strings = table(
            "Strings",
            &[Col::Row("StringValue")],
            &[vec![V::Str("BGM_SPEAKER")], vec![V::Str("BGM_HEADPHONE")]],
        );
        let awb_hash = table(
            "StreamAwb",
            &[Col::Row("Name")],
            &[vec![V::Str("BANK")], vec![V::Str("BANK_V2")]],
        );
        let awb_headers = table(
            "StreamAwbHeader",
            &[Col::Row("Header")],
            &[
                vec![V::Data(afs2_header(&[10], &[0x20, 0x40], 32))],
                vec![V::Data(afs2_header(&[11], &[0x20, 0x50], 32))],
            ],
        );
        let header = table(
            "Header",
            &[
                Col::Row("Name"),
                Col::Row("CueTable"),
                Col::Row("CueNameTable"),
                Col::Row("WaveformTable"),
                Col::Row("SynthTable"),
                Col::Row("SequenceTable"),
                Col::Row("TrackTable"),
                Col::Row("TrackEventTable"),
                Col::Row("TrackCommandTable"),
                Col::Row("StringValueTable"),
                Col::Row("StreamAwbHash"),
                Col::Row("StreamAwbAfs2Header"),
            ],
            &[vec![
                V::Str("TEST"),
                V::Data(cue),
                V::Data(cue_name),
                V::Data(waveform),
                V::Data(synth),
                V::Data(sequence),
                V::Data(track),
                V::Data(track_event),
                V::Data(track_command),
                V::Data(strings),
                V::Data(awb_hash),
                V::Data(awb_headers),
            ]],
        );
        // Wrapped like UE4 bulk data.
        let mut uexp = vec![0u8; 13];
        uexp.extend(header);
        uexp
    }

    #[test]
    fn resolves_sequence_cue_to_both_mixes() {
        let acb = Acb::parse(&fixture()).unwrap();
        assert_eq!(acb.name, "TEST");
        assert_eq!(
            acb.cue_names().collect::<Vec<_>>(),
            ["bad", "blank", "direct", "song"]
        );
        let tracks = acb.tracks("song").unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0].waveform.source,
            Source::Stream { port: 0, id: 10 }
        );
        assert_eq!(
            tracks[0].sends,
            [
                BusSend {
                    bus: "BGM_SPEAKER".into(),
                    level: 0.0
                },
                BusSend {
                    bus: "BGM_HEADPHONE".into(),
                    level: 1.0
                }
            ]
        );
        assert_eq!(tracks[1].waveform.sample_rate, 48000);
    }

    #[test]
    fn picks_speaker_mix() {
        let acb = Acb::parse(&fixture()).unwrap();
        let t = acb.speaker_track("song").unwrap();
        assert_eq!(t.waveform.source, Source::Stream { port: 1, id: 11 });
        // No sends at all: falls back to the first waveform.
        let d = acb.speaker_track("direct").unwrap();
        assert_eq!(d.waveform.index, 1);
        assert!(d.sends.is_empty());
    }

    #[test]
    fn silent_missing_and_broken_cues() {
        let acb = Acb::parse(&fixture()).unwrap();
        assert!(acb.tracks("blank").unwrap().is_empty());
        assert!(matches!(
            acb.speaker_track("blank"),
            Err(Error::CueSilent(_))
        ));
        assert!(matches!(acb.tracks("nope"), Err(Error::CueNotFound(_))));
        // Out-of-range synth index is logged and the cue treated as silent.
        assert!(acb.tracks("bad").unwrap().is_empty());
    }

    #[test]
    fn exposes_embedded_awb_headers() {
        let acb = Acb::parse(&fixture()).unwrap();
        let awb = acb.stream_awb(1).unwrap();
        assert_eq!(awb.name, "BANK_V2");
        let e = awb.index.as_ref().unwrap().find(11).unwrap();
        assert_eq!((e.start, e.end), (0x20, 0x50));
        assert!(matches!(acb.stream_awb(2), Err(Error::UnknownAwbPort(2))));
    }

    #[test]
    fn command_list_parsing() {
        let blob = [cmd(1, &[9]), cmd(2, &[]), vec![0, 3, 5, 1]].concat();
        let got: Vec<_> = commands(&blob).collect();
        assert_eq!(got, [(1, &[9][..]), (2, &[][..])]);
    }

    #[test]
    fn no_utf_is_an_error() {
        assert!(matches!(
            Acb::parse(b"nothing"),
            Err(Error::BadMagic { .. })
        ));
    }
}
