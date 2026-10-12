//! `.mer` parser. Spec: `docs/research/01-chart-format.md` §1–4 and §6 (parsing rules).

use std::collections::HashMap;

use crate::model::{
    Arc, BeatPos, Bonus, Chart, ChartHeader, HoldNode, LANES, MaskDir, MaskEvent, Note, NoteBody,
    ReverseSection, SlideDir, SnapDir, TICKS_PER_MEASURE,
};
use crate::timing::{self, ScrollEvent, TempoChange};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("no #BODY line")]
    MissingBody,
    #[error("no EOF row (note kind 14)")]
    MissingEof,
}

/// Parses a `.mer` file (UTF-8, or UTF-16 with BOM). Recoverable oddities are logged and skipped.
pub fn parse(bytes: &[u8]) -> Result<Chart, ParseError> {
    let text = decode(bytes);
    let mut lines = text.lines().enumerate().map(|(n, l)| (n + 1, l));
    let header = parse_header(&mut lines)?;
    let mut rows: Vec<Row> = lines.filter_map(|(n, l)| parse_row(n, l)).collect();
    rows.sort_by_key(|r| r.pos); // stable: file order breaks ties
    build(header, &rows)
}

#[derive(Clone, Copy, Debug)]
struct Row {
    pos: BeatPos,
    line: usize,
    kind: RowKind,
}

#[derive(Clone, Copy, Debug)]
enum RowKind {
    Note(NoteRow),
    Tempo(TempoChange),
    Scroll(ScrollEvent),
    Reverse(ReverseMark),
}

#[derive(Clone, Copy, Debug)]
struct NoteRow {
    kind: i64,
    index: u32,
    arc: Arc,
    /// Column 9: `ChainRearNoteIndex` (holds) or `GuideLaneDirection` (masks); ignored elsewhere.
    extra: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
enum ReverseMark {
    Start,
    End,
    NoteEnd,
}

fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    if !bytes.len().is_multiple_of(2) {
        log::warn!("odd UTF-16 byte count; ignoring the last byte");
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|&c| unit(c));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// C `atoi`: optional sign and leading digits (`480zxx` -> 480); 0 if there are none. Saturates.
fn lead_int(s: &str) -> i64 {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let v = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |acc, d| {
            acc.saturating_mul(10).saturating_add(i64::from(d - b'0'))
        });
    if neg { -v } else { v }
}

/// C `strtod` on the leading decimal number (`1.5e2x` -> 150); 0 if there is none.
fn lead_float(s: &str) -> f64 {
    let b = s.as_bytes();
    let digits = |i: &mut usize| {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let mut end = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let mut mantissa = digits(&mut end);
    if b.get(end) == Some(&b'.') {
        end += 1;
        mantissa += digits(&mut end);
    }
    if mantissa == 0 {
        return 0.0;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let mut exp = end + 1;
        if matches!(b.get(exp), Some(b'+' | b'-')) {
            exp += 1;
        }
        if digits(&mut exp) > 0 {
            end = exp;
        }
    }
    s.get(..end).and_then(|n| n.parse().ok()).unwrap_or(0.0)
}

/// Reads `#TAG value` lines up to `#BODY`; the last occurrence of a tag wins, non-`#` lines are ignored.
fn parse_header<'a>(
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
) -> Result<ChartHeader, ParseError> {
    let mut h = ChartHeader::default();
    for (n, line) in lines {
        let line = line.trim();
        let Some(tag_line) = line.strip_prefix('#') else {
            if !line.is_empty() {
                log::warn!("line {n}: ignoring non-tag header line {line:?}");
            }
            continue;
        };
        let (tag, value) = tag_line
            .split_once(char::is_whitespace)
            .map_or((tag_line, ""), |(t, v)| (t, v.trim()));
        match tag {
            "BODY" => return Ok(h),
            "MUSIC_SCORE_ID" => h.music_score_id = lead_int(value) as i32,
            "MUSIC_SCORE_VERSION" => h.music_score_version = lead_int(value) as i32,
            "GAME_VERSION" => h.game_version = value.to_string(),
            "MUSIC_FILE_PATH" => h.music_file_path = value.to_string(),
            "OFFSET" => h.offset = lead_float(value),
            "MOVIEOFFSET" => h.movie_offset = lead_float(value),
            _ => h.extra.push((tag.to_string(), value.to_string())),
        }
    }
    Err(ParseError::MissingBody)
}

