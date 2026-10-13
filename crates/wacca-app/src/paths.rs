//! Where the game files and the save live. Portable layout: `assets/` and `data/` next to the
//! executable, and with a `portable.txt` marker (or `--portable`) the save in `save/save.json`
//! there instead of `%APPDATA%`. Explicit `--assets` / `--data` / `--save` always win. Pure: the
//! caller passes the executable's folder, the marker check and `%APPDATA%`.

use std::path::{Path, PathBuf};

use wacca_game::save;

/// Marker file next to the executable that turns on the portable save.
pub const PORTABLE_MARKER: &str = "portable.txt";

/// Resolved locations for this run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// Game `Content` folder.
    pub assets: PathBuf,
    /// Converted data (`songs.json`, `ui/`, `jackets/`, `fonts/`).
    pub data: PathBuf,
    /// Save file (`None`: nowhere to save).
    pub save: Option<PathBuf>,
    /// Where first-run setup writes `assets/` and `data/`.
    pub setup_root: PathBuf,
}

/// What the command line and the environment say.
#[derive(Clone, Debug, Default)]
pub struct Inputs<'a> {
    pub assets: Option<&'a Path>,
    pub data: Option<&'a Path>,
    pub save: Option<&'a Path>,
    /// `--portable`.
    pub portable_flag: bool,
    /// Folder of the running executable (`None` if unknown).
    pub exe_dir: Option<&'a Path>,
    /// [`PORTABLE_MARKER`] exists in `exe_dir`.
    pub marker: bool,
    /// `%APPDATA%`.
    pub appdata: Option<&'a Path>,
}

pub fn resolve(i: &Inputs) -> Paths {
    let base = i.exe_dir.map_or_else(PathBuf::new, Path::to_path_buf);
    let assets = i
        .assets
        .map_or_else(|| base.join("assets"), Path::to_path_buf);
    // With an explicit `--assets` the old default (`data` in the working folder) stays.
    let data = match (i.data, i.assets) {
        (Some(d), _) => d.to_path_buf(),
        (None, Some(_)) => PathBuf::from("data"),
        (None, None) => base.join("data"),
    };
    let save = match i.save {
        Some(s) => Some(s.to_path_buf()),
        None if i.portable_flag || i.marker => Some(base.join("save").join("save.json")),
        None => i.appdata.map(save::save_path_in),
    };
    let setup_root = sibling_root(&assets, &data).unwrap_or(base);
    Paths {
        assets,
        data,
        save,
        setup_root,
    }
}

/// `root` when `assets` = `root/assets` and `data` = `root/data`, so setup can fill both.
fn sibling_root(assets: &Path, data: &Path) -> Option<PathBuf> {
    let root = assets.parent()?;
    (assets.file_name()? == "assets" && data == root.join("data")).then(|| root.to_path_buf())
}

/// Files the game cannot start without, as `(path, label)`: the BGM bank, the charts and the
/// converted song list. Empty = complete.
pub fn missing(assets: &Path, data: &Path, exists: impl Fn(&Path) -> bool) -> Vec<String> {
    [
        (
            assets.join("Sound").join("Bgm"),
            "game audio (assets/Sound/Bgm)",
        ),
        (assets.join("MusicData"), "charts (assets/MusicData)"),
        (data.join("songs.json"), "song list (data/songs.json)"),
    ]
    .into_iter()
    .filter(|(p, _)| !exists(p))
    .map(|(_, label)| label.to_string())
    .collect()
}

/// The running executable's folder and whether the portable marker is there.
pub fn exe_dir() -> Option<(PathBuf, bool)> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let marker = dir.join(PORTABLE_MARKER).is_file();
    Some((dir, marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn no_flags_use_the_exe_folder_and_appdata() {
        let exe = p("C:/W");
        let r = resolve(&Inputs {
            exe_dir: Some(&exe),
            appdata: Some(Path::new("C:/AD")),
            ..Default::default()
        });
        assert_eq!(r.assets, p("C:/W").join("assets"));
        assert_eq!(r.data, p("C:/W").join("data"));
        assert_eq!(r.save, Some(save::save_path_in(Path::new("C:/AD"))));
        assert_eq!(r.setup_root, exe);
    }

    #[test]
    fn marker_or_flag_puts_the_save_next_to_the_exe() {
        let exe = p("C:/W");
        let portable = Some(exe.join("save").join("save.json"));
        for (marker, portable_flag) in [(true, false), (false, true)] {
            let r = resolve(&Inputs {
                exe_dir: Some(&exe),
                marker,
                portable_flag,
                appdata: Some(Path::new("C:/AD")),
                ..Default::default()
            });
            assert_eq!(r.save, portable);
        }
        // `--save` still wins.
        let r = resolve(&Inputs {
            exe_dir: Some(&exe),
            marker: true,
            save: Some(Path::new("S.json")),
            ..Default::default()
        });
        assert_eq!(r.save, Some(p("S.json")));
        // No %APPDATA%, not portable: nowhere to save.
        let r = resolve(&Inputs {
            exe_dir: Some(&exe),
            ..Default::default()
        });
        assert_eq!(r.save, None);
    }

    #[test]
    fn explicit_flags_keep_working() {
        let exe = p("C:/W");
        let r = resolve(&Inputs {
            assets: Some(Path::new("A")),
            exe_dir: Some(&exe),
            ..Default::default()
        });
        assert_eq!((r.assets, r.data), (p("A"), p("data")));
        // Not `<root>/assets` + `<root>/data`: setup installs next to the exe.
        assert_eq!(r.setup_root, exe);
        let r = resolve(&Inputs {
            assets: Some(Path::new("D:/G/assets")),
            data: Some(Path::new("D:/G/data")),
            exe_dir: Some(&exe),
            ..Default::default()
        });
        assert_eq!(r.setup_root, p("D:/G"));
        let r = resolve(&Inputs {
            data: Some(Path::new("D:/X")),
            exe_dir: Some(&exe),
            ..Default::default()
        });
        assert_eq!((r.assets, r.data), (exe.join("assets"), p("D:/X")));
    }

    #[test]
    fn unknown_exe_folder_falls_back_to_the_working_folder() {
        let r = resolve(&Inputs::default());
        assert_eq!((r.assets, r.data), (p("assets"), p("data")));
        assert_eq!(r.setup_root, p(""));
    }

    #[test]
    fn missing_lists_what_is_absent() {
        let (a, d) = (Path::new("A"), Path::new("D"));
        assert!(missing(a, d, |_| true).is_empty());
        assert_eq!(missing(a, d, |_| false).len(), 3);
        let m = missing(a, d, |p| !p.ends_with("songs.json"));
        assert_eq!(m, vec!["song list (data/songs.json)".to_string()]);
    }
}
