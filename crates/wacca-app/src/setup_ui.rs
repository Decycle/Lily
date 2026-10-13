//! Setup screens ([`crate::setup_flow`]) in the menus' round look. They show before any game data
//! exists, so they use plain shapes and the system fonts only (no exported textures).

use std::f32::consts::TAU;

use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::canvas::{CIRCLE_R, Canvas, MID, Painter, hex};
use crate::chrome::{INK, PINK, WHITE};
use crate::setup_backend::Finding;
use crate::setup_flow::{
    Flow, Reason, Severity, Step, archives, bytes, finding_text, movies_available, progress_text,
    shown,
};
use crate::ui::{Action, GREY, View};

const WARN: Rgba = hex(0xffc94a, 1.0);
const BAD: Rgba = hex(0xff5a6e, 1.0);
const GOOD: Rgba = hex(0x6ef0c8, 1.0);
const PANEL: Rgba = hex(0x3a0a66, 1.0);
/// Widest text line (design units).
const LINE_W: f32 = 760.0;

/// The setup screen: `now` = the shell clock (s), `t` = seconds on this screen.
pub fn view(c: Canvas, flow: &Flow, now: f64, t: f32) -> View {
    let mut p = Painter::new(c);
    backdrop(&mut p, t);
    p.text("LILY SETUP", [0.0, -400.0], 30.0, PINK, Align::Center);
    match &flow.step {
        Step::Welcome => welcome(&mut p, &flow.reason),
        Step::Picking => {
            lines(
                &mut p,
                -60.0,
                &[("Choose the folder in the window that opened.", WHITE)],
            );
        }
        Step::Detecting(dir) => {
            let shown = shown(dir);
            lines(&mut p, -60.0, &[("Looking at", GREY), (&shown, WHITE)]);
            dots(&mut p, 40.0, t);
            button(&mut p, 330.0, "BACK", Action::Back, false);
        }
        Step::Found {
            dir,
            detected,
            movies,
            verify,
            mismatched,
        } => {
            let version = format!(
                "Version: {}",
                detected.version.as_deref().unwrap_or("unknown")
            );
            let shown = shown(dir);
            lines(&mut p, -320.0, &[(&version, WHITE), (&shown, GREY)]);
            let report: Vec<(String, Rgba)> = detected
                .report
                .iter()
                .take(7)
                .map(|f| {
                    let (text, severity) = finding_text(f);
                    let color = match severity {
                        Severity::Info => WHITE,
                        Severity::Warning => WARN,
                        Severity::Bad => BAD,
                    };
                    (text, color)
                })
                .collect();
            let mut refs: Vec<(&str, Rgba)> =
                report.iter().map(|(s, c)| (s.as_str(), *c)).collect();
            let bad: Vec<String> = mismatched
                .iter()
                .map(|a| format!("Checksum mismatch: {a} (START continues anyway)"))
                .collect();
            refs.extend(bad.iter().map(|s| (s.as_str(), BAD)));
            small_lines(&mut p, -230.0, &refs[..refs.len().min(8)]);
            let size = detected.report.iter().find_map(|f| match f {
                Finding::Movies {
                    present: true,
                    bytes,
                } => Some(*bytes),
                _ => None,
            });
            if movies_available(detected) {
                let note = format!(
                    "{} more; playing them needs FFmpeg.",
                    bytes(size.unwrap_or(0))
                );
                toggle(&mut p, 50.0, ("BACKGROUND VIDEOS", *movies, &note), 0);
            } else {
                small_lines(
                    &mut p,
                    50.0,
                    &[(
                        "No background videos in these files: songs play without them.",
                        GREY,
                    )],
                );
            }
            if archives(detected).is_some() {
                let note = "Reads every archive once more (a few minutes).";
                toggle(&mut p, 160.0, ("CHECK ARCHIVES (MD5)", *verify, note), 1);
            }
            button(&mut p, 260.0, "START", Action::Confirm, true);
            button(&mut p, 350.0, "BACK", Action::Back, false);
        }
        Step::Running {
            progress,
            step_since,
            cancelling,
            ..
        } => {
            match progress {
                Some(pr) => {
                    let [step, pct, amount, eta] = progress_text(pr, now - step_since);
                    p.text(step, [0.0, -200.0], 18.0, WHITE, Align::Center);
                    p.text(pct, [0.0, -90.0], 48.0, PINK, Align::Center);
                    let frac = (pr.total > 0).then(|| pr.done as f32 / pr.total as f32);
                    bar(&mut p, 0.0, frac, t);
                    p.text(amount, [0.0, 70.0], 16.0, WHITE, Align::Center);
                    p.text(eta, [0.0, 120.0], 14.0, GREY, Align::Center);
                }
                None => {
                    p.text("Starting…", [0.0, -90.0], 18.0, WHITE, Align::Center);
                    bar(&mut p, 0.0, None, t);
                }
            }
            if *cancelling {
                p.text("Cancelling…", [0.0, 300.0], 16.0, WARN, Align::Center);
            } else {
                button(&mut p, 300.0, "CANCEL", Action::Back, false);
            }
        }
        Step::Done(s) => {
            p.text("SETUP COMPLETE", [0.0, -300.0], 24.0, GOOD, Align::Center);
            let songs = format!("{} songs playable", s.songs_playable);
            p.text(songs, [0.0, -220.0], 22.0, WHITE, Align::Center);
            let movies = if s.movies_present > 0 {
                format!("Background videos: {}", s.movies_present)
            } else {
                "Background videos: not included".into()
            };
            let mut out: Vec<(String, Rgba)> = vec![(movies, GREY)];
            if s.songs_skipped > 0 {
                out.push((
                    format!("{} songs left out (no playable chart)", s.songs_skipped),
                    WARN,
                ));
            }
            if !s.charts_skipped.is_empty() {
                out.push((
                    format!("Charts skipped ({}):", s.charts_skipped.len()),
                    WARN,
                ));
                for k in s.charts_skipped.iter().take(4) {
                    out.push((format!("{} ({}): {}", k.song, k.chart_path, k.reason), WARN));
                }
                if s.charts_skipped.len() > 4 {
                    out.push((format!("… and {} more", s.charts_skipped.len() - 4), WARN));
                }
            }
            for w in s.warnings.iter().take(3) {
                out.push((w.clone(), WARN));
            }
            let refs: Vec<(&str, Rgba)> = out.iter().map(|(s, c)| (s.as_str(), *c)).collect();
            small_lines(&mut p, -150.0, &refs);
            button(&mut p, 290.0, "PLAY", Action::Confirm, true);
        }
        Step::Failed { failure, .. } => {
            p.text(&failure.title, [0.0, -260.0], 24.0, BAD, Align::Center);
            let refs: Vec<(&str, Rgba)> =
                failure.lines.iter().map(|l| (l.as_str(), WHITE)).collect();
            small_lines(&mut p, -170.0, &refs);
            button(&mut p, 230.0, "TRY AGAIN", Action::Confirm, true);
            button(&mut p, 330.0, "BACK", Action::Back, false);
        }
    }
    p.finish()
}

