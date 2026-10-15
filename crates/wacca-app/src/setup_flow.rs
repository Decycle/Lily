//! First-run setup as a pure state machine: welcome → folder picker → detection report (with the
//! background-video and checksum choices) → optional md5 check → running (progress, cancel) → summary → title. Errors end on a
//! failure step that explains what to do. With [`Flow::by_url`] (iPad) the picker and the report
//! are replaced by asking for the address of a Lily pack, which is downloaded and installed. The shell performs the [`Effect`]s (dialog, worker
//! threads) and feeds their results back as [`Input`]s.

use std::path::PathBuf;

use crate::setup_backend::{
    ArchiveFile, Detected, Finding, Phase, Progress, SetupError, Source, Summary,
};

/// Why setup is showing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The game cannot start: these parts are missing (labels from [`crate::paths::missing`]).
    Missing(Vec<String>),
    /// Opened from the title screen (add videos, repair).
    Requested,
}

/// An error, worded for the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug)]
pub enum Step {
    Welcome,
    /// The folder dialog (or, by URL, the address prompt) is open.
    Picking,
    Detecting(PathBuf),
    Found {
        dir: PathBuf,
        detected: Detected,
        movies: bool,
        /// Check the archives' md5 first (archives only).
        verify: bool,
        /// Archives whose checksum did not match (START continues anyway).
        mismatched: Vec<String>,
    },
    Running {
        dir: PathBuf,
        progress: Option<Progress>,
        /// When the current step started (s, the shell's clock).
        step_since: f64,
        cancelling: bool,
        /// While the checksums are checked: what to run afterwards (report, videos).
        checking: Option<(Detected, bool)>,
    },
    Done(Summary),
    Failed {
        failure: Failure,
        /// Folder to inspect again on "try again".
        dir: Option<PathBuf>,
    },
}

#[derive(Debug)]
pub enum Input {
    Confirm,
    Back,
    /// Flip the background-video choice.
    ToggleMovies,
    /// Flip the checksum check.
    ToggleVerify,
    /// The dialog closed: the chosen folder, if any.
    Picked(Option<PathBuf>),
    /// The address prompt closed: the entered address, if any.
    Url(Option<String>),
    Detected(Result<Detected, Failure>),
    /// Checksums checked: the archives that did not match.
    Verified(Result<Vec<String>, Failure>),
    /// Progress at time `at` (s).
    Progress(Progress, f64),
    Finished(Result<Summary, Failure>),
}

/// What the shell has to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    OpenPicker,
    /// Ask for a pack's address, pre-filled with the last one.
    AskUrl(Option<String>),
    /// Download and install the pack at this address.
    Download(String),
    Detect(PathBuf),
    /// Check these archives' md5.
    Verify(Vec<ArchiveFile>),
    Run {
        source: Source,
        include_movies: bool,
    },
    Cancel,
    /// Setup finished: reload the game and show the title.
    Finish,
    /// Back to the title without changes.
    Leave,
    Quit,
}

#[derive(Debug)]
pub struct Flow {
    pub step: Step,
    pub reason: Reason,
    /// Set up from a downloaded pack instead of the user's archives or game folder.
    pub by_url: bool,
    /// The last address entered.
    pub url: Option<String>,
}

/// The archives of an archive source.
pub fn archives(d: &Detected) -> Option<&[ArchiveFile]> {
    match &d.source {
        Source::Archives(a) => Some(a),
        Source::Installed { .. } => None,
    }
}

fn running(dir: PathBuf, checking: Option<(Detected, bool)>) -> Step {
    Step::Running {
        dir,
        progress: None,
        step_since: 0.0,
        cancelling: false,
        checking,
    }
}

fn found(dir: PathBuf, detected: Detected, movies: bool, mismatched: Vec<String>) -> Step {
    Step::Found {
        dir,
        detected,
        movies,
        verify: false,
        mismatched,
    }
}

impl Flow {
    pub fn new(reason: Reason) -> Self {
        Self {
            step: Step::Welcome,
            reason,
            by_url: false,
            url: None,
        }
    }

    /// Setup from a Lily pack's address (see the module docs).
    pub fn by_url(reason: Reason) -> Self {
        Self {
            by_url: true,
            ..Self::new(reason)
        }
    }

