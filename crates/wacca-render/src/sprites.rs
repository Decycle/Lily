//! Playfield sprites from the original textures (`data/ui/<asset path>.png`, written by
//! `wacca-convert --export-ui Vfx/Textures --export-ui UI/Textures/FONT`). Spec: 06 §3.
//!
//! Most Vfx textures are channel-packed masks for UE materials we do not run; each sprite names
//! its source, the rows it uses and how the channels are baked into the atlas convention of
//! [`Paint`](crate::layout::Paint): plain premultiplied images, or masks with `r` = white
//! highlight, `g` = shade of the vertex colour, `a` = coverage. Baking and packing are pure;
//! [`load_sources`] is the only IO. Missing files are logged and the scene falls back to flat
//! colours for that sprite.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::layout::{ATLAS_SIZE, UvRect, WHITE_BLOCK};
use crate::text::Shelves;

/// Judgement word art (`uT_FTdecision01..04`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JudgeSprite {
    Marvelous,
    Great,
    Good,
    Miss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite {
    /// Bevelled note face (mask), arc along u, outer edge at v = 0.
    NoteFace,
    /// Chain-note diagonal stripes (alpha).
    ChainStripes,
    /// Bonus-note sparkle star (alpha).
    Sparkle,
    /// Slide chevron (mask), pointing towards +v.
    SlideArrow,
    /// Snap double chevron (mask), pointing towards −u.
    SnapArrow,
    /// Guide-lane gradient (alpha), bright at v = 0.
    GuideLane,
    /// Soft round glow (alpha).
    Glow,
    Judge(JudgeSprite),
    /// "COMBO" word art.
    Combo,
    /// Hit spark triangle (`vT_HitMarvPt01`).
    Spark,
    /// Combo digit `0..=9` rendered from the SHINBI font (see [`bake_digit`]).
    Digit(u8),
    /// Text along the rim, rendered from the UI font (see [`bake_text`]).
    Text(TextSprite),
}

/// The rim texts of the play screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextSprite {
    /// The fixed "SCORE" label.
    ScoreLabel,
    /// Difficulty and level, e.g. `NORMAL/Lv.1`.
    Difficulty,
    Title,
}

impl Sprite {
    /// Sprites read from exported textures (digits come from a font).
    pub const ALL: [Sprite; 13] = [
        Sprite::NoteFace,
        Sprite::ChainStripes,
        Sprite::Sparkle,
        Sprite::SlideArrow,
        Sprite::SnapArrow,
        Sprite::GuideLane,
        Sprite::Glow,
        Sprite::Judge(JudgeSprite::Marvelous),
        Sprite::Judge(JudgeSprite::Great),
        Sprite::Judge(JudgeSprite::Good),
        Sprite::Judge(JudgeSprite::Miss),
        Sprite::Combo,
        Sprite::Spark,
    ];
}

/// Colour channel index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ch {
    R = 0,
    G = 1,
    B = 2,
    A = 3,
}

/// How a source texture becomes an atlas sprite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bake {
    /// Colour image with straight alpha.
    Image,
    /// White, alpha from one channel.
    Alpha(Ch),
    /// Mask: highlight = `hi` × `hi_gain`, shade = `shade_lo + (1 − shade_lo) × shade`,
    /// coverage from `alpha` (opaque if `None`).
    Mask {
        hi: Ch,
        hi_gain: f32,
        shade: Ch,
        shade_lo: f32,
        alpha: Option<Ch>,
    },
}

/// Where a sprite comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    /// PNG path relative to the `ui` export directory, without extension.
    pub file: &'static str,
    /// Rows used, as fractions of the height.
    pub rows: (f32, f32),
    pub bake: Bake,
    /// Halve the resolution this many times (big masks shown small; there are no mipmaps).
    pub shrink: u32,
}

const ALL_ROWS: (f32, f32) = (0.0, 1.0);

