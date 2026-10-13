//! Runs the setup flow's effects off the UI thread: the folder dialog, detection and the setup
//! itself each on a worker thread, reporting back over a channel polled every frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::folder_dialog;
use crate::setup_backend::{Backend, Plan};
use crate::setup_flow::{Effect, Failure, Flow, Input, Reason, Step, failure};

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
    /// Window that owns the folder dialog (`HWND`).
    owner: isize,
    tx: Sender<Input>,
    rx: Receiver<Input>,
    cancel: Arc<AtomicBool>,
    clock: Instant,
    /// Start right after detection (`--setup-start`).
    auto_start: bool,
}

impl SetupRun {
    pub fn new(reason: Reason, backend: Arc<dyn Backend>, root: PathBuf, owner: isize) -> Self {
        let (tx, rx) = channel();
        Self {
            flow: Flow::new(reason),
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

    /// Inspect `dir` right away (skipping the dialog); `start`: then run without videos.
    pub fn preselect(&mut self, dir: PathBuf, start: bool) -> Option<Exit> {
        self.auto_start = start;
        self.flow.step = Step::Picking;
        self.input(Input::Picked(Some(dir)))
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
            let detected = matches!(input, Input::Detected(Ok(_)));
            if let Some(exit) = self.input(input) {
                return Some(exit);
            }
            if detected && std::mem::take(&mut self.auto_start) {
                self.input(Input::Confirm);
            }
        }
        None
    }

    fn perform(&mut self, effect: Effect) -> Option<Exit> {
        let tx = self.tx.clone();
        match effect {
            Effect::OpenPicker => {
                let owner = self.owner;
                spawn("setup-picker", move || {
                    let picked = folder_dialog::pick_folder(owner, "Choose your WACCA files");
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
            Effect::Run {
                source,
                include_movies,
            } => {
                let backend = Arc::clone(&self.backend);
                let plan = Plan {
                    include_movies,
                    dest: self.root.clone(),
                };
                self.cancel = Arc::new(AtomicBool::new(false));
                let cancel = Arc::clone(&self.cancel);
                let clock = self.clock;
                log::info!("setup: {source:?} → {}", plan.dest.display());
                spawn("setup-run", move || {
                    let progress_tx = tx.clone();
                    // Returning `false` cancels the run.
                    let mut progress = |p| {
                        let at = clock.elapsed().as_secs_f64();
                        let _ = progress_tx.send(Input::Progress(p, at));
                        !cancel.load(Ordering::Relaxed)
                    };
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

/// A failure shown when the game files are still unusable after setup.
pub fn still_missing(parts: &[String]) -> Failure {
    let mut lines = vec!["Setup finished, but the game still cannot start:".to_string()];
    lines.extend(parts.iter().cloned());
    Failure {
        title: "FILES STILL MISSING".into(),
        lines,
    }
}
