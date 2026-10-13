//! First-run setup backend behind a trait. The real work (archive extraction, conversion) lives
//! in the `wacca-setup` crate; until it is in the workspace this module mirrors its public types
//! (`Source`, `Finding`, `Detected`, `Plan`, `Phase`, `Progress`, `Summary`, `SetupError`) and
//! provides a minimal stand-in ([`Stub`]): it handles an installed game folder only (copies
//! `Content`, then runs `wacca-convert` from next to the executable) and reports archives as not
//! supported yet.
// TODO(wacca-setup): replace the mirrored types with `pub use wacca_setup::{...}` and `Stub` with
// a backend that calls `wacca_setup::{detect, run}` (same signatures).
#![allow(dead_code)] // the mirror keeps fields and variants only the real crate fills in

use std::path::{Path, PathBuf};
use std::time::Duration;

/// One recognised archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: PathBuf,
    pub version: String,
    pub size: u64,
    pub md5: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The archives found, in version order (base first).
    Archives(Vec<ArchiveFile>),
    /// An unpacked game's `Content` directory.
    Installed { content_dir: PathBuf },
}

/// Something the user should know before [`Backend::run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    MissingUpdates(Vec<String>),
    ChecksumNotVerified,
    Renamed {
        archive: PathBuf,
        version: String,
    },
    SizeMismatch {
        archive: PathBuf,
        expected: u64,
        actual: u64,
    },
    Movies {
        present: bool,
        bytes: u64,
    },
    Songs(usize),
    UnknownVersion,
    UnsupportedVersion(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub source: Source,
    pub version: Option<String>,
    pub report: Vec<Finding>,
}

/// What to install where: writes `<dest>/assets` and `<dest>/data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub include_movies: bool,
    pub dest: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Verify,
    Prepare,
    Extract,
    Copy,
    Convert,
    Validate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
    /// Archive version, converter step, …
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedChart {
    pub song: String,
    pub chart_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub songs_playable: usize,
    pub charts_playable: usize,
    pub songs_skipped: usize,
    pub charts_skipped: Vec<SkippedChart>,
    pub movies_present: usize,
    pub movies_missing: usize,
    pub warnings: Vec<String>,
    pub assets_bytes: u64,
    pub data_bytes: u64,
    pub timings: Vec<(Phase, Duration)>,
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no WACCA Reverse archives or game files found in {}", .0.display())]
    NothingFound(PathBuf),
    #[error("the base archive `WACCA Reverse (3.00.00).7z` is needed too")]
    BaseArchiveMissing,
    #[error("7-Zip was not found")]
    SevenZipMissing,
    #[error("7-Zip, {}: {message}", archive.display())]
    SevenZip { archive: PathBuf, message: String },
    #[error("{} does not contain the game's Content folder", .0.display())]
    UnexpectedArchive(PathBuf),
    #[error("not enough disk space on {}", path.display())]
    DiskSpace {
        path: PathBuf,
        needed: u64,
        available: u64,
    },
    #[error("the destination {} overlaps the game files", .0.display())]
    InvalidDestination(PathBuf),
    #[error("cancelled")]
    Cancelled,
    #[error("a required game file is missing or broken: {0}")]
    MissingCore(String),
    #[error("unsupported game version: {0}")]
    UnsupportedVersion(String),
    /// Stand-in for the crate's `Convert(wacca_convert::Error)`.
    #[error("conversion failed: {0}")]
    Convert(String),
}

/// The setup operations the app needs (`wacca_setup::{detect, run}`).
pub trait Backend: Send + Sync + std::fmt::Debug {
    fn detect(&self, dir: &Path) -> Result<Detected, SetupError>;
    /// `progress` returning `false` cancels (→ [`SetupError::Cancelled`]).
    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError>;
}

/// The stand-in backend (see the module docs).
#[derive(Debug, Default)]
pub struct Stub;

/// The `Content` folder of an installed game at or under `dir`.
fn content_dir(dir: &Path) -> Option<PathBuf> {
    [
        dir.to_path_buf(),
        dir.join("Content"),
        dir.join("Mercury").join("Content"),
        dir.join("WindowsNoEditor").join("Mercury").join("Content"),
    ]
    .into_iter()
    .find(|c| c.join("MusicData").is_dir() && c.join("Sound").is_dir())
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SetupError + '_ {
    move |source| SetupError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Files under `dir` (recursively) with their sizes, skipping top-level folders in `skip`.
fn files(dir: &Path, skip: &[&str]) -> Result<Vec<(PathBuf, u64)>, SetupError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(io(&d))? {
            let e = e.map_err(io(&d))?;
            let path = e.path();
            let rel = path.strip_prefix(dir).unwrap_or(&path);
            if skip.iter().any(|s| rel == Path::new(s)) {
                continue;
            }
            let meta = e.metadata().map_err(io(&path))?;
            if meta.is_dir() {
                stack.push(path);
            } else {
                out.push((path, meta.len()));
            }
        }
    }
    Ok(out)
}

