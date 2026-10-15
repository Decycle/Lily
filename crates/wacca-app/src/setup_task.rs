//! Runs the setup flow's effects off the UI thread: the folder dialog, detection, the setup
//! itself and a pack download each on a worker thread (the address prompt on the UI thread, as
//! UIKit wants), reporting back over a channel polled every frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::folder_dialog;
use crate::setup_backend::{Backend, Plan, Progress};
use crate::setup_flow::{Effect, Failure, Flow, Input, Reason, Step, failure};
use crate::url_prompt;

/// What the shell does when setup ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Files were installed: reload the game.
    Finished,
    /// Back to the title, nothing changed.
    Left,
    Quit,
}

#[derive(Debug)]
pub struct SetupRun {
    pub flow: Flow,
    backend: Arc<dyn Backend>,
    /// `<root>/assets` and `<root>/data` are written.
    root: PathBuf,
    /// Window that owns the folder dialog (`HWND`), or the view the address prompt shows over
    /// (`UIView`).
    owner: isize,
    tx: Sender<Input>,
    rx: Receiver<Input>,
    cancel: Arc<AtomicBool>,
    clock: Instant,
    /// Start right after detection and continue after the run (`--setup-start`).
    auto_start: bool,
}

impl SetupRun {
    /// `by_url`: set up from a downloaded pack ([`Flow::by_url`]).
    pub fn new(
        reason: Reason,
        backend: Arc<dyn Backend>,
        root: PathBuf,
        owner: isize,
        by_url: bool,
    ) -> Self {
        let (tx, rx) = channel();
        Self {
            flow: if by_url {
                Flow::by_url(reason)
            } else {
                Flow::new(reason)
            },
            backend,
            root,
            owner,
            tx,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            clock: Instant::now(),
            auto_start: false,
        }
    }

    /// Inspect `dir` right away (skipping the dialog); `start`: then run without videos and
    /// continue to the title.
    pub fn preselect(&mut self, dir: PathBuf, start: bool) -> Option<Exit> {
        self.auto_start = start;
        self.flow.step = Step::Picking;
        self.input(Input::Picked(Some(dir)))
    }

    /// Download the pack at `url` right away (skipping the prompt); `start`: continue to the
    /// title when it is installed.
    pub fn preselect_url(&mut self, url: String, start: bool) -> Option<Exit> {
        self.auto_start = start;
        self.flow.step = Step::Picking;
        self.input(Input::Url(Some(url)))
    }

    /// Shell clock for progress timing (s).
    pub fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    /// Feeds `input` to the flow and performs what it asks for.
    pub fn input(&mut self, input: Input) -> Option<Exit> {
        let effect = self.flow.input(input)?;
        self.perform(effect)
    }

    /// Results that arrived from the workers since the last frame.
    pub fn poll(&mut self) -> Option<Exit> {
        while let Ok(input) = self.rx.try_recv() {
            // `--setup-start` presses START after detection and PLAY after a successful run.
            let ok = matches!(input, Input::Detected(Ok(_)) | Input::Finished(Ok(_)));
            let finished = matches!(input, Input::Finished(_));
            if let Some(exit) = self.input(input) {
                return Some(exit);
            }
            let auto = self.auto_start;
            if finished {
                self.auto_start = false;
            }
            if ok
                && auto
                && let Some(exit) = self.input(Input::Confirm)
            {
                return Some(exit);
            }
        }
        None
    }