/// Texture source of `s` (`None` for font-rendered sprites).
pub fn source(s: Sprite) -> Option<Source> {
    let src = |file, bake| Source {
        file,
        rows: ALL_ROWS,
        bake,
        shrink: 0,
    };
    Some(match s {
        // UNVERIFIED: which atlas rows the note meshes map is a guess from the channel layout
        // (rows 0.40..0.58 hold a bevelled rectangle: R = rim highlight, G = face shading).
        Sprite::NoteFace => Source {
            rows: (0.40, 0.58),
            shrink: 2,
            ..src(
                "Vfx/Textures/vT_NoteBase00_00_m",
                Bake::Mask {
                    hi: Ch::R,
                    hi_gain: 0.6,
                    shade: Ch::G,
                    shade_lo: 0.55,
                    alpha: None,
                },
            )
        },
        // UNVERIFIED: the alpha channel's diagonal stripes are taken as the chain pattern
        // (material switch `ChainNote`).
        Sprite::ChainStripes => Source {
            rows: (0.60, 1.0),
            shrink: 2,
            ..src("Vfx/Textures/vT_NoteBase00_00_m", Bake::Alpha(Ch::A))
        },
        Sprite::Sparkle => src("Vfx/Textures/vT_BNotePat01_00_m", Bake::Alpha(Ch::B)),
        Sprite::SlideArrow => src(
            "Vfx/Textures/vT_SlideNote00_00_m",
            Bake::Mask {
                hi: Ch::G,
                hi_gain: 0.8,
                shade: Ch::R,
                shade_lo: 0.5,
                alpha: Some(Ch::B),
            },
        ),
        Sprite::SnapArrow => src(
            "Vfx/Textures/vT_SnapNote00_00_m",
            Bake::Mask {
                hi: Ch::R,
                hi_gain: 0.5,
                shade: Ch::G,
                shade_lo: 0.6,
                alpha: Some(Ch::B),
            },
        ),
        Sprite::GuideLane => src("Vfx/Textures/vT_GuideLane01_00_m", Bake::Alpha(Ch::R)),
        Sprite::Glow => src("Vfx/Textures/vT_Glow00_00_m", Bake::Alpha(Ch::R)),
        Sprite::Judge(j) => src(
            match j {
                JudgeSprite::Marvelous => "UI/Textures/FONT/uT_FTdecision01",
                JudgeSprite::Great => "UI/Textures/FONT/uT_FTdecision02",
                JudgeSprite::Good => "UI/Textures/FONT/uT_FTdecision03",
                JudgeSprite::Miss => "UI/Textures/FONT/uT_FTdecision04",
            },
            Bake::Image,
        ),
        // The white variant matches the attract-demo capture at low combos.
        Sprite::Combo => src("UI/Textures/FONT/uT_FTcombo01", Bake::Image),
        Sprite::Spark => src("Vfx/Textures/vT_HitMarvPt01_00_m", Bake::Image),
        Sprite::Digit(_) | Sprite::Text(_) => return None,
    })
}

/// RGBA8 image.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

impl Image {
    fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.size[0] + x) * 4) as usize;
        let mut p = [0; 4];
        p.copy_from_slice(&self.rgba[i..i + 4]);
        p
    }

    fn from_fn(size: [u32; 2], f: impl Fn(u32, u32) -> [u8; 4]) -> Self {
        let mut rgba = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for y in 0..size[1] {
            for x in 0..size[0] {
                rgba.extend_from_slice(&f(x, y));
            }
        }
        Self { size, rgba }
    }

    fn rows(&self, (a, b): (f32, f32)) -> Self {
        let h = self.size[1] as f32;
        let y0 = (a * h).round() as u32;
        let y1 = ((b * h).round() as u32).clamp(y0 + 1, self.size[1]);
        Self::from_fn([self.size[0], y1 - y0], |x, y| self.px(x, y + y0))
    }

    /// 2×2 box filter.
    fn half(&self) -> Self {
        let [w, h] = [(self.size[0] / 2).max(1), (self.size[1] / 2).max(1)];
        Self::from_fn([w, h], |x, y| {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = self.px(
                    (2 * x + dx).min(self.size[0] - 1),
                    (2 * y + dy).min(self.size[1] - 1),
                );
                for c in 0..4 {
                    sum[c] += u32::from(p[c]);
                }
            }
            sum.map(|s| (s / 4) as u8)
        })
    }
}