fn welcome(p: &mut Painter, reason: &Reason) {
    lines(
        p,
        -300.0,
        &[
            ("Point me to your WACCA files:", WHITE),
            ("the update archives (.7z)", WHITE),
            ("or an existing game folder.", WHITE),
        ],
    );
    small_lines(
        p,
        -140.0,
        &[
            (
                "You need your own copy of WACCA Reverse 3.07.01: either the",
                GREY,
            ),
            (
                "Reverse update archives (3.00.00 base + 3.01.00 … 3.07.01)",
                GREY,
            ),
            ("or an installed/unpacked game folder.", GREY),
        ],
    );
    match reason {
        Reason::Missing(parts) if !parts.is_empty() => {
            let mut out = vec![("Not found yet:", WARN)];
            out.extend(parts.iter().take(3).map(|s| (s.as_str(), WARN)));
            small_lines(p, 0.0, &out);
        }
        _ => small_lines(
            p,
            0.0,
            &[(
                "Run setup again to add background videos or repair files.",
                GREY,
            )],
        ),
    }
    button(p, 230.0, "CHOOSE FOLDER", Action::Confirm, true);
    let back = match reason {
        Reason::Missing(_) => "QUIT",
        Reason::Requested => "BACK",
    };
    button(p, 330.0, back, Action::Back, false);
}

