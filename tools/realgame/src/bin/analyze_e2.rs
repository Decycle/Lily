//! `analyze_e2 <run dir> [--chart <mer>] [--assets DIR] [--ffmpeg PATH] [--speed 1.5] [--video-latency-ms 0]
//!  [--compare 10] [--timing-block 20] [--ours-every 15] [--geometry cx,cy,r] [--ui data/ui] [--script taps.json] [--movie true]`: experiment E2 analysis of a song run (doc 09).
//!
//! 1. Song time: aligns `audio.wav` with the chart's BGM ([`wacca_realgame::sync`]); the match, its correlation
//!    and residuals at 25/50/75 % of the song give `bgm0_ms` and its confidence (compared with the live match).
//! 2. Every video frame gets a chart time `(first_frame_ms + pts - latency - bgm0_ms) + #OFFSET`.
//! 3. The judgement rim is fitted on a frame just before the first note (centre, inner radius). In every frame,
//!    isolated touch notes in the lower half are found along their lane's ray ([`wacca_realgame::playfield`]);
//!    the outer edge radius vs time-to-hit is fitted with the approach model (twice, the second pass matching runs
//!    against the first fit), and each note's arc centre angle gives the lane-0 angle. The same measurement runs on
//!    our own playfield rasterised on the same geometry ([`wacca_realgame::raster`]) as a check of the method.
//! 4. `compare/NN-<t>.png`: real | ours at `--compare` evenly spaced moments; `result-*.png`: frames after the song.
//!
//! Writes `e2.json` into the run directory and prints a summary.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use wacca_chart::Chart;
use wacca_realgame::frame::Rgb;
use wacca_realgame::gpu::Offscreen;
use wacca_realgame::ours;
use wacca_realgame::playfield::{self, Approach, Candidate, Geometry};
use wacca_realgame::record::{video, wav};
use wacca_realgame::{raster, reference, sync};
use wacca_render::{SceneOptions, build_scene, layout};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(
        "usage: analyze_e2 <run dir> [--chart <mer>] [--assets DIR] [--ffmpeg PATH] [--speed 1.5] [--video-latency-ms 0] [--compare 10] [--timing-block 20] [--ours-every 15] [--geometry cx,cy,r] [--ui data/ui] [--script taps.json] [--movie true]"
    )]
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
    #[error(transparent)]
    Find(#[from] wacca_video::ffmpeg::FindError),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.into(),
        source,
    }
}

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    chart: Option<PathBuf>,
    assets: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    speed: f64,
    latency_ms: f64,
    compare: usize,
    /// Notes per block of the visual timing check.
    timing_block: usize,
    ours_every: usize,
    /// Overrides of the fitted rim: centre x, centre y, judgement radius (px).
    geometry: Option<[f64; 3]>,
    /// Sprite directory for our renderer.
    ui: PathBuf,
    /// The injected script (default: `inject.script` in meta.json), fed to our engine for judgement art.
    script: Option<PathBuf>,
    /// The chart has a background movie (our guide lanes are then translucent).
    movie: bool,
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut it = args.iter();
    let mut a = Args {
        dir: PathBuf::from(it.next()?),
        chart: None,
        assets: std::env::var_os("WACCA_ASSETS").map(PathBuf::from),
        ffmpeg: None,
        speed: 1.5,
        latency_ms: 0.0,
        compare: 10,
        timing_block: 20,
        ours_every: 15,
        geometry: None,
        ui: PathBuf::from("data/ui"),
        script: None,
        movie: true,
    };
    while let Some(flag) = it.next() {
        let v = it.next()?;
        match flag.as_str() {
            "--chart" => a.chart = Some(v.into()),
            "--assets" => a.assets = Some(v.into()),
            "--ffmpeg" => a.ffmpeg = Some(v.into()),
            "--speed" => a.speed = v.parse().ok()?,
            "--video-latency-ms" => a.latency_ms = v.parse().ok()?,
            "--compare" => a.compare = v.parse().ok()?,
            "--timing-block" => a.timing_block = v.parse().ok().filter(|&n: &usize| n > 0)?,
            "--ours-every" => a.ours_every = v.parse().ok().filter(|&n| n > 0)?,
            "--ui" => a.ui = v.into(),
            "--script" => a.script = Some(v.into()),
            "--movie" => a.movie = v.parse().ok()?,
            "--geometry" => {
                let n: Vec<f64> = v
                    .split(',')
                    .map(|x| x.parse().ok())
                    .collect::<Option<_>>()?;
                a.geometry = Some(n.try_into().ok()?);
            }
            _ => return None,
        }
    }
    Some(a)
}