/// Bake `img` per `src` into a premultiplied atlas sprite.
pub fn bake(img: &Image, src: &Source) -> Image {
    let mut img = img.rows(src.rows);
    for _ in 0..src.shrink {
        img = img.half();
    }
    let unit = |v: u8| f32::from(v) / 255.0;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Image::from_fn(img.size, |x, y| {
        let p = img.px(x, y);
        let (rgb, a) = match src.bake {
            Bake::Image => ([unit(p[0]), unit(p[1]), unit(p[2])], unit(p[3])),
            Bake::Alpha(c) => ([1.0; 3], unit(p[c as usize])),
            Bake::Mask {
                hi,
                hi_gain,
                shade,
                shade_lo,
                alpha,
            } => {
                let s = shade_lo + (1.0 - shade_lo) * unit(p[shade as usize]);
                let a = alpha.map_or(1.0, |c| unit(p[c as usize]));
                ([unit(p[hi as usize]) * hi_gain, s, 0.0], a)
            }
        };
        [
            byte(rgb[0] * a),
            byte(rgb[1] * a),
            byte(rgb[2] * a),
            byte(a),
        ]
    })
}

/// Combo digits are rasterised at this font size (px).
pub const DIGIT_PX: f32 = 96.0;
/// Padding around a digit cell's cap height, as a fraction of the cap height (room for the
/// outline). The scene subtracts it again when placing cells.
pub const DIGIT_PAD: f32 = 0.12;
/// Dark outline baked around the digits so they read over bright movies (straight RGB).
const DIGIT_OUTLINE: [f32; 3] = [0.12, 0.06, 0.22];

/// Digit `d` of `font` as a premultiplied cell: one advance wide and the cap height of `0`
/// tall (all digits), plus [`DIGIT_PAD`] on every side; a white glyph inside a dark outline.
pub fn bake_digit(font: &impl ab_glyph::Font, d: u8) -> Option<Image> {
    use ab_glyph::{PxScale, ScaleFont};
    let scaled = font.as_scaled(PxScale::from(DIGIT_PX));
    let outline = |c: char| scaled.outline_glyph(scaled.scaled_glyph(c));
    // Vertical extent shared by all ten digits (overshoots included).
    let (top, bottom) = ('0'..='9')
        .filter_map(|c| outline(c).map(|g| g.px_bounds()))
        .fold((f32::MAX, f32::MIN), |(t, b), r| {
            (t.min(r.min.y), b.max(r.max.y))
        });
    let cap = bottom - top;
    let pad = (cap * DIGIT_PAD).ceil();
    let c = char::from(b'0' + d.min(9));
    let advance = scaled.h_advance(scaled.glyph_id(c));
    let (w, h) = (
        (advance + 2.0 * pad).ceil() as u32,
        (cap + 2.0 * pad).ceil() as u32,
    );
    let mut cov = vec![0.0f32; (w * h) as usize];
    let g = outline(c)?;
    let b = g.px_bounds();
    // The shared top lands at `pad`.
    let (ox, oy) = (pad + b.min.x, pad + b.min.y - top);
    g.draw(|x, y, v| {
        let (px, py) = (ox as i64 + i64::from(x), oy as i64 + i64::from(y));
        if (0..i64::from(w)).contains(&px) && (0..i64::from(h)).contains(&py) {
            cov[(py as u32 * w + px as u32) as usize] = v;
        }
    });
    let r = (pad * 0.7).max(1.0) as i64;
    let at = |x: i64, y: i64| {
        let inside = (0..i64::from(w)).contains(&x) && (0..i64::from(h)).contains(&y);
        if inside {
            cov[(y as u32 * w + x as u32) as usize]
        } else {
            0.0
        }
    };
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Image::from_fn([w, h], |x, y| {
        let (x, y) = (i64::from(x), i64::from(y));
        let mut ring = 0.0f32;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    ring = ring.max(at(x + dx, y + dy));
                }
            }
        }
        let fill = at(x, y);
        let edge = ring * (1.0 - fill) * 0.85;
        let rgb = DIGIT_OUTLINE.map(|o| byte(fill + edge * o));
        [rgb[0], rgb[1], rgb[2], byte(fill + edge)]
    }))
}

