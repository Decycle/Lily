//! Disk-space estimate and check.

use std::collections::HashMap;
use std::path::Path;

use crate::SetupError;
use crate::readset::{ARCHIVE_CONTENT, classify};
use crate::sevenzip::ListEntry;

/// Generous size of the converter output (`data/` is ≈ 0.45 GB for 3.07).
pub const DATA_BYTES: u64 = 1 << 30;
/// Safety margin on top of every estimate.
pub const MARGIN: u64 = 512 << 20;

/// Peak bytes the staged extraction occupies: every read-set file at the largest size any of the
/// `listings` (applied in order, later archives overwriting earlier ones) gives it.
pub fn staged_bytes(listings: &[Vec<ListEntry>], include_movies: bool) -> u64 {
    let prefix = format!("{}\\", ARCHIVE_CONTENT.to_ascii_lowercase());
    let mut sizes: HashMap<String, u64> = HashMap::new();
    for e in listings.iter().flatten().filter(|e| !e.dir) {
        let path = e.path.to_ascii_lowercase();
        let Some(rel) = path.strip_prefix(&prefix) else {
            continue;
        };
        if classify(rel, include_movies).is_some() {
            let s = sizes.entry(rel.to_owned()).or_default();
            *s = (*s).max(e.size);
        }
    }
    sizes.values().sum()
}

/// Errors with [`SetupError::DiskSpace`] unless `needed` (+ [`MARGIN`]) fits in `available`.
pub fn check(path: &Path, needed: u64, available: u64) -> Result<(), SetupError> {
    if needed.saturating_add(MARGIN) <= available {
        return Ok(());
    }
    Err(SetupError::DiskSpace {
        path: path.to_owned(),
        needed: needed + MARGIN,
        available,
    })
}

/// Free bytes for the current user on the volume of `path` (or of its closest existing
/// ancestor); `None` if unknown.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    free_bytes_at(existing)
}

#[cfg(windows)]
fn free_bytes_at(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `free` is a valid out pointer and
    // the two other outputs are optional (null).
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn free_bytes_at(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(path: &str, size: u64) -> ListEntry {
        ListEntry {
            path: format!(r"App\WindowsNoEditor\Mercury\Content\{path}"),
            size,
            dir: false,
        }
    }

    #[test]
    fn staged_bytes_take_the_max_per_path_and_skip_unused() {
        let base = vec![
            e(r"Table\MusicParameterTable.uexp", 100),
            e(r"Sound\Bgm\MER_BGM.awb", 1000),
            e(r"Sound\Voice\v.awb", 5000),
            e(r"Movie\mv.usm", 7000),
            ListEntry {
                path: r"amfs\x".into(),
                size: 9,
                dir: false,
            },
        ];
        let update = vec![
            e(r"Table\MusicParameterTable.uexp", 120),
            e(r"MusicData\S\S_00.mer", 3),
        ];
        let lists = [base, update];
        assert_eq!(staged_bytes(&lists, false), 120 + 1000 + 3);
        assert_eq!(staged_bytes(&lists, true), 120 + 1000 + 3 + 7000);
    }

    #[test]
    fn space_check_includes_margin() {
        let p = Path::new("d");
        assert!(check(p, 100, 100 + MARGIN).is_ok());
        assert!(matches!(
            check(p, 101, 100 + MARGIN),
            Err(SetupError::DiskSpace { needed, available, .. }) if needed == 101 + MARGIN && available == 100 + MARGIN
        ));
    }

    #[test]
    fn free_bytes_of_a_missing_dir_uses_its_ancestor() {
        let p = std::env::temp_dir()
            .join("wacca-setup-no-such-dir")
            .join("x");
        if cfg!(windows) {
            assert!(free_bytes(&p).is_some_and(|b| b > 0));
        }
    }
}
