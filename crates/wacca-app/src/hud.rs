//! In-play HUD: score, combo and a fading judgement popup at the centre of the playfield, plus a
//! pause button in the window's top-right corner and, while paused, RESUME / RETRY / QUIT. The
//! arcade has no pause (only automatic GIVE UP settings); this is for touch screens without Esc.

use wacca_game::rules::{JudgeEvent, Judgement};
use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::ui::{Action, GREY, View, WHITE};

/// How long the judgement popup stays visible (chart seconds).
pub const POPUP_SECS: f64 = 0.4;

/// `1234567` → `1,234,567`.
pub fn format_score(score: u32) -> String {
    let digits = score.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn judgement_label(j: Judgement) -> &'static str {
    match j {
        Judgement::Marvelous => "MARVELOUS",
        Judgement::Great => "GREAT",
        Judgement::Good => "GOOD",
        Judgement::Miss => "MISS",
    }
}

// UNVERIFIED: placeholder colours until the judgement sprites are extracted (M6).
pub fn judgement_color(j: Judgement) -> Rgba {
    match j {
        Judgement::Marvelous => [1.0, 0.85, 0.30, 1.0],
        Judgement::Great => [1.0, 0.40, 0.75, 1.0],
        Judgement::Good => [0.35, 0.85, 1.0, 1.0],
        Judgement::Miss => [0.60, 0.60, 0.65, 1.0],
    }
}

/// Popup text and colour (alpha faded) for the last judgement at chart time `now`.
pub fn popup(last: Option<JudgeEvent>, now: f64) -> Option<(&'static str, Rgba)> {
    let e = last?;
    let age = now - e.time;
    if !(0.0..POPUP_SECS).contains(&age) {
        return None;
    }
    let mut c = judgement_color(e.judgement);
    c[3] = (1.0 - age / POPUP_SECS) as f32;
    Some((judgement_label(e.judgement), c))
}

/// Live play state shown by the HUD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hud {
    pub score: u32,
    pub combo: u32,
    pub last: Option<JudgeEvent>,
    pub now: f64,
    pub note_speed: f64,
    pub paused: bool,
    pub autoplay: bool,
    /// The playfield draws the judgement word art, the combo counter and the rim's score and
    /// title; skip their text.
    pub art: bool,
}

/// HUD for a `window` with the square `playfield` inside it: title and score in the top-left
/// corner (the side margin on a landscape screen, above the field in portrait), combo and the
/// judgement popup at the centre, status in the bottom-left corner.
pub fn view(h: &Hud, title: &str, window: Rect, playfield: Rect) -> View {
    let u = playfield.w.min(playfield.h) / 100.0;
    let mut v = View::default();
    let (x, top) = (window.x + 2.0 * u, window.y + 2.0 * u);
    let margin = playfield.x - window.x;
    let side = if margin > 20.0 * u { margin } else { window.w } - 4.0 * u;
    // With the art the playfield writes title and score along its rim, like the game.
    if !h.art {
        v.text_in(title, [x, top], 3.5 * u, GREY, Align::Left, Some(side));
        v.text(format_score(h.score), [x, top + 4.5 * u], 6.0 * u, WHITE);
    }
    let cx = playfield.x + playfield.w / 2.0;
    let mid = playfield.y + playfield.h / 2.0;
    // Fallback text where the sprites are missing, placed like the art (combo ~0.19 R above
    // the centre, judgement ~0.2 R below; R ≈ 43 u).
    if h.combo > 0 && !h.art {
        v.text_in(
            h.combo.to_string(),
            [cx, mid - 10.0 * u],
            4.5 * u,
            WHITE,
            Align::Center,
            None,
        );
    }
    if let Some((label, color)) = popup(h.last, h.now).filter(|_| !h.art) {
        v.text_in(
            label,
            [cx, mid + 7.0 * u],
            4.0 * u,
            color,
            Align::Center,
            None,
        );
    }
    let mut status = format!("x{:.1}", h.note_speed);
    if h.autoplay {
        status.push_str("  AUTO");
    }
    if h.paused {
        status.push_str("  PAUSED");
    }
    let bottom = window.y + window.h - 5.0 * u;
    v.text(status, [x, bottom], 3.0 * u, GREY);
    if h.paused {
        pause_menu(&mut v, window, u);
    } else {
        pause_button(&mut v, window, u);
    }
    v
}

const PANEL: Rgba = [1.0, 1.0, 1.0, 0.10];
const PINK: Rgba = [1.0, 0.31, 0.72, 1.0];

/// Side of the pause button (playfield units of 1/100).
const PAUSE_SIDE: f32 = 9.0;

/// "II" in the window's top-right corner, outside the round screen. [`Action::Back`] pauses.
fn pause_button(v: &mut View, window: Rect, u: f32) {
    let side = PAUSE_SIDE * u;
    let r = Rect {
        x: window.x + window.w - side - 1.5 * u,
        y: window.y + 1.5 * u,
        w: side,
        h: side,
    };
    v.fill(r, PANEL);
    let (bar_w, bar_h) = (1.3 * u, 4.4 * u);
    let top = r.y + (side - bar_h) / 2.0;
    for dx in [-1.4 * u, 1.4 * u] {
        let cx = r.x + side / 2.0 + dx;
        v.fill(
            Rect {
                x: cx - bar_w / 2.0,
                y: top,
                w: bar_w,
                h: bar_h,
            },
            [1.0, 1.0, 1.0, 0.7],
        );
    }
    v.button(r, Action::Back);
}

