//! First-run setup backend behind a trait. The real work (archive extraction, conversion) lives
//! in the `wacca-setup` crate; until it is in the workspace this module mirrors its API
//! (`detect`, `Plan`, `run`, `Summary`) with a minimal stand-in: it understands an installed game
//! folder only (copies `Content`, then runs `wacca-convert` from next to the executable) and
//! reports archives as not supported yet.
// TODO(wacca-setup): replace `Stub` with an adapter over `wacca_setup::{detect, run}`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Where the game files come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An installed / unpacked game: its `Content` folder.
    Installed(PathBuf),
    /// The downloaded update archives (`.7z`).
    Archives(Vec<PathBuf>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    /// Something needed is missing (e.g. an update archive).
    #[allow(dead_code)] // reported by wacca-setup
    Missing,
}

/// One line of the detection report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    pub source: Source,
    pub version: Option<String>,
    pub report: Vec<Finding>,
}

/// What to install where: writes `<dest>/assets` and `<dest>/data`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub include_movies: bool,
    pub dest: PathBuf,
}

/// Progress of a running setup: the current step and its byte counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub step: String,
    /// 0-based index of the step and the number of steps.
    pub index: usize,
    pub steps: usize,
    pub done: u64,
    /// 0 = unknown.
    pub total: u64,
}

/// An item that was not installed, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub what: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub songs_playable: usize,
    pub charts_skipped: Vec<Skipped>,
    pub movies_present: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("7-Zip (7z.exe) not found")]
    SevenZipMissing,
    #[error("not enough disk space: {needed} bytes needed, {available} free")]
    NoSpace { needed: u64, available: u64 },
    #[allow(dead_code)] // reported by wacca-setup
    #[error("unsupported game version {0}")]
    UnsupportedVersion(String),
    #[error("no WACCA files found in {0}")]
    NothingFound(PathBuf),
    #[error("cancelled")]
    Cancelled,
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// The setup operations the app needs.
pub trait Backend: Send + Sync + std::fmt::Debug {
    fn detect(&self, dir: &Path) -> Result<Detected, SetupError>;
    /// Runs the setup; checks `cancel` between items.
    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
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
    move |e| SetupError::Io(path.to_path_buf(), e)
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
            let movies = std::fs::read_dir(content.join("Movie"))
                .map(|r| r.flatten().count())
                .unwrap_or(0);
            let report = vec![
                Finding {
                    severity: Severity::Info,
                    message: format!("Installed game: {}", content.display()),
                },
                if movies > 0 {
                    Finding {
                        severity: Severity::Info,
                        message: format!("{movies} background videos found"),
                    }
                } else {
                    Finding {
                        severity: Severity::Warning,
                        message: "No background videos found".into(),
                    }
                },
            ];
            return Ok(Detected {
                source: Source::Installed(content),
                version: None,
                report,
            });
        }
        let archives: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(io(dir))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("7z")))
            .collect();
        if archives.is_empty() {
            return Err(SetupError::NothingFound(dir.to_path_buf()));
        }
        Ok(Detected {
            report: vec![Finding {
                severity: Severity::Info,
                message: format!("{} archives", archives.len()),
            }],
            source: Source::Archives(archives),
            version: None,
        })
    }

    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Summary, SetupError> {
        let Source::Installed(content) = source else {
            if seven_zip().is_none() {
                return Err(SetupError::SevenZipMissing);
            }
            return Err(SetupError::Other(
                "installing from archives needs the wacca-setup crate (not built in yet)".into(),
            ));
        };
        let skip: &[&str] = if plan.include_movies { &[] } else { &["Movie"] };
        let list = files(content, skip)?;
        let total = list.iter().map(|(_, n)| n).sum();
        // Copies plus about 1 GB of converted data.
        let needed = total + 1_000_000_000;
        if let Some(available) = free_space(&plan.dest)
            && available < needed
        {
            return Err(SetupError::NoSpace { needed, available });
        }
        let assets = plan.dest.join("assets");
        let mut done = 0;
        for (path, len) in &list {
            if cancel.load(Ordering::Relaxed) {
                return Err(SetupError::Cancelled);
            }
            let to = assets.join(path.strip_prefix(content).unwrap_or(path));
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::copy(path, &to).map_err(io(&to))?;
            done += len;
            progress(Progress {
                step: "Copying game files".into(),
                index: 0,
                steps: 2,
                done,
                total,
            });
        }
        progress(Progress {
            step: "Converting".into(),
            index: 1,
            steps: 2,
            done: 0,
            total: 0,
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
            return Err(SetupError::Other(format!(
                "wacca-convert failed ({status})"
            )));
        }
        let songs = crate::record::load_songs(&data.join("songs.json"))
            .map_err(|e| SetupError::Other(e.to_string()))?
            .unwrap_or_default();
        Ok(Summary {
            songs_playable: songs.len(),
            charts_skipped: Vec::new(),
            movies_present: assets.join("Movie").is_dir(),
            warnings: Vec::new(),
        })
    }
}

/// `7z.exe` next to the executable or in 7-Zip's install folder.
fn seven_zip() -> Option<PathBuf> {
    let local = std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name("7z.exe"));
    let installed =
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("7-Zip").join("7z.exe"));
    local.into_iter().chain(installed).find(|p| p.is_file())
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
        assert_eq!(d.source, Source::Installed(content));
        assert_eq!(d.report[1].severity, Severity::Warning);
        assert!(free_space(&root.join("not/yet")).is_some_and(|n| n > 0));
        std::fs::remove_dir_all(&root).unwrap();
        assert!(matches!(
            Stub.detect(&root),
            Err(SetupError::Io(..) | SetupError::NothingFound(_))
        ));
    }
}
