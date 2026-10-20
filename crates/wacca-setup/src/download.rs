//! Setup from a URL: an uncompressed `.tar` of a finished setup (`assets/` + `data/`, as
//! `tools/make-pack.sh` writes it on a desktop) is streamed and unpacked on the fly into
//! `<dest>/.setup-tmp`, so the iPad never holds the archive and the files at the same time. A
//! dropped connection is resumed with an HTTP `Range` request at the byte it stopped. Without
//! movies (iPad, Android: no player there yet) `assets/Movie/` is skipped, and big skips jump over
//! the bytes with a new `Range` request instead of downloading them. After [`crate::validate`],
//! `<dest>/assets` and `<dest>/data` are replaced as in [`crate::run`].

use std::io::{self, Read, Seek, SeekFrom};
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
/// Skips up to this size are read and dropped; longer ones reconnect further on.
const SKIP_BY_READING: u64 = 8 << 20;

/// Downloads the pack at `url` and installs it into `dest` (see the module docs);
/// `include_movies`: also keep `assets/Movie/`. `progress` returning `false` cancels; nothing in
/// `dest` changes unless the whole pack arrived and checks out.
pub fn download(
    url: &str,
    dest: &Path,
    include_movies: bool,
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
    log::debug!("download: connecting to {url}");
    let result = download_in(url, dest, &work, include_movies, progress);
    if let Err(e) = remove_dir(&work.root) {
        log::warn!("could not remove the temporary folder: {e}");
    }
    result
}

fn download_in(
    url: &str,
    dest: &Path,
    work: &Work,
    include_movies: bool,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Summary, SetupError> {
    let started = Instant::now();
    let mut stream = Resuming::open(url)?;
    // A web page or the game's own archives are common mistakes: say so instead of a tar error.
    let head = stream.peek(512).map_err(read_err)?.to_vec();
    if let Some(why) = not_a_pack(&head) {
        return Err(SetupError::Download(why.into()));
    }
    // Everything is kept: the whole file must fit. Without movies the size is only known as the
    // files arrive; running out of space is reported then.
    if include_movies
        && let Some(total) = stream.total
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
    let unpacked = unpack(reader, &work.root, include_movies);
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
/// (`._*`, `__MACOSX`), `assets/Movie/` without `include_movies`, and anything else is skipped
/// (seeking past it). Paths that would leave `root` are refused.
fn unpack(r: impl Read + Seek, root: &Path, include_movies: bool) -> Result<(), SetupError> {
    let mut archive = tar::Archive::new(r);
    let entries = archive.entries_with_seek().map_err(read_err)?;
    for entry in entries {
        let mut entry = entry.map_err(read_err)?;
        let path = entry.path().map_err(read_err)?.into_owned();
        let Some(rel) = pack_path(&path) else {
            continue;
        };
        if !include_movies && rel.starts_with("assets/Movie") {
            continue;
        }
        let kind = entry.header().entry_type();
        let target = root.join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(io_err(&target))?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            entry.unpack(&target).map_err(|e| {
                if cancel_of(&e) {
                    SetupError::Cancelled
                } else if e.kind() == io::ErrorKind::StorageFull {
                    SetupError::Download(
                        "this device ran out of storage space: free some and try again".into(),
                    )
                } else {
                    io_err(&target)(e)
                }
            })?;
        }
    }
    Ok(())
}

/// Why the first bytes of a download show it is not a Lily pack (`None`: looks like a tar, or
/// unknown, which the tar reader then judges).
fn not_a_pack(head: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(&head[..head.len().min(256)]).to_ascii_lowercase();
    let trimmed = text.trim_start();
    if head.is_empty() {
        Some("the address gave an empty file")
    } else if trimmed.starts_with('<') || text.contains("<html") || text.contains("<!doctype") {
        Some(
            "the address is a web page, not a Lily pack file (a lily-pack.tar made on a computer \
             with tools/make-pack.sh)",
        )
    } else if head.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(
            "this is a 7-Zip archive (like the game's own WACCA Reverse .7z files), not a Lily \
             pack: run Lily's setup on a computer and make a pack with tools/make-pack.sh",
        )
    } else if head.starts_with(b"PK\x03\x04") {
        Some("this is a .zip file, not a Lily pack (an uncompressed .tar from tools/make-pack.sh)")
    } else if head.starts_with(b"\x1F\x8B") {
        Some(
            "this is a compressed (.gz) file: Lily packs are plain .tar files from tools/make-pack.sh",
        )
    } else {
        None
    }
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

impl<R: Seek> Seek for Counted<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.done = self.inner.seek(pos)?;
        if !(self.on_read)(self.done) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        Ok(self.done)
    }
}

/// The response body of `url` as one [`Read`], reopened with `Range` where it broke off. The
/// socket is read on its own thread, so a stalled connection times out after [`IDLE`].
struct Resuming {
    url: String,
    agent: ureq::Agent,
    /// The read position: bytes handed out or skipped.
    offset: u64,
    /// Bytes received from the network so far (`offset` plus what is buffered); a reconnect
    /// asks for the body from here.
    received: u64,
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
            received: 0,
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

