//! Setup from a URL: an uncompressed `.tar` of a finished setup (`assets/` + `data/`, as
//! `tools/make-pack.sh` writes it on a desktop) is streamed and unpacked on the fly into
//! `<dest>/.setup-tmp`, so the iPad never holds the archive and the files at the same time. A
//! dropped connection is resumed with an HTTP `Range` request at the byte it stopped. After
//! [`crate::validate`], `<dest>/assets` and `<dest>/data` are replaced as in [`crate::run`].

use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

use crate::{Phase, Progress, SetupError, Summary, Work, install, io_err, remove_dir, validate};

/// No data for this long: the connection is treated as dropped and reopened.
const IDLE: Duration = Duration::from_secs(30);
/// Reconnects in a row without receiving a byte before giving up.
const RETRIES: u32 = 5;
/// Bytes per chunk handed from the network thread.
const CHUNK: usize = 256 << 10;
/// Progress is reported at most this often (bytes).
const REPORT_EVERY: u64 = 1 << 20;

/// Downloads the pack at `url` and installs it into `dest` (see the module docs). `progress`
/// returning `false` cancels; nothing in `dest` changes unless the whole pack arrived and checks
/// out.
pub fn download(
    url: &str,
    dest: &Path,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Summary, SetupError> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(SetupError::Download(format!(
            "`{url}` is not a web address (it must start with http:// or https://)"
        )));
    }
    let work = Work::new(dest);
    remove_dir(&work.root)?;
    std::fs::create_dir_all(&work.root).map_err(io_err(&work.root))?;
    let result = download_in(url, dest, &work, progress);
    if let Err(e) = remove_dir(&work.root) {
        log::warn!("could not remove the temporary folder: {e}");
    }
    result
}

fn download_in(
    url: &str,
    dest: &Path,
    work: &Work,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Summary, SetupError> {
    let started = Instant::now();
    let mut stream = Resuming::open(url)?;
    if let Some(total) = stream.total
        && let Some(free) = crate::space::free_bytes(dest)
    {
        crate::space::check(dest, total, free)?;
    }
    let total = stream.total.unwrap_or(0);
    let mut cancelled = false;
    let mut last = 0;
    let reader = Counted {
        inner: &mut stream,
        on_read: &mut |done| {
            if done < last + REPORT_EVERY && done != total {
                return true;
            }
            last = done;
            let ok = progress(Progress {
                phase: Phase::Download,
                done,
                total,
                detail: String::new(),
            });
            cancelled |= !ok;
            ok
        },
        done: 0,
    };
    let unpacked = unpack(reader, &work.root);
    if cancelled {
        return Err(SetupError::Cancelled);
    }
    unpacked?;
    let download_time = started.elapsed();
    if !work.assets.is_dir() || !work.data.is_dir() {
        return Err(SetupError::Download(
            "the file is not a Lily pack: it has no assets/ and data/ folders".into(),
        ));
    }
    let step = Progress {
        phase: Phase::Validate,
        done: 0,
        total: 0,
        detail: String::new(),
    };
    if !progress(step) {
        return Err(SetupError::Cancelled);
    }
    let clock = Instant::now();
    let movies = work.assets.join("Movie").is_dir();
    let mut summary = validate::validate(&work.assets, &work.data, movies)?;
    summary.timings = vec![
        (Phase::Download, download_time),
        (Phase::Validate, clock.elapsed()),
    ];
    install(work, dest, &mut summary)?;
    Ok(summary)
}

/// Unpacks the tar stream `r` into `root`. Entries live under `assets/` or `data/`, optionally
/// inside one top-level folder (a pack made of the whole game folder); macOS metadata
/// (`._*`, `__MACOSX`) and anything else is skipped. Paths that would leave `root` are refused.
fn unpack(r: impl Read, root: &Path) -> Result<(), SetupError> {
    let mut archive = tar::Archive::new(r);
    let entries = archive.entries().map_err(read_err)?;
    for entry in entries {
        let mut entry = entry.map_err(read_err)?;
        let path = entry.path().map_err(read_err)?.into_owned();
        let Some(rel) = pack_path(&path) else {
            continue;
        };
        let kind = entry.header().entry_type();
        let target = root.join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(io_err(&target))?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            entry.unpack(&target).map_err(|e| match cancel_of(&e) {
                true => SetupError::Cancelled,
                false => io_err(&target)(e),
            })?;
        }
    }
    Ok(())
}

