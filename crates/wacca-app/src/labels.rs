//! Display names and colours of game values.

use wacca_chart::Difficulty;
use wacca_render::style::Rgba;

pub fn difficulty(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Normal => "NORMAL",
        Difficulty::Hard => "HARD",
        Difficulty::Expert => "EXPERT",
        Difficulty::Inferno => "INFERNO",
    }
}

/// The game's difficulty colour (`uMPC_DifficultyColor` `*FlagL`, linear → sRGB).
pub fn difficulty_color(d: Difficulty) -> Rgba {
    difficulty_flags(d)[0]
}

/// Difficulty plate colour (`*FlagL`) and level text colour (`*FontInner`) from
/// `uMPC_DifficultyColor`, in sRGB. (`*FlagR` is a slightly different shade not visible on
/// the reference frames.)
pub fn difficulty_flags(d: Difficulty) -> [Rgba; 2] {
    let c = |rgb: u32| crate::canvas::hex(rgb, 1.0);
    match d {
        Difficulty::Normal => [c(0x1b7cff), c(0xffffff)],
        Difficulty::Hard => [c(0xffc200), c(0xffffff)],
        Difficulty::Expert => [c(0xff0084), c(0xffffff)],
        Difficulty::Inferno => [c(0x400044), c(0x13142d)],
    }
}

/// Level as shown in lists: integer part plus `+` from .7 up (`12.7` → `12+`).
// UNVERIFIED: the "+" cutoff (doc 05 §3.1).
pub fn level(l: f32) -> String {
    let whole = l.trunc();
    let plus = if l - whole >= 0.7 - 1e-4 { "+" } else { "" };
    format!("{whole:.0}{plus}")
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
    fn names_and_colours() {
        assert_eq!(difficulty(Difficulty::Expert), "EXPERT");
        // `uMPC_DifficultyColor` NormalFlagL (0.011, 0.202, 1.0) linear.
        assert_eq!(
            difficulty_color(Difficulty::Normal),
            crate::canvas::hex(0x1b7cff, 1.0)
        );
    }
}
