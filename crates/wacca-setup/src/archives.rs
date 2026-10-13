//! The known WACCA Reverse `.7z` archives (base 3.00.00 plus eight updates): identification by
//! name or size, version order, md5 verification.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{ArchiveFile, Finding, Phase, Progress, SetupError, io_err};

/// One known archive: version, file name, size in bytes, md5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    pub version: &'static str,
    pub size: u64,
    pub md5: &'static str,
}

impl Known {
    /// The archive's original file name, e.g. `WACCA Reverse (3.00.00).7z`.
    pub fn file_name(&self) -> String {
        format!("WACCA Reverse ({}).7z", self.version)
    }
}

/// In version order; the first one is the base game, the others are updates applied on top.
pub const KNOWN: [Known; 9] = [
    Known {
        version: "3.00.00",
        size: 21_135_668_332,
        md5: "fa8d60a61a10cd1a6903d4d4c74f7c7a",
    },
    Known {
        version: "3.01.00",
        size: 503_592_370,
        md5: "facd480112a5225233462cc0c9cce7b6",
    },
    Known {
        version: "3.02.00",
        size: 90_085_838,
        md5: "586e14d020182f8bb0b527a37c6c2afd",
    },
    Known {
        version: "3.03.00",
        size: 51_816_287,
        md5: "5307f229c6634c51079f40b3cdfdd04c",
    },
    Known {
        version: "3.04.00",
        size: 110_579_809,
        md5: "50aa285d5fbf111bc6e3783d5a683fe5",
    },
    Known {
        version: "3.05.00",
        size: 102_469_544,
        md5: "3f694dc4312c038a1382240e29b5d49d",
    },
    Known {
        version: "3.06.00",
        size: 756_307_658,
        md5: "5f90acf09121c84371228376dd3cb16d",
    },
    Known {
        version: "3.07.00",
        size: 242_249_712,
        md5: "61cf6587fd39b6783b3971b4b2e72110",
    },
    Known {
        version: "3.07.01",
        size: 17_296_902,
        md5: "2d1ef12c3fe45ca94ae1237e411cbbc3",
    },
];

/// Total size of all `Movie/*.usm` in the final (3.07.01) state, from the archive listings.
pub const MOVIE_BYTES: u64 = 21_369_353_312;

/// The version in a file name like `WACCA Reverse (3.07.01).7z` (also `wacca_reverse_3.07.01.7z`,
/// `... (3.07.01) (1).7z`): the first `d.dd.dd` in the name, if the name mentions "reverse".
pub fn version_in_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".7z") || !lower.contains("reverse") {
        return None;
    }
    KNOWN
        .iter()
        .find(|k| lower.contains(k.version))
        .map(|k| k.version)
}

/// Picks the known archives among `(path, size)` candidates: by version in the name, else
/// (renamed files) by exact size. The first file per version wins; the result is in version order.
pub fn select(candidates: &[(PathBuf, u64)]) -> (Vec<ArchiveFile>, Vec<Finding>) {
    let mut found: Vec<Option<ArchiveFile>> = vec![None; KNOWN.len()];
    let mut findings = Vec::new();
    for (path, size) in candidates {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let by_name = version_in_name(name).and_then(|v| KNOWN.iter().position(|k| k.version == v));
        let by_size = || KNOWN.iter().position(|k| k.size == *size);
        let Some(i) = by_name.or_else(by_size) else {
            continue;
        };
        if found[i].is_some() {
            continue;
        }
        let k = KNOWN[i];
        if by_name.is_none() {
            findings.push(Finding::Renamed {
                archive: path.clone(),
                version: k.version.into(),
            });
        } else if k.size != *size {
            findings.push(Finding::SizeMismatch {
                archive: path.clone(),
                expected: k.size,
                actual: *size,
            });
        }
        found[i] = Some(ArchiveFile {
            path: path.clone(),
            version: k.version.into(),
            size: *size,
            md5: k.md5,
        });
    }
    let missing: Vec<String> = KNOWN
        .iter()
        .zip(&found)
        .skip(1)
        .filter(|(_, f)| f.is_none())
        .map(|(k, _)| k.version.to_owned())
        .collect();
    if !missing.is_empty() && found.iter().any(Option::is_some) {
        findings.push(Finding::MissingUpdates(missing));
    }
    (found.into_iter().flatten().collect(), findings)
}