fn parse_row(line: usize, text: &str) -> Option<Row> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let [measure, tick, kind, rest @ ..] = tokens.as_slice() else {
        if !tokens.is_empty() {
            log::warn!("line {line}: ignoring short row {text:?}");
        }
        return None;
    };
    let (Ok(measure), Ok(tick)) = (
        u32::try_from(lead_int(measure)),
        u16::try_from(lead_int(tick)),
    ) else {
        log::warn!("line {line}: ignoring row with invalid position {text:?}");
        return None;
    };
    if u32::from(tick) >= TICKS_PER_MEASURE {
        log::warn!("line {line}: ignoring row with tick {tick} >= {TICKS_PER_MEASURE}");
        return None;
    }
    let kind = match (lead_int(kind), rest) {
        (1, [kind, index, lane, width, _visible, extra @ ..]) => {
            let Ok(index) = u32::try_from(lead_int(index)) else {
                log::warn!("line {line}: ignoring note with invalid index {index:?}");
                return None;
            };
            RowKind::Note(NoteRow {
                kind: lead_int(kind),
                index,
                arc: arc(line, lead_int(lane), lead_int(width)),
                extra: extra.first().map(|s| lead_int(s)),
            })
        }
        (2, [bpm, ..]) => match lead_float(bpm) {
            bpm if bpm.is_finite() && bpm > 0.0 => RowKind::Tempo(TempoChange::Bpm(bpm)),
            bpm => {
                log::warn!("line {line}: ignoring invalid BPM {bpm}");
                return None;
            }
        },
        (3, [num, den @ ..]) => {
            // A numerator-only time signature means x/4.
            let den = den.first().map_or(4, |d| lead_int(d));
            match (u32::try_from(lead_int(num)), u32::try_from(den)) {
                (Ok(num @ 1..), Ok(den @ 1..)) => RowKind::Tempo(TempoChange::Sig(num, den)),
                _ => {
                    log::warn!("line {line}: ignoring invalid time signature {text:?}");
                    return None;
                }
            }
        }
        (5, [rate, ..]) => match lead_float(rate) {
            rate if rate.is_finite() => RowKind::Scroll(ScrollEvent::Speed(rate)),
            rate => {
                log::warn!("line {line}: ignoring invalid hi-speed {rate}");
                return None;
            }
        },
        (6, _) => RowKind::Reverse(ReverseMark::Start),
        (7, _) => RowKind::Reverse(ReverseMark::End),
        (8, _) => RowKind::Reverse(ReverseMark::NoteEnd),
        (9, _) => RowKind::Scroll(ScrollEvent::StopStart),
        (10, _) => RowKind::Scroll(ScrollEvent::StopEnd),
        (kind, _) => {
            log::warn!(
                "line {line}: ignoring row kind {kind} ({} columns)",
                tokens.len()
            );
            return None;
        }
    };
    Some(Row {
        pos: BeatPos { measure, tick },
        line,
        kind,
    })
}

/// Lane wraps mod 60; width is clamped to 1..=60 (hold joints with width 0 exist in the data).
fn arc(line: usize, lane: i64, width: i64) -> Arc {
    let lanes = i64::from(LANES);
    if !(0..lanes).contains(&lane) || !(1..=lanes).contains(&width) {
        log::debug!("line {line}: normalising lane {lane} width {width}");
    }
    Arc {
        lane: lane.rem_euclid(lanes) as u8,
        width: width.clamp(1, lanes) as u8,
    }
}