/// Dimmed screen with RESUME ([`Action::Confirm`]), RETRY ([`Action::Retry`]) and QUIT
/// ([`Action::Back`]) stacked in the middle.
fn pause_menu(v: &mut View, window: Rect, u: f32) {
    v.fill(window, [0.0, 0.0, 0.0, 0.6]);
    let cx = window.x + window.w / 2.0;
    let mid = window.y + window.h / 2.0;
    v.text_in(
        "PAUSED",
        [cx, mid - 30.0 * u],
        5.0 * u,
        WHITE,
        Align::Center,
        None,
    );
    let (w, h, gap) = (36.0 * u, 10.0 * u, 3.0 * u);
    for (i, (label, action, color)) in [
        ("RESUME", Action::Confirm, PINK),
        ("RETRY", Action::Retry, PANEL),
        ("QUIT", Action::Back, PANEL),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect {
            x: cx - w / 2.0,
            y: mid - 18.0 * u + i as f32 * (h + gap),
            w,
            h,
        };
        v.fill(r, color);
        let px = 4.0 * u;
        v.text_in(
            label,
            [cx, r.y + (h - px) / 2.0],
            px,
            WHITE,
            Align::Center,
            None,
        );
        v.button(r, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::Bonus;
    use wacca_game::rules::JudgeClass;

    fn event(j: Judgement, time: f64) -> JudgeEvent {
        JudgeEvent {
            note: 0,
            class: JudgeClass::Touch,
            bonus: Bonus::None,
            judgement: j,
            offset: None,
            time,
        }
    }

    #[test]
    fn scores_get_thousands_separators() {
        assert_eq!(format_score(0), "0");
        assert_eq!(format_score(999), "999");
        assert_eq!(format_score(1000), "1,000");
        assert_eq!(format_score(1_000_000), "1,000,000");
        assert_eq!(format_score(987_654), "987,654");
    }

    #[test]
    fn popup_fades_and_expires() {
        let e = Some(event(Judgement::Great, 10.0));
        let (label, c) = popup(e, 10.0).unwrap();
        assert_eq!((label, c[3]), ("GREAT", 1.0));
        assert!((popup(e, 10.2).unwrap().1[3] - 0.5).abs() < 1e-6);
        assert!(popup(e, 10.0 + POPUP_SECS).is_none());
        assert!(popup(e, 9.9).is_none());
        assert!(popup(None, 10.0).is_none());
    }

    #[test]
    fn hud_shows_score_combo_and_popup() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 90.0,
            h: 160.0,
        };
        let h = Hud {
            score: 123_456,
            combo: 12,
            last: Some(event(Judgement::Marvelous, 1.0)),
            now: 1.1,
            note_speed: 4.0,
            paused: false,
            autoplay: true,
            art: false,
        };
        let v = view(&h, "Song", r, r);
        assert_eq!(
            v.texts(),
            ["Song", "123,456", "12", "MARVELOUS", "x4.0  AUTO"]
        );
        let idle = view(
            &Hud {
                combo: 0,
                now: 5.0,
                ..h
            },
            "Song",
            r,
            r,
        );
        assert_eq!(idle.texts(), ["Song", "123,456", "x4.0  AUTO"]);
        let art = view(&Hud { art: true, ..h }, "Song", r, r);
        assert_eq!(art.texts(), ["x4.0  AUTO"]);
    }

    #[test]
    fn pause_button_then_pause_menu() {
        // Landscape window with the square playfield in the middle.
        let window = Rect {
            x: 0.0,
            y: 0.0,
            w: 1600.0,
            h: 1000.0,
        };
        let field = Rect {
            x: 300.0,
            y: 0.0,
            w: 1000.0,
            h: 1000.0,
        };
        let h = Hud {
            score: 0,
            combo: 0,
            last: None,
            now: 0.0,
            note_speed: 3.0,
            paused: false,
            autoplay: false,
            art: true,
        };
        let v = view(&h, "Song", window, field);
        // Top-right corner: pause. The playfield itself has no buttons.
        assert_eq!(v.hit([1550.0, 60.0]), Some(Action::Back));
        assert_eq!(v.hit([800.0, 500.0]), None);
        assert_eq!(v.hit([800.0, 950.0]), None);
        let p = view(&Hud { paused: true, ..h }, "Song", window, field);
        let texts = p.texts();
        for t in ["PAUSED", "RESUME", "RETRY", "QUIT"] {
            assert!(texts.contains(&t), "{t}");
        }
        // Stacked from 18 u above the middle, 10 u tall with 3 u gaps (u = 10 px).
        assert_eq!(p.hit([800.0, 370.0]), Some(Action::Confirm));
        assert_eq!(p.hit([800.0, 500.0]), Some(Action::Retry));
        assert_eq!(p.hit([800.0, 630.0]), Some(Action::Back));
        assert_eq!(p.hit([1550.0, 60.0]), None);
    }
}