/// Dark disc on black with a slowly turning pink rim.
fn backdrop(p: &mut Painter, t: f32) {
    p.fill(
        [0.0, 0.0],
        [4.0 * CIRCLE_R, 4.0 * CIRCLE_R],
        MID,
        hex(0, 1.0),
    );
    let ring = p.c.ring();
    let at = |r: f32, a: f32| ring.pt([r * a.cos(), r * a.sin()]);
    const N: usize = 96;
    let centre = ring.pt([0.0, 0.0]);
    // A fan from the centre (convex pieces).
    for k in 0..N {
        let a = k as f32 * TAU / N as f32;
        let b = (k + 1) as f32 * TAU / N as f32;
        p.v.poly(
            vec![centre, at(CIRCLE_R, a), at(CIRCLE_R, b)],
            vec![INK, PANEL, PANEL],
        );
    }
    for k in 0..N {
        let a = k as f32 * TAU / N as f32;
        let b = (k + 1) as f32 * TAU / N as f32;
        let glow = 0.35 + 0.65 * (0.5 + 0.5 * (a - 0.4 * t).sin()).powi(3);
        let mut c = PINK;
        c[3] = glow;
        p.v.poly(
            vec![
                at(CIRCLE_R - 10.0, a),
                at(CIRCLE_R, a),
                at(CIRCLE_R, b),
                at(CIRCLE_R - 10.0, b),
            ],
            vec![c],
        );
    }
}

/// Centred lines from `y`, 40 units apart.
fn lines(p: &mut Painter, y: f32, ls: &[(&str, Rgba)]) {
    for (i, (s, c)) in ls.iter().enumerate() {
        let at = [0.0, y + 40.0 * i as f32];
        p.text_fit(*s, at, 18.0, *c, Align::Center, Some(LINE_W));
    }
}

fn small_lines(p: &mut Painter, y: f32, ls: &[(&str, Rgba)]) {
    for (i, (s, c)) in ls.iter().enumerate() {
        let at = [0.0, y + 30.0 * i as f32];
        p.text_fit(*s, at, 13.0, *c, Align::Center, Some(LINE_W));
    }
}

fn button(p: &mut Painter, y: f32, label: &str, action: Action, main: bool) {
    let size = if main { [360.0, 74.0] } else { [260.0, 60.0] };
    let (fill, text) = if main {
        (PINK, WHITE)
    } else {
        (hex(0xffffff, 0.12), WHITE)
    };
    p.fill([0.0, y], size, MID, fill);
    p.text(
        label,
        [0.0, y],
        if main { 20.0 } else { 16.0 },
        text,
        Align::Center,
    );
    p.button([0.0, y], size, action);
}

/// A yes/no choice on report row `row` (tap it to flip; keys: see the shell).
fn toggle(p: &mut Painter, y: f32, (name, on, note): (&str, bool, &str), row: usize) {
    p.fill([0.0, y], [620.0, 70.0], MID, hex(0xffffff, 0.08));
    p.text(name, [-280.0, y], 16.0, WHITE, Align::Left);
    let (label, color) = if on {
        ("◀ YES ▶", GOOD)
    } else {
        ("◀ NO ▶", GREY)
    };
    p.text(label, [280.0, y], 18.0, color, Align::Right);
    p.text(note, [0.0, y + 52.0], 11.0, GREY, Align::Center);
    p.button([0.0, y], [620.0, 70.0], Action::Pick(row));
}

/// Progress bar; `None` = unknown (a sliding block).
fn bar(p: &mut Painter, y: f32, frac: Option<f32>, t: f32) {
    let w = 640.0;
    p.fill([0.0, y], [w, 26.0], MID, hex(0xffffff, 0.12));
    match frac {
        Some(f) => {
            let f = f.clamp(0.0, 1.0);
            p.fill([-w / 2.0, y], [w * f, 26.0], [0.0, 0.5], PINK);
        }
        None => {
            let x = -w / 2.0 + (w - 120.0) * (0.5 + 0.5 * (t * 2.0).sin());
            p.fill([x, y], [120.0, 26.0], [0.0, 0.5], PINK);
        }
    }
}