/// Where a tar entry goes, relative to the unpack root (`None`: skip it).
fn pack_path(path: &Path) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str()?),
            Component::CurDir => {}
            // `..`, `/`, `C:` would leave the root.
            _ => return None,
        }
    }
    if parts
        .iter()
        .any(|p| p.starts_with("._") || *p == "__MACOSX" || *p == ".DS_Store")
    {
        return None;
    }
    let start = match parts.first() {
        Some(&("assets" | "data")) => 0,
        _ if matches!(parts.get(1), Some(&("assets" | "data"))) => 1,
        _ => return None,
    };
    Some(parts[start..].iter().collect())
}

fn read_err(e: io::Error) -> SetupError {
    if cancel_of(&e) {
        return SetupError::Cancelled;
    }
    match e.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof | io::ErrorKind::Other => {
            SetupError::Download(format!("the download is not a readable .tar pack ({e})"))
        }
        _ => SetupError::Download(e.to_string()),
    }
}

/// The error [`Counted`] returns when progress asks to stop.
fn cancel_of(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted && e.to_string() == "cancelled"
}

/// Counts bytes and asks `on_read` (total so far) whether to go on.
struct Counted<'a, R> {
    inner: R,
    on_read: &'a mut dyn FnMut(u64) -> bool,
    done: u64,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.done += n as u64;
        if !(self.on_read)(self.done) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        Ok(n)
    }
}

/// The response body of `url` as one [`Read`], reopened with `Range` where it broke off. The
/// socket is read on its own thread, so a stalled connection times out after [`IDLE`].
struct Resuming {
    url: String,
    agent: ureq::Agent,
    /// Bytes delivered so far.
    offset: u64,
    /// From `Content-Length` (`None`: unknown, then a clean end is the end).
    total: Option<u64>,
    chunks: Option<Receiver<io::Result<Vec<u8>>>>,
    pending: Vec<u8>,
    pos: usize,
    failures: u32,
}

impl Resuming {
    fn open(url: &str) -> Result<Self, SetupError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .into();
        let mut s = Self {
            url: url.to_owned(),
            agent,
            offset: 0,
            total: None,
            chunks: None,
            pending: Vec::new(),
            pos: 0,
            failures: 0,
        };
        s.connect()
            .map_err(|e| SetupError::Download(e.to_string()))?;
        Ok(s)
    }

    /// (Re)opens the body at `offset`.
    fn connect(&mut self) -> io::Result<()> {
        let mut req = self.agent.get(&self.url);
        if self.offset > 0 {
            req = req.header("Range", format!("bytes={}-", self.offset));
        }
        let resp = req.call().map_err(http_err)?;
        let status = resp.status().as_u16();
        let length = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let mut body = resp.into_body().into_reader();
        if self.offset == 0 {
            self.total = length;
        } else if status != 206 {
            // The server ignored `Range`: skip what we already have.
            log::warn!(
                "download: no resume support, skipping {} bytes",
                self.offset
            );
            io::copy(&mut (&mut body).take(self.offset), &mut io::sink())?;
        }
        log::info!(
            "download: {} at byte {} (status {status}, total {:?})",
            self.url,
            self.offset,
            self.total
        );
        let (tx, rx) = sync_channel(16);
        std::thread::Builder::new()
            .name("setup-download".into())
            .spawn(move || {
                loop {
                    let mut buf = vec![0; CHUNK];
                    let r = match body.read(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            Ok(buf)
                        }
                        Err(e) => Err(e),
                    };
                    let end = !matches!(&r, Ok(b) if !b.is_empty());
                    // The reader is gone (reconnected or finished): stop.
                    if tx.send(r).is_err() || end {
                        return;
                    }
                }
            })?;
        self.chunks = Some(rx);
        Ok(())
    }

    /// Next chunk from the network (empty: end of body), reconnecting on errors.
    fn next_chunk(&mut self) -> io::Result<Vec<u8>> {
        loop {
            let got = match &self.chunks {
                Some(rx) => match rx.recv_timeout(IDLE) {
                    Ok(r) => r,
                    Err(RecvTimeoutError::Timeout) => {
                        Err(io::Error::new(io::ErrorKind::TimedOut, "no data for 30 s"))
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        Err(io::Error::other("download thread stopped"))
                    }
                },
                None => Err(io::Error::other("not connected")),
            };
            let short = |t: u64| self.offset < t;
            match got {
                Ok(b) if !b.is_empty() => {
                    self.failures = 0;
                    return Ok(b);
                }
                Ok(_) if !self.total.is_some_and(short) => return Ok(Vec::new()),
                Ok(_) => log::warn!("download: connection closed early at byte {}", self.offset),
                Err(e) => log::warn!("download: {e} at byte {}", self.offset),
            }
            self.chunks = None;
            self.failures += 1;
            if self.failures > RETRIES {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!(
                        "the connection keeps dropping (stopped at {} MB)",
                        self.offset >> 20
                    ),
                ));
            }
            std::thread::sleep(Duration::from_secs(1 << self.failures.min(4)));
            if let Err(e) = self.connect() {
                log::warn!("download: reconnect failed: {e}");
            }
        }
    }
}