fn num(meta: &serde_json::Value, ptr: &str) -> Result<f64, Error> {
    meta.pointer(ptr)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| Error::Data(format!("meta.json has no {ptr}")))
}

/// Song-time alignment of the run's loopback audio.
#[derive(Debug, serde::Serialize)]
struct AudioSync {
    bgm0_frame: i64,
    bgm0_ms: f64,
    coarse: f64,
    fine: f64,
    /// (song fraction, residual ms, r) at later points.
    checks: Vec<(f64, f64, f64)>,
    live_bgm0_ms: Option<f64>,
}

fn align_audio(dir: &Path, bgm: &[f32], rate: u32, live: Option<f64>) -> Result<AudioSync, Error> {
    let wav_path = dir.join("audio.wav");
    let bytes = std::fs::read(&wav_path).map_err(io(&wav_path))?;
    let (cap_rate, ch, samples) =
        wav::read_f32(&bytes).ok_or_else(|| Error::Data("audio.wav: not a float WAV".into()))?;
    if cap_rate != rate {
        return Err(Error::Data(format!("capture {cap_rate} Hz, BGM {rate} Hz")));
    }
    let capture = sync::mono(&samples, usize::from(ch));
    let csv_path = dir.join("audio_packets.csv");
    let packets: Vec<sync::Packet> = std::fs::read_to_string(&csv_path)
        .map_err(io(&csv_path))?
        .lines()
        .skip(1)
        .filter_map(|l| {
            let (f, ms) = l.split_once(',')?;
            Some(sync::Packet {
                frame: f.parse().ok()?,
                ms: ms.parse().ok()?,
            })
        })
        .collect();
    let opts = sync::DetectorOptions {
        rate,
        ..Default::default()
    };
    let mut det =
        sync::Detector::new(bgm, opts).ok_or_else(|| Error::Data("BGM is silent".into()))?;
    let mut first = None;
    for chunk in capture.chunks(rate as usize / 2) {
        let m = det.push(chunk);
        first = first.or(m);
    }
    let m = first
        .or_else(|| det.best())
        .ok_or_else(|| Error::Data("no alignment".into()))?;
    let bgm0_ms = sync::frame_ms(&packets, rate, m.bgm0 as f64)
        .ok_or_else(|| Error::Data("audio_packets.csv is empty".into()))?;
    let checks = [0.25, 0.5, 0.75]
        .iter()
        .filter_map(|&frac| {
            let at = (bgm.len() as f64 * frac) as usize;
            let len = rate as usize / 2;
            let radius = rate as usize * 30 / 1000;
            sync::check_at(&capture, bgm, &m, at, len, radius, rate).map(|(res, r)| (frac, res, r))
        })
        .collect();
    Ok(AudioSync {
        bgm0_frame: m.bgm0,
        bgm0_ms,
        coarse: m.coarse,
        fine: m.fine,
        checks,
        live_bgm0_ms: live,
    })
}

/// Rim fit on one frame: inner edge of the rim (the longest saturated run 250..340 px out) on rays all around the
/// frame centre; rays without one (e.g. a masked half) are skipped.
fn fit_rim(f: &Rgb) -> Option<(Geometry, f64)> {
    let g0 = Geometry {
        cx: f.width as f64 / 2.0,
        cy: f.height as f64 * 0.4675,
    };
    let pts: Vec<(f64, f64)> = (0..360)
        .step_by(3)
        .filter_map(|d| {
            let deg = f64::from(d);
            let runs = playfield::ray_runs(f, &g0, deg, 250.0, 340.0, 3.0, playfield::is_rim);
            let r = runs
                .iter()
                .filter(|r| r.1 - r.0 >= 10.0)
                .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))?
                .0;
            let a = deg.to_radians();
            Some((g0.cx + r * a.cos(), g0.cy - r * a.sin()))
        })
        .collect();
    let (cx, cy, r) = playfield::fit_circle(&pts)?;
    Some((Geometry { cx, cy }, r))
}

