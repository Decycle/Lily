//! Reference frames for the visual-parity audit (doc 09 §6): which chart times of a recorded song to export.
//!
//! - every [`EVERY_S`] seconds from just before the first note to after the last;
//! - around the first two hits of every note kind (touch / snap in / snap out / slide cw / slide ccw / hold / chain,
//!   each as plain, bonus and R): [`HIT_OFFSETS_S`] relative to the note time, which covers the approach, the hit
//!   effect and the judgement popup appearing and fading; holds also mid-body and around their end;
//! - just after the 10th, 50th, 100th, 200th... note (combo counter values) and the last note;
//! - the song start and end.

use wacca_chart::{Bonus, Chart, Note, NoteBody, SlideDir, SnapDir};

pub const EVERY_S: f64 = 2.0;
pub const HIT_OFFSETS_S: [f64; 10] = [-0.2, -0.1, -0.05, 0.0, 0.05, 0.1, 0.2, 0.35, 0.5, 0.8];
/// Combo counter frames: this long after the note that reaches the value.
const COMBO_DELAY_S: f64 = 0.15;

/// A chart time to export and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub t: f64,
    pub label: String,
}

/// Note kind label, e.g. `touch`, `snap-in-r`, `slide-cw-bonus`, `hold`.
pub fn kind_label(n: &Note) -> String {
    let body = match &n.body {
        NoteBody::Touch => "touch",
        NoteBody::Chain => "chain",
        NoteBody::Hold { .. } => "hold",
        NoteBody::Snap(SnapDir::In) => "snap-in",
        NoteBody::Snap(SnapDir::Out) => "snap-out",
        NoteBody::Slide(SlideDir::Cw) => "slide-cw",
        NoteBody::Slide(SlideDir::Ccw) => "slide-ccw",
    };
    match n.bonus {
        Bonus::None => body.to_string(),
        Bonus::Bonus => format!("{body}-bonus"),
        Bonus::R => format!("{body}-r"),
    }
}

/// All export targets of `chart`, sorted by time.
pub fn targets(chart: &Chart) -> Vec<Target> {
    let mut out = Vec::new();
    let mut push = |t: f64, label: String| out.push(Target { t, label });
    let (Some(first), Some(last)) = (chart.notes.first(), chart.notes.last()) else {
        return Vec::new();
    };
    let end = last.time.max(chart.end_time.min(last.time + 10.0));
    let mut t = (first.time - 3.0).max(0.0);
    while t <= end + 3.0 {
        push(t, format!("every-{t:07.3}"));
        t += EVERY_S;
    }
    push(0.0, "song-start".into());
    push(last.time + 1.0, "song-end+1s".into());
    push(last.time + 4.0, "song-end+4s".into());
    let mut seen: Vec<(String, usize)> = Vec::new();
    for (i, n) in chart.notes.iter().enumerate() {
        let kind = kind_label(n);
        let count = match seen.iter_mut().find(|(k, _)| *k == kind) {
            Some(e) => {
                e.1 += 1;
                e.1
            }
            None => {
                seen.push((kind.clone(), 1));
                1
            }
        };
        if count <= 2 {
            for dt in HIT_OFFSETS_S {
                push(n.time + dt, format!("{kind}{count}-note{i}{dt:+.2}"));
            }
            if let NoteBody::Hold { nodes } = &n.body
                && let Some(e) = nodes.last()
            {
                push(
                    (n.time + e.time) / 2.0,
                    format!("{kind}{count}-note{i}-body"),
                );
                for dt in [-0.1, 0.0, 0.1, 0.3] {
                    push(e.time + dt, format!("{kind}{count}-note{i}-end{dt:+.2}"));
                }
            }
        }
        let combo = i + 1;
        if combo == 10 || combo % 50 == 0 || combo == chart.notes.len() {
            push(n.time + COMBO_DELAY_S, format!("combo{combo}"));
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_regular_kind_combo_and_end_frames() {
        let chart = wacca_chart::parse(
            b"#BODY\n   0    0    2 120.000000\n   0    0    3    4\n\
              2    0    1    1    0   40    4    1\n\
              2  480    1    2    1   30    4    1\n\
              2  960    1    3    2   30    4    1\n\
              3    0    1    1    3   10    4    1\n\
              3  480    1    1    4   10    4    1\n\
              3  960    1    1    5   10    4    1\n\
              9    0    1   14    6    0   60    1\n",
        )
        .unwrap();
        let t = targets(&chart);
        assert!(t.windows(2).all(|w| w[0].t <= w[1].t));
        let labels: Vec<&str> = t.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"touch1-note0+0.00"));
        assert!(labels.contains(&"touch2-note3-0.20"));
        assert!(!labels.iter().any(|l| l.starts_with("touch3")));
        assert!(labels.contains(&"touch-bonus1-note1+0.80"));
        assert!(labels.contains(&"snap-in1-note2+0.05"));
        assert!(labels.contains(&"combo6"));
        assert!(labels.contains(&"song-start") && labels.contains(&"song-end+4s"));
        // Regular frames start 3 s before the first note (4.0 s) and are 2 s apart.
        assert_eq!(
            t.iter().find(|x| x.label.starts_with("every")).unwrap().t,
            1.0
        );
    }
}
