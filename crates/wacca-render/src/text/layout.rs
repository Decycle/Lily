//! Pure text layout: measuring, truncating, wrapping and aligning a line, given per-character
//! advances (so it is testable without a font).

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

pub const ELLIPSIS: char = '…';

/// Width of `text` (no kerning).
pub fn measure(text: &str, advance: impl Fn(char) -> f32) -> f32 {
    text.chars().map(advance).sum()
}

/// Left edge of a line of width `width` anchored at `x` with `align`.
pub fn align_x(x: f32, width: f32, align: Align) -> f32 {
    match align {
        Align::Left => x,
        Align::Center => x - width / 2.0,
        Align::Right => x - width,
    }
}

/// Corners (top-left, top-right, bottom-right, bottom-left) of `[x, y, w, h]` turned by `angle`
/// radians (clockwise on screen, y down) about its centre.
pub fn corners([x, y, w, h]: [f32; 4], angle: f32) -> [[f32; 2]; 4] {
    let (c, (sin, cos)) = ([x + w / 2.0, y + h / 2.0], angle.sin_cos());
    [[-w, -h], [w, -h], [w, h], [-w, h]].map(|[dx, dy]| {
        let (dx, dy) = (dx / 2.0, dy / 2.0);
        [c[0] + dx * cos - dy * sin, c[1] + dx * sin + dy * cos]
    })
}

/// `text` if it fits in `max`, else its longest prefix that fits followed by [`ELLIPSIS`].
pub fn fit(text: &str, max: f32, advance: impl Fn(char) -> f32) -> Cow<'_, str> {
    if measure(text, &advance) <= max {
        return Cow::Borrowed(text);
    }
    let mut width = advance(ELLIPSIS);
    let mut out = String::new();
    for c in text.chars() {
        width += advance(c);
        if width > max {
            break;
        }
        out.push(c);
    }
    out.push(ELLIPSIS);
    Cow::Owned(out)
}

/// CJK characters can be broken before and after (no spaces between words).
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{2E80}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FFEF}')
}

/// Split one paragraph into break units: a word with its trailing spaces, or one CJK character.
fn units(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|&(_, n)| n);
        let end = i + c.len_utf8();
        let boundary = match next {
            None => true,
            Some(n) => is_cjk(c) || is_cjk(n) || (c.is_whitespace() && !n.is_whitespace()),
        };
        if boundary {
            out.push(&text[start..end]);
            start = end;
        }
    }
    out
}

/// Greedy word wrap to lines no wider than `max` (trailing spaces ignored). Breaks at spaces
/// and around CJK characters; a word wider than `max` is broken between characters. `\n`
/// forces a break.
pub fn wrap(text: &str, max: f32, advance: impl Fn(char) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut width = 0.0;
        for unit in units(para) {
            let w = measure(unit, &advance);
            let visible = measure(unit.trim_end(), &advance);
            if !line.is_empty() && width + visible > max {
                lines.push(line.trim_end().to_string());
                line.clear();
                width = 0.0;
            }
            if line.is_empty() && visible > max {
                for c in unit.chars() {
                    let cw = advance(c);
                    if !line.is_empty() && width + cw > max && !c.is_whitespace() {
                        lines.push(std::mem::take(&mut line));
                        width = 0.0;
                    }
                    line.push(c);
                    width += cw;
                }
                continue;
            }
            line.push_str(unit);
            width += w;
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is 10 px wide.
    fn adv(_: char) -> f32 {
        10.0
    }

    #[test]
    fn corners_turn_about_the_centre() {
        let r = [10.0, 20.0, 4.0, 2.0];
        assert_eq!(
            corners(r, 0.0),
            [[10.0, 20.0], [14.0, 20.0], [14.0, 22.0], [10.0, 22.0]]
        );
        // A quarter turn clockwise moves the top-left corner to the top-right.
        let [tl, ..] = corners(r, std::f32::consts::FRAC_PI_2);
        assert!(
            (tl[0] - 13.0).abs() < 1e-4 && (tl[1] - 19.0).abs() < 1e-4,
            "{tl:?}"
        );
    }

    #[test]
    fn measure_and_align() {
        assert_eq!(measure("abc", adv), 30.0);
        assert_eq!(measure("ヒトガタ", adv), 40.0);
        assert_eq!(align_x(100.0, 30.0, Align::Left), 100.0);
        assert_eq!(align_x(100.0, 30.0, Align::Center), 85.0);
        assert_eq!(align_x(100.0, 30.0, Align::Right), 70.0);
    }

    #[test]
    fn fit_truncates_with_ellipsis() {
        assert_eq!(fit("abc", 30.0, adv), "abc");
        assert!(matches!(fit("abc", 30.0, adv), Cow::Borrowed(_)));
        assert_eq!(fit("abcdef", 40.0, adv), "abc…");
        assert_eq!(fit("ヒトガタ", 25.0, adv), "ヒ…");
        assert_eq!(fit("abc", 5.0, adv), "…");
    }

    #[test]
    fn wraps_words_cjk_and_long_words() {
        assert_eq!(wrap("aa bb cc", 50.0, adv), ["aa bb", "cc"]);
        assert_eq!(wrap("aa bb", 20.0, adv), ["aa", "bb"]);
        assert_eq!(wrap("あいうえお", 30.0, adv), ["あいう", "えお"]);
        assert_eq!(wrap("ab あい", 40.0, adv), ["ab あ", "い"]);
        assert_eq!(wrap("abcdefg", 30.0, adv), ["abc", "def", "g"]);
        assert_eq!(wrap("a\nb", 100.0, adv), ["a", "b"]);
        assert_eq!(wrap("", 100.0, adv), [""]);
    }
}
