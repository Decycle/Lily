//! `analyze_e1 <run dir> [--ffmpeg <path>]`: experiment E1 analysis of a `run_experiment` run recorded on the
//! input-test screen. Decodes `video.mkv`, calibrates the cell grid on its first frame, reads which cells are lit in
//! every frame and compares them with `inject.jsonl` ([`wacca_realgame::e1`]). Prints a Markdown table and writes
//! `e1.json` into the run directory.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use wacca_realgame::e1::{self, Analysis, Seen};
use wacca_realgame::frame::Gray;
use wacca_realgame::inject::Record;
use wacca_realgame::record::video;
use wacca_realgame::testscreen::{self, Geometry};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("usage: analyze_e1 <run dir> [--ffmpeg <path>]")]
    Usage,
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Data(String),
    #[error(transparent)]
    Video(#[from] video::VideoError),
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.into(),
        source,
    })
}

fn num(meta: &serde_json::Value, ptr: &str) -> Result<f64, Error> {
    meta.pointer(ptr)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| Error::Data(format!("meta.json has no {ptr}")))
}

/// Decodes every frame, fits the grid on the first one, returns the geometry and the lit cells per frame.
fn read_video(
    ffmpeg: &Path,
    video_path: &Path,
    width: usize,
    height: usize,
    first_frame_ms: f64,
) -> Result<(Geometry, f64, Vec<Seen>), Error> {
    let mut child = video::spawn_decoder(ffmpeg, video_path, "gray")?;
    let stderr = child.stderr.take();
    let pts_reader = std::thread::spawn(move || {
        let mut pts = Vec::new();
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                pts.extend(video::parse_showinfo_pts(&line));
            }
        }
        pts
    });
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| Error::Data("no decoder stdout".into()))?;
    let mut buf = vec![0u8; width * height];
    let mut geo: Option<(Geometry, f64)> = None;
    let mut levels = Vec::new();
    while out.read_exact(&mut buf).is_ok() {
        let g = Gray::new(width, height, buf.clone())
            .ok_or_else(|| Error::Data("frame size".into()))?;
        let (geometry, _) =
            *geo.get_or_insert_with(|| testscreen::fit(&g, Geometry::initial(width, height)));
        levels.push(testscreen::lit_cells(&testscreen::cell_levels(
            &g, &geometry,
        )));
    }
    let _ = child.wait();
    let pts = pts_reader
        .join()
        .map_err(|_| Error::Data("stderr reader panicked".into()))?;
    if pts.len() != levels.len() {
        return Err(Error::Data(format!(
            "{} frames but {} timestamps",
            levels.len(),
            pts.len()
        )));
    }
    let (geometry, score) = geo.ok_or_else(|| Error::Data("video has no frames".into()))?;
    let seen = pts
        .iter()
        .zip(levels)
        .map(|(&p, lit)| Seen {
            t_ms: first_frame_ms + p * 1000.0,
            lit,
        })
        .collect();
    Ok((geometry, score, seen))
}

fn ms(v: Option<f64>) -> String {
    v.map_or("-".into(), |v| format!("{v:.1}"))
}

fn cells(v: &[(u8, usize)]) -> String {
    let mut lanes: Vec<u8> = v.iter().map(|c| c.0).collect();
    lanes.dedup();
    let rings: u8 = v.iter().fold(0, |m, c| m | 1 << c.1);
    if v.is_empty() {
        "-".into()
    } else {
        format!("L{lanes:?} R{rings:04b}")
    }
}

fn table(a: &Analysis) -> String {
    let mut s = String::from(
        "| t_script ms | held ms | expected | observed | det | map | on lat ms | off lat ms | visible ms | ack ms |\n|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for t in &a.taps {
        s += &format!(
            "| {:.0} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            t.t_script_ms,
            ms(t.held_ms),
            cells(&t.expected),
            cells(&t.observed),
            if t.detected { "y" } else { "n" },
            if t.mapping_ok { "ok" } else { "x" },
            ms(t.on_latency_ms),
            ms(t.off_latency_ms),
            ms(t.visible_ms),
            t.ack_ms.map_or("-".into(), |v| format!("{v:.2}")),
        );
    }
    let st = |s: Option<wacca_realgame::stats::Stats>| s.map_or("none".into(), |s| s.to_string());
    s += &format!(
        "\ndetected {}/{}, mapping ok {}/{}, stray frames {}\non latency: {}\noff latency: {}\nack: {}\n",
        a.detected,
        a.taps.len(),
        a.mapping_ok,
        a.taps.len(),
        a.stray_frames,
        st(a.on_latency),
        st(a.off_latency),
        st(a.ack)
    );
    s
}

fn run(args: &[String]) -> Result<(), Error> {
    let (dir, ffmpeg) = match args {
        [d] => (PathBuf::from(d), None),
        [d, f, p] if f == "--ffmpeg" => (PathBuf::from(d), Some(PathBuf::from(p))),
        _ => return Err(Error::Usage),
    };
    let meta: serde_json::Value = serde_json::from_str(&read(&dir.join("meta.json"))?)
        .map_err(|e| Error::Data(format!("meta.json: {e}")))?;
    let records: Vec<Record> = read(&dir.join("inject.jsonl"))?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| Error::Data(format!("inject.jsonl: {e}")))?;
    let presses = e1::presses(&records, num(&meta, "/inject/clock_start_ms")?);

    let path_var = std::env::var_os("PATH");
    let lad = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let ff = video::find_ffmpeg(ffmpeg.as_deref(), path_var.as_deref(), lad.as_deref())?;
    let (w, h) = (
        num(&meta, "/window/width")? as usize,
        num(&meta, "/window/height")? as usize,
    );
    let (geo, score, frames) = read_video(
        &ff,
        &dir.join("video.mkv"),
        w,
        h,
        num(&meta, "/video/first_frame_ms")?,
    )?;
    let analysis = e1::analyze(&presses, &frames);
    println!(
        "{} frames, grid {geo:?} (score {score:.1}), {} presses\n",
        frames.len(),
        presses.len()
    );
    print!("{}", table(&analysis));
    let out = serde_json::json!({ "geometry": geo, "grid_score": score, "frames": frames.len(), "analysis": analysis });
    let path = dir.join("e1.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&out).unwrap_or_default(),
    )
    .map_err(|source| Error::Io { path, source })
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_summary() {
        assert_eq!(cells(&[(0, 0), (0, 1), (0, 2), (0, 3)]), "L[0] R1111");
        assert_eq!(cells(&[(20, 0), (21, 0)]), "L[20, 21] R0001");
        assert_eq!(cells(&[]), "-");
    }

    #[test]
    fn usage() {
        assert!(matches!(run(&[]), Err(Error::Usage)));
        assert!(matches!(
            run(&["a".into(), "--bogus".into(), "x".into()]),
            Err(Error::Usage)
        ));
    }
}
