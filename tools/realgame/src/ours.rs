//! Our side of a comparison frame: the judgement events our engine produces for the injected script, turned into
//! the renderer's live state (judged notes, judgement art, combo, hit effects) at any chart time, like the app
//! does during play.

use wacca_chart::{Chart, NoteBody};
use wacca_game::input::InputFrame;
use wacca_game::judge::{Judge, JudgeTable};
use wacca_game::rules::{JudgeEvent, Judgement};
use wacca_render::{Effects, JudgeSprite};

/// Effects are stepped at this rate (as the playfield example's snapshot does).
const STEP_S: f64 = 1.0 / 120.0;

/// Owned data behind a [`wacca_render::LiveState`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub judged: Vec<bool>,
    pub judgement: Option<(JudgeSprite, f32)>,
    /// Combo and seconds since it last changed.
    pub combo: (u32, f32),
}

impl Live {
    pub fn state<'a>(&'a self, effects: &'a Effects) -> wacca_render::LiveState<'a> {
        wacca_render::LiveState {
            judged: &self.judged,
            effects: Some(effects),
            judgement: self.judgement,
            combo: self.combo,
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

/// Judged notes, last judgement and combo at chart time `t` from the events up to `t`.
pub fn live_at(chart: &Chart, events: &[JudgeEvent], t: f64) -> Live {
    let mut live = Live {
        judged: vec![false; chart.notes.len()],
        ..Live::default()
    };
    let mut changed = f64::NEG_INFINITY;
    for e in events.iter().take_while(|e| e.time <= t) {
        if let Some(j) = live.judged.get_mut(e.note) {
            *j = true;
        }
        let combo = if e.judgement == Judgement::Miss {
            0
        } else {
            live.combo.0 + 1
        };
        if combo != live.combo.0 {
            changed = e.time;
        }
        live.combo.0 = combo;
        live.judgement = Some((sprite(e.judgement), (t - e.time) as f32));
    }
    live.combo.1 = (t - changed).min(1e3) as f32;
    live
}

/// Hit effects stepped forward through the song (call [`EffectClock::advance`] with non-decreasing times).
#[derive(Debug, Default)]
pub struct EffectClock {
    pub effects: Effects,
    t: Option<f64>,
}

impl EffectClock {
    /// Steps the effects to chart time `t`: hits for events passed, hold glows for holds in progress.
    pub fn advance(&mut self, chart: &Chart, events: &[JudgeEvent], t: f64) {
        // Start shortly before `t` the first time; effects older than that are gone anyway.
        let mut now = self.t.unwrap_or(t - 1.0);
        while now + STEP_S <= t {
            let next = now + STEP_S;
            for e in events.iter().filter(|e| e.time > now && e.time <= next) {
                if let Some(n) = chart.notes.get(e.note) {
                    self.effects
                        .hit(n.arc.lane, n.arc.width, sprite(e.judgement));
                }
            }
            let holds = chart.notes.iter().filter_map(|n| match &n.body {
                NoteBody::Hold { nodes } if n.time <= next && nodes.last()?.time > next => {
                    wacca_render::effects::hold_arc_at(nodes, next)
                }
                _ => None,
            });
            self.effects.set_holds(holds.collect());
            self.effects.update(STEP_S as f32);
            now = next;
        }
        self.t = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{Tap, expand};

    #[test]
    fn perfect_taps_give_combo_effects_and_judged_notes() {
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
        let before = live_at(&chart, &ev, 1.9);
        assert_eq!(
            (before.combo.0, before.judged.clone()),
            (0, vec![false, false])
        );
        let after_first = live_at(&chart, &ev, 2.1);
        assert_eq!(after_first.combo.0, 1);
        assert!((after_first.combo.1 - 0.1).abs() < 0.02);
        assert_eq!(after_first.judged, [true, false]);
        let (kind, age) = after_first.judgement.unwrap();
        assert_eq!(kind, JudgeSprite::Marvelous);
        assert!((age - 0.1).abs() < 0.02);
        assert_eq!(live_at(&chart, &ev, 5.0).combo.0, 2);

        let mut fx = EffectClock::default();
        fx.advance(&chart, &ev, 1.9);
        assert!(fx.effects.is_empty());
        fx.advance(&chart, &ev, 2.05);
        assert!(!fx.effects.is_empty());
        let live = live_at(&chart, &ev, 2.05);
        assert_eq!(live.state(&fx.effects).combo.0, 1);
    }
}
