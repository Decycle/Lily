//! Screen-independent UI vocabulary: what a screen wants drawn ([`View`]) and what input means
//! ([`Action`]). Screens build views from their state (pure); the shell draws them and maps
//! keys, taps and drags to actions.

use wacca_render::layout::Rect;
use wacca_render::style::Rgba;
use wacca_render::text::{Align, Face, UiMaterial};

pub const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];
pub const GREY: Rgba = [0.65, 0.65, 0.72, 1.0];

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
    /// The DIFFICULTY button: next existing difficulty, wrapping.
    Difficulty,
    /// Jump to the previous / next genre folder.
    Genre(i32),
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
        face: Face,
    },
    /// One line turned by `angle` radians (clockwise) about `at` (anchor x per `align`, top).
    TurnedText {
        text: String,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        angle: f32,
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
    /// Exported game texture `data/ui/<key>.png` (e.g. `UI/Textures/uT_STmenu_obj008`): region
    /// `uv` (ends swapped = mirrored) times `tint`, turned by `angle` radians (clockwise) about
    /// the centre of `rect`. Missing textures draw nothing.
    Image {
        key: String,
        rect: Rect,
        uv: [f32; 4],
        angle: f32,
        tint: Rgba,
    },
    /// Part of an exported UI texture on a convex polygon: one texture coordinate per point.
    ImagePoly {
        key: String,
        points: Vec<[f32; 2]>,
        uvs: Vec<[f32; 2]>,
        tint: Rgba,
    },
    /// A ported UI material ([`UiMaterial`]) on `rect` (turned like [`Item::Image`]) with its
    /// exported `textures` (keys as for images), float4 `params` and a linear `tint`.
    Material {
        material: UiMaterial,
        rect: Rect,
        uv: [f32; 4],
        angle: f32,
        textures: Vec<String>,
        params: Vec<[f32; 4]>,
        tint: Rgba,
    },
}

/// Draw list (back to front) plus tappable areas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// Window rectangle of the animated `uM_WCbg000` backdrop, drawn under `items`.
    pub backdrop: Option<Rect>,
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
            face: Face::Ui,
        });
    }

    /// One line in the game's numeral face (counters, scores).
    pub fn numerals(
        &mut self,
        text: impl Into<String>,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
    ) {
        self.items.push(Item::Text {
            text: text.into(),
            at,
            px,
            color,
            align,
            max_width: None,
            face: Face::Numerals,
        });
    }

    /// One line in `face`.
    #[allow(clippy::too_many_arguments)]
    pub fn text_face(
        &mut self,
        text: impl Into<String>,
        at: [f32; 2],
        px: f32,
        color: Rgba,
        align: Align,
        face: Face,
    ) {
        self.items.push(Item::Text {
            text: text.into(),
            at,
            px,
            color,
            align,
            max_width: None,
            face,
        });
    }

    pub fn poly(&mut self, points: Vec<[f32; 2]>, colors: Vec<Rgba>) {
        self.items.push(Item::Poly { points, colors });
    }

    pub fn image(&mut self, key: impl Into<String>, rect: Rect, tint: Rgba) {
        self.items.push(Item::Image {
            key: key.into(),
            rect,
            uv: [0.0, 0.0, 1.0, 1.0],
            angle: 0.0,
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
                Item::Text { text, .. } | Item::TurnedText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

pub fn contains(r: Rect, p: [f32; 2]) -> bool {
    p[0] >= r.x && p[0] < r.x + r.w && p[1] >= r.y && p[1] < r.y + r.h
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
}
