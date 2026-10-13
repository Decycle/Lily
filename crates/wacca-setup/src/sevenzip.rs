//! 7-Zip command line: locating it, listing archives (`l -slt`) and extracting a subset
//! (`x -aoa -i!...`) with progress parsed from `-bsp1`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::SetupError;

/// Executable names, in preference order (`7zr.exe` only handles `.7z`, which is all we need).
const NAMES: [&str; 4] = ["7z.exe", "7za.exe", "7zz.exe", "7zr.exe"];

/// Where to look for 7-Zip, in order: `WACCA_7Z`, next to the running exe (`tools\` first), the
/// standard install folders, then `PATH`.
pub fn candidates(
    env_override: Option<PathBuf>,
    exe_dir: Option<&Path>,
    program_dirs: &[PathBuf],
    path_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = env_override.into_iter().collect();
    if let Some(dir) = exe_dir {
        for sub in [dir.join("tools"), dir.to_owned()] {
            out.extend(NAMES.iter().map(|n| sub.join(n)));
        }
    }
    out.extend(program_dirs.iter().map(|d| d.join("7-Zip").join("7z.exe")));
    for d in path_dirs {
        out.extend(NAMES.iter().map(|n| d.join(n)));
    }
    out
}

/// The first existing 7-Zip executable from [`candidates`].
pub fn find() -> Result<PathBuf, SetupError> {
    let exe = std::env::current_exe().ok();
    let program_dirs: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .chain([PathBuf::from(r"C:\Program Files")])
        .collect();
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    candidates(
        std::env::var_os("WACCA_7Z").map(PathBuf::from),
        exe.as_deref().and_then(Path::parent),
        &program_dirs,
        &path_dirs,
    )
    .into_iter()
    .find(|p| p.is_file())
    .ok_or(SetupError::SevenZipMissing)
}

/// One entry of an archive listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// As stored, `\`-separated.
    pub path: String,
    pub size: u64,
    pub dir: bool,
}

/// Parses `7z l -slt` output (entries follow the `----------` line, one `Key = value` block each).
pub fn parse_listing(text: &str) -> Vec<ListEntry> {
    let mut out = Vec::new();
    let mut cur: Option<ListEntry> = None;
    let Some((_, body)) = text.split_once("\n----------") else {
        return out;
    };
    for line in body.lines().map(str::trim_end) {
        if let Some(p) = line.strip_prefix("Path = ") {
            out.extend(cur.take());
            cur = Some(ListEntry {
                path: p.to_owned(),
                size: 0,
                dir: false,
            });
        } else if let Some(e) = cur.as_mut() {
            if let Some(s) = line.strip_prefix("Size = ") {
                e.size = s.parse().unwrap_or(0);
            } else if let Some(a) = line.strip_prefix("Attributes = ") {
                e.dir = a.split_whitespace().next().is_some_and(|a| a.contains('D'));
            } else if line.starts_with("Folder = +") {
                e.dir = true;
            }
        }
    }
    out.extend(cur);
    out
}

/// The last `NN%` in a chunk of `-bsp1` output (7-Zip redraws its progress line with `\r` or
/// backspaces).
pub fn parse_percent(chunk: &str) -> Option<u8> {
    chunk
        .split(['\r', '\n', '\u{8}'])
        .filter_map(|seg| {
            let (num, _) = seg.trim_start().split_once('%')?;
            num.trim().parse::<u8>().ok().filter(|p| *p <= 100)
        })
        .next_back()
}

fn command(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null());
    c
}

fn failed(archive: &Path, message: String) -> SetupError {
    SetupError::SevenZip {
        archive: archive.to_owned(),
        message,
    }
}