/// Rim texts are rasterised at this font size (px).
pub const TEXT_PX: f32 = 64.0;

/// `text` in `font` as a premultiplied white sprite one line box (ascent to descent) tall;
/// `None` for empty text.
pub fn bake_text(font: &impl ab_glyph::Font, text: &str) -> Option<Image> {
    use ab_glyph::{PxScale, ScaleFont};
    let scaled = font.as_scaled(PxScale::from(TEXT_PX));
    let mut x = 0.0f32;
    let mut glyphs = Vec::new();
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(TEXT_PX, ab_glyph::point(x, scaled.ascent())));
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    let (w, h) = (
        x.ceil() as u32 + 2,
        (scaled.ascent() - scaled.descent()).ceil() as u32,
    );
    if glyphs.is_empty() || h == 0 {
        return None;
    }
    let mut cov = vec![0.0f32; (w * h) as usize];
    for g in glyphs.into_iter().filter_map(|g| scaled.outline_glyph(g)) {
        let b = g.px_bounds();
        g.draw(|gx, gy, v| {
            let (px, py) = (
                b.min.x as i64 + i64::from(gx) + 1,
                b.min.y as i64 + i64::from(gy),
            );
            if (0..i64::from(w)).contains(&px) && (0..i64::from(h)).contains(&py) {
                let c = &mut cov[(py as u32 * w + px as u32) as usize];
                *c = (*c + v).min(1.0);
            }
        });
    }
    Some(Image::from_fn([w, h], |x, y| {
        let a = (cov[(y * w + x) as usize] * 255.0).round() as u8;
        [a; 4]
    }))
}

/// The font at `path` (first face of a collection); `None` (logged) if missing or unreadable.
pub fn load_font(path: &Path) -> Option<ab_glyph::FontVec> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::warn!("font {}: {e}", path.display()))
        .ok()?;
    ab_glyph::FontVec::try_from_vec_and_index(bytes, 0)
        .map_err(|e| log::warn!("font {}: {e}", path.display()))
        .ok()
}

/// The ten combo digits from the font file at `path` (none if it is missing or unreadable).
pub fn load_digits(path: &Path) -> Vec<(Sprite, Image)> {
    let Some(font) = load_font(path) else {
        return Vec::new();
    };
    (0..10)
        .filter_map(|d| bake_digit(&font, d).map(|img| (Sprite::Digit(d), img)))
        .collect()
}

/// Where each loaded sprite sits in the atlas. Empty (`default`) = everything flat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpriteSheet {
    uvs: HashMap<Sprite, UvRect>,
    /// Pixel size of each sprite (for aspect ratios).
    sizes: HashMap<Sprite, [u32; 2]>,
}

impl SpriteSheet {
    pub fn get(&self, s: Sprite) -> Option<UvRect> {
        self.uvs.get(&s).copied()
    }

    /// Height / width of a loaded sprite.
    pub fn aspect(&self, s: Sprite) -> Option<f32> {
        self.sizes.get(&s).map(|[w, h]| *h as f32 / *w as f32)
    }

    pub fn is_empty(&self) -> bool {
        self.uvs.is_empty()
    }

