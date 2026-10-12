//! Results screen (`docs/research/05-save-and-song-select.md` §4).

use wacca_chart::Difficulty;
use wacca_game::grade::PlayResult;
use wacca_render::layout::Rect;
use wacca_render::text::Align;

use crate::hud::{format_score, judgement_color};
use crate::labels;
use crate::ui::{Action, GREY, HIGHLIGHT, PANEL, View, WHITE};
use wacca_game::rules::Judgement;

/// Everything the results screen shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub title: String,
    pub jacket: String,
    pub difficulty: Option<Difficulty>,
    pub level: Option<f32>,
    pub result: PlayResult,
    /// Played to the end (not given up).
    pub completed: bool,
    /// Best score before this play, if the chart was played before.
    pub previous_best: Option<u32>,
    pub new_best: bool,
    /// Why the play was not saved (autoplay, unknown song, write error).
    pub not_saved: Option<String>,
}

/// `+12,345` / `-500` against the previous best.
pub fn best_diff(score: u32, previous: Option<u32>) -> Option<String> {
    let p = previous?;
    Some(if score >= p {
        format!("+{}", format_score(score - p))
    } else {
        format!("-{}", format_score(p - score))
    })
}

pub fn view(s: &Summary, area: Rect) -> View {
    let u = area.w / 100.0;
    let mut v = View::default();
    let cx = area.x + area.w / 2.0;
    let r = &s.result;
    let j = Rect {
        x: area.x + 6.0 * u,
        y: area.y + 5.0 * u,
        w: 26.0 * u,
        h: 26.0 * u,
    };
    v.items.push(crate::ui::Item::Jacket {
        rect: j,
        key: s.jacket.clone(),
    });
    let tx = j.x + j.w + 4.0 * u;
    let tw = Some(area.x + area.w - tx - 4.0 * u);
    v.text_in(
        &s.title,
        [tx, j.y + 2.0 * u],
        5.5 * u,
        WHITE,
        Align::Left,
        tw,
    );
    if let Some(d) = s.difficulty {
        let level = s
            .level
            .map_or(String::new(), |l| format!("  {}", labels::level(l)));
        v.text(
            format!("{}{level}", labels::difficulty(d)),
            [tx, j.y + 11.0 * u],
            5.0 * u,
            labels::difficulty_color(d),
        );
    }
    if !s.completed {
        v.text("GIVEN UP", [tx, j.y + 19.0 * u], 4.0 * u, GREY);
    }

    let mut y = area.y + 38.0 * u;
    v.text_in(
        format_score(r.score),
        [cx, y],
        14.0 * u,
        WHITE,
        Align::Center,
        None,
    );
    y += 17.0 * u;
    if s.new_best {
        let diff = best_diff(r.score, s.previous_best).map_or(String::new(), |d| format!("  {d}"));
        v.text_in(
            format!("NEW RECORD{diff}"),
            [cx, y],
            5.0 * u,
            [1.0, 0.85, 0.3, 1.0],
            Align::Center,
            None,
        );
    } else if let Some(p) = s.previous_best {
        v.text_in(
            format!("BEST {}", format_score(p)),
            [cx, y],
            4.5 * u,
            GREY,
            Align::Center,
            None,
        );
    }
    y += 9.0 * u;
    v.text_in(
        format!("{}   {}", labels::grade(r.grade), labels::lamp(r.lamp)),
        [cx, y],
        7.0 * u,
        WHITE,
        Align::Center,
        None,
    );
    y += 13.0 * u;
    let c = &r.counts;
    for (j, n) in [
        (Judgement::Marvelous, c.marvelous),
        (Judgement::Great, c.great),
        (Judgement::Good, c.good),
        (Judgement::Miss, c.miss),
    ] {
        v.text(
            crate::hud::judgement_label(j),
            [area.x + 12.0 * u, y],
            5.0 * u,
            judgement_color(j),
        );
        v.text_in(
            n.to_string(),
            [area.x + 88.0 * u, y],
            5.0 * u,
            WHITE,
            Align::Right,
            None,
        );
        y += 7.0 * u;
    }
    y += 2.0 * u;
    for line in [
        format!("FAST {}   LATE {}", c.fast, c.late),
        format!("MAX COMBO {} / {}", r.max_combo, r.total_notes),
        format!("GAUGE {:.1} %", r.gauge * 100.0),
    ] {
        v.text_in(line, [cx, y], 4.5 * u, GREY, Align::Center, None);
        y += 7.0 * u;
    }
    if let Some(why) = &s.not_saved {
        v.text_in(
            why,
            [cx, y],
            4.0 * u,
            GREY,
            Align::Center,
            Some(area.w - 6.0 * u),
        );
    }

    let by = area.y + area.h - 16.0 * u;
    for (n, text, action) in [
        (0.0, "RETRY", Action::Retry),
        (1.0, "NEXT", Action::Confirm),
    ] {
        let b = Rect {
            x: area.x + 4.0 * u + n * 48.0 * u,
            y: by,
            w: 44.0 * u,
            h: 11.0 * u,
        };
        v.fill(b, if n > 0.0 { HIGHLIGHT } else { PANEL });
        v.text_in(
            text,
            [b.x + b.w / 2.0, b.y + 3.0 * u],
            5.0 * u,
            WHITE,
            Align::Center,
            None,
        );
        v.button(b, action);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_game::grade::{Grade, Lamp};
    use wacca_game::score::Counts;

    fn summary(new_best: bool, previous_best: Option<u32>) -> Summary {
        Summary {
            title: "Song".into(),
            jacket: "J".into(),
            difficulty: Some(Difficulty::Expert),
            level: Some(12.7),
            result: PlayResult {
                score: 950_000,
                grade: Grade::SS,
                lamp: Lamp::Clear,
                counts: Counts {
                    marvelous: 90,
                    great: 5,
                    good: 2,
                    miss: 3,
                    fast: 4,
                    late: 3,
                },
                max_combo: 60,
                total_notes: 100,
                gauge: 0.85,
            },
            completed: true,
            previous_best,
            new_best,
            not_saved: None,
        }
    }

    #[test]
    fn diff_against_previous_best() {
        assert_eq!(best_diff(950_000, Some(900_000)).unwrap(), "+50,000");
        assert_eq!(best_diff(900_000, Some(950_000)).unwrap(), "-50,000");
        assert_eq!(best_diff(1, None), None);
    }

    #[test]
    fn shows_score_grade_lamp_counts_and_new_record() {
        let v = view(
            &summary(true, Some(940_000)),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 900.0,
                h: 1600.0,
            },
        );
        let t = v.texts();
        for want in [
            "Song",
            "EXPERT  12+",
            "950,000",
            "NEW RECORD  +10,000",
            "SS   CLEAR",
            "MARVELOUS",
            "90",
            "FAST 4   LATE 3",
            "MAX COMBO 60 / 100",
        ] {
            assert!(t.contains(&want), "{want} missing from {t:?}");
        }
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Retry));
        let mut old = summary(false, Some(990_000));
        old.completed = false;
        old.not_saved = Some("AUTOPLAY: NOT SAVED".into());
        let v = view(
            &old,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 900.0,
                h: 1600.0,
            },
        );
        let t = v.texts();
        assert!(t.contains(&"BEST 990,000") && t.contains(&"GIVEN UP"));
        assert!(t.contains(&"AUTOPLAY: NOT SAVED"));
    }
}