    /// Opens the folder dialog or the address prompt.
    fn pick(&self) -> (Step, Option<Effect>) {
        let effect = if self.by_url {
            Effect::AskUrl(self.url.clone())
        } else {
            Effect::OpenPicker
        };
        (Step::Picking, Some(effect))
    }

    pub fn input(&mut self, input: Input) -> Option<Effect> {
        let step = std::mem::replace(&mut self.step, Step::Welcome);
        let (step, effect) = self.next(step, input);
        self.step = step;
        effect
    }

    fn next(&mut self, step: Step, input: Input) -> (Step, Option<Effect>) {
        use Input::*;
        match (step, input) {
            (Step::Welcome, Confirm) => self.pick(),
            (Step::Welcome, Back) => (
                Step::Welcome,
                Some(match self.reason {
                    Reason::Requested => Effect::Leave,
                    Reason::Missing(_) => Effect::Quit,
                }),
            ),
            (Step::Picking, Picked(None)) => (Step::Welcome, None),
            (Step::Picking, Picked(Some(dir))) => detect(dir),
            (Step::Picking, Url(None)) => (Step::Welcome, None),
            (Step::Picking, Url(Some(url))) => {
                let url = url.trim().to_string();
                if url.is_empty() {
                    return (Step::Welcome, None);
                }
                self.url = Some(url.clone());
                (running(PathBuf::new(), None), Some(Effect::Download(url)))
            }
            (Step::Detecting(_), Back) => (Step::Welcome, None),
            (Step::Detecting(dir), Detected(r)) => match r {
                Ok(detected) => (found(dir, detected, false, Vec::new()), None),
                Err(failure) => (
                    Step::Failed {
                        failure,
                        dir: Some(dir),
                    },
                    None,
                ),
            },
            (
                Step::Found {
                    dir,
                    detected,
                    movies,
                    verify,
                    mismatched,
                },
                input @ (ToggleMovies | ToggleVerify),
            ) => {
                // Only what applies: videos when there are some, checksums for archives.
                let (movies, verify) = match input {
                    ToggleMovies => (!movies && movies_available(&detected), verify),
                    _ => (movies, !verify && archives(&detected).is_some()),
                };
                (
                    Step::Found {
                        dir,
                        detected,
                        movies,
                        verify,
                        mismatched,
                    },
                    None,
                )
            }
            (
                Step::Found {
                    dir,
                    detected,
                    movies,
                    verify,
                    ..
                },
                Confirm,
            ) => match archives(&detected).filter(|_| verify).map(<[_]>::to_vec) {
                Some(list) => (
                    running(dir, Some((detected, movies))),
                    Some(Effect::Verify(list)),
                ),
                None => (
                    running(dir, None),
                    Some(Effect::Run {
                        source: detected.source,
                        include_movies: movies,
                    }),
                ),
            },
            (Step::Found { .. }, Back) => (Step::Welcome, None),
            (
                Step::Running {
                    dir,
                    progress,
                    step_since,
                    cancelling: false,
                    checking,
                },
                Back,
            ) => (
                Step::Running {
                    dir,
                    progress,
                    step_since,
                    cancelling: true,
                    checking,
                },
                Some(Effect::Cancel),
            ),
            (
                Step::Running {
                    dir,
                    progress: old,
                    step_since,
                    cancelling,
                    checking,
                },
                Progress(p, at),
            ) => {
                let new_step = old
                    .as_ref()
                    .is_none_or(|o| (o.phase, &o.detail) != (p.phase, &p.detail));
                (
                    Step::Running {
                        dir,
                        progress: Some(p),
                        step_since: if new_step { at } else { step_since },
                        cancelling,
                        checking,
                    },
                    None,
                )
            }
            (
                Step::Running {
                    dir,
                    checking: Some((detected, movies)),
                    ..
                },
                Verified(r),
            ) => match r {
                Ok(bad) if bad.is_empty() => (
                    running(dir, None),
                    Some(Effect::Run {
                        source: detected.source,
                        include_movies: movies,
                    }),
                ),
                Ok(bad) => (found(dir, detected, movies, bad), None),
                Err(failure) => (
                    Step::Failed {
                        failure,
                        dir: Some(dir),
                    },
                    None,
                ),
            },
            (
                Step::Running {
                    dir,
                    checking: None,
                    ..
                },
                Finished(r),
            ) => match r {
                Ok(summary) => (Step::Done(summary), None),
                Err(failure) => (
                    Step::Failed {
                        failure,
                        // By URL, "try again" asks for the address again (pre-filled).
                        dir: (!self.by_url).then_some(dir),
                    },
                    None,
                ),
            },
            (Step::Done(_), Confirm | Back) => (Step::Welcome, Some(Effect::Finish)),
            (Step::Failed { dir: Some(dir), .. }, Confirm) => detect(dir),
            (Step::Failed { dir: None, .. }, Confirm) => self.pick(),
            (Step::Failed { .. }, Back) => (Step::Welcome, None),
            // Anything else (a late result, a key during the dialog) changes nothing.
            (step, _) => (step, None),
        }
    }
}