    /// Register a sprite (tests and [`pack`]).
    pub fn insert(&mut self, s: Sprite, uv: UvRect, size: [u32; 2]) {
        self.uvs.insert(s, uv);
        self.sizes.insert(s, size);
    }
}

/// An image placed at `at` (texels) in the atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub at: [u32; 2],
    pub image: Image,
}

/// Pack baked sprites into the atlas, after the opaque white block at the origin. Sprites that
/// do not fit are logged and left out.
pub fn pack(sprites: Vec<(Sprite, Image)>) -> (SpriteSheet, Vec<Placed>) {
    let mut shelves = Shelves::new([ATLAS_SIZE, ATLAS_SIZE]);
    let mut placed = Vec::new();
    if let Some(at) = shelves.alloc(WHITE_BLOCK, WHITE_BLOCK) {
        let rgba = vec![255; (WHITE_BLOCK * WHITE_BLOCK * 4) as usize];
        let size = [WHITE_BLOCK; 2];
        placed.push(Placed {
            at,
            image: Image { size, rgba },
        });
    }
    let mut sheet = SpriteSheet::default();
    let mut sprites = sprites;
    sprites.sort_by_key(|(_, img)| std::cmp::Reverse(img.size[1]));
    let s = ATLAS_SIZE as f32;
    for (sprite, image) in sprites {
        let [w, h] = image.size;
        let Some(at) = shelves.alloc(w, h) else {
            log::warn!("sprite atlas full; {sprite:?} ({w}x{h}) left out");
            continue;
        };
        // Half-texel inset so bilinear sampling stays inside the sprite.
        let uv = [
            (at[0] as f32 + 0.5) / s,
            (at[1] as f32 + 0.5) / s,
            (at[0] + w) as f32 / s - 0.5 / s,
            (at[1] + h) as f32 / s - 0.5 / s,
        ];
        sheet.insert(sprite, uv, [w, h]);
        placed.push(Placed { at, image });
    }
    (sheet, placed)
}

#[derive(Debug, thiserror::Error)]
pub enum SpriteError {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {source}", path.display())]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
}

pub(crate) fn read_png(path: &Path) -> Result<Image, SpriteError> {
    let bytes = std::fs::read(path).map_err(|source| SpriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|source| SpriteError::Decode {
            path: path.to_path_buf(),
            source,
        })?
        .to_rgba8();
    Ok(Image {
        size: [img.width(), img.height()],
        rgba: img.into_raw(),
    })
}

