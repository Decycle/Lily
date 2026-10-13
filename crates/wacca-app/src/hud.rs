//! In-play HUD: score, combo and a fading judgement popup at the centre of the playfield.

use wacca_game::rules::{JudgeEvent, Judgement};
use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

use crate::ui::{GREY, View, WHITE};

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
    /// The playfield draws the judgement word art and the combo counter; skip their text.
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
    v.text_in(title, [x, top], 3.5 * u, GREY, Align::Left, Some(side));
    v.text(format_score(h.score), [x, top + 4.5 * u], 6.0 * u, WHITE);
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
    v
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
        assert_eq!(art.texts(), ["Song", "123,456", "x4.0  AUTO"]);
    }
}
