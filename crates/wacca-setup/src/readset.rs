//! The game files WACCA_slim reads, relative to the game's `Content` directory.
//!
//! Verified against the code (Oct 2026):
//! - **Runtime** (`wacca-app` reads these from `--assets`, so they are kept in `<dest>/assets`):
//!   `MusicData/<song>/*.mer` (charts, `shell.rs`/`play.rs`), `Sound/Bgm/MER_BGM.uexp` + the
//!   `Sound/Bgm/*.awb` it streams from (`BgmBank`), `Sound/Se/MER_SE.uexp` (`SeBank`, optional),
//!   `Movie/<name>.usm` (background movies, optional). `Table/MusicParameterTable.*` is kept too
//!   (3 MB) so the result can be re-detected and re-validated as an installed game.
//! - **Convert only** (`wacca-convert` turns them into `<dest>/data`; not kept): the two tables
//!   `Table/{MusicParameterTable,TouchEffectTable}.*`, `UI/Textures/**` (jackets + `--export-ui`),
//!   `UI/Font/*.ufont` (`--fonts`; the app's `UI/Font` reads are fallbacks after `data/fonts`),
//!   `Vfx/Textures/**`, `Vfx/Mesh/**` (note meshes + particle meshes), `Vfx/Particle/**` and
//!   `Vfx/Material/**` (`--effects`: hit effects and the no-movie background scene).
//! - Not read: `Sound/MerSoundDATA.acf`, `Sound/Voice`, `UI/Material`, the per-cue BGM packages,
//!   `L10N`, `Message`, maps, `Movie/*.uasset`, everything outside `Content`.

use std::path::{Path, PathBuf};

use crate::{SetupError, io_err};

/// What a file is needed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Read by the app at runtime: kept in `<dest>/assets`.
    Runtime,
    /// Only read by the converter.
    Convert,
    /// A background movie (runtime, optional).
    Movie,
}

/// One rule: a directory below `Content` and either the whole subtree (`file == None`) or the
/// files directly in it whose name matches a `*` wildcard.
#[derive(Debug, Clone, Copy)]
struct Rule {
    dir: &'static str,
    file: Option<&'static str>,
    used: Use,
}

const fn rule(dir: &'static str, file: Option<&'static str>, used: Use) -> Rule {
    Rule { dir, file, used }
}

const RULES: [Rule; 13] = [
    rule("MusicData", None, Use::Runtime),
    rule("Sound/Bgm", Some("MER_BGM.uexp"), Use::Runtime),
    rule("Sound/Bgm", Some("*.awb"), Use::Runtime),
    rule("Sound/Se", Some("MER_SE.uexp"), Use::Runtime),
    rule("Table", Some("MusicParameterTable.*"), Use::Runtime),
    rule("Table", Some("TouchEffectTable.*"), Use::Convert),
    rule("UI/Textures", None, Use::Convert),
    rule("UI/Font", Some("*.ufont"), Use::Convert),
    rule("Vfx/Textures", None, Use::Convert),
    rule("Vfx/Mesh", None, Use::Convert),
    rule("Vfx/Particle", None, Use::Convert),
    rule("Vfx/Material", None, Use::Convert),
    rule("Movie", Some("*.usm"), Use::Movie),
];

/// Path of the `Content` directory inside the archives.
pub const ARCHIVE_CONTENT: &str = r"App\WindowsNoEditor\Mercury\Content";

fn rules(include_movies: bool) -> impl Iterator<Item = &'static Rule> {
    RULES
        .iter()
        .filter(move |r| include_movies || r.used != Use::Movie)
}

/// Match against a pattern with at most one `*`, ASCII case-insensitive (Windows paths).
fn wildcard(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    match p.split_once('*') {
        None => p == n,
        Some((pre, post)) => {
            n.len() >= pre.len() + post.len() && n.starts_with(pre) && n.ends_with(post)
        }
    }
}

/// What `rel` (a file path relative to `Content`, `/` or `\` separated) is used for.
pub fn classify(rel: &str, include_movies: bool) -> Option<Use> {
    let rel = rel.replace('\\', "/").to_ascii_lowercase();
    rules(include_movies).find_map(|r| {
        let dir = r.dir.to_ascii_lowercase();
        let rest = rel.strip_prefix(&dir)?.strip_prefix('/')?;
        let ok = match r.file {
            None => !rest.is_empty(),
            Some(f) => !rest.contains('/') && wildcard(f, rest),
        };
        ok.then_some(r.used)
    })
}

/// 7-Zip `-i!` switches selecting [`classify`]'s files inside the archives.
pub fn seven_zip_includes(include_movies: bool) -> Vec<String> {
    rules(include_movies)
        .map(|r| {
            let dir = r.dir.replace('/', "\\");
            match r.file {
                None => format!(r"-i!{ARCHIVE_CONTENT}\{dir}"),
                Some(f) => format!(r"-i!{ARCHIVE_CONTENT}\{dir}\{f}"),
            }
        })
        .collect()
}

/// A file of the read set found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative to the `Content` dir.
    pub rel: PathBuf,
    pub size: u64,
    pub used: Use,
}

/// Every read-set file below `content` (only the rule directories are walked), sorted.
pub fn collect(content: &Path, include_movies: bool) -> Result<Vec<FileEntry>, SetupError> {
    let mut dirs: Vec<&str> = rules(include_movies).map(|r| r.dir).collect();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        let root = dir.split('/').fold(content.to_owned(), |p, c| p.join(c));
        if root.is_dir() {
            walk(content, &root, include_movies, &mut out)?;
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out.dedup_by(|a, b| a.rel == b.rel);
    Ok(out)
}

fn walk(
    content: &Path,
    dir: &Path,
    include_movies: bool,
    out: &mut Vec<FileEntry>,
) -> Result<(), SetupError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        let meta = entry.metadata().map_err(io_err(&path))?;
        if meta.is_dir() {
            walk(content, &path, include_movies, out)?;
            continue;
        }
        let Ok(rel) = path.strip_prefix(content) else {
            continue;
        };
        if let Some(used) = rel.to_str().and_then(|r| classify(r, include_movies)) {
            out.push(FileEntry {
                rel: rel.to_owned(),
                size: meta.len(),
                used,
            });
        }
    }
    Ok(())
}

