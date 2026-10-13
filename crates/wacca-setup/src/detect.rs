//! Scanning a user-picked folder: the Reverse archives, or an unpacked game's `Content` dir.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::archives::{self, KNOWN, MOVIE_BYTES};
use crate::readset::is_content_dir;
use crate::{Detected, Finding, SetupError, Source};

/// How many directory levels below the picked folder are searched for `Content`
/// (`<pick>/App/WindowsNoEditor/Mercury/Content` is 4).
pub const MAX_DEPTH: usize = 5;
/// Upper bound on directories visited while searching (keeps a pick like `C:\` quick).
const MAX_VISITED: usize = 20_000;

/// See [`crate::detect`].
pub fn detect(path: &Path) -> Result<Detected, SetupError> {
    let dir = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    if !dir.is_dir() {
        return Err(SetupError::NothingFound(path.to_owned()));
    }
    let (found, mut report) = archives::select(&archive_candidates(dir));
    if found.first().is_some_and(|a| a.version == KNOWN[0].version) {
        report.push(Finding::ChecksumNotVerified);
        report.push(Finding::Movies {
            present: true,
            bytes: MOVIE_BYTES,
        });
        let version = found.last().map(|a| a.version.clone());
        return Ok(Detected {
            source: Source::Archives(found),
            version,
            report,
        });
    }
    if let Some(content_dir) = find_content(dir, MAX_DEPTH) {
        return Ok(installed(content_dir));
    }
    if !found.is_empty() {
        return Err(SetupError::BaseArchiveMissing);
    }
    Err(SetupError::NothingFound(path.to_owned()))
}

/// `.7z` files in `dir` and its direct subdirectories, with sizes.
fn archive_candidates(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let scan = |d: &Path, out: &mut Vec<(PathBuf, u64)>, subdirs: &mut Vec<PathBuf>| {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.filter_map(Result::ok) {
            let Ok(meta) = e.metadata() else { continue };
            let path = e.path();
            if meta.is_dir() {
                subdirs.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("7z"))
            {
                out.push((path, meta.len()));
            }
        }
    };
    let mut subdirs = Vec::new();
    scan(dir, &mut out, &mut subdirs);
    for sub in subdirs {
        scan(&sub, &mut out, &mut Vec::new());
    }
    out.sort();
    out
}

/// Breadth-first search for the shallowest [`is_content_dir`] at most `max_depth` levels below
/// `dir` (`dir` itself included).
pub fn find_content(dir: &Path, max_depth: usize) -> Option<PathBuf> {
    let mut queue = VecDeque::from([(dir.to_owned(), 0)]);
    let mut visited = 0;
    while let Some((d, depth)) = queue.pop_front() {
        if is_content_dir(&d) {
            return Some(d);
        }
        visited += 1;
        if depth == max_depth || visited > MAX_VISITED {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut subs: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        subs.sort();
        queue.extend(subs.into_iter().map(|s| (s, depth + 1)));
    }
    None
}

/// Version from the BGM archive names: `MER_BGM_V3_07.awb` → `3.07` (the highest present). The
/// patch digit (3.07.00 vs 3.07.01) cannot be told apart from the files we read.
pub fn version_from_awbs<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    names
        .into_iter()
        .filter_map(|n| {
            let n = n.to_ascii_uppercase();
            let v = n.strip_prefix("MER_BGM_V")?.strip_suffix(".AWB")?;
            let (major, minor) = v.split_once('_')?;
            Some((major.parse::<u8>().ok()?, minor.parse::<u8>().ok()?))
        })
        .max()
        .map(|(major, minor)| format!("{major}.{minor:02}"))
}