/// Lower-case hex md5 of everything `r` yields; `tick(bytes so far)` returning `false` cancels.
pub fn md5_of(
    mut r: impl Read,
    tick: &mut dyn FnMut(u64) -> bool,
) -> std::io::Result<Option<String>> {
    let mut ctx = md5::Context::new();
    let mut buf = vec![0; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(Some(format!("{:x}", ctx.finalize())));
        }
        ctx.consume(&buf[..n]);
        total += n as u64;
        if !tick(total) {
            return Ok(None);
        }
    }
}

/// Checks `archive` against its known md5 (reads the whole file). `Ok(false)` = mismatch.
pub fn verify(
    archive: &ArchiveFile,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<bool, SetupError> {
    let path: &Path = &archive.path;
    let file = std::fs::File::open(path).map_err(io_err(path))?;
    let detail = archive.version.clone();
    let mut tick = |done| {
        progress(Progress {
            phase: Phase::Verify,
            done,
            total: archive.size,
            detail: detail.clone(),
        })
    };
    match md5_of(file, &mut tick).map_err(io_err(path))? {
        Some(sum) => Ok(sum == archive.md5),
        None => Err(SetupError::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_from_names() {
        assert_eq!(
            version_in_name("WACCA Reverse (3.07.01).7z"),
            Some("3.07.01")
        );
        assert_eq!(
            version_in_name("wacca reverse (3.00.00) (1).7Z"),
            Some("3.00.00")
        );
        assert_eq!(version_in_name("WACCA_Reverse_3.05.00.7z"), Some("3.05.00"));
        assert_eq!(version_in_name("WACCA Reverse (3.08.00).7z"), None);
        assert_eq!(version_in_name("WACCA Lily (3.00.00).7z"), None);
        assert_eq!(version_in_name("WACCA Reverse (3.00.00).zip"), None);
        assert_eq!(KNOWN[0].file_name(), "WACCA Reverse (3.00.00).7z");
    }

    #[test]
    fn known_table_is_in_version_order() {
        assert!(KNOWN.windows(2).all(|w| w[0].version < w[1].version));
        assert!(KNOWN.iter().all(|k| k.md5.len() == 32));
    }

    #[test]
    fn selects_by_name_then_size_in_version_order() {
        let c = vec![
            (PathBuf::from("d/WACCA Reverse (3.07.01).7z"), KNOWN[8].size),
            (PathBuf::from("d/renamed.7z"), KNOWN[1].size),
            (PathBuf::from("d/WACCA Reverse (3.00.00).7z"), 5), // truncated download
            (PathBuf::from("d/other.7z"), 1234),
            (
                PathBuf::from("d/WACCA Reverse (3.07.01) (1).7z"),
                KNOWN[8].size,
            ), // duplicate
        ];
        let (found, findings) = select(&c);
        let versions: Vec<_> = found.iter().map(|a| a.version.as_str()).collect();
        assert_eq!(versions, ["3.00.00", "3.01.00", "3.07.01"]);
        assert_eq!(found[2].path, PathBuf::from("d/WACCA Reverse (3.07.01).7z"));
        assert_eq!(found[0].md5, KNOWN[0].md5);
        assert!(findings.contains(&Finding::Renamed {
            archive: "d/renamed.7z".into(),
            version: "3.01.00".into()
        }));
        assert!(findings.contains(&Finding::SizeMismatch {
            archive: "d/WACCA Reverse (3.00.00).7z".into(),
            expected: KNOWN[0].size,
            actual: 5
        }));
        let missing = [
            "3.02.00", "3.03.00", "3.04.00", "3.05.00", "3.06.00", "3.07.00",
        ];
        assert!(findings.contains(&Finding::MissingUpdates(
            missing.iter().map(|s| s.to_string()).collect()
        )));
        assert_eq!(select(&[]), (vec![], vec![]));
    }

    #[test]
    fn md5_matches_rfc1321_and_cancels() {
        let sum = md5_of(&b"abc"[..], &mut |_| true).unwrap();
        assert_eq!(sum.as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        assert_eq!(md5_of(&b"abc"[..], &mut |_| false).unwrap(), None);
    }

    #[test]
    fn verify_reads_the_file() {
        let dir = crate::tests::temp_dir("verify");
        let path = dir.join("a.7z");
        std::fs::write(&path, b"abc").unwrap();
        let mut a = ArchiveFile {
            path,
            version: "3.07.01".into(),
            size: 3,
            md5: "900150983cd24fb0d6963f7d28e17f72",
        };
        assert!(verify(&a, &mut |_| true).unwrap());
        a.md5 = KNOWN[0].md5;
        assert!(!verify(&a, &mut |_| true).unwrap());
        assert!(matches!(
            verify(&a, &mut |_| false),
            Err(SetupError::Cancelled)
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