    /// A progress callback for a new worker: forwards progress, returns `false` once cancelled.
    fn progress_sender(&mut self) -> impl FnMut(Progress) -> bool + Send + 'static {
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, clock) = (self.tx.clone(), self.clock);
        move |p| {
            let _ = tx.send(Input::Progress(p, clock.elapsed().as_secs_f64()));
            !cancel.load(Ordering::Relaxed)
        }
    }

    fn perform(&mut self, effect: Effect) -> Option<Exit> {
        let tx = self.tx.clone();
        match effect {
            Effect::OpenPicker => {
                let owner = self.owner;
                spawn("setup-picker", move || {
                    let picked = folder_dialog::pick_folder(owner, "Lily: choose your WACCA files");
                    let input = match picked {
                        Ok(dir) => Input::Picked(dir),
                        Err(e) => {
                            log::error!("folder dialog: {e}");
                            Input::Picked(None)
                        }
                    };
                    let _ = tx.send(input);
                });
            }
            Effect::AskUrl(prefill) => {
                url_prompt::ask(
                    self.owner,
                    prefill.as_deref(),
                    Box::new(move |url| {
                        let _ = tx.send(Input::Url(url));
                    }),
                );
            }
            Effect::Download(url) => {
                let backend = Arc::clone(&self.backend);
                let root = self.root.clone();
                let mut progress = self.progress_sender();
                log::info!("setup: download {url} → {}", root.display());
                spawn("setup-download", move || {
                    let _awake = url_prompt::KeepAwake::new();
                    let r = backend.download(&url, &root, &mut progress).map_err(|e| {
                        log::error!("setup: {e}");
                        failure(&e)
                    });
                    if let Ok(s) = &r {
                        log::info!("setup done: {s:?}");
                    }
                    let _ = tx.send(Input::Finished(r));
                });
            }
            Effect::Detect(dir) => {
                let backend = Arc::clone(&self.backend);
                spawn("setup-detect", move || {
                    let r = backend.detect(&dir).map_err(|e| {
                        log::warn!("setup: {e}");
                        failure(&e)
                    });
                    let _ = tx.send(Input::Detected(r));
                });
            }
            Effect::Verify(archives) => {
                let backend = Arc::clone(&self.backend);
                let mut progress = self.progress_sender();
                spawn("setup-verify", move || {
                    let mut bad = Vec::new();
                    let mut r = Ok(());
                    for a in &archives {
                        match backend.verify(a, &mut progress) {
                            Ok(true) => {}
                            Ok(false) => {
                                log::warn!("checksum mismatch: {}", a.path.display());
                                bad.push(file_name(&a.path));
                            }
                            Err(e) => {
                                log::error!("verify: {e}");
                                r = Err(failure(&e));
                                break;
                            }
                        }
                    }
                    let _ = tx.send(Input::Verified(r.map(|()| bad)));
                });
            }
            Effect::Run {
                source,
                include_movies,
            } => {
                let backend = Arc::clone(&self.backend);
                let plan = Plan {
                    include_movies,
                    dest: self.root.clone(),
                };
                let mut progress = self.progress_sender();
                log::info!("setup: {source:?} → {}", plan.dest.display());
                spawn("setup-run", move || {
                    let r = backend.run(&source, &plan, &mut progress).map_err(|e| {
                        log::error!("setup: {e}");
                        failure(&e)
                    });
                    if let Ok(s) = &r {
                        log::info!("setup done: {s:?}");
                    }
                    let _ = tx.send(Input::Finished(r));
                });
            }
            Effect::Cancel => self.cancel.store(true, Ordering::Relaxed),
            Effect::Finish => return Some(Exit::Finished),
            Effect::Leave => return Some(Exit::Left),
            Effect::Quit => return Some(Exit::Quit),
        }
        None
    }
}

/// Runs `f` on a named thread; a thread that cannot start is logged (the step then waits for
/// Back).
fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        log::error!("{name}: {e}");
    }
}

