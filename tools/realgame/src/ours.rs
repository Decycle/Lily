//! Our side of a comparison frame: the judgement events our engine produces for the injected script, turned into
//! the renderer's live state (judged notes, hit flashes, judgement art, combo) at any chart time, like the app does
//! during play.

use wacca_chart::{Arc, Chart, NoteBody};
use wacca_game::input::InputFrame;
use wacca_game::judge::{Judge, JudgeTable};
use wacca_game::rules::{JudgeClass, JudgeEvent, Judgement};
use wacca_render::{Flash, JudgeSprite};

/// Owned data behind a [`wacca_render::LiveState`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub judged: Vec<bool>,
    pub flashes: Vec<Flash>,
    pub judgement: Option<(JudgeSprite, f32)>,
    pub combo: u32,
}

impl Live {
    pub fn state(&self) -> wacca_render::LiveState<'_> {
        wacca_render::LiveState {
            judged: &self.judged,
            flashes: &self.flashes,
            judgement: self.judgement,
            combo: self.combo,
            ..Default::default()
        }
    }
}

/// All judgement events of `frames` on `chart` (our engine), in time order.
pub fn events(chart: &Chart, frames: &[InputFrame]) -> Vec<JudgeEvent> {
    let mut judge = Judge::new(chart, JudgeTable::reverse());
    let mut out: Vec<JudgeEvent> = frames.iter().flat_map(|f| judge.update(f)).collect();
    if let Some(&last) = frames.last() {
        // Let the clock run on so notes ending at the last release are resolved.
        out.extend(judge.update(&InputFrame {
            time: last.time + 1.0,
            ..last
        }));
    }
    out.extend(judge.finish());
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

fn sprite(j: Judgement) -> JudgeSprite {
    match j {
        Judgement::Marvelous => JudgeSprite::Marvelous,
        Judgement::Great => JudgeSprite::Great,
        Judgement::Good => JudgeSprite::Good,
        Judgement::Miss => JudgeSprite::Miss,
    }
}

/// Live state at chart time `t` from the events up to `t` (flashes for the last `flash_secs`).
pub fn live_at(chart: &Chart, events: &[JudgeEvent], t: f64, flash_secs: f64) -> Live {
    let mut live = Live {
        judged: vec![false; chart.notes.len()],
        ..Live::default()
    };
    for e in events.iter().take_while(|e| e.time <= t) {
        if let Some(j) = live.judged.get_mut(e.note) {
            *j = true;
        }
        live.combo = if e.judgement == Judgement::Miss {
            0
        } else {
            live.combo + 1
        };
        live.judgement = Some((sprite(e.judgement), (t - e.time) as f32));
        if t - e.time < flash_secs {
            let note = chart.notes.get(e.note);
            let arc = match (e.class, note.map(|n| &n.body)) {
                (JudgeClass::HoldEnd, Some(NoteBody::Hold { nodes })) => nodes.last().map(|n| n.arc),
                _ => note.map(|n| n.arc),
            }
            .unwrap_or(Arc { lane: 0, width: 0 });
            live.flashes.push(Flash {
                lane: arc.lane,
                width: arc.width,
                kind: sprite(e.judgement),
                age: (t - e.time) as f32,
            });
        }
    }
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{Tap, expand};

    #[test]
    fn perfect_taps_give_combo_flashes_and_judged_notes() {
        let chart = wacca_chart::parse(
            b"#BODY\n   0    0    2 120.000000\n   0    0    3    4\n\
              1    0    1    1    0   40    4    1\n\
              1  960    1    1    1   10    4    1\n\
              9    0    1   14    2    0   60    1\n",
        )
        .unwrap();
        let tap = |at_ms: f64, lane: u8| Tap {
            at_ms,
            dur_ms: 34.0,
            lane,
            width: 4,
            depths: 1,
        };
        let ev = events(&chart, &expand(&[tap(2000.0, 40), tap(3000.0, 10)]));
        assert_eq!(ev.len(), 2);
        assert!(ev.iter().all(|e| e.judgement == Judgement::Marvelous));
        let before = live_at(&chart, &ev, 1.9, 0.25);
        assert_eq!((before.combo, before.judged.clone()), (0, vec![false, false]));
        let after_first = live_at(&chart, &ev, 2.1, 0.25);
        assert_eq!(after_first.combo, 1);
        assert_eq!(after_first.judged, [true, false]);
        assert_eq!(after_first.flashes.len(), 1);
        assert_eq!(after_first.flashes[0].lane, 40);
        let (kind, age) = after_first.judgement.unwrap();
        assert_eq!(kind, JudgeSprite::Marvelous);
        assert!((age - 0.1).abs() < 0.02);
        let end = live_at(&chart, &ev, 5.0, 0.25);
        assert_eq!((end.combo, end.flashes.len()), (2, 0));
        assert_eq!(end.state().combo, 2);
    }
}