/// One measured note: time-to-hit, the runs of note pixels along its ray and, per run, the offset (degrees) of
/// the arc centre found 3 px inside its outer edge from the expected lane-centre angle.
#[derive(Clone, Debug)]
struct Sample {
    note: usize,
    tau: f64,
    runs: Vec<(f64, f64)>,
    angle_offsets: Vec<Option<f64>>,
}

impl Sample {
    fn angle_at(&self, edge: f64) -> Option<f64> {
        let i = self.runs.iter().position(|r| r.1 == edge)?;
        self.angle_offsets.get(i).copied().flatten()
    }
}

fn measure(f: &Rgb, g: &Geometry, rim: f64, cands: &[Candidate]) -> Vec<Sample> {
    cands
        .iter()
        .map(|c| {
            let runs = playfield::ray_runs(
                f,
                g,
                c.deg,
                0.04 * rim,
                0.97 * rim,
                2.0,
                playfield::is_touch_note,
            );
            let angle_offsets = runs
                .iter()
                .map(|r| {
                    let (a0, a1) = playfield::arc_span(
                        f,
                        g,
                        r.1 - 3.0,
                        c.deg,
                        60.0,
                        0.25,
                        playfield::is_touch_note,
                    )?;
                    Some((a0 + a1) / 2.0 - c.deg)
                })
                .collect();
            Sample {
                note: c.note,
                tau: c.tau,
                runs,
                angle_offsets,
            }
        })
        .collect()
}

/// Our real renderer (headless, with sprites) fed with our engine's judgements of the injected script, placed on
/// the real frame's geometry (centre, judgement line at `rim` px).
struct Ours<'a> {
    chart: &'a Chart,
    events: Vec<wacca_game::rules::JudgeEvent>,
    gpu: Offscreen,
    opts: SceneOptions,
    size: (usize, usize),
    g: Geometry,
    rim: f64,
    fx: ours::EffectClock,
}

impl Ours<'_> {
    fn frame(&mut self, t: f64) -> Result<Rgb, Error> {
        let live = ours::live_at(self.chart, &self.events, t);
        self.fx.advance(self.chart, &self.events, t);
        let scene = build_scene(
            self.chart,
            t,
            &self.opts,
            &live.state(&self.fx.effects),
            self.gpu.sprites(),
        );
        let square = self
            .gpu
            .render(&scene)
            .map_err(|e| Error::Data(e.to_string()))?;
        Ok(raster::place(
            &square,
            f64::from(wacca_render::style::VIEW_EXTENT),
            self.size,
            (self.g.cx, self.g.cy),
            self.rim,
        ))
    }
}