impl Backend for Stub {
    fn detect(&self, dir: &Path) -> Result<Detected, SetupError> {
        if let Some(content) = content_dir(dir) {
            let movies = files(&content.join("Movie"), &[]).unwrap_or_default();
            let report = vec![
                Finding::UnknownVersion,
                Finding::Movies {
                    present: !movies.is_empty(),
                    bytes: movies.iter().map(|(_, n)| n).sum(),
                },
            ];
            return Ok(Detected {
                source: Source::Installed {
                    content_dir: content,
                },
                version: None,
                report,
            });
        }
        let archives: Vec<ArchiveFile> = std::fs::read_dir(dir)
            .map_err(io(dir))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("7z")))
            .map(|path| ArchiveFile {
                size: std::fs::metadata(&path).map_or(0, |m| m.len()),
                path,
                version: String::new(),
                md5: "",
            })
            .collect();
        if archives.is_empty() {
            return Err(SetupError::NothingFound(dir.to_path_buf()));
        }
        Ok(Detected {
            report: vec![Finding::ChecksumNotVerified],
            source: Source::Archives(archives),
            version: None,
        })
    }

    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError> {
        let Source::Installed { content_dir } = source else {
            if seven_zip().is_none() {
                return Err(SetupError::SevenZipMissing);
            }
            return Err(SetupError::Convert(
                "installing from archives needs the wacca-setup crate (not built in yet)".into(),
            ));
        };
        let skip: &[&str] = if plan.include_movies { &[] } else { &["Movie"] };
        let list = files(content_dir, skip)?;
        let total = list.iter().map(|(_, n)| n).sum();
        // Copies plus about 1 GB of converted data.
        let needed = total + 1_000_000_000;
        if let Some(available) = free_space(&plan.dest)
            && available < needed
        {
            return Err(SetupError::DiskSpace {
                path: plan.dest.clone(),
                needed,
                available,
            });
        }
        let assets = plan.dest.join("assets");
        let mut done = 0;
        for (path, len) in &list {
            let to = assets.join(path.strip_prefix(content_dir).unwrap_or(path));
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::copy(path, &to).map_err(io(&to))?;
            done += len;
            let p = Progress {
                phase: Phase::Copy,
                done,
                total,
                detail: String::new(),
            };
            if !progress(p) {
                return Err(SetupError::Cancelled);
            }
        }
        progress(Progress {
            phase: Phase::Convert,
            done: 0,
            total: 0,
            detail: String::new(),
        });
        let exe = std::env::current_exe().map_err(io(Path::new("wacca-app")))?;
        let convert = exe.with_file_name(format!("wacca-convert{}", std::env::consts::EXE_SUFFIX));
        let data = plan.dest.join("data");
        let status = std::process::Command::new(&convert)
            .arg("--assets")
            .arg(&assets)
            .arg("--out")
            .arg(&data)
            .args(["--textures", "--fonts", "--meshes"])
            .args(["--export-ui", "UI/Textures", "--export-ui", "Vfx/Textures"])
            .status()
            .map_err(io(&convert))?;
        if !status.success() {
            return Err(SetupError::Convert(format!("wacca-convert: {status}")));
        }
        let songs = crate::record::load_songs(&data.join("songs.json"))
            .map_err(|e| SetupError::MissingCore(e.to_string()))?
            .unwrap_or_default();
        Ok(Summary {
            songs_playable: songs.len(),
            charts_playable: songs.iter().map(|s| s.charts.len()).sum(),
            movies_present: files(&assets.join("Movie"), &[]).map_or(0, |m| m.len()),
            assets_bytes: total,
            ..Summary::default()
        })
    }
}

/// `7z.exe` next to the executable (or in `tools\`) or in 7-Zip's install folder.
fn seven_zip() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let local = exe.iter().flat_map(|e| {
        [
            e.with_file_name("7z.exe"),
            e.with_file_name("tools").join("7z.exe"),
        ]
    });
    let installed =
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("7-Zip").join("7z.exe"));
    local.chain(installed).find(|p| p.is_file())
}

/// Free bytes for the user on the drive of `dir` (or its nearest existing parent).
#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let existing = dir.ancestors().find(|a| a.is_dir())?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain([0]).collect();
    let (mut available, mut total, mut free) = (0, 0, 0);
    // SAFETY: a NUL-terminated path and three valid out pointers.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    (ok != 0).then_some(available)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_folder_is_found_at_several_depths() {
        let root = std::env::temp_dir().join(format!("wacca-setup-detect-{}", std::process::id()));
        let content = root.join("WindowsNoEditor").join("Mercury").join("Content");
        for d in ["MusicData", "Sound"] {
            std::fs::create_dir_all(content.join(d)).unwrap();
        }
        assert_eq!(content_dir(&root), Some(content.clone()));
        assert_eq!(content_dir(&content), Some(content.clone()));
        let d = Stub.detect(&root).unwrap();
        assert_eq!(
            d.source,
            Source::Installed {
                content_dir: content
            }
        );
        assert!(d.report.contains(&Finding::Movies {
            present: false,
            bytes: 0
        }));
        assert!(free_space(&root.join("not/yet")).is_some_and(|n| n > 0));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(matches!(
            Stub.detect(&root),
            Err(SetupError::Io { .. } | SetupError::NothingFound(_))
        ));
    }
}