/// Read and bake every sprite found under `ui_dir`; missing or unreadable ones are logged.
pub fn load_sources(ui_dir: &Path) -> Vec<(Sprite, Image)> {
    let mut cache: HashMap<&str, Option<Image>> = HashMap::new();
    let mut out = Vec::new();
    for sprite in Sprite::ALL {
        let Some(src) = source(sprite) else { continue };
        let img = cache.entry(src.file).or_insert_with(|| {
            let path = ui_dir.join(format!("{}.png", src.file));
            read_png(&path).map_err(|e| log::warn!("{e}")).ok()
        });
        if let Some(img) = img {
            out.push((sprite, bake(img, &src)));
        }
    }
    log::info!(
        "{} of {} playfield sprites loaded from {}",
        out.len(),
        Sprite::ALL.len(),
        ui_dir.display()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(size: [u32; 2], f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        Image::from_fn(size, f)
    }

    #[test]
    fn rows_and_shrink() {
        let i = img([4, 10], |x, y| [x as u8, y as u8, 0, 255]);
        let src = Source {
            file: "",
            rows: (0.2, 0.6),
            bake: Bake::Image,
            shrink: 1,
        };
        let b = bake(&i, &src);
        assert_eq!(b.size, [2, 2]);
        // Rows 2..6 halved: first texel averages x 0..1, y 2..3.
        assert_eq!(b.px(0, 0), [0, 2, 0, 255]);
        assert_eq!(b.px(1, 1), [2, 4, 0, 255]);
    }

    #[test]
    fn alpha_and_mask_bakes_are_premultiplied() {
        let i = img([1, 1], |_, _| [255, 0, 128, 255]);
        let alpha = |c| Source {
            file: "",
            rows: ALL_ROWS,
            bake: Bake::Alpha(c),
            shrink: 0,
        };
        assert_eq!(bake(&i, &alpha(Ch::B)).rgba, vec![128, 128, 128, 128]);
        let mask = Source {
            bake: Bake::Mask {
                hi: Ch::R,
                hi_gain: 0.5,
                shade: Ch::G,
                shade_lo: 0.6,
                alpha: Some(Ch::B),
            },
            ..alpha(Ch::R)
        };
        // a = 128/255: hi 0.5·a, shade 0.6·a.
        assert_eq!(bake(&i, &mask).rgba, vec![64, 77, 0, 128]);
    }

    #[test]
    fn pack_places_white_block_first_and_insets_uvs() {
        let sprite = img([8, 4], |_, _| [255; 4]);
        let (sheet, placed) = pack(vec![(Sprite::Glow, sprite)]);
        assert_eq!(placed[0].at, [0, 0]);
        assert_eq!(placed[0].image.size, [WHITE_BLOCK; 2]);
        let uv = sheet.get(Sprite::Glow).unwrap();
        let at = placed[1].at;
        let s = ATLAS_SIZE as f32;
        assert!((uv[0] - (at[0] as f32 + 0.5) / s).abs() < 1e-7);
        assert!((uv[3] - (at[1] as f32 + 3.5) / s).abs() < 1e-7);
        assert_eq!(sheet.aspect(Sprite::Glow), Some(0.5));
        assert!(sheet.get(Sprite::Combo).is_none());
    }

    #[test]
    fn missing_files_load_nothing() {
        let dir = std::env::temp_dir().join("wacca-render-no-such-ui-dir");
        assert!(load_sources(&dir).is_empty());
    }

    /// Needs the exported game font: `WACCA_DATA` = the `data` dir; `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn shinbi_digits_share_one_cell_height() {
        let dir = std::path::PathBuf::from(std::env::var("WACCA_DATA").unwrap());
        let digits = load_digits(&dir.join("fonts").join("SHINBI.otf"));
        assert_eq!(digits.len(), 10);
        let h = digits[0].1.size[1];
        for (s, img) in &digits {
            assert_eq!(img.size[1], h, "{s:?}");
            assert!(img.size[0] > h / 3, "{s:?} {:?}", img.size);
            // Glyph pixels are opaque white, and the padding row stays empty.
            assert!(img.rgba.chunks(4).any(|p| p == [255; 4]), "{s:?}");
            assert!(img.rgba[..img.size[0] as usize * 4].iter().all(|&b| b == 0));
        }
    }

    /// Needs the exported game font: `WACCA_DATA` = the `data` dir; `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn rim_texts_render_latin_and_kana() {
        let dir = std::path::PathBuf::from(std::env::var("WACCA_DATA").unwrap());
        let font = load_font(&dir.join("fonts").join("DF-KaGei-W6.ttc")).unwrap();
        let short = bake_text(&font, "SCORE").unwrap();
        let long = bake_text(&font, "NORMAL/Lv.1").unwrap();
        let kana = bake_text(&font, "フラジール").unwrap();
        assert_eq!(short.size[1], long.size[1]);
        assert!(long.size[0] > short.size[0]);
        for img in [&short, &kana] {
            assert!(img.rgba.chunks(4).any(|p| p == [255; 4]));
        }
        assert!(bake_text(&font, "").is_none());
    }

    #[test]
    fn every_sprite_has_a_source() {
        for s in Sprite::ALL {
            let src = source(s).unwrap();
            assert!(src.rows.0 < src.rows.1 && !src.file.is_empty(), "{s:?}");
        }
    }
}
