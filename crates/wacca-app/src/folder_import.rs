//! "Use a folder on this device": a Lily folder made by setup on a computer (`assets` + `data`),
//! copied to the phone, is picked with the system folder picker and copied into the app. Android
//! only for now ([`crate::android::import_folder`]); elsewhere the picker answers "nothing picked".

use std::path::Path;

/// Callbacks of an import; see [`crate::android::ImportEvents`].
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub struct Events {
    pub picked: Box<dyn FnOnce(bool) + Send>,
    pub progress: Box<dyn FnMut(u64, u64) -> bool + Send>,
    pub copied: Box<dyn FnOnce(Result<(), String>) + Send>,
}

/// Whether this platform offers the import (the setup screen shows its button only then).
pub const AVAILABLE: bool = cfg!(target_os = "android");

/// Opens the picker; the picked folder is copied into `staging`.
#[cfg(target_os = "android")]
pub fn start(staging: &Path, ev: Events) {
    crate::android::import_folder(
        staging,
        crate::android::ImportEvents {
            picked: ev.picked,
            progress: ev.progress,
            copied: ev.copied,
        },
    );
}

#[cfg(not(target_os = "android"))]
pub fn start(_staging: &Path, ev: Events) {
    log::error!("folder import is only available on Android");
    (ev.picked)(false);
}

/// Deletes the picked folder once its files are installed (the import moves them).
pub fn delete_source() {
    #[cfg(target_os = "android")]
    crate::android::delete_import_source();
}
