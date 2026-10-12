//! Display names and colours of game values.

use wacca_chart::Difficulty;
use wacca_game::grade::{Grade, Lamp};
use wacca_game::save::ChartRecord;
use wacca_render::style::Rgba;

use crate::hud::format_score;

pub fn difficulty(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Normal => "NORMAL",
        Difficulty::Hard => "HARD",
        Difficulty::Expert => "EXPERT",
        Difficulty::Inferno => "INFERNO",
    }
}

// UNVERIFIED: hues from doc 05 §3.1; exact RGB to come from the UI assets.
pub fn difficulty_color(d: Difficulty) -> Rgba {
    match d {
        Difficulty::Normal => [0.20, 0.70, 1.0, 1.0],
        Difficulty::Hard => [1.0, 0.80, 0.10, 1.0],
        Difficulty::Expert => [1.0, 0.20, 0.45, 1.0],
        Difficulty::Inferno => [0.60, 0.25, 0.90, 1.0],
    }
}

/// Level as shown in lists: integer part plus `+` from .7 up (`12.7` → `12+`).
// UNVERIFIED: the "+" cutoff (doc 05 §3.1).
pub fn level(l: f32) -> String {
    let whole = l.trunc();
    let plus = if l - whole >= 0.7 - 1e-4 { "+" } else { "" };
    format!("{whole:.0}{plus}")
}

pub fn grade(g: Grade) -> &'static str {
    match g {
        Grade::D => "D",
        Grade::C => "C",
        Grade::B => "B",
        Grade::A => "A",
        Grade::AA => "AA",
        Grade::AAA => "AAA",
        Grade::S => "S",
        Grade::SPlus => "S+",
        Grade::SS => "SS",
        Grade::SsPlus => "SS+",
        Grade::SSS => "SSS",
        Grade::SssPlus => "SSS+",
        Grade::Master => "MASTER",
    }
}

pub fn lamp(l: Lamp) -> &'static str {
    match l {
        Lamp::Failed => "FAILED",
        Lamp::Clear => "CLEAR",
        Lamp::Missless => "MISSLESS",
        Lamp::FullCombo => "FULL COMBO",
        Lamp::AllMarvelous => "ALL MARVELOUS",
    }
}

/// Personal best summary, e.g. `987,654  SSS  FULL COMBO`, or `NO PLAY`.
pub fn record(r: Option<&ChartRecord>) -> String {
    match r.filter(|r| r.play_count > 0) {
        None => "NO PLAY".into(),
        Some(r) => format!(
            "{}  {}  {}",
            format_score(r.best_score),
            r.best_grade.map_or("-", grade),
            lamp(r.best_lamp)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_get_plus_from_point_seven() {
        assert_eq!(level(12.0), "12");
        assert_eq!(level(12.6), "12");
        assert_eq!(level(12.7), "12+");
        assert_eq!(level(13.9), "13+");
        assert_eq!(level(3.0), "3");
    }

    #[test]
    fn record_summary() {
        assert_eq!(record(None), "NO PLAY");
        assert_eq!(record(Some(&ChartRecord::default())), "NO PLAY");
        let r = ChartRecord {
            best_score: 987_654,
            best_grade: Some(Grade::SssPlus),
            best_lamp: Lamp::FullCombo,
            play_count: 3,
            ..ChartRecord::default()
        };
        assert_eq!(record(Some(&r)), "987,654  SSS+  FULL COMBO");
        assert_eq!(difficulty(Difficulty::Expert), "EXPERT");
    }
}
