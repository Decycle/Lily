//! Locating the external `ffmpeg` executable and the decode command line.
//!
//! FFmpeg's MPEG-1 decoder recognises Sofdec.Prime's 11-bit intra-DC precision (the `TMPGEXS` user data,
//! 06-visuals §1); verified by decoding a game movie to PNG and checking it by eye.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const FFMPEG_EXE: &str = if cfg!(windows) {
    "ffmpeg.exe"
} else {
    "ffmpeg"
};

/// Environment variable naming the ffmpeg executable explicitly.
pub const FFMPEG_ENV: &str = "WACCA_FFMPEG";

/// Why no ffmpeg executable was found.
#[derive(Debug, thiserror::Error)]
pub enum FindError {
    #[error(
        "ffmpeg not found (tried the explicit path / {FFMPEG_ENV}, PATH, %LOCALAPPDATA%\\Microsoft\\WinGet); install it with `winget install Gyan.FFmpeg` or name it explicitly (--ffmpeg <path> / {FFMPEG_ENV})"
    )]
    NotFound,
    #[error("ffmpeg path {} does not exist", .0.display())]
    BadPath(PathBuf),
}

/// `explicit` (must exist), else `ffmpeg` on `path_var`, else winget's `Links\ffmpeg.exe`, else the newest
/// `Packages\Gyan.FFmpeg*\*\bin\ffmpeg.exe` under `local_app_data`.
pub fn find_ffmpeg(
    explicit: Option<&Path>,
    path_var: Option<&OsStr>,
    local_app_data: Option<&Path>,
) -> Result<PathBuf, FindError> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok(p.to_path_buf())
        } else {
            Err(FindError::BadPath(p.to_path_buf()))
        };
    }
    let on_path = path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|d| d.join(FFMPEG_EXE));
    let winget = local_app_data.map(|l| l.join("Microsoft").join("WinGet"));
    let link = winget.iter().map(|w| w.join("Links").join(FFMPEG_EXE));
    let packages = winget
        .iter()
        .flat_map(|w| winget_packages(&w.join("Packages")));
    on_path
        .chain(link)
        .chain(packages)
        .find(|p| p.is_file())
        .ok_or(FindError::NotFound)
}

/// [`find_ffmpeg`] with [`FFMPEG_ENV`], `PATH` and `LOCALAPPDATA` from the environment.
pub fn find_ffmpeg_from_env() -> Result<PathBuf, FindError> {
    let explicit = std::env::var_os(FFMPEG_ENV).map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    find_ffmpeg(
        explicit.as_deref(),
        std::env::var_os("PATH").as_deref(),
        local.as_deref(),
    )
}

/// `Gyan.FFmpeg*/<build>/bin/ffmpeg.exe`, newest name first.
fn winget_packages(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = subdirs(dir)
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("Gyan.FFmpeg"))
        })
        .flat_map(|pkg| subdirs(&pkg).map(|b| b.join("bin").join(FFMPEG_EXE)))
        .collect();
    found.sort();
    found.reverse();
    found
}

fn subdirs(dir: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
}

/// Decode an MPEG-1 elementary stream from stdin to raw `yuv420p` frames on stdout, dropping everything
/// before `seek` seconds and optionally scaling to `size` × `size`.
///
/// Single-threaded decode and filtering: measured on a game movie (1080², 60 fps, scaled to 674²) it costs
/// ~31 % of one core at 6.6× real time, versus ~45 % with FFmpeg's default threading (spin/sync overhead).
pub fn decode_args(seek: f64, size: Option<u32>) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-filter_threads",
        "1",
        "-threads",
        "1",
        "-f",
        "mpegvideo",
        "-i",
        "pipe:0",
    ]
    .map(String::from)
    .to_vec();
    if seek > 0.0 {
        a.extend(["-ss".into(), format!("{seek:.6}")]);
    }
    if let Some(s) = size {
        a.extend(["-vf".into(), format!("scale={s}:{s}:flags=bilinear")]);
    }
    a.extend(["-an", "-f", "rawvideo", "-pix_fmt", "yuv420p", "pipe:1"].map(String::from));
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_for_plain_decode() {
        assert_eq!(
            decode_args(0.0, None).join(" "),
            "-hide_banner -loglevel error -filter_threads 1 -threads 1 -f mpegvideo -i pipe:0 -an -f rawvideo -pix_fmt yuv420p pipe:1"
        );
    }

    #[test]
    fn args_with_seek_and_scale() {
        let a = decode_args(12.5, Some(540)).join(" ");
        assert!(a.contains("-i pipe:0 -ss 12.500000 -vf scale=540:540:flags=bilinear -an"));
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn discovery_order() {
        let root = std::env::temp_dir().join(format!("wacca-video-ffmpeg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lad = root.join("lad");
        let pkgs = lad.join("Microsoft").join("WinGet").join("Packages");
        let old = pkgs
            .join("Gyan.FFmpeg_x/ffmpeg-8.0-full_build/bin")
            .join(FFMPEG_EXE);
        let new = pkgs
            .join("Gyan.FFmpeg_x/ffmpeg-9.0.2-full_build/bin")
            .join(FFMPEG_EXE);
        touch(&old);
        touch(&new);
        touch(&pkgs.join("Other.Tool/x/bin").join(FFMPEG_EXE));
        // Newest winget build wins.
        assert_eq!(find_ffmpeg(None, None, Some(&lad)).unwrap(), new);

        // PATH beats winget.
        let path_dir = root.join("pathdir");
        touch(&path_dir.join(FFMPEG_EXE));
        let path_var = std::env::join_paths([root.join("empty"), path_dir.clone()]).unwrap();
        assert_eq!(
            find_ffmpeg(None, Some(&path_var), Some(&lad)).unwrap(),
            path_dir.join(FFMPEG_EXE)
        );

        // An explicit path must exist and is never silently replaced.
        let explicit = root.join("custom.exe");
        assert!(matches!(
            find_ffmpeg(Some(&explicit), Some(&path_var), Some(&lad)),
            Err(FindError::BadPath(_))
        ));
        touch(&explicit);
        assert_eq!(find_ffmpeg(Some(&explicit), None, None).unwrap(), explicit);

        let none = find_ffmpeg(None, None, Some(&root.join("nothing")));
        assert!(matches!(none, Err(FindError::NotFound)));
        assert!(
            none.unwrap_err()
                .to_string()
                .contains("winget install Gyan.FFmpeg")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