/// Two-pass fit: match runs against `first` (within 6 px + 30 % of the expected radius), fit, re-match against
/// the fit (6 px + 12 %), drop > 3 RMS outliers, refit. A relative tolerance keeps far-away notes on the same ray
/// from being matched to a note that is still small near the centre.
fn fit_samples(
    samples: &[Sample],
    first: &Approach,
) -> Option<(playfield::Fit, Vec<(Sample, f64)>)> {
    let ks = playfield::grid(0.30, 0.95, 0.001);
    let pick = |m: &Approach, frac: f64| -> Vec<(Sample, f64)> {
        samples
            .iter()
            .filter_map(|s| {
                let exp = playfield::model_radius(s.tau, m)?;
                playfield::nearest_run(&s.runs, exp, 6.0 + frac * exp).map(|r| (s.clone(), r))
            })
            .collect()
    };
    let pts = |v: &[(Sample, f64)]| v.iter().map(|(s, r)| (s.tau, *r)).collect::<Vec<_>>();
    // Start from the time shift of `first` that matches the most runs closely.
    let first = playfield::grid(-0.3, 0.3, 0.01)
        .into_iter()
        .map(|delta| Approach { delta, ..*first })
        .max_by_key(|m| pick(m, 0.08).len())?;
    let pass1 = pick(&first, 0.3);
    let fit1 = playfield::fit_approach(&pts(&pass1), &ks, first.r)?;
    let pass2 = pick(&fit1.model, 0.12);
    let fit2 = playfield::fit_approach(&pts(&pass2), &ks, first.r)?;
    let kept: Vec<(Sample, f64)> = pass2
        .into_iter()
        .filter(|(s, r)| {
            playfield::model_radius(s.tau, &fit2.model)
                .is_some_and(|m| (m - r).abs() <= 3.0 * fit2.rms_px.max(1.0))
        })
        .collect();
    let fit3 = playfield::fit_approach(&pts(&kept), &ks, first.r)?;
    Some((fit3, kept))
}

/// Mean and standard deviation.
fn mean_sd(v: &[f64]) -> Option<(f64, f64)> {
    let n = v.len() as f64;
    if v.is_empty() {
        return None;
    }
    let m = v.iter().sum::<f64>() / n;
    Some((
        m,
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n).sqrt(),
    ))
}

/// Arc-centre offsets (measured - expected, degrees) of matched samples whose edge lies within `radii`.
fn angle_offsets(kept: &[(Sample, f64)], radii: std::ops::Range<f64>) -> Vec<f64> {
    kept.iter()
        .filter(|(_, r)| radii.contains(r))
        .filter_map(|(s, r)| s.angle_at(*r))
        .collect()
}

#[derive(Debug, serde::Serialize)]
struct FitOut {
    t_s: f64,
    t_at_speed1_s: f64,
    k: f64,
    rim_px: f64,
    delta_ms: f64,
    rms_px: f64,
    n: usize,
    notes: usize,
}

fn fit_out(f: &playfield::Fit, speed: f64, kept: &[(Sample, f64)]) -> FitOut {
    let mut notes: Vec<usize> = kept.iter().map(|(s, _)| s.note).collect();
    notes.sort_unstable();
    notes.dedup();
    FitOut {
        t_s: f.model.t,
        t_at_speed1_s: f.model.t * speed,
        k: f.model.k,
        rim_px: f.model.r,
        delta_ms: f.model.delta * 1000.0,
        rms_px: f.rms_px,
        n: f.n,
        notes: notes.len(),
    }
}

/// Raw samples for inspection: `[note, tau, [[run start, run end], ...]]`.
fn raw(samples: &[Sample]) -> serde_json::Value {
    samples
        .iter()
        .map(|s| serde_json::json!([s.note, s.tau, s.runs]))
        .collect()
}

fn write_png(path: &Path, img: &Rgb) -> Result<(), Error> {
    let bytes = raster::png(img).map_err(|e| Error::Data(format!("png: {e}")))?;
    std::fs::write(path, bytes).map_err(io(path))
}

