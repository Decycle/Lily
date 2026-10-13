//! First-run setup as a pure state machine: welcome → folder picker → detection report (with the
//! background-video choice) → running (progress, cancel) → summary → title. Errors end on a
//! failure step that explains what to do. The shell performs the [`Effect`]s (dialog, worker
//! threads) and feeds their results back as [`Input`]s.

use std::path::PathBuf;

use crate::setup_backend::{Detected, Progress, SetupError, Source, Summary};

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
    /// The folder dialog is open.
    Picking,
    Detecting(PathBuf),
    Found {
        dir: PathBuf,
        detected: Detected,
        movies: bool,
    },
    Running {
        dir: PathBuf,
        progress: Option<Progress>,
        /// When the current step started (s, the shell's clock).
        step_since: f64,
        cancelling: bool,
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
    Toggle,
    /// The dialog closed: the chosen folder, if any.
    Picked(Option<PathBuf>),
    Detected(Result<Detected, Failure>),
    /// Progress at time `at` (s).
    Progress(Progress, f64),
    Finished(Result<Summary, Failure>),
}

/// What the shell has to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    OpenPicker,
    Detect(PathBuf),
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
}

impl Flow {
    pub fn new(reason: Reason) -> Self {
        Self {
            step: Step::Welcome,
            reason,
        }
    }

    pub fn input(&mut self, input: Input) -> Option<Effect> {
        let step = std::mem::replace(&mut self.step, Step::Welcome);
        let (step, effect) = self.next(step, input);
        self.step = step;
        effect
    }

