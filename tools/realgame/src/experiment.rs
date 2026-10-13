//! Small helpers of `run_experiment` (playback itself is [`crate::inject`]).

use std::collections::BTreeMap;
use std::path::Path;

/// Files under a directory: relative path -> (size, modification time in ms since the Unix epoch).
pub type Listing = BTreeMap<String, (u64, u128)>;

/// Recursive listing of `root` (empty if it cannot be read); used to see which files the game writes in a run.
pub fn list_files(root: &Path) -> Listing {
    fn walk(dir: &Path, root: &Path, out: &mut Listing) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis());
                out.insert(rel.display().to_string(), (meta.len(), mtime));
            }
        }
    }
    let mut out = Listing::new();
    walk(root, root, &mut out);
    out
}

/// `added|changed|removed <path> <size>` lines for the differences between two listings.
pub fn diff_files(before: &Listing, after: &Listing) -> Vec<String> {
    let mut out = Vec::new();
    for (path, &(size, mtime)) in after {
        match before.get(path) {
            None => out.push(format!("added {path} {size}")),
            Some(&(s, m)) if (s, m) != (size, mtime) => out.push(format!("changed {path} {size}")),
            Some(_) => {}
        }
    }
    for (path, &(size, _)) in before {
        if !after.contains_key(path) {
            out.push(format!("removed {path} {size}"));
        }
    }
    out
}

/// `YYYYMMDD-HHMMSS` (UTC) for a Unix time, used as the run directory name.
pub fn run_name(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs = unix_secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_diff() {
        let l = |v: &[(&str, u64, u128)]| -> Listing {
            v.iter().map(|&(p, s, m)| (p.to_string(), (s, m))).collect()
        };
        let before = l(&[("a.csv", 10, 1), ("b.log", 5, 1), ("c", 1, 1)]);
        let after = l(&[("a.csv", 10, 1), ("b.log", 7, 2), ("d.csv", 3, 2)]);
        assert_eq!(
            diff_files(&before, &after),
            ["changed b.log 7", "added d.csv 3", "removed c 1"]
        );
        assert!(list_files(Path::new("does/not/exist")).is_empty());
    }

    #[test]
    fn run_names() {
        assert_eq!(run_name(0), "19700101-000000");
        assert_eq!(run_name(1_791_798_777), "20261012-095257");
        assert_eq!(run_name(951_782_400), "20000229-000000");
    }
}