/// Body and bonus of a single-row judged note kind.
fn simple_note(kind: i64) -> Option<(NoteBody, Bonus)> {
    use NoteBody::{Chain, Slide, Snap, Touch};
    Some(match kind {
        1 => (Touch, Bonus::None),
        2 => (Touch, Bonus::Bonus),
        20 => (Touch, Bonus::R),
        3 => (Snap(SnapDir::In), Bonus::None),
        21 => (Snap(SnapDir::In), Bonus::R),
        4 => (Snap(SnapDir::Out), Bonus::None),
        22 => (Snap(SnapDir::Out), Bonus::R),
        5 => (Slide(SlideDir::Cw), Bonus::None),
        6 => (Slide(SlideDir::Cw), Bonus::Bonus),
        23 => (Slide(SlideDir::Cw), Bonus::R),
        7 => (Slide(SlideDir::Ccw), Bonus::None),
        8 => (Slide(SlideDir::Ccw), Bonus::Bonus),
        24 => (Slide(SlideDir::Ccw), Bonus::R),
        16 => (Chain, Bonus::None),
        26 => (Chain, Bonus::R),
        _ => return None,
    })
}

const HOLD_JOINT: i64 = 10;
const HOLD_END: i64 = 11;

#[derive(Clone, Copy, Debug)]
enum ReverseState {
    Idle,
    Started(f64),
    Ended(f64, f64),
}

fn build(header: ChartHeader, rows: &[Row]) -> Result<Chart, ParseError> {
    let tempo_changes: Vec<(u64, TempoChange)> = rows
        .iter()
        .filter_map(|r| match r.kind {
            RowKind::Tempo(c) => Some((r.pos.abs_tick(), c)),
            _ => None,
        })
        .collect();
    let tempo = timing::build_tempo(&tempo_changes);
    let time = |pos: BeatPos| timing::tick_to_sec(&tempo, pos.abs_tick());

    let mut by_index = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        if let RowKind::Note(n) = row.kind
            && by_index.insert(n.index, i).is_some()
        {
            log::warn!("line {}: duplicate note index {}", row.line, n.index);
        }
    }

    let mut used = vec![false; rows.len()];
    let (mut notes, mut masks, mut tutorial_tags) = (Vec::new(), Vec::new(), Vec::new());
    let (mut scroll_events, mut reverses) = (Vec::new(), Vec::new());
    let mut reverse = ReverseState::Idle;
    let mut end = None;
    for (i, row) in rows.iter().enumerate() {
        let t = time(row.pos);
        let n = match row.kind {
            RowKind::Note(n) => n,
            RowKind::Tempo(_) => continue,
            RowKind::Scroll(e) => {
                scroll_events.push((t, e));
                continue;
            }
            RowKind::Reverse(mark) => {
                reverse = match (reverse, mark) {
                    (ReverseState::Started(s), ReverseMark::End) => ReverseState::Ended(s, t),
                    (ReverseState::Ended(start, end), ReverseMark::NoteEnd) => {
                        reverses.push(ReverseSection {
                            start,
                            end,
                            note_end: t,
                        });
                        ReverseState::Idle
                    }
                    (state, ReverseMark::Start) => {
                        if !matches!(state, ReverseState::Idle) {
                            log::warn!("line {}: reverse restarted; dropping {state:?}", row.line);
                        }
                        ReverseState::Started(t)
                    }
                    (state, mark) => {
                        log::warn!("line {}: ignoring stray reverse row {mark:?}", row.line);
                        state
                    }
                };
                continue;
            }
        };
        let note = |body, bonus| Note {
            pos: row.pos,
            time: t,
            arc: n.arc,
            bonus,
            body,
            file_index: n.index,
        };
        if let Some((body, bonus)) = simple_note(n.kind) {
            notes.push(note(body, bonus));
            continue;
        }
        match n.kind {
            9 | 25 => {
                let nodes = walk_hold(rows, i, &by_index, &mut used, time);
                let bonus = if n.kind == 25 { Bonus::R } else { Bonus::None };
                notes.push(note(NoteBody::Hold { nodes }, bonus));
            }
            HOLD_JOINT | HOLD_END => {} // consumed by walk_hold; orphans reported below
            12 | 13 => masks.push(MaskEvent {
                pos: row.pos,
                time: t,
                arc: n.arc,
                add: n.kind == 12,
                dir: mask_dir(row.line, n.extra),
            }),
            14 => {
                if end.is_some() {
                    log::warn!("line {}: extra EOF row; the last one wins", row.line);
                }
                end = Some(row.pos);
            }
            18 => tutorial_tags.push(row.pos),
            k => log::warn!("line {}: ignoring note kind {k}", row.line),
        }
    }
    if !matches!(reverse, ReverseState::Idle) {
        log::warn!("dropping incomplete reverse section {reverse:?}");
    }
    for (row, _) in rows.iter().zip(&used).filter(|(_, used)| !**used) {
        if let RowKind::Note(n) = row.kind
            && matches!(n.kind, HOLD_JOINT | HOLD_END)
        {
            log::warn!("line {}: dropping orphan hold node {}", row.line, n.index);
        }
    }
    let end = end.ok_or(ParseError::MissingEof)?;
    Ok(Chart {
        header,
        notes,
        masks,
        scroll: timing::build_scroll(&scroll_events),
        reverses,
        tutorial_tags,
        end,
        end_time: time(end),
        tempo,
    })
}