    fn next(&self, step: Step, input: Input) -> (Step, Option<Effect>) {
        use Input::*;
        match (step, input) {
            (Step::Welcome, Confirm) => (Step::Picking, Some(Effect::OpenPicker)),
            (Step::Welcome, Back) => (
                Step::Welcome,
                Some(match self.reason {
                    Reason::Requested => Effect::Leave,
                    Reason::Missing(_) => Effect::Quit,
                }),
            ),
            (Step::Picking, Picked(None)) => (Step::Welcome, None),
            (Step::Picking, Picked(Some(dir))) => detect(dir),
            (Step::Detecting(_), Back) => (Step::Welcome, None),
            (Step::Detecting(dir), Detected(r)) => match r {
                Ok(detected) => (
                    Step::Found {
                        dir,
                        detected,
                        movies: false,
                    },
                    None,
                ),
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
                },
                Toggle,
            ) => (
                Step::Found {
                    dir,
                    detected,
                    movies: !movies,
                },
                None,
            ),
            (
                Step::Found {
                    dir,
                    detected,
                    movies,
                },
                Confirm,
            ) => (
                Step::Running {
                    dir,
                    progress: None,
                    step_since: 0.0,
                    cancelling: false,
                },
                Some(Effect::Run {
                    source: detected.source,
                    include_movies: movies,
                }),
            ),
            (Step::Found { .. }, Back) => (Step::Welcome, None),
            (
                Step::Running {
                    dir,
                    progress,
                    step_since,
                    cancelling: false,
                },
                Back,
            ) => (
                Step::Running {
                    dir,
                    progress,
                    step_since,
                    cancelling: true,
                },
                Some(Effect::Cancel),
            ),
            (
                Step::Running {
                    dir,
                    progress: old,
                    step_since,
                    cancelling,
                },
                Progress(p, at),
            ) => {
                let new_step = old.as_ref().is_none_or(|o| o.step != p.step);
                (
                    Step::Running {
                        dir,
                        progress: Some(p),
                        step_since: if new_step { at } else { step_since },
                        cancelling,
                    },
                    None,
                )
            }
            (Step::Running { dir, .. }, Finished(r)) => match r {
                Ok(summary) => (Step::Done(summary), None),
                Err(failure) => (
                    Step::Failed {
                        failure,
                        dir: Some(dir),
                    },
                    None,
                ),
            },
            (Step::Done(_), Confirm | Back) => (Step::Welcome, Some(Effect::Finish)),
            (Step::Failed { dir: Some(dir), .. }, Confirm) => detect(dir),
            (Step::Failed { dir: None, .. }, Confirm) => (Step::Picking, Some(Effect::OpenPicker)),
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

/// The progress lines: step (`2/3 Converting`), percent, `bytes / bytes`, `ETA m:ss`.
pub fn progress_text(p: &Progress, elapsed: f64) -> [String; 4] {
    let step = format!("{}/{}  {}", p.index + 1, p.steps.max(p.index + 1), p.step);
    let pct = percent(p.done, p.total).map_or_else(|| "…".to_string(), |v| format!("{v}%"));
    let amount = if p.total > 0 {
        format!("{} / {}", bytes(p.done), bytes(p.total))
    } else if p.done > 0 {
        bytes(p.done)
    } else {
        String::new()
    };
    let eta = eta(p.done, p.total, elapsed)
        .map_or_else(String::new, |s| format!("about {} left", clock(s)));
    [step, pct, amount, eta]
}

/// The screen's wording of a setup error.
pub fn failure(e: &SetupError) -> Failure {
    let (title, lines): (&str, Vec<String>) = match e {
        SetupError::SevenZipMissing => (
            "7-ZIP NOT FOUND",
            vec![
                "Unpacking the .7z archives needs 7-Zip.".into(),
                "Install 7-Zip, or put 7z.exe and 7z.dll next to wacca-app.exe,".into(),
                "then try again.".into(),
            ],
        ),
        SetupError::NoSpace { needed, available } => (
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
                format!("These files are WACCA {v}."),
                "WACCA_slim needs WACCA Reverse 3.07.01: the Reverse update".into(),
                "archives (3.00.00 base + 3.01.00 … 3.07.01) or a game folder.".into(),
            ],
        ),
        SetupError::NothingFound(dir) => (
            "NO WACCA FILES FOUND",
            vec![
                format!("Nothing usable in {}.", dir.display()),
                "Choose the folder with the update archives (.7z)".into(),
                "or the game folder (the one containing WindowsNoEditor).".into(),
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
    use crate::setup_backend::{Finding, Severity};

    fn detected() -> Detected {
        Detected {
            source: Source::Installed(PathBuf::from("G/Content")),
            version: Some("3.07.01".into()),
            report: vec![Finding {
                severity: Severity::Info,
                message: "ok".into(),
            }],
        }
    }

    fn progress(step: &str, done: u64) -> Progress {
        Progress {
            step: step.into(),
            index: 0,
            steps: 2,
            done,
            total: 100,
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
        f.input(Input::Toggle);
        assert!(matches!(f.step, Step::Found { movies: true, .. }));
        assert_eq!(
            f.input(Input::Confirm),
            Some(Effect::Run {
                source: Source::Installed(PathBuf::from("G/Content")),
                include_movies: true
            })
        );
        f.input(Input::Progress(progress("Copy", 10), 5.0));
        f.input(Input::Progress(progress("Copy", 20), 6.0));
        assert!(matches!(
            f.step,
            Step::Running {
                step_since: 5.0,
                ..
            }
        ));
        f.input(Input::Progress(progress("Convert", 0), 9.0));
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
        assert_eq!(f.input(Input::Toggle), None);
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

    #[test]
    fn cancel_while_running() {
        let mut f = Flow::new(Reason::Requested);
        f.step = Step::Found {
            dir: "G".into(),
            detected: detected(),
            movies: false,
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
            step: "Copying".into(),
            index: 0,
            steps: 3,
            done: 2_000_000_000,
            total: 6_000_000_000,
        };
        assert_eq!(
            progress_text(&p, 60.0),
            [
                "1/3  Copying".to_string(),
                "33%".into(),
                "2.00 GB / 6.00 GB".into(),
                "about 2:00 left".into()
            ]
        );
        let unknown = Progress {
            total: 0,
            done: 0,
            ..p
        };
        assert_eq!(progress_text(&unknown, 60.0)[1], "…");
        assert_eq!(progress_text(&unknown, 60.0)[2], "");
    }

    #[test]
    fn errors_map_to_screen_text() {
        let f = failure(&SetupError::SevenZipMissing);
        assert_eq!(f.title, "7-ZIP NOT FOUND");
        let f = failure(&SetupError::NoSpace {
            needed: 6_000_000_000,
            available: 1_000_000_000,
        });
        assert_eq!(f.title, "NOT ENOUGH DISK SPACE");
        assert!(f.lines[0].contains("6.00 GB") && f.lines[0].contains("1.00 GB"));
        let f = failure(&SetupError::UnsupportedVersion("2.50.00".into()));
        assert!(f.lines[0].contains("2.50.00"));
        let f = failure(&SetupError::NothingFound("D:/x".into()));
        assert_eq!(f.title, "NO WACCA FILES FOUND");
        let f = failure(&SetupError::Other("boom".into()));
        assert_eq!(
            (f.title.as_str(), f.lines[0].as_str()),
            ("SETUP FAILED", "boom")
        );
    }
}
