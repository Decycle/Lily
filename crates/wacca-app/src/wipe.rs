//! The striped wipe between the menus before the category screen: rounded purple / pink bars at
//! 60° slide along their length over the old screen (half from the top, half from the bottom),
//! the screen changes underneath, then the bars slide on and uncover the new one.
//!
//! Measured on the attract recording (`runs/reference/20261012-223942`, title → logo at 58.9 s,
//! 120 fps): the bars cover the circle in 0.35 s and leave it in 0.35 s. The real game holds the
//! cover while it loads; we have nothing to load, so there is no hold.
// UNVERIFIED: the recording only shows this wipe leaving the title; the mode → category change
// is not recorded, so it reuses the same wipe.

use crate::canvas::{Canvas, Painter, hex};
use crate::screen::Screen;
use crate::ui::View;

/// Seconds until the circle is covered.
pub const COVER: f32 = 0.35;
/// Seconds from covered until the new screen is fully visible.
pub const REVEAL: f32 = 0.35;

/// Changes between these screens are wiped (the later screens have their own transitions).
pub fn applies(from: Screen, to: Screen) -> bool {
    let early = |s| matches!(s, Screen::Title | Screen::ModeSelect);
    from != to && (early(from) || early(to))
}

/// The old screen is still shown `t` seconds into the wipe.
pub fn covering(t: f32) -> bool {
    t < COVER
}

pub fn done(t: f32) -> bool {
    t >= COVER + REVEAL
}

/// One bar: offset across the bars (design units), width, colour, entry side, start delay as a
/// fraction of the phase.
struct Bar {
    across: f32,
    width: f32,
    color: u32,
    from_top: bool,
    delay: f32,
}

const PURPLE: u32 = 0x7800a0;
const PINK: u32 = 0xe0006e;
const VIOLET: u32 = 0x3c0080;

const fn bar(across: f32, width: f32, color: u32, from_top: bool, delay: f32) -> Bar {
    Bar {
        across,
        width,
        color,
        from_top,
        delay,
    }
}

/// Colours and widths sampled from the covered frame (59.4 s). The purple base bars overlap so
/// the circle is always fully covered at the end of [`COVER`].
const BARS: [Bar; 14] = [
    bar(-640.0, 260.0, PURPLE, false, 0.30),
    bar(-420.0, 260.0, PURPLE, true, 0.10),
    bar(-200.0, 260.0, PURPLE, false, 0.20),
    bar(20.0, 260.0, PURPLE, true, 0.25),
    bar(240.0, 260.0, PURPLE, false, 0.0),
    bar(460.0, 260.0, PURPLE, true, 0.15),
    bar(660.0, 220.0, PURPLE, false, 0.05),
    bar(-480.0, 90.0, VIOLET, true, 0.05),
    bar(-330.0, 24.0, PINK, false, 0.45),
    bar(-60.0, 200.0, PINK, true, 0.40),
    bar(130.0, 150.0, PURPLE, false, 0.35),
    bar(330.0, 220.0, PINK, false, 0.30),
    bar(480.0, 40.0, VIOLET, true, 0.45),
    bar(560.0, 120.0, PINK, true, 0.40),
];

/// Half the length of the strip the bars travel along (covers the circle at any offset).
const REACH: f32 = 800.0;

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Extent `[tail, head]` along the bar's own direction (positive = travel direction) at `t`.
fn extent(b: &Bar, t: f32) -> [f32; 2] {
    let progress = |phase: f32, len: f32| ease_out((phase / len - b.delay) / (1.0 - b.delay));
    let head = -REACH + 2.0 * REACH * progress(t, COVER);
    let tail = if covering(t) {
        -3.0 * REACH
    } else {
        -REACH + 2.0 * REACH * progress(t - COVER, REVEAL)
    };
    [tail, head.max(tail)]
}

/// The wipe overlay `t` seconds after it started, masked to the circle.
pub fn view(c: Canvas, t: f32) -> View {
    let mut p = Painter::new(c);
    // Bars run lower-left → upper-right (60°); `across` is perpendicular to them.
    let (s, k) = (60f32.to_radians().sin(), 60f32.to_radians().cos());
    let along = [k, -s];
    let normal = [s, k];
    for b in &BARS {
        let [tail, head] = extent(b, t);
        if head - tail < 1.0 {
            continue;
        }
        let dir = if b.from_top { -1.0 } else { 1.0 };
        let at = |u: f32, v: f32| {
            let u = u * dir;
            c.pt([u * along[0] + v * normal[0], u * along[1] + v * normal[1]])
        };
        let half = b.width / 2.0;
        let (v0, v1) = (b.across - half, b.across + half);
        // Inset by the cap radius so the round cap ends the bar.
        let cap = head - half;
        let color = hex(b.color, 1.0);
        if cap > tail {
            let points = vec![at(tail, v0), at(cap, v0), at(cap, v1), at(tail, v1)];
            p.v.poly(points, vec![color]);
        }
        let u = cap.max(tail) * dir;
        let centre = [
            u * along[0] + b.across * normal[0],
            u * along[1] + b.across * normal[1],
        ];
        p.tex("uT_CMen64_00", centre, [b.width, b.width], color);
    }
    p.circle_mask();
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_early_menus_are_wiped() {
        assert!(applies(Screen::Title, Screen::ModeSelect));
        assert!(applies(Screen::ModeSelect, Screen::Category));
        assert!(applies(Screen::Category, Screen::ModeSelect));
        assert!(!applies(Screen::Category, Screen::SongSelect));
        assert!(!applies(Screen::Title, Screen::Title));
    }

    #[test]
    fn bars_cover_then_leave() {
        // Nothing yet at the start; every bar reaches across the circle when covered.
        assert!(BARS.iter().all(|b| extent(b, 0.0)[1] <= -REACH + 1.0));
        for b in &BARS {
            let [tail, head] = extent(b, COVER - 1e-4);
            assert!(tail < -REACH && head > REACH - 1.0, "{}", b.across);
            // Gone at the end.
            let [tail, head] = extent(b, COVER + REVEAL);
            assert!(tail >= REACH - 1.0 && head >= tail);
        }
        assert!(covering(0.3) && !covering(0.36));
        assert!(!done(0.6) && done(0.7));
    }

    #[test]
    fn base_bars_overlap_across_the_circle() {
        let mut purple: Vec<_> = BARS
            .iter()
            .filter(|b| b.color == PURPLE && b.width >= 220.0)
            .map(|b| (b.across - b.width / 2.0, b.across + b.width / 2.0))
            .collect();
        purple.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(purple[0].0 <= -crate::canvas::CIRCLE_R);
        assert!(purple.last().unwrap().1 >= crate::canvas::CIRCLE_R);
        assert!(purple.windows(2).all(|w| w[1].0 <= w[0].1));
    }
}