/// Lists `archive` (headers only; quick).
pub fn list(exe: &Path, archive: &Path) -> Result<Vec<ListEntry>, SetupError> {
    let out = command(exe)
        .args(["l", "-slt", "-sccUTF-8", "--"])
        .arg(archive)
        .output()
        .map_err(|e| failed(archive, e.to_string()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(failed(
            archive,
            format!("listing failed ({}): {err}", out.status),
        ));
    }
    Ok(parse_listing(&String::from_utf8_lossy(&out.stdout)))
}

/// Extracts the files matching `includes` (`-i!...` switches) from `archive` into `out_dir`,
/// overwriting. `percent(p)` is called on progress and at least every 200 ms; returning `false`
/// kills 7-Zip and returns [`SetupError::Cancelled`].
pub fn extract(
    exe: &Path,
    archive: &Path,
    out_dir: &Path,
    includes: &[String],
    percent: &mut dyn FnMut(u8) -> bool,
) -> Result<(), SetupError> {
    let mut out_arg = std::ffi::OsString::from("-o");
    out_arg.push(out_dir);
    let mut child = command(exe)
        .args(["x", "-aoa", "-y", "-bso0", "-bsp1", "-bse2", "-sccUTF-8"])
        .arg(out_arg)
        .args(includes)
        .arg("--")
        .arg(archive)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(archive, e.to_string()))?;
    let (tx, rx) = mpsc::channel();
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Some(Ok(n @ 1..)) = stdout.as_mut().map(|s| s.read(&mut buf)) {
            if tx
                .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                .is_err()
            {
                break;
            }
        }
    });
    let mut stderr = child.stderr.take();
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut last = 0u8;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => last = parse_percent(&chunk).unwrap_or(last),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if !percent(last) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SetupError::Cancelled);
        }
    }
    let _ = reader.join();
    let status = child.wait().map_err(|e| failed(archive, e.to_string()))?;
    let err = err_reader.join().unwrap_or_default();
    match status.code() {
        Some(0) => Ok(()),
        // 1 = warning (e.g. some file could not be opened); the validation step catches real gaps.
        Some(1) => {
            log::warn!("7-Zip warning for {}: {}", archive.display(), err.trim());
            Ok(())
        }
        _ => Err(failed(
            archive,
            format!("extraction failed ({status}): {}", err.trim()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "7-Zip 26.04 (x64)\r\n\r\nListing archive: x.7z\r\n\r\n--\r\nPath = x.7z\r\nType = 7z\r\nPhysical Size = 99\r\n\r\n----------\r\nPath = App\r\nSize = 0\r\nAttributes = D\r\n\r\nPath = App\\WindowsNoEditor\\Mercury\\Content\\Table\\MusicParameterTable.uexp\r\nSize = 745760\r\nPacked Size = \r\nAttributes = A\r\nCRC = 1234\r\n\r\nPath = App\\WindowsNoEditor\\Mercury\\Content\\Movie\\mv.usm\r\nSize = 12\r\nAttributes = A\r\n";

    #[test]
    fn parses_slt_listing() {
        let l = parse_listing(LISTING);
        assert_eq!(l.len(), 3);
        assert_eq!(
            l[0],
            ListEntry {
                path: "App".into(),
                size: 0,
                dir: true
            }
        );
        assert_eq!(l[1].size, 745760);
        assert!(l[1].path.ends_with(r"Table\MusicParameterTable.uexp") && !l[1].dir);
        assert_eq!(l[2].size, 12);
        assert!(parse_listing("no entries").is_empty());
    }

    #[test]
    fn parses_progress_redraws() {
        assert_eq!(
            parse_percent("  0%\r 19% 27\r\n 99% 49 - App\\x.mer"),
            Some(99)
        );
        assert_eq!(
            parse_percent("\u{8}\u{8}\u{8} 42% 3 - a\u{8}\u{8}"),
            Some(42)
        );
        assert_eq!(parse_percent("Everything is Ok"), None);
        assert_eq!(parse_percent("100%"), Some(100));
    }

    #[test]
    fn candidate_order() {
        let c = candidates(
            Some("env/7z.exe".into()),
            Some(Path::new("app")),
            &["pf".into()],
            &["bin".into()],
        );
        let s: Vec<_> = c
            .iter()
            .map(|p| p.to_str().unwrap().replace('\\', "/"))
            .collect();
        assert_eq!(s[0], "env/7z.exe");
        assert_eq!(s[1], "app/tools/7z.exe");
        assert_eq!(s[4], "app/tools/7zr.exe");
        assert_eq!(s[5], "app/7z.exe");
        assert_eq!(s[9], "pf/7-Zip/7z.exe");
        assert_eq!(s[10], "bin/7z.exe");
        assert_eq!(s.len(), 14);
    }

    #[test]
    fn missing_executable_is_an_error() {
        let r = extract(
            Path::new("no-such-7z.exe"),
            Path::new("a.7z"),
            Path::new("o"),
            &[],
            &mut |_| true,
        );
        assert!(matches!(r, Err(SetupError::SevenZip { .. })));
    }
}
