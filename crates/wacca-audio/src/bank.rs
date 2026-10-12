//! The song bank in `Sound/Bgm`: `MER_BGM.uexp` (the ACB) plus the stream AWBs next to it.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use crate::cri::acb::{Acb, Source, Waveform};
use crate::cri::afs2::{self, Entry};
use crate::hca::Slice;
use crate::{Error, HcaStream};

/// UE4 asset that carries the BGM ACB.
pub const ACB_FILE: &str = "MER_BGM.uexp";
/// ACB `EncodeType` of HCA waveforms.
const ENCODE_HCA: u8 = 2;

/// Byte range of one cue's speaker-mix waveform.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub awb: PathBuf,
    pub entry: Entry,
    pub waveform: Waveform,
}

/// Opened BGM bank: resolves cue names (`#MUSIC_FILE_PATH`) to streaming decoders.
#[derive(Debug, Clone)]
pub struct BgmBank {
    dir: PathBuf,
    acb: Acb,
}

/// Stream decoder for one bank entry.
pub type BgmStream = HcaStream<Slice<BufReader<File>>>;

impl BgmBank {
    /// Open the bank in `dir` (`<assets>/Sound/Bgm`).
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        let path = dir.join(ACB_FILE);
        let bytes = std::fs::read(&path).map_err(Error::file(&path))?;
        Ok(BgmBank {
            acb: Acb::parse(&bytes)?,
            dir,
        })
    }

    pub fn from_acb(dir: impl Into<PathBuf>, acb: Acb) -> Self {
        BgmBank {
            dir: dir.into(),
            acb,
        }
    }

    pub fn acb(&self) -> &Acb {
        &self.acb
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// AWB file and byte range of `cue`'s speaker mix. Uses the AFS2 header copy embedded in
    /// the ACB; only reads the `.awb` header if the ACB has none.
    pub fn locate(&self, cue: &str) -> Result<Location, Error> {
        let track = self.acb.speaker_track(cue)?;
        let w = &track.waveform;
        let (Source::Stream { port, id }, ENCODE_HCA) = (w.source, w.encode_type) else {
            return Err(Error::UnsupportedSource {
                cue: cue.to_string(),
            });
        };
        let awb = self.acb.stream_awb(port)?;
        let path = self.dir.join(format!("{}.awb", awb.name));
        let read;
        let index = match &awb.index {
            Some(i) => i,
            None => {
                let mut f = File::open(&path).map_err(Error::file(&path))?;
                read = afs2::Index::read(&mut f)?;
                &read
            }
        };
        let entry = index.find(id).ok_or_else(|| Error::AwbIdNotFound {
            awb: awb.name.clone(),
            id,
        })?;
        Ok(Location {
            awb: path,
            entry,
            waveform: w.clone(),
        })
    }

    /// Streaming decoder for `cue` (opens the AWB and seeks to the entry; nothing else is read).
    pub fn open_cue(&self, cue: &str) -> Result<BgmStream, Error> {
        let loc = self.locate(cue)?;
        let file = File::open(&loc.awb).map_err(Error::file(&loc.awb))?;
        let slice = Slice::new(BufReader::new(file), loc.entry.start, loc.entry.len())?;
        HcaStream::new(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cri::acb::tests::fixture;

    #[test]
    fn locates_speaker_mix_via_embedded_header() {
        let bank = BgmBank::from_acb("bgm", Acb::parse(&fixture()).unwrap());
        let loc = bank.locate("song").unwrap();
        assert_eq!(loc.awb, Path::new("bgm").join("BANK_V2.awb"));
        assert_eq!(
            (loc.entry.id, loc.entry.start, loc.entry.end),
            (11, 0x20, 0x50)
        );
    }

    #[test]
    fn reports_missing_and_silent_cues() {
        let bank = BgmBank::from_acb("bgm", Acb::parse(&fixture()).unwrap());
        assert!(matches!(
            bank.locate("MER_BGM_S01_007"),
            Err(Error::CueNotFound(_))
        ));
        assert!(matches!(bank.locate("blank"), Err(Error::CueSilent(_))));
        assert!(matches!(bank.open_cue("blank"), Err(Error::CueSilent(_))));
    }
}