/// True if `dir` looks like the game's `Content` directory: the song table and at least one BGM
/// `.awb` (judged by contents, so renamed or modded installs are found too).
pub fn is_content_dir(dir: &Path) -> bool {
    let table = dir.join("Table");
    let has_table = ["MusicParameterTable.uasset", "MusicParameterTable.uexp"]
        .iter()
        .all(|f| table.join(f).is_file());
    has_table
        && std::fs::read_dir(dir.join("Sound").join("Bgm")).is_ok_and(|rd| {
            rd.filter_map(Result::ok)
                .any(|e| e.file_name().to_str().is_some_and(|n| wildcard("*.awb", n)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard("*.awb", "MER_BGM_V3_07.AWB"));
        assert!(wildcard(
            "MusicParameterTable.*",
            "musicparametertable.uexp"
        ));
        assert!(wildcard("MER_BGM.uexp", "MER_BGM.uexp"));
        assert!(!wildcard("MER_BGM.uexp", "MER_BGM_S00_003.uexp"));
        assert!(!wildcard("*.awb", "awb"));
    }

    #[test]
    fn classifies_the_read_set() {
        let c = |p| classify(p, false);
        assert_eq!(c("MusicData/S03-011/S03-011_02.mer"), Some(Use::Runtime));
        assert_eq!(c(r"Sound\Bgm\MER_BGM_V3_07.awb"), Some(Use::Runtime));
        assert_eq!(c("Sound/Bgm/MER_BGM.uexp"), Some(Use::Runtime));
        assert_eq!(c("Sound/Bgm/MER_BGM_S00_003.uexp"), None);
        assert_eq!(c("Sound/Bgm/MER_BGM.uasset"), None);
        assert_eq!(c("Sound/Se/MER_SE.uexp"), Some(Use::Runtime));
        assert_eq!(c("Sound/Se/MER_SE_UI_001.uexp"), None);
        assert_eq!(c("Sound/Voice/x.awb"), None);
        assert_eq!(c("Sound/MerSoundDATA.acf"), None);
        assert_eq!(c("Table/MusicParameterTable.uexp"), Some(Use::Runtime));
        assert_eq!(c("Table/TouchEffectTable.uasset"), Some(Use::Convert));
        assert_eq!(c("Table/JudgeParameterTable.uexp"), None);
        assert_eq!(
            c("UI/Textures/JACKET/S03/uT_J_S03_30.uasset"),
            Some(Use::Convert)
        );
        assert_eq!(c("UI/TexturesX/a.uasset"), None);
        assert_eq!(c("UI/Font/SHINBI.ufont"), Some(Use::Convert));
        assert_eq!(c("UI/Material/Glitch/x.uasset"), None);
        assert_eq!(
            c("Vfx/Mesh/Notes/vMsh_DefaultNote00_01_s.uexp"),
            Some(Use::Convert)
        );
        assert_eq!(
            c("Vfx/Particle/tap_0/vo_tap000_03.uasset"),
            Some(Use::Convert)
        );
        assert_eq!(c("Movie/mv_S02_204.usm"), None);
        assert_eq!(classify("Movie/mv_S02_204.usm", true), Some(Use::Movie));
        assert_eq!(classify("Movie/mv_S02_204.uasset", true), None);
        assert_eq!(classify("Movie/SystemMovie/op.usm", true), None);
        assert_eq!(c("MusicData"), None);
    }

    #[test]
    fn includes_match_the_rules() {
        let inc = seven_zip_includes(false);
        assert_eq!(inc.len(), 12);
        assert_eq!(inc[0], r"-i!App\WindowsNoEditor\Mercury\Content\MusicData");
        assert!(inc.contains(&r"-i!App\WindowsNoEditor\Mercury\Content\Sound\Bgm\*.awb".into()));
        assert!(!inc.iter().any(|i| i.contains("Movie")));
        let with = seven_zip_includes(true);
        assert_eq!(
            with.last().map(String::as_str),
            Some(r"-i!App\WindowsNoEditor\Mercury\Content\Movie\*.usm")
        );
    }

    #[test]
    fn collects_and_detects_content_dirs() {
        let root = crate::tests::temp_dir("collect");
        let files = [
            "Table/MusicParameterTable.uasset",
            "Table/MusicParameterTable.uexp",
            "Table/Other.uexp",
            "Sound/Bgm/MER_BGM.awb",
            "Sound/Voice/v.awb",
            "MusicData/S00-001/S00-001_00.mer",
            "Movie/mv.usm",
        ];
        crate::tests::touch_all(&root, &files);
        assert!(is_content_dir(&root));
        assert!(!is_content_dir(&root.join("Table")));
        let got = collect(&root, false).unwrap();
        let rels: Vec<_> = got
            .iter()
            .map(|e| e.rel.to_str().unwrap().replace('\\', "/"))
            .collect();
        assert_eq!(
            rels,
            [
                "MusicData/S00-001/S00-001_00.mer",
                "Sound/Bgm/MER_BGM.awb",
                "Table/MusicParameterTable.uasset",
                "Table/MusicParameterTable.uexp"
            ]
        );
        assert_eq!(collect(&root, true).unwrap().len(), 5);
        std::fs::remove_dir_all(root).unwrap();
    }
}