impl Read for Resuming {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.pending.len() {
            self.pending = self.next_chunk()?;
            self.pos = 0;
        }
        let n = buf.len().min(self.pending.len() - self.pos);
        buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        self.offset += n as u64;
        Ok(n)
    }
}

fn http_err(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::StatusCode(404) => io::Error::new(
            io::ErrorKind::NotFound,
            "nothing at this address (HTTP 404)",
        ),
        ureq::Error::StatusCode(c) => io::Error::other(format!("the server answered HTTP {c}")),
        ureq::Error::Io(e) => e,
        e => io::Error::other(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn pack_paths_keep_assets_and_data_only() {
        let p = |s: &str| pack_path(Path::new(s));
        assert_eq!(
            p("assets/Sound/Bgm/a.awb"),
            Some(PathBuf::from("assets/Sound/Bgm/a.awb"))
        );
        assert_eq!(
            p("./data/songs.json"),
            Some(PathBuf::from("data/songs.json"))
        );
        assert_eq!(
            p("Lily/data/songs.json"),
            Some(PathBuf::from("data/songs.json"))
        );
        assert_eq!(p("Lily/lily.exe"), None);
        assert_eq!(p("data/._songs.json"), None);
        assert_eq!(p("__MACOSX/data/x"), None);
        assert_eq!(p("data/../../etc/passwd"), None);
        assert_eq!(p("/data/x"), None);
    }

    /// A tiny pack: `assets/a.bin` and `data/b.txt`.
    fn pack() -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (path, body) in [
            ("Lily/assets/a.bin", vec![7u8; 300_000]),
            ("Lily/data/b.txt", b"hello".to_vec()),
            ("Lily/._junk", b"x".to_vec()),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, path, body.as_slice()).unwrap();
        }
        b.into_inner().unwrap()
    }

    /// Serves `body` on localhost; the first response is cut off after `cut` bytes, later ones
    /// honour `Range`. Returns the URL.
    fn serve(body: Vec<u8>, cut: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/pack.tar", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for (i, conn) in listener.incoming().enumerate() {
                let mut conn = conn.unwrap();
                let mut req = [0u8; 2048];
                let n = conn.read(&mut req).unwrap();
                let req = String::from_utf8_lossy(&req[..n]).to_ascii_lowercase();
                let from = req
                    .split("range: bytes=")
                    .nth(1)
                    .and_then(|r| r.split('-').next())
                    .and_then(|v| v.parse::<usize>().ok());
                let (status, start) = match from {
                    Some(s) => ("206 Partial Content", s),
                    None => ("200 OK", 0),
                };
                let rest = &body[start..];
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    rest.len()
                );
                conn.write_all(head.as_bytes()).unwrap();
                let send = if i == 0 { &rest[..cut] } else { rest };
                let _ = conn.write_all(send);
            }
        });
        url
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wacca-dl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn unpacks_a_pack_across_a_dropped_connection() {
        let body = pack();
        let url = serve(body.clone(), 100_000);
        let mut stream = Resuming::open(&url).unwrap();
        assert_eq!(stream.total, Some(body.len() as u64));
        let root = temp("resume");
        unpack(&mut stream, &root).unwrap();
        assert_eq!(
            std::fs::read(root.join("assets/a.bin")).unwrap(),
            vec![7u8; 300_000]
        );
        assert_eq!(std::fs::read(root.join("data/b.txt")).unwrap(), b"hello");
        assert!(!root.join("._junk").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelling_stops_and_leaves_dest_alone() {
        let body = pack();
        let url = serve(body.clone(), body.len());
        let dest = temp("cancel");
        let r = download(&url, &dest, &mut |_| false);
        assert!(matches!(r, Err(SetupError::Cancelled)), "{r:?}");
        assert!(!dest.join("assets").exists() && !dest.join(".setup-tmp").exists());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn rejects_non_web_addresses() {
        let r = download("ftp://x/pack.tar", &temp("ftp"), &mut |_| true);
        assert!(matches!(r, Err(SetupError::Download(_))));
    }
}