#[allow(clippy::too_many_lines)]
fn run(a: &Args) -> Result<(), Error> {
    let meta_path = a.dir.join("meta.json");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).map_err(io(&meta_path))?)
            .map_err(|e| Error::Data(format!("meta.json: {e}")))?;
    let chart_path = a
        .chart
        .clone()
        .or_else(|| meta.pointer("/sync/chart")?.as_str().map(PathBuf::from))
        .ok_or_else(|| Error::Data("no --chart and no sync.chart in meta.json".into()))?;
    let chart = wacca_chart::parse(&std::fs::read(&chart_path).map_err(io(&chart_path))?)
        .map_err(|e| Error::Data(e.to_string()))?;
    let assets = a
        .assets
        .as_deref()
        .ok_or_else(|| Error::Data("--assets or WACCA_ASSETS needed".into()))?;
    let (bgm, rate) = sync::load_bgm(assets, &chart.header.music_file_path)
        .map_err(|e| Error::Data(e.to_string()))?;

    // 1. Song time.
    let live = meta
        .pointer("/sync/bgm0_ms")
        .and_then(serde_json::Value::as_f64);
    let audio = align_audio(&a.dir, &bgm, rate, live)?;
    let offset = chart.header.offset;
    let chart_t = |epoch_ms: f64| (epoch_ms - audio.bgm0_ms) / 1000.0 + offset;
    println!(
        "audio: BGM sample 0 at epoch {:.2} ms (capture frame {}), coarse r {:.3}, fine r {:.3}; live {:?}; checks {:?}",
        audio.bgm0_ms, audio.bgm0_frame, audio.coarse, audio.fine, audio.live_bgm0_ms, audio.checks
    );

    // 2.-3. Video.
    let path_var = std::env::var_os("PATH");
    let lad = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let ff = video::find_ffmpeg(a.ffmpeg.as_deref(), path_var.as_deref(), lad.as_deref())?;
    let (w, h) = (
        num(&meta, "/window/width")? as usize,
        num(&meta, "/window/height")? as usize,
    );
    let first_frame_ms = num(&meta, "/video/first_frame_ms")?;
    let first_note = chart.notes.first().map_or(0.0, |n| n.time);
    let last_note = chart.notes.last().map_or(0.0, |n| n.time);
    let compare_dir = a.dir.join("compare");
    std::fs::create_dir_all(&compare_dir).map_err(io(&compare_dir))?;
    let targets: Vec<f64> = (0..a.compare)
        .map(|i| first_note + (last_note - first_note) * (i as f64 + 0.5) / a.compare as f64)
        .collect();
    let (default_k, default_t) = (
        // The 2D edge-curve constant E2 measured (doc 09); the renderer now projects in 3D.
        0.750,
        layout::APPROACH_SECS / a.speed,
    );
    let script_path = a
        .script
        .clone()
        .or_else(|| meta.pointer("/inject/script")?.as_str().map(PathBuf::from))
        .ok_or_else(|| Error::Data("no --script and no inject.script in meta.json".into()))?;
    let script = wacca_realgame::timeline::parse(
        &std::fs::read_to_string(&script_path).map_err(io(&script_path))?,
    )
    .map_err(|e| Error::Data(format!("{}: {e}", script_path.display())))?;
    let mut our_events = Some(ours::events(
        &chart,
        &wacca_realgame::timeline::expand(&script.taps),
    ));
    let mut our_view: Option<Ours> = None;
    let lower = |d: f64| (195.0..345.0).contains(&d);

    let mut child = video::spawn_decoder(&ff, &a.dir.join("video.mkv"), "rgb24")?;
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(p) = video::parse_showinfo_pts(&line) {
                    let _ = tx.send(p);
                }
            }
        }
    });
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| Error::Data("no decoder stdout".into()))?;
    let mut buf = vec![0u8; w * h * 3];
    let mut geo: Option<(Geometry, f64)> = None;
    let mut real: Vec<Sample> = Vec::new();
    let mut ours: Vec<Sample> = Vec::new();
    let mut last: Option<(f64, Rgb)> = None;
    let mut after_song: Vec<(f64, Rgb)> = Vec::new();
    let mut next_target = 0;
    let mut frames = 0usize;
    let refs = reference::targets(&chart);
    let ref_dir = a.dir.join("reference");
    std::fs::create_dir_all(&ref_dir).map_err(io(&ref_dir))?;
    let mut ref_csv = String::from("target,label,chart_t_target,frame,pts_s,chart_t,file\n");
    let mut next_ref = 0;
    let mut prev_pts = 0.0;
    let mut frames_csv = String::from("frame,pts_s,chart_t\n");
    while out.read_exact(&mut buf).is_ok() {
        let pts = rx
            .recv()
            .map_err(|_| Error::Data(format!("no timestamp for frame {frames}")))?;
        frames += 1;
        let t = chart_t(first_frame_ms + pts * 1000.0 - a.latency_ms);
        let f = Rgb::new(w, h, buf.clone()).ok_or_else(|| Error::Data("frame size".into()))?;
        // Reference frames: the captured frame nearest each target time.
        while let Some(target) = refs.get(next_ref).filter(|r| r.t <= t) {
            let (img, fi, p, ft) = match &last {
                Some((lt, lf)) if target.t - lt < t - target.t => (lf, frames - 2, prev_pts, *lt),
                _ => (&f, frames - 1, pts, t),
            };
            let file = format!("{next_ref:04}-{}.png", target.label);
            write_png(&ref_dir.join(&file), img)?;
            ref_csv += &format!(
                "{next_ref},{},{:.4},{fi},{p:.4},{ft:.4},{file}\n",
                target.label, target.t
            );
            next_ref += 1;
        }
        prev_pts = pts;
        frames_csv += &format!("{},{pts:.4},{t:.4}\n", frames - 1);
        if geo.is_none() && t >= first_note - 0.3 {
            let (g, r) = match a.geometry {
                Some([cx, cy, r]) => (Geometry { cx, cy }, r),
                None => fit_rim(&f).ok_or_else(|| Error::Data(format!("no rim at t {t:.2}")))?,
            };
            println!(
                "rim: centre ({:.1}, {:.1}), inner radius {r:.1} px (frame at t {t:.3})",
                g.cx, g.cy
            );
            geo = Some((g, r));
            let side = (2.0 * r * f64::from(wacca_render::style::VIEW_EXTENT)).ceil() as u32;
            our_view = Some(Ours {
                chart: &chart,
                events: our_events.take().unwrap_or_default(),
                gpu: Offscreen::new(&a.ui, side).map_err(|e| Error::Data(e.to_string()))?,
                opts: SceneOptions {
                    note_speed: a.speed,
                    movie: a.movie,
                    // Real guest play shows bar lines (E2 frames).
                    bar_lines: true,
                    touch_guide: false,
                    mesh_notes: false,
                },
                size: (w, h),
                g,
                rim: r,
                fx: ours::EffectClock::default(),
            });
        }
        if let (Some((g, rim)), Some(view)) = (geo, our_view.as_mut()) {
            let first = Approach {
                t: default_t,
                k: default_k,
                r: rim,
                delta: 0.0,
            };
            let cands = playfield::candidates(&chart, t, first.t * 1.3, 0.4, 0.0, lower);
            if !cands.is_empty() && t <= last_note + 0.1 {
                real.extend(measure(&f, &g, rim, &cands));
                if frames.is_multiple_of(a.ours_every) {
                    let o = view.frame(t)?;
                    ours.extend(measure(&o, &g, rim, &cands));
                }
            }
            if next_target < targets.len() && t >= targets[next_target] {
                let o = view.frame(t)?;
                if let Some(img) = raster::side_by_side(&f, &o) {
                    write_png(
                        &compare_dir.join(format!("{next_target:02}-{t:07.3}.png")),
                        &img,
                    )?;
                }
                next_target += 1;
            }
        }
        if t > last_note + 2.0
            && after_song.len() < 6
            && after_song.last().is_none_or(|(at, _)| t - at >= 5.0)
        {
            after_song.push((t, f.clone()));
        }
        last = Some((t, f));
    }
    let _ = child.wait();
    for (path, csv) in [
        (ref_dir.join("frames.csv"), &ref_csv),
        (a.dir.join("frames.csv"), &frames_csv),
    ] {
        std::fs::write(&path, csv).map_err(io(&path))?;
    }
    println!("{next_ref} reference frames in {}", ref_dir.display());
    let (g, rim) = geo.ok_or_else(|| Error::Data("video never reached the first note".into()))?;
    println!(
        "{frames} frames decoded, {} real / {} ours note samples",
        real.len(),
        ours.len()
    );
    for (i, (t, f)) in after_song.iter().chain(last.iter()).enumerate() {
        write_png(&a.dir.join(format!("result-{i}-{t:07.2}.png")), f)?;
    }

    // Fits.
    let first = Approach {
        t: default_t,
        k: default_k,
        r: rim,
        delta: 0.0,
    };
    let (fit_real, kept_real) = fit_samples(&real, &first)
        .ok_or_else(|| Error::Data("too few real samples to fit".into()))?;
    let fit_ours = fit_samples(&ours, &first);
    let radii = 0.3 * rim..0.9 * rim;
    let ang_real = angle_offsets(&kept_real, radii.clone());
    let ang_ours = fit_ours
        .as_ref()
        .map(|(_, k)| angle_offsets(k, radii))
        .unwrap_or_default();
    let out_real = fit_out(&fit_real, a.speed, &kept_real);
    let out_ours = fit_ours.as_ref().map(|(f, k)| fit_out(f, a.speed, k));
    println!("real fit: {out_real:?}");
    println!("ours fit: {out_ours:?} (expected T {default_t:.3} s, k {default_k:.4})");
    println!(
        "lane-0 angle offset real: {:?} (n {}), ours: {:?}",
        mean_sd(&ang_real),
        ang_real.len(),
        mean_sd(&ang_ours)
    );

    // Visual timing check: the offset per block of 20 notes must stay constant (doc 09 §2: it can jump).
    let deltas = playfield::grid(-0.4, 0.4, 0.005);
    let mut timing = Vec::new();
    let size = a.timing_block;
    for block in 0..chart.notes.len().div_ceil(size) {
        let notes = block * size..(block * size + size).min(chart.notes.len());
        let samples: Vec<(f64, &[(f64, f64)])> = real
            .iter()
            .filter(|s| notes.contains(&s.note))
            .map(|s| (s.tau, s.runs.as_slice()))
            .collect();
        if let Some((d, n)) = playfield::best_delta(&samples, &first, &deltas) {
            timing.push((chart.notes[notes.start].time, d * 1000.0, n));
        }
    }
    let timing_ms: Vec<f64> = timing.iter().map(|x| x.1).collect();
    let timing_jumps: Vec<f64> = playfield::jumps(&timing_ms, 30.0)
        .into_iter()
        .map(|i| timing[i + 1].0)
        .collect();
    println!("visual timing per 20 notes (chart s, delta ms, matches): {timing:.0?}");
    if !timing_jumps.is_empty() {
        println!(
            "WARNING: visual-vs-audio offset jumps > 30 ms before chart times {timing_jumps:.1?}"
        );
    }

    let residuals: Vec<(f64, f64, f64)> = kept_real
        .iter()
        .filter_map(|(s, r)| Some((s.tau, *r, playfield::model_radius(s.tau, &fit_real.model)?)))
        .collect();
    let json = serde_json::json!({
        "chart": chart_path.display().to_string(),
        "audio": audio,
        "rim": { "cx": g.cx, "cy": g.cy, "inner_radius_px": rim },
        "speed": a.speed,
        "video_latency_ms": a.latency_ms,
        "frames": frames,
        "fit_real": out_real,
        "fit_ours": out_ours,
        "model_default": { "t_s": default_t, "k": default_k },
        "lane0_offset_deg_real": mean_sd(&ang_real),
        "lane0_offset_deg_ours": mean_sd(&ang_ours),
        "angle_samples_real": ang_real.len(),
        "visual_timing": timing,
        "timing_jumps_at": timing_jumps,
        "samples_real": residuals,
        "raw_real": raw(&real),
        "raw_ours": raw(&ours),
    });
    let path = a.dir.join("e2.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    )
    .map_err(io(&path))
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(a) = parse_args(&args) else {
        eprintln!("{}", Error::Usage);
        return ExitCode::FAILURE;
    };
    match run(&a) {
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
    fn args() {
        let a = parse_args(&[
            "run".into(),
            "--speed".into(),
            "2".into(),
            "--compare".into(),
            "4".into(),
        ])
        .unwrap();
        assert_eq!((a.speed, a.compare, a.ours_every), (2.0, 4, 15));
        assert!(parse_args(&[]).is_none());
        assert!(parse_args(&["run".into(), "--bogus".into(), "1".into()]).is_none());
        assert_eq!(mean_sd(&[1.0, 3.0]), Some((2.0, 1.0)));
        assert_eq!(mean_sd(&[]), None);
    }
}