/// Three pulsing dots (working).
fn dots(p: &mut Painter, y: f32, t: f32) {
    for i in 0..3 {
        let mut c = PINK;
        c[3] = 0.3 + 0.7 * (0.5 + 0.5 * (t * 4.0 - i as f32).sin());
        p.fill([-40.0 + 40.0 * i as f32, y], [18.0, 18.0], MID, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_backend::{Detected, Phase, Progress, Source, Summary};
    use crate::setup_flow::failure;

    fn c() -> Canvas {
        Canvas::fit(1080.0, 1920.0)
    }

    #[test]
    fn welcome_explains_and_offers_the_picker() {
        let f = Flow::new(Reason::Missing(vec!["charts (assets/MusicData)".into()]));
        let v = view(c(), &f, 0.0, 0.0);
        let texts = v.texts();
        assert!(texts.contains(&"CHOOSE FOLDER") && texts.contains(&"QUIT"));
        assert!(texts.contains(&"charts (assets/MusicData)"));
        assert!(!texts.iter().any(|t| t.contains("http")));
        assert_eq!(v.hit(c().pt([0.0, 230.0])), Some(Action::Confirm));
        assert_eq!(v.hit(c().pt([0.0, 330.0])), Some(Action::Back));
    }

    #[test]
    fn found_shows_report_and_choices() {
        let mut f = Flow::new(Reason::Requested);
        let movies = Finding::Movies {
            present: true,
            bytes: 21_369_353_312,
        };
        f.step = Step::Found {
            dir: "G".into(),
            detected: Detected {
                source: Source::Installed {
                    content_dir: "G".into(),
                },
                version: Some("3.07.01".into()),
                report: vec![
                    Finding::MissingUpdates(vec!["3.05.00".into()]),
                    movies.clone(),
                ],
            },
            movies: true,
            verify: false,
            mismatched: Vec::new(),
        };
        let v = view(c(), &f, 0.0, 0.0);
        let texts = v.texts();
        for s in [
            "Version: 3.07.01",
            "Missing updates: 3.05.00 (older version)",
            "◀ YES ▶",
            "21.4 GB more; playing them needs FFmpeg.",
            "START",
        ] {
            assert!(texts.contains(&s), "{s}");
        }
        assert_eq!(v.hit(c().pt([0.0, 50.0])), Some(Action::Pick(0)));
        // The checksum choice is for archives only.
        assert!(!texts.contains(&"CHECK ARCHIVES (MD5)"));
        f.step = Step::Found {
            dir: "G".into(),
            detected: Detected {
                source: Source::Archives(Vec::new()),
                version: Some("3.07.01".into()),
                report: vec![movies],
            },
            movies: false,
            verify: true,
            mismatched: vec!["WACCA Reverse (3.01.00).7z".into()],
        };
        let v = view(c(), &f, 0.0, 0.0);
        let texts = v.texts();
        assert!(texts.contains(&"CHECK ARCHIVES (MD5)"));
        assert!(
            texts.contains(
                &"Checksum mismatch: WACCA Reverse (3.01.00).7z (START continues anyway)"
            )
        );
        assert_eq!(v.hit(c().pt([0.0, 160.0])), Some(Action::Pick(1)));
    }

    #[test]
    fn running_done_and_failed() {
        let mut f = Flow::new(Reason::Requested);
        f.step = Step::Running {
            dir: "G".into(),
            progress: Some(Progress {
                phase: Phase::Copy,
                done: 50,
                total: 100,
                detail: String::new(),
            }),
            step_since: 0.0,
            cancelling: false,
            checking: None,
        };
        let v = view(c(), &f, 10.0, 0.0);
        assert!(v.texts().contains(&"50%") && v.texts().contains(&"CANCEL"));
        f.step = Step::Done(Summary {
            songs_playable: 250,
            ..Default::default()
        });
        assert!(
            view(c(), &f, 0.0, 0.0)
                .texts()
                .contains(&"250 songs playable")
        );
        f.step = Step::Failed {
            failure: failure(&crate::setup_backend::SetupError::SevenZipMissing),
            dir: None,
        };
        assert!(view(c(), &f, 0.0, 0.0).texts().contains(&"7-ZIP NOT FOUND"));
    }
}