    /// (Re)opens the body at `received`.
    fn connect(&mut self) -> io::Result<()> {
        let mut req = self.agent.get(&self.url);
        if self.received > 0 {
            req = req.header("Range", format!("bytes={}-", self.received));
        }
        let resp = req.call().map_err(http_err)?;
        let status = resp.status().as_u16();
        let length = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let mut body = resp.into_body().into_reader();
        if self.received == 0 {
            self.total = length;
        } else if status != 206 {
            // The server ignored `Range`: skip what we already have.
            log::warn!(
                "download: no resume support, skipping {} bytes",
                self.received
            );
            io::copy(&mut (&mut body).take(self.received), &mut io::sink())?;
        }
        log::info!(
            "download: {} at byte {} (status {status}, total {:?})",
            self.url,
            self.received,
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
            let short = |t: u64| self.received < t;
            match got {
                Ok(b) if !b.is_empty() => {
                    self.failures = 0;
                    self.received += b.len() as u64;
                    return Ok(b);
                }
                Ok(_) if !self.total.is_some_and(short) => return Ok(Vec::new()),
                Ok(_) => log::warn!(
                    "download: connection closed early at byte {}",
                    self.received
                ),
                Err(e) => log::warn!("download: {e} at byte {}", self.received),
            }
            self.chunks = None;
            self.failures += 1;
            if self.failures > RETRIES {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    format!(
                        "the connection keeps dropping (stopped at {} MB)",
                        self.received >> 20
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

impl Resuming {
    /// Up to `n` bytes ahead of the read position, without consuming them (fewer at the end).
    fn peek(&mut self, n: usize) -> io::Result<&[u8]> {
        while self.pending.len() - self.pos < n {
            let chunk = self.next_chunk()?;
            if chunk.is_empty() {
                break;
            }
            self.pending.drain(..self.pos);
            self.pos = 0;
            self.pending.extend_from_slice(&chunk);
        }
        let end = self.pending.len().min(self.pos + n);
        Ok(&self.pending[self.pos..end])
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

/// Forward only (all a tar reader needs to skip entries).
impl Seek for Resuming {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.offset.checked_add_signed(d),
            SeekFrom::End(_) => None,
        }
        .filter(|&t| t >= self.offset)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "can only skip forward"))?;
        let buffered = (self.pending.len() - self.pos) as u64;
        if target - self.offset <= buffered {
            self.pos += (target - self.offset) as usize;
            self.offset = target;
            return Ok(target);
        }
        self.offset += buffered;
        self.pending.clear();
        self.pos = 0;
        let rest = target - self.offset;
        if rest <= SKIP_BY_READING {
            io::copy(&mut self.by_ref().take(rest), &mut io::sink())?;
            if self.offset < target {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        } else {
            log::info!("download: skipping {} MB", rest >> 20);
            (self.offset, self.received) = (target, target);
            self.chunks = None;
            // A failed reconnect is retried by the next read, like a dropped connection.
            if let Err(e) = self.connect() {
                log::warn!("download: reconnect after skip failed: {e}");
            }
        }
        Ok(target)
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
        serve_counted(body, cut).0
    }

    /// [`serve`], plus the number of body bytes sent so far.
    fn serve_counted(
        body: Vec<u8>,
        cut: usize,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let sent = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&sent);
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
                // Small writes, so a dropped connection really stops the transfer.
                for piece in send.chunks(64 << 10) {
                    if conn.write_all(piece).is_err() {
                        break;
                    }
                    counter.fetch_add(piece.len(), Ordering::Relaxed);
                }
            }
        });
        (url, sent)
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
        unpack(&mut stream, &root, true).unwrap();
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
        let r = download(&url, &dest, true, &mut |_| false);
        assert!(matches!(r, Err(SetupError::Cancelled)), "{r:?}");
        assert!(!dest.join("assets").exists() && !dest.join(".setup-tmp").exists());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn common_wrong_files_are_named() {
        assert!(
            not_a_pack(b"<!DOCTYPE html><html>")
                .unwrap()
                .contains("web page")
        );
        assert!(
            not_a_pack(b"  \n<html lang=en>")
                .unwrap()
                .contains("web page")
        );
        assert!(
            not_a_pack(b"7z\xBC\xAF\x27\x1C\x00\x04")
                .unwrap()
                .contains("7-Zip")
        );
        assert!(not_a_pack(b"PK\x03\x04rest").unwrap().contains(".zip"));
        assert!(not_a_pack(b"").is_some());
        assert_eq!(not_a_pack(&pack()[..512]), None);
    }

    #[test]
    fn a_web_page_is_reported_as_one() {
        let url = serve(b"<!DOCTYPE html><title>x</title>".to_vec(), 31);
        let r = download(&url, &temp("html"), true, &mut |_| true);
        assert!(
            matches!(&r, Err(SetupError::Download(m)) if m.contains("web page")),
            "{r:?}"
        );
    }

    #[test]
    fn without_movies_the_movie_bytes_are_skipped_not_downloaded() {
        use std::sync::atomic::Ordering;
        let mut b = tar::Builder::new(Vec::new());
        for (path, len) in [
            ("assets/a.bin", 1000),
            ("assets/Movie/big.usm", 20 << 20),
            ("data/b.txt", 5),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(len as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, path, vec![1u8; len].as_slice())
                .unwrap();
        }
        let body = b.into_inner().unwrap();
        let (url, sent) = serve_counted(body.clone(), body.len());
        let mut stream = Resuming::open(&url).unwrap();
        let root = temp("skip");
        unpack(&mut stream, &root, false).unwrap();
        assert!(root.join("assets/a.bin").is_file() && root.join("data/b.txt").is_file());
        assert!(!root.join("assets/Movie").exists());
        drop(stream);
        // The first connection is dropped at the skip; at most a few MB of the 20 MB went out.
        std::thread::sleep(Duration::from_millis(200));
        let sent = sent.load(Ordering::Relaxed);
        assert!(sent < 12 << 20, "{sent} bytes sent");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_non_web_addresses() {
        let r = download("ftp://x/pack.tar", &temp("ftp"), true, &mut |_| true);
        assert!(matches!(r, Err(SetupError::Download(_))));
    }
}