fn detect(dir: PathBuf) -> (Step, Option<Effect>) {
    (Step::Detecting(dir.clone()), Some(Effect::Detect(dir)))
}

/// `n` bytes for people: `512 B`, `12.3 MB`, `6.10 GB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut unit = "";
    for u in UNITS {
        v /= 1000.0;
        unit = u;
        if v < 1000.0 {
            break;
        }
    }
    if v < 10.0 {
        format!("{v:.2} {unit}")
    } else if v < 100.0 {
        format!("{v:.1} {unit}")
    } else {
        format!("{v:.0} {unit}")
    }
}

/// Whole percent of `done` / `total` (`None` when the total is unknown).
pub fn percent(done: u64, total: u64) -> Option<u32> {
    (total > 0).then(|| ((done.min(total) as f64 / total as f64) * 100.0).floor() as u32)
}

/// Seconds left at the average rate so far, once there is enough to go on (2 s, some bytes).
pub fn eta(done: u64, total: u64, elapsed: f64) -> Option<f64> {
    if total == 0 || done == 0 || elapsed < 2.0 {
        return None;
    }
    let rate = done as f64 / elapsed;
    Some(total.saturating_sub(done) as f64 / rate)
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A path for the screen, with `/` separators: the fallback UI font (Meiryo) draws `\` as `¥`.
pub fn shown(p: &std::path::Path) -> String {
    p.display().to_string().replace('\\', "/")
}

/// What a phase is doing, for people; `true` when its counts are bytes.
pub fn phase_name(phase: Phase) -> (&'static str, bool) {
    match phase {
        Phase::Verify => ("Checking archives", true),
        Phase::Prepare => ("Preparing", false),
        Phase::Extract => ("Unpacking archives", true),
        Phase::Copy => ("Copying game files", true),
        Phase::Convert => ("Converting", false),
        Phase::Validate => ("Checking charts", false),
        Phase::Download => ("Downloading", true),
    }
}

/// The progress lines: step (`Unpacking archives 3.05.00`), percent, `bytes / bytes` (or
/// `n / total`), `about m:ss left`.
pub fn progress_text(p: &Progress, elapsed: f64) -> [String; 4] {
    let (name, in_bytes) = phase_name(p.phase);
    let step = if p.detail.is_empty() {
        name.to_string()
    } else {
        format!("{name}  {}", p.detail)
    };
    let pct = percent(p.done, p.total).map_or_else(|| "…".to_string(), |v| format!("{v}%"));
    let count = |n| if in_bytes { bytes(n) } else { n.to_string() };
    let amount = if p.total > 0 {
        format!("{} / {}", count(p.done), count(p.total))
    } else if p.done > 0 {
        count(p.done)
    } else {
        String::new()
    };
    let eta = eta(p.done, p.total, elapsed)
        .map_or_else(String::new, |s| format!("about {} left", clock(s)));
    [step, pct, amount, eta]
}

/// How serious a report line is (its colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    /// Setup will fail or the result will lack something important.
    Bad,
}

/// One detection finding, worded for the screen.
pub fn finding_text(f: &Finding) -> (String, Severity) {
    let name = |p: &std::path::Path| {
        p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    match f {
        Finding::MissingUpdates(v) => (
            format!("Missing updates: {} (older version)", v.join(", ")),
            Severity::Warning,
        ),
        Finding::ChecksumNotVerified => ("Archive checksums not verified".into(), Severity::Info),
        Finding::Renamed { archive, version } => (
            format!("{} recognised as {version}", name(archive)),
            Severity::Info,
        ),
        Finding::SizeMismatch {
            archive,
            expected,
            actual,
        } => (
            format!(
                "{}: {} instead of {} (incomplete download?)",
                name(archive),
                bytes(*actual),
                bytes(*expected)
            ),
            Severity::Bad,
        ),
        Finding::Movies {
            present: true,
            bytes: b,
        } => (
            format!("Background videos found ({})", bytes(*b)),
            Severity::Info,
        ),
        Finding::Movies { present: false, .. } => {
            ("No background videos found".into(), Severity::Warning)
        }
        Finding::Songs(n) => (format!("{n} songs"), Severity::Info),
        Finding::UnknownVersion => (
            "Game version unknown (may still work)".into(),
            Severity::Warning,
        ),
        Finding::UnsupportedVersion(why) => (format!("Unsupported: {why}"), Severity::Bad),
    }
}

/// Whether the detection report offers background videos.
pub fn movies_available(d: &Detected) -> bool {
    d.report
        .iter()
        .any(|f| matches!(f, Finding::Movies { present: true, .. }))
}

/// The screen's wording of a setup error.
pub fn failure(e: &SetupError) -> Failure {
    let (title, lines): (&str, Vec<String>) = match e {
        SetupError::SevenZipMissing => (
            "7-ZIP NOT FOUND",
            vec![
                "Unpacking the .7z archives needs 7-Zip.".into(),
                "Install 7-Zip, or put 7z.exe and 7z.dll next to lily.exe".into(),
                "(or in a tools folder there),".into(),
                "then try again.".into(),
            ],
        ),
        SetupError::DiskSpace {
            needed, available, ..
        } => (
            "NOT ENOUGH DISK SPACE",
            vec![
                format!(
                    "Setup needs {}, but only {} is free.",
                    bytes(*needed),
                    bytes(*available)
                ),
                "Free some space, or leave the background videos out.".into(),
            ],
        ),
        SetupError::UnsupportedVersion(v) => (
            "UNSUPPORTED VERSION",
            vec![
                format!("These game files cannot be used: {v}."),
                "Lily needs WACCA Reverse 3.07.01: the Reverse update".into(),
                "archives (3.00.00 base + 3.01.00 … 3.07.01) or a game folder.".into(),
            ],
        ),
        SetupError::BaseArchiveMissing => (
            "BASE ARCHIVE MISSING",
            vec![
                "Only update archives were found.".into(),
                "The base archive WACCA Reverse (3.00.00).7z is needed too.".into(),
            ],
        ),
        SetupError::NothingFound(dir) => (
            "NO WACCA FILES FOUND",
            vec![
                format!("Nothing usable in {}.", shown(dir)),
                "Choose the folder with the update archives (.7z)".into(),
                "or the game folder (the one containing WindowsNoEditor).".into(),
            ],
        ),
        SetupError::Download(why) => (
            "DOWNLOAD FAILED",
            vec![
                why.clone(),
                "Check the address, and that the computer sharing the pack".into(),
                "is on and on the same network as this device.".into(),
            ],
        ),
        SetupError::Cancelled => (
            "SETUP CANCELLED",
            vec!["Nothing was changed in your game files.".into()],
        ),
        other => ("SETUP FAILED", vec![other.to_string()]),
    };
    Failure {
        title: title.into(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected() -> Detected {
        Detected {
            source: Source::Installed {
                content_dir: PathBuf::from("G/Content"),
            },
            version: Some("3.07".into()),
            report: vec![Finding::Movies {
                present: true,
                bytes: 21_000_000_000,
            }],
        }
    }

    fn progress(phase: Phase, done: u64) -> Progress {
        Progress {
            phase,
            done,
            total: 100,
            detail: String::new(),
        }
    }

    #[test]
    fn happy_path_reaches_the_title() {
        let mut f = Flow::new(Reason::Missing(vec!["x".into()]));
        assert_eq!(f.input(Input::Confirm), Some(Effect::OpenPicker));
        assert!(matches!(f.step, Step::Picking));
        let dir = PathBuf::from("G");
        assert_eq!(
            f.input(Input::Picked(Some(dir.clone()))),
            Some(Effect::Detect(dir.clone()))
        );
        assert_eq!(f.input(Input::Detected(Ok(detected()))), None);
        assert!(matches!(f.step, Step::Found { movies: false, .. }));
        f.input(Input::ToggleMovies);
        assert!(matches!(f.step, Step::Found { movies: true, .. }));
        assert_eq!(
            f.input(Input::Confirm),
            Some(Effect::Run {
                source: detected().source,
                include_movies: true
            })
        );
        f.input(Input::Progress(progress(Phase::Copy, 10), 5.0));
        f.input(Input::Progress(progress(Phase::Copy, 20), 6.0));
        assert!(matches!(
            f.step,
            Step::Running {
                step_since: 5.0,
                ..
            }
        ));
        f.input(Input::Progress(progress(Phase::Convert, 0), 9.0));
        assert!(matches!(
            f.step,
            Step::Running {
                step_since: 9.0,
                ..
            }
        ));
        assert_eq!(f.input(Input::Finished(Ok(Summary::default()))), None);
        assert!(matches!(f.step, Step::Done(_)));
        assert_eq!(f.input(Input::Confirm), Some(Effect::Finish));
    }

    #[test]
    fn videos_can_only_be_chosen_when_found() {
        let mut d = detected();
        d.report = vec![Finding::Movies {
            present: false,
            bytes: 0,
        }];
        let mut f = Flow::new(Reason::Requested);
        f.step = Step::Found {
            dir: "G".into(),
            detected: d,
            movies: false,
            verify: false,
            mismatched: Vec::new(),
        };
        f.input(Input::ToggleMovies);
        assert!(matches!(f.step, Step::Found { movies: false, .. }));
    }

    #[test]
    fn back_quits_on_first_run_and_leaves_when_requested() {
        let mut f = Flow::new(Reason::Missing(vec![]));
        assert_eq!(f.input(Input::Back), Some(Effect::Quit));
        let mut f = Flow::new(Reason::Requested);
        assert_eq!(f.input(Input::Back), Some(Effect::Leave));
    }

    #[test]
    fn cancelled_dialog_and_late_results_are_harmless() {
        let mut f = Flow::new(Reason::Requested);
        f.input(Input::Confirm);
        assert_eq!(f.input(Input::Picked(None)), None);
        assert!(matches!(f.step, Step::Welcome));
        // A detection result after Back is ignored.
        f.input(Input::Confirm);
        f.input(Input::Picked(Some("G".into())));
        f.input(Input::Back);
        assert_eq!(f.input(Input::Detected(Ok(detected()))), None);
        assert!(matches!(f.step, Step::Welcome));
        assert_eq!(f.input(Input::ToggleMovies), None);
    }

    #[test]
    fn errors_show_and_retry_the_same_folder() {
        let mut f = Flow::new(Reason::Requested);
        f.input(Input::Confirm);
        f.input(Input::Picked(Some("G".into())));
        let fail = failure(&SetupError::SevenZipMissing);
        f.input(Input::Detected(Err(fail.clone())));
        assert!(matches!(&f.step, Step::Failed { failure, .. } if *failure == fail));
        assert_eq!(f.input(Input::Confirm), Some(Effect::Detect("G".into())));
        f.input(Input::Detected(Err(fail)));
        assert_eq!(f.input(Input::Back), None);
        assert!(matches!(f.step, Step::Welcome));
    }

    fn archive_set() -> Detected {
        Detected {
            source: Source::Archives(vec![ArchiveFile {
                path: "D/WACCA Reverse (3.00.00).7z".into(),
                version: "3.00.00".into(),
                size: 1,
                md5: "x",
            }]),
            version: Some("3.07.01".into()),
            report: vec![Finding::ChecksumNotVerified],
        }
    }

    fn found_step(detected: Detected) -> Step {
        Step::Found {
            dir: "D".into(),
            detected,
            movies: false,
            verify: false,
            mismatched: Vec::new(),
        }
    }

    #[test]
    fn checksums_first_when_chosen_then_run() {
        let mut f = Flow::new(Reason::Requested);
        f.step = found_step(archive_set());
        f.input(Input::ToggleVerify);
        let Some(Effect::Verify(list)) = f.input(Input::Confirm) else {
            panic!("{:?}", f.step)
        };
        assert_eq!(list.len(), 1);
        assert!(matches!(
            f.step,
            Step::Running {
                checking: Some(_),
                ..
            }
        ));
        // A late `Finished` while checking is ignored.
        assert_eq!(f.input(Input::Finished(Ok(Summary::default()))), None);
        assert_eq!(
            f.input(Input::Verified(Ok(Vec::new()))),
            Some(Effect::Run {
                source: archive_set().source,
                include_movies: false
            })
        );
        assert!(matches!(f.step, Step::Running { checking: None, .. }));
    }

    #[test]
    fn checksum_mismatch_warns_and_allows_continuing() {
        let mut f = Flow::new(Reason::Requested);
        f.step = found_step(archive_set());
        f.input(Input::ToggleVerify);
        f.input(Input::Confirm);
        f.input(Input::Verified(Ok(vec!["a.7z".into()])));
        assert!(matches!(
            &f.step,
            Step::Found { verify: false, mismatched, .. } if mismatched == &["a.7z"]
        ));
        assert!(matches!(f.input(Input::Confirm), Some(Effect::Run { .. })));
    }

    #[test]
    fn no_checksum_choice_for_an_installed_game() {
        let mut f = Flow::new(Reason::Requested);
        f.step = found_step(detected());
        f.input(Input::ToggleVerify);
        assert!(matches!(f.step, Step::Found { verify: false, .. }));
        assert!(matches!(f.input(Input::Confirm), Some(Effect::Run { .. })));
    }

    #[test]
    fn cancel_while_running() {
        let mut f = Flow::new(Reason::Requested);
        f.step = Step::Found {
            dir: "G".into(),
            detected: detected(),
            movies: false,
            verify: false,
            mismatched: Vec::new(),
        };
        f.input(Input::Confirm);
        assert_eq!(f.input(Input::Back), Some(Effect::Cancel));
        assert!(matches!(
            f.step,
            Step::Running {
                cancelling: true,
                ..
            }
        ));
        // A second Back does not cancel twice.
        assert_eq!(f.input(Input::Back), None);
        f.input(Input::Finished(Err(failure(&SetupError::Cancelled))));
        assert!(
            matches!(&f.step, Step::Failed { failure, .. } if failure.title == "SETUP CANCELLED")
        );
    }

    #[test]
    fn progress_formatting() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_500), "1.50 KB");
        assert_eq!(bytes(12_345_678), "12.3 MB");
        assert_eq!(bytes(6_100_000_000), "6.10 GB");
        assert_eq!(bytes(250_000_000_000), "250 GB");
        assert_eq!(percent(1, 3), Some(33));
        assert_eq!(percent(5, 0), None);
        assert_eq!(percent(9, 5), Some(100));
        assert_eq!(eta(50, 100, 10.0), Some(10.0));
        assert_eq!(eta(50, 100, 1.0), None);
        assert_eq!(eta(0, 100, 10.0), None);
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(3723.0), "1:02:03");
        let p = Progress {
            phase: Phase::Extract,
            done: 2_000_000_000,
            total: 6_000_000_000,
            detail: "3.05.00".into(),
        };
        assert_eq!(
            progress_text(&p, 60.0),
            [
                "Unpacking archives  3.05.00".to_string(),
                "33%".into(),
                "2.00 GB / 6.00 GB".into(),
                "about 2:00 left".into()
            ]
        );
        let songs = Progress {
            phase: Phase::Validate,
            done: 12,
            total: 300,
            detail: String::new(),
        };
        assert_eq!(progress_text(&songs, 1.0)[0], "Checking charts");
        assert_eq!(progress_text(&songs, 1.0)[2], "12 / 300");
        let unknown = Progress {
            total: 0,
            done: 0,
            ..p
        };
        assert_eq!(progress_text(&unknown, 60.0)[1], "…");
        assert_eq!(progress_text(&unknown, 60.0)[2], "");
    }

    #[test]
    fn findings_are_worded_and_coloured() {
        let (t, s) = finding_text(&Finding::MissingUpdates(vec!["3.07.01".into()]));
        assert!(t.contains("3.07.01") && s == Severity::Warning);
        let (t, s) = finding_text(&Finding::SizeMismatch {
            archive: "D:/a/WACCA Reverse (3.01.00).7z".into(),
            expected: 2_000_000_000,
            actual: 1_000_000_000,
        });
        assert!(t.starts_with("WACCA Reverse (3.01.00).7z") && s == Severity::Bad);
        let (t, _) = finding_text(&Finding::Movies {
            present: true,
            bytes: 21_369_353_312,
        });
        assert_eq!(t, "Background videos found (21.4 GB)");
        assert_eq!(
            finding_text(&Finding::UnsupportedVersion("x".into())).1,
            Severity::Bad
        );
        assert!(movies_available(&detected()));
    }

    #[test]
    fn by_url_asks_for_an_address_downloads_and_retries_with_it() {
        let mut f = Flow::by_url(Reason::Missing(vec!["x".into()]));
        assert_eq!(f.input(Input::Confirm), Some(Effect::AskUrl(None)));
        // Cancelled or empty: back to the welcome.
        assert_eq!(f.input(Input::Url(None)), None);
        assert!(matches!(f.step, Step::Welcome));
        f.input(Input::Confirm);
        assert_eq!(f.input(Input::Url(Some("  ".into()))), None);
        assert!(matches!(f.step, Step::Welcome));
        f.input(Input::Confirm);
        let url = "http://mac.local:8000/lily-pack.tar";
        assert_eq!(
            f.input(Input::Url(Some(format!(" {url} ")))),
            Some(Effect::Download(url.into()))
        );
        assert!(matches!(f.step, Step::Running { .. }));
        f.input(Input::Progress(progress(Phase::Download, 10), 1.0));
        let failure = failure(&SetupError::Download("HTTP 404".into()));
        assert_eq!(f.input(Input::Finished(Err(failure))), None);
        assert!(matches!(f.step, Step::Failed { dir: None, .. }));
        // Try again: the prompt comes back with the last address.
        assert_eq!(
            f.input(Input::Confirm),
            Some(Effect::AskUrl(Some(url.into())))
        );
        f.input(Input::Url(Some(url.into())));
        assert_eq!(f.input(Input::Finished(Ok(Summary::default()))), None);
        assert_eq!(f.input(Input::Confirm), Some(Effect::Finish));
    }

    #[test]
    fn errors_map_to_screen_text() {
        let f = failure(&SetupError::SevenZipMissing);
        assert_eq!(f.title, "7-ZIP NOT FOUND");
        let f = failure(&SetupError::DiskSpace {
            path: "D:/".into(),
            needed: 6_000_000_000,
            available: 1_000_000_000,
        });
        assert_eq!(f.title, "NOT ENOUGH DISK SPACE");
        assert!(f.lines[0].contains("6.00 GB") && f.lines[0].contains("1.00 GB"));
        let f = failure(&SetupError::UnsupportedVersion("no song table".into()));
        assert_eq!(f.title, "UNSUPPORTED VERSION");
        assert!(f.lines[0].contains("no song table"));
        let f = failure(&SetupError::NothingFound("D:/x".into()));
        assert_eq!(f.title, "NO WACCA FILES FOUND");
        assert_eq!(shown(std::path::Path::new(r"C:\G\x")), "C:/G/x");
        assert_eq!(
            failure(&SetupError::BaseArchiveMissing).title,
            "BASE ARCHIVE MISSING"
        );
        let f = failure(&SetupError::MissingCore("boom".into()));
        assert_eq!(
            (f.title.as_str(), f.lines[0].as_str()),
            (
                "SETUP FAILED",
                "a required game file is missing or broken: boom"
            )
        );
        // No download locations anywhere on the screens.
        for e in [
            SetupError::SevenZipMissing,
            SetupError::BaseArchiveMissing,
            SetupError::NothingFound("x".into()),
        ] {
            assert!(!failure(&e).lines.iter().any(|l| l.contains("http")));
        }
    }
}
