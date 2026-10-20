//! First-run setup backend: the `wacca-setup` crate behind a trait, so the flow and its worker
//! threads can be exercised without game files.

use std::path::Path;

pub use wacca_setup::{
    ArchiveFile, Detected, Finding, Phase, Plan, Progress, SetupError, Source, Summary,
};

/// The setup operations the app needs. Every `progress` callback returning `false` cancels
/// (→ [`SetupError::Cancelled`]).
pub trait Backend: Send + Sync + std::fmt::Debug {
    fn detect(&self, dir: &Path) -> Result<Detected, SetupError>;
    /// md5 check of one archive: `Ok(false)` = mismatch.
    fn verify(
        &self,
        archive: &ArchiveFile,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<bool, SetupError>;
    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError>;
    /// Installs a Lily folder that an import copied into `wacca_setup::import_staging(dest)`.
    fn finish_import(&self, dest: &Path) -> Result<Summary, SetupError>;
    /// Drops an unfinished import's copy.
    fn discard_import(&self, dest: &Path);
    /// Downloads the Lily pack at `url` and installs it into `dest` (with its background movies
    /// if `include_movies`).
    fn download(
        &self,
        url: &str,
        dest: &Path,
        include_movies: bool,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError>;
}

/// [`wacca_setup`] itself.
#[derive(Debug, Default)]
pub struct Real;

impl Backend for Real {
    fn detect(&self, dir: &Path) -> Result<Detected, SetupError> {
        wacca_setup::detect(dir)
    }

    fn verify(
        &self,
        archive: &ArchiveFile,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<bool, SetupError> {
        wacca_setup::verify(archive, progress)
    }

    fn run(
        &self,
        source: &Source,
        plan: &Plan,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError> {
        wacca_setup::run(source, plan, progress)
    }

    fn download(
        &self,
        url: &str,
        dest: &Path,
        include_movies: bool,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Summary, SetupError> {
        wacca_setup::download(url, dest, include_movies, progress)
    }

    fn finish_import(&self, dest: &Path) -> Result<Summary, SetupError> {
        wacca_setup::finish_import(dest)
    }

    fn discard_import(&self, dest: &Path) {
        wacca_setup::discard_import(dest);
    }
}