fn installed(content_dir: PathBuf) -> Detected {
    let mut report = Vec::new();
    let awbs: Vec<String> = std::fs::read_dir(content_dir.join("Sound").join("Bgm"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let version = version_from_awbs(awbs.iter().map(String::as_str));
    if version.is_none() {
        report.push(Finding::UnknownVersion);
    }
    match wacca_convert::read_songs(&content_dir) {
        Ok(songs) if !songs.is_empty() => report.push(Finding::Songs(songs.len())),
        Ok(_) => report.push(Finding::UnsupportedVersion(
            "the song table has no playable songs".into(),
        )),
        Err(e) => report.push(Finding::UnsupportedVersion(format!(
            "the song table could not be read: {e}"
        ))),
    }
    let movies: u64 = std::fs::read_dir(content_dir.join("Movie"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("usm"))
                })
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);
    report.push(Finding::Movies {
        present: movies > 0,
        bytes: movies,
    });
    Detected {
        source: Source::Installed { content_dir },
        version,
        report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{temp_dir, touch_all};

    const CONTENT: [&str; 3] = [
        "Table/MusicParameterTable.uasset",
        "Table/MusicParameterTable.uexp",
        "Sound/Bgm/MER_BGM.awb",
    ];

    fn with_prefix(prefix: &str) -> Vec<String> {
        CONTENT.iter().map(|f| format!("{prefix}{f}")).collect()
    }

    #[test]
    fn versions_from_awb_names() {
        let v = version_from_awbs([
            "MER_BGM.awb",
            "MER_BGM_V3_01.awb",
            "mer_bgm_v3_07.awb",
            "bgmpool.awb",
        ]);
        assert_eq!(v.as_deref(), Some("3.07"));
        assert_eq!(version_from_awbs(["MER_BGM.awb"]), None);
    }

    #[test]
    fn finds_content_at_any_supported_depth() {
        for prefix in [
            "",
            "Content/",
            "Mercury/Content/",
            "WindowsNoEditor/Mercury/Content/",
            "App/WindowsNoEditor/Mercury/Content/",
            "Games/WACCA/WindowsNoEditor/Mercury/Content/",
            "renamed/assets/",
        ] {
            let root = temp_dir("depth");
            let files = with_prefix(prefix);
            touch_all(&root, &files.iter().map(String::as_str).collect::<Vec<_>>());
            // Decoys: a table without BGM, and a Content dir name without contents.
            touch_all(
                &root,
                &[
                    "Other/Table/MusicParameterTable.uexp",
                    "x/Content/readme.txt",
                ],
            );
            let found = find_content(&root, MAX_DEPTH).unwrap();
            assert_eq!(found, root.join(prefix.trim_end_matches('/')), "{prefix}");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn too_deep_or_empty_is_not_found() {
        let root = temp_dir("deep");
        let files = with_prefix("a/b/c/d/e/f/");
        touch_all(&root, &files.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(find_content(&root, MAX_DEPTH), None);
        assert!(matches!(detect(&root), Err(SetupError::NothingFound(_))));
        assert!(matches!(
            detect(&root.join("missing")),
            Err(SetupError::NothingFound(_))
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_game_with_unreadable_table_is_reported() {
        let root = temp_dir("installed");
        let mut files = with_prefix("WindowsNoEditor/Mercury/Content/");
        files.push("WindowsNoEditor/Mercury/Content/Sound/Bgm/MER_BGM_V3_05.awb".into());
        touch_all(&root, &files.iter().map(String::as_str).collect::<Vec<_>>());
        let d = detect(&root).unwrap();
        assert_eq!(d.version.as_deref(), Some("3.05"));
        assert!(
            matches!(&d.source, Source::Installed { content_dir } if content_dir.ends_with("Content"))
        );
        assert!(
            d.report
                .iter()
                .any(|f| matches!(f, Finding::UnsupportedVersion(_)))
        );
        assert!(d.report.contains(&Finding::Movies {
            present: false,
            bytes: 0
        }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archives_need_the_base() {
        let root = temp_dir("archives");
        touch_all(
            &root,
            &[
                "WACCA Reverse (3.07.01).7z",
                "dl/WACCA Reverse (3.01.00).7z",
            ],
        );
        assert!(matches!(detect(&root), Err(SetupError::BaseArchiveMissing)));
        touch_all(&root, &["dl/WACCA Reverse (3.00.00).7z", "notes.txt"]);
        let d = detect(&root.join("WACCA Reverse (3.07.01).7z")).unwrap();
        let Source::Archives(a) = &d.source else {
            panic!("{d:?}")
        };
        let v: Vec<_> = a.iter().map(|a| a.version.as_str()).collect();
        assert_eq!(v, ["3.00.00", "3.01.00", "3.07.01"]);
        assert_eq!(d.version.as_deref(), Some("3.07.01"));
        assert!(d.report.contains(&Finding::ChecksumNotVerified));
        assert!(
            d.report
                .iter()
                .any(|f| matches!(f, Finding::MissingUpdates(m) if m.len() == 6))
        );
        // Empty fake files: every one is flagged as a size mismatch (incomplete download).
        let mismatches = d
            .report
            .iter()
            .filter(|f| matches!(f, Finding::SizeMismatch { .. }));
        assert_eq!(mismatches.count(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
}