/// The file name of `p` (the whole path if it has none).
fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A failure shown when the game files are still unusable after setup.
pub fn still_missing(parts: &[String]) -> Failure {
    let mut lines = vec!["Setup finished, but the game still cannot start:".to_string()];
    lines.extend(parts.iter().cloned());
    Failure {
        title: "FILES STILL MISSING".into(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::setup_backend::{ArchiveFile, Detected, Phase, SetupError, Source, Summary};

    /// Finds an installed game anywhere; `run` reports 3 progress steps (slowly with videos, for
    /// the cancel test) unless cancelled.
    #[derive(Debug)]
    struct Fake;

    impl Backend for Fake {
        fn detect(&self, dir: &Path) -> Result<Detected, SetupError> {
            Ok(Detected {
                source: Source::Installed {
                    content_dir: dir.to_path_buf(),
                },
                version: None,
                report: Vec::new(),
            })
        }

        fn verify(
            &self,
            _: &ArchiveFile,
            _: &mut dyn FnMut(Progress) -> bool,
        ) -> Result<bool, SetupError> {
            Ok(true)
        }

        fn download(
            &self,
            url: &str,
            _: &Path,
            _: &mut dyn FnMut(Progress) -> bool,
        ) -> Result<Summary, SetupError> {
            match url {
                "http://ok/pack.tar" => Ok(Summary {
                    songs_playable: 9,
                    ..Summary::default()
                }),
                _ => Err(SetupError::Download("HTTP 404".into())),
            }
        }

        fn run(
            &self,
            _: &Source,
            plan: &Plan,
            progress: &mut dyn FnMut(Progress) -> bool,
        ) -> Result<Summary, SetupError> {
            for done in 1..=3 {
                if plan.include_movies {
                    std::thread::sleep(Duration::from_millis(100));
                }
                let p = Progress {
                    phase: Phase::Copy,
                    done,
                    total: 3,
                    detail: String::new(),
                };
                if !progress(p) {
                    return Err(SetupError::Cancelled);
                }
            }
            Ok(Summary {
                songs_playable: 7,
                ..Summary::default()
            })
        }
    }

    fn run() -> SetupRun {
        SetupRun::new(Reason::Requested, Arc::new(Fake), "dest".into(), 0, false)
    }

    #[test]
    fn a_url_downloads_and_continues_to_the_title() {
        let mut r = SetupRun::new(Reason::Requested, Arc::new(Fake), "d".into(), 0, true);
        assert_eq!(r.preselect_url("http://ok/pack.tar".into(), true), None);
        assert_eq!(wait(&mut r, |_| false), Some(Exit::Finished));
        let mut r = SetupRun::new(Reason::Requested, Arc::new(Fake), "d".into(), 0, true);
        r.preselect_url("http://bad/pack.tar".into(), false);
        wait(
            &mut r,
            |s| matches!(s, Step::Failed { failure, dir: None } if failure.title == "DOWNLOAD FAILED"),
        );
    }

    /// Polls until `done` holds for the step or setup exits (max 5 s).
    fn wait(r: &mut SetupRun, done: impl Fn(&Step) -> bool) -> Option<Exit> {
        for _ in 0..500 {
            if let Some(exit) = r.poll() {
                return Some(exit);
            }
            if done(&r.flow.step) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out in {:?}", r.flow.step)
    }

    #[test]
    fn auto_start_runs_through_to_the_title() {
        let mut r = run();
        assert_eq!(r.preselect("G".into(), true), None);
        assert_eq!(wait(&mut r, |_| false), Some(Exit::Finished));
    }

    #[test]
    fn workers_report_back_and_cancel_stops_the_run() {
        let mut r = run();
        r.preselect("G".into(), false);
        wait(&mut r, |s| matches!(s, Step::Found { .. }));
        r.input(Input::Confirm);
        wait(
            &mut r,
            |s| matches!(s, Step::Done(s) if s.songs_playable == 7),
        );
        // A slow run, cancelled.
        r.flow.step = Step::Found {
            dir: "G".into(),
            detected: Fake.detect(Path::new("G")).unwrap(),
            movies: true,
            verify: false,
            mismatched: Vec::new(),
        };
        r.input(Input::Confirm);
        r.input(Input::Back);
        wait(
            &mut r,
            |s| matches!(s, Step::Failed { failure, .. } if failure.title == "SETUP CANCELLED"),
        );
    }
}
