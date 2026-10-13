//! Screen-independent UI vocabulary: what a screen wants drawn ([`View`]) and what input means
//! ([`Action`]). Screens build views from their state (pure); the shell draws them and maps
//! keys, taps and drags to actions.

use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::Align;

pub const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
pub const GREY: Rgba = [0.65, 0.65, 0.72, 1.0];
pub const PANEL: Rgba = [0.12, 0.12, 0.18, 1.0];
pub const HIGHLIGHT: Rgba = [0.25, 0.30, 0.55, 1.0];

/// User intent, from the keyboard or a tapped button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    /// Tap on list row / slot `n` of the current screen.
    Pick(usize),
    /// Move the list cursor by this many rows (drag scrolling).
    Scroll(i32),
    NextSort,
    ToggleFilter,
    Retry,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Fill {
        rect: Rect,
        color: Rgba,
    },
    /// One line; `at` = (anchor x per `align`, top).
    Text {
        text: String,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        max_width: Option<f32>,
    },
    /// Song jacket by asset name (image if available, else a coloured square).
    Jacket {
        rect: Rect,
        key: String,
    },
    /// Convex polygon; one colour per point (gradient) or a single flat colour.
    Poly {
        points: Vec<[f32; 2]>,
        colors: Vec<Rgba>,
    },
    /// Exported UI texture `data/ui/<key>.png` (e.g. `UI/Textures/FONT/uT_FTcombo00`), tinted;
    /// nothing is drawn if it is missing.
    Image {
        rect: Rect,
        key: String,
        tint: Rgba,
    },
}

/// Draw list (back to front) plus tappable areas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub items: Vec<Item>,
    pub buttons: Vec<(Rect, Action)>,
}

impl View {
    pub fn fill(&mut self, rect: Rect, color: Rgba) {
        self.items.push(Item::Fill { rect, color });
    }

    pub fn text(&mut self, text: impl Into<String>, at: [f32; 2], px: f32, color: Rgba) {
        self.text_in(text, at, px, color, Align::Left, None);
    }

    pub fn text_in(
        &mut self,
        text: impl Into<String>,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        max_width: Option<f32>,
    ) {
        self.items.push(Item::Text {
            text: text.into(),
            at,
            px,
            color,
            align,
            max_width,
        });
    }

    pub fn poly(&mut self, points: Vec<[f32; 2]>, colors: Vec<Rgba>) {
        self.items.push(Item::Poly { points, colors });
    }

    pub fn image(&mut self, key: impl Into<String>, rect: Rect, tint: Rgba) {
        self.items.push(Item::Image {
            rect,
            key: key.into(),
            tint,
        });
    }

    pub fn button(&mut self, rect: Rect, action: Action) {
        self.buttons.push((rect, action));
    }

    /// Action of the topmost (last added) button containing `p`.
    pub fn hit(&self, p: [f32; 2]) -> Option<Action> {
        self.buttons
            .iter()
            .rev()
            .find(|(r, _)| contains(*r, p))
            .map(|&(_, a)| a)
    }

    /// All text lines, in order (for tests).
    #[cfg(test)]
    pub fn texts(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

pub fn contains(r: Rect, p: [f32; 2]) -> bool {
    p[0] >= r.x && p[0] < r.x + r.w && p[1] >= r.y && p[1] < r.y + r.h
}

/// The original UI is a 1080 × 1920 portrait canvas around the round screen.
pub const CANVAS: [f32; 2] = [1080.0, 1920.0];

/// Canvas point → window pixels: the canvas scaled to the height of the 9:16 menu `area` and
/// centred in it.
pub fn canvas(area: Rect, [x, y]: [f32; 2]) -> [f32; 2] {
    let s = area.h / CANVAS[1];
    [
        area.x + area.w / 2.0 + (x - CANVAS[0] / 2.0) * s,
        area.y + y * s,
    ]
}

/// `n`-th horizontal band of height `h` starting at `top` inside `area` (full width, inset).
pub fn row(area: Rect, top: f32, h: f32, n: usize) -> Rect {
    let inset = area.w * 0.03;
    Rect {
        x: area.x + inset,
        y: top + h * n as f32,
        w: area.w - 2.0 * inset,
        h: h * 0.92,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
    };

    #[test]
    fn hit_prefers_topmost_button() {
        let mut v = View::default();
        v.button(R, Action::Confirm);
        v.button(
            Rect {
                x: 10.0,
                y: 20.0,
                w: 10.0,
                h: 10.0,
            },
            Action::Back,
        );
        assert_eq!(v.hit([15.0, 25.0]), Some(Action::Back));
        assert_eq!(v.hit([50.0, 60.0]), Some(Action::Confirm));
        assert_eq!(v.hit([110.0, 60.0]), None);
        assert_eq!(v.hit([0.0, 0.0]), None);
    }

    #[test]
    fn canvas_scales_to_height_and_centres() {
        let area = Rect {
            x: 100.0,
            y: 0.0,
            w: 540.0,
            h: 960.0,
        };
        assert_eq!(canvas(area, [540.0, 0.0]), [370.0, 0.0]);
        assert_eq!(canvas(area, [0.0, 1920.0]), [100.0, 960.0]);
        assert_eq!(canvas(area, [1080.0, 960.0]), [640.0, 480.0]);
    }

    #[test]
    fn rows_stack_downwards() {
        let a = row(R, 30.0, 10.0, 0);
        let b = row(R, 30.0, 10.0, 2);
        assert_eq!((a.y, b.y), (30.0, 50.0));
        assert!(a.x > R.x && a.w < R.w);
    }
}