fn mask_dir(line: usize, extra: Option<i64>) -> MaskDir {
    match extra {
        Some(0) => MaskDir::Plus,
        Some(1) => MaskDir::Minus,
        Some(2) | None => MaskDir::Center,
        Some(d) => {
            log::warn!("line {line}: unknown mask direction {d}; using centre");
            MaskDir::Center
        }
    }
}

/// Follows `ChainRearNoteIndex` from the hold head at `rows[head]` to a HOLD_END. A broken chain (missing,
/// unknown, reused, backwards or non-joint link) is truncated at the last reachable node with a warning.
fn walk_hold(
    rows: &[Row],
    head: usize,
    by_index: &HashMap<u32, usize>,
    used: &mut [bool],
    time: impl Fn(BeatPos) -> f64,
) -> Vec<HoldNode> {
    let mut nodes: Vec<HoldNode> = Vec::new();
    let mut i = head;
    loop {
        let row = rows[i];
        let RowKind::Note(n) = row.kind else { break };
        used[i] = true;
        let lane_unwrapped = match nodes.last() {
            Some(prev) => prev.lane_unwrapped + lane_delta(prev.arc.lane, n.arc.lane) as f32,
            None => f32::from(n.arc.lane),
        };
        nodes.push(HoldNode {
            pos: row.pos,
            time: time(row.pos),
            arc: n.arc,
            lane_unwrapped,
        });
        if n.kind == HOLD_END {
            break;
        }
        let next = n
            .extra
            .and_then(|l| u32::try_from(l).ok())
            .and_then(|l| by_index.get(&l).copied())
            .filter(|&j| {
                !used[j]
                    && rows[j].pos >= row.pos
                    && matches!(rows[j].kind, RowKind::Note(m) if matches!(m.kind, HOLD_JOINT | HOLD_END))
            });
        match next {
            Some(j) => i = j,
            None => {
                log::warn!(
                    "line {}: hold node {} has a missing or invalid link {:?}; truncating hold",
                    row.line,
                    n.index,
                    n.extra
                );
                break;
            }
        }
    }
    nodes
}

/// Shortest signed lane step from `from` to `to` on the 60-lane ring (59 -> 0 is +1).
fn lane_delta(from: u8, to: u8) -> i32 {
    let lanes = i32::from(LANES);
    (i32::from(to) - i32::from(from) + lanes / 2).rem_euclid(lanes) - lanes / 2
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 120 BPM 4/4: one measure = 2 s, 960 ticks = 1 s.
    const TEMPO: &str = "0 0 2 120.0\n0 0 3 4 4\n";
    const EOF: &str = "9 0 1 14 999 0 60 1\n";

    fn chart(body: &str) -> Chart {
        parse(format!("#BODY\n{TEMPO}{body}{EOF}").as_bytes()).unwrap()
    }

    fn hold_nodes(note: &Note) -> &[HoldNode] {
        match &note.body {
            NoteBody::Hold { nodes } => nodes,
            other => panic!("not a hold: {other:?}"),
        }
    }

    #[test]
    fn leading_number_tokens() {
        assert_eq!(lead_int("480zxx"), 480);
        assert_eq!(lead_int("002"), 2);
        assert_eq!(lead_int("-7a"), -7);
        assert_eq!(lead_int("+3"), 3);
        assert_eq!(lead_int("zxx"), 0);
        assert_eq!(lead_int("4.9"), 4);
        assert_eq!(lead_int("99999999999999999999999"), i64::MAX);
        assert_eq!(lead_float("180.000000"), 180.0);
        assert_eq!(lead_float("-0.06205"), -0.06205);
        assert_eq!(lead_float("1.5e2x"), 150.0);
        assert_eq!(lead_float("1e"), 1.0);
        assert_eq!(lead_float(".5"), 0.5);
        assert_eq!(lead_float("3."), 3.0);
        assert_eq!(lead_float("."), 0.0);
        assert_eq!(lead_float("abc"), 0.0);
    }

    #[test]
    fn header_last_wins_and_merge_markers_ignored() {
        let text = "#MUSIC_SCORE_ID 1\n#MUSIC_SCORE_VERSION 2\n#GAME_VERSION \n#MUSIC_FILE_PATH MER_BGM_S02_221\n\
            <<<<<<< .mine\n#OFFSET 0.001000\n||||||| .r22894\n#OFFSET 0.000000\n=======\n#OFFSET 0.002\n\
            >>>>>>> .r22915\n#MOVIEOFFSET 1.2\n#DISPLAY_BPM 180\n#BODY\n";
        let c = parse(format!("{text}{TEMPO}{EOF}").as_bytes()).unwrap();
        let h = &c.header;
        assert_eq!((h.music_score_id, h.music_score_version), (1, 2));
        assert_eq!(h.game_version, "");
        assert_eq!(h.music_file_path, "MER_BGM_S02_221");
        assert_eq!((h.offset, h.movie_offset), (0.002, 1.2));
        assert_eq!(
            h.extra,
            vec![("DISPLAY_BPM".to_string(), "180".to_string())]
        );
    }

    #[test]
    fn utf16_and_utf8_boms_with_crlf() {
        let text =
            format!("#OFFSET 0.5\n#BODY\n{TEMPO}0 960 1 1 0 5 10 1\n{EOF}").replace('\n', "\r\n");
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        let u8bom: Vec<u8> = [0xEF, 0xBB, 0xBF].into_iter().chain(text.bytes()).collect();
        for bytes in [le, be, u8bom] {
            let c = parse(&bytes).unwrap();
            assert_eq!(c.header.offset, 0.5);
            assert_eq!(c.notes.len(), 1);
            assert_eq!(c.notes[0].time, 1.0);
            assert_eq!(c.end_time, 18.0);
        }
    }

    #[test]
    fn junk_suffix_and_leading_zeros() {
        let c = chart("002 480zxx 1 18 000 50 10 1 0\n");
        assert_eq!(
            c.tutorial_tags,
            vec![BeatPos {
                measure: 2,
                tick: 480
            }]
        );
        assert!(c.notes.is_empty());
    }

    #[test]
    fn short_and_unknown_rows_are_skipped() {
        let c = chart(
            "\n0 0\n0 0 4\n0 0 99 1\n1 0 1 15 0 0 10 1\n1 0 1 1\n#NOTE\n1 2000 1 1 1 0 10 1\n",
        );
        assert!(c.notes.is_empty());
    }

    #[test]
    fn missing_body_or_eof_is_an_error() {
        assert_eq!(
            parse(b"#OFFSET 0\n0 0 2 120\n"),
            Err(ParseError::MissingBody)
        );
        assert_eq!(
            parse(format!("#BODY\n{TEMPO}").as_bytes()),
            Err(ParseError::MissingEof)
        );
    }

    #[test]
    fn rows_are_stably_sorted_by_tick() {
        let c = chart("2 0 1 1 0 10 5 1\n1 0 1 1 1 20 5 1\n1 0 1 2 2 30 5 1\n0 960 5 2.0\n");
        let idx: Vec<u32> = c.notes.iter().map(|n| n.file_index).collect();
        assert_eq!(idx, [1, 2, 0]);
        assert_eq!(c.notes[0].time, 2.0);
        assert_eq!(c.scroll[1].start_sec, 1.0);
    }

    #[test]
    fn stacked_tempo_rows_last_wins_and_numerator_only() {
        let c = parse(
            b"#BODY\n0 0 2 60\n0 0 3 4 4\n0 0 3 3 4\n0 0 2 120\n1 0 3 6 8\n1 0 3 2\n2 0 1 1 0 0 10 1\n3 0 1 14 1 0 60 1\n",
        )
        .unwrap();
        assert_eq!(c.tempo.len(), 2);
        assert_eq!(
            (c.tempo[0].bpm, c.tempo[0].num, c.tempo[0].den),
            (120.0, 3, 4)
        );
        assert_eq!((c.tempo[1].num, c.tempo[1].den), (2, 4));
        assert_eq!(c.notes[0].time, 2.5);
        assert_eq!(c.end_time, 3.5);
    }

    #[test]
    fn note_kinds() {
        let kinds = [1, 2, 3, 4, 5, 6, 7, 8, 16, 20, 21, 22, 23, 24, 26];
        let body: String = kinds
            .iter()
            .enumerate()
            .map(|(i, k)| format!("1 {i} 1 {k} {i} 0 10 1\n"))
            .collect();
        let c = chart(&body);
        let got: Vec<(NoteBody, Bonus)> =
            c.notes.iter().map(|n| (n.body.clone(), n.bonus)).collect();
        use NoteBody::*;
        let (cw, ccw) = (Slide(SlideDir::Cw), Slide(SlideDir::Ccw));
        assert_eq!(
            got,
            [
                (Touch, Bonus::None),
                (Touch, Bonus::Bonus),
                (Snap(SnapDir::In), Bonus::None),
                (Snap(SnapDir::Out), Bonus::None),
                (cw.clone(), Bonus::None),
                (cw.clone(), Bonus::Bonus),
                (ccw.clone(), Bonus::None),
                (ccw.clone(), Bonus::Bonus),
                (Chain, Bonus::None),
                (Touch, Bonus::R),
                (Snap(SnapDir::In), Bonus::R),
                (Snap(SnapDir::Out), Bonus::R),
                (cw, Bonus::R),
                (ccw, Bonus::R),
                (Chain, Bonus::R),
            ]
        );
    }

    #[test]
    fn trailing_column_ignored_on_non_hold_notes() {
        // The tutorial writes a 9th column (0) on every note, including hold ends.
        let c = chart("1 0 1 1 0 5 10 1 0\n1 0 1 9 1 5 10 1 2\n1 960 1 11 2 5 10 1 0\n");
        assert_eq!(c.notes.len(), 2);
        assert_eq!(hold_nodes(&c.notes[1]).len(), 2);
    }

    #[test]
    fn hold_chain_collapses_with_times() {
        let c = chart("1 0 1 9 0 10 4 1 1\n2 0 1 11 2 14 4 1\n1 960 1 10 1 12 0 0 2\n");
        assert_eq!(c.notes.len(), 1);
        let n = &c.notes[0];
        assert_eq!((n.time, n.bonus, n.file_index), (2.0, Bonus::None, 0));
        let nodes = hold_nodes(n);
        let t: Vec<f64> = nodes.iter().map(|n| n.time).collect();
        assert_eq!(t, [2.0, 3.0, 4.0]);
        assert_eq!(nodes[1].arc, Arc { lane: 12, width: 1 }); // width 0 clamped
        let r = chart("1 0 1 25 0 0 4 1 1\n1 960 1 11 1 0 4 1\n");
        assert_eq!(r.notes[0].bonus, Bonus::R);
    }

    #[test]
    fn hold_lanes_unwrap_across_zero() {
        let c = chart(
            "1 0 1 9 0 58 4 1 1\n1 100 1 10 1 1 4 0 2\n1 200 1 10 2 59 4 0 3\n1 300 1 11 3 57 4 1\n",
        );
        let lanes: Vec<f32> = hold_nodes(&c.notes[0])
            .iter()
            .map(|n| n.lane_unwrapped)
            .collect();
        assert_eq!(lanes, [58.0, 61.0, 59.0, 57.0]);
        assert_eq!(lane_delta(0, 59), -1);
        assert_eq!(lane_delta(59, 0), 1);
    }

    #[test]
    fn same_tick_hold_links() {
        let c = chart(
            "1 0 1 9 0 0 10 1 1\n1 0 1 10 1 0 54 1 2\n1 0 1 10 2 0 56 1 3\n1 960 1 11 3 0 10 1\n",
        );
        let w: Vec<u8> = hold_nodes(&c.notes[0])
            .iter()
            .map(|n| n.arc.width)
            .collect();
        assert_eq!(w, [10, 54, 56, 10]);
    }

    #[test]
    fn broken_hold_links_truncate() {
        // Missing link column, unknown index, backwards link, link to a non-joint, and an orphan end.
        let c = chart(concat!(
            "1 0 1 9 0 0 10 1 1\n1 960 1 10 1 0 10 1\n",
            "2 0 1 9 2 0 10 1 77\n",
            "3 0 1 9 3 0 10 1 4\n2 960 1 11 4 0 10 1\n",
            "4 0 1 9 5 0 10 1 6\n4 960 1 1 6 0 10 1\n",
            "5 0 1 11 7 0 10 1\n",
        ));
        let lens: Vec<usize> = c
            .notes
            .iter()
            .filter_map(|n| match &n.body {
                NoteBody::Hold { nodes } => Some(nodes.len()),
                _ => None,
            })
            .collect();
        assert_eq!(lens, [2, 1, 1, 1]);
        assert_eq!(c.notes.len(), 5); // 4 holds + the touch the last hold wrongly linked to
    }

    #[test]
    fn masks_with_direction() {
        let c = chart(
            "0 0 1 12 0 10 20 1 0\n0 0 1 12 1 30 5 1 1\n0 0 1 13 2 0 60 1 2\n0 0 1 13 3 0 60 1\n",
        );
        let got: Vec<(bool, MaskDir, Arc)> =
            c.masks.iter().map(|m| (m.add, m.dir, m.arc)).collect();
        assert_eq!(
            got,
            [
                (
                    true,
                    MaskDir::Plus,
                    Arc {
                        lane: 10,
                        width: 20
                    }
                ),
                (true, MaskDir::Minus, Arc { lane: 30, width: 5 }),
                (false, MaskDir::Center, Arc { lane: 0, width: 60 }),
                (false, MaskDir::Center, Arc { lane: 0, width: 60 }),
            ]
        );
        assert!(c.notes.is_empty());
    }

    #[test]
    fn lane_and_width_normalised() {
        let c = chart("0 0 1 1 0 61 0 1\n0 0 1 1 1 -1 70 1\n");
        assert_eq!(c.notes[0].arc, Arc { lane: 1, width: 1 });
        assert_eq!(
            c.notes[1].arc,
            Arc {
                lane: 59,
                width: 60
            }
        );
    }

    #[test]
    fn stops_and_hi_speed_scroll() {
        let c = chart("1 0 9\n1 960 10\n0 960 5 0.5\n");
        assert_eq!(c.dist_at(1.0), 1.0);
        assert_eq!(c.dist_at(2.0), 1.5);
        assert_eq!(c.dist_at(2.5), 1.5);
        assert_eq!(c.dist_at(4.0), 2.0);
    }

    #[test]
    fn reverse_sections() {
        let c = chart("1 0 6\n2 0 7\n3 0 8\n4 0 7\n5 0 6\n");
        assert_eq!(
            c.reverses,
            [ReverseSection {
                start: 2.0,
                end: 4.0,
                note_end: 6.0
            }]
        );
    }

    #[test]
    fn offset_maps_audio_time() {
        let c = parse(format!("#OFFSET -0.06205\n#BODY\n{TEMPO}{EOF}").as_bytes()).unwrap();
        assert!((c.audio_time(0.0) - 0.06205).abs() < 1e-12);
    }
}
