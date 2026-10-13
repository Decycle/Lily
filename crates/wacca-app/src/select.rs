//! Song list (sort, filter, cursor, difficulty) and the song-select / difficulty screens
//! (`docs/research/05-save-and-song-select.md` §3).

use std::cmp::Ordering;

use wacca_chart::Difficulty;
use wacca_game::catalog::{ChartEntry, Song};
use wacca_game::save::SaveData;
use wacca_render::layout::Rect;
use wacca_render::text::Align;

use crate::labels;
use crate::ui::{Action, GREY, HIGHLIGHT, PANEL, View, WHITE, row};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// By reading (kana), then title.
    Title,
    /// By level of the selected difficulty.
    Level,
    /// Newest version first.
    Version,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Title => Sort::Level,
            Sort::Level => Sort::Version,
            Sort::Version => Sort::Title,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Title => "TITLE",
            Sort::Level => "LEVEL",
            Sort::Version => "VERSION",
        }
    }
}

/// The chart played for `wanted`: that difficulty if the song has it, else its highest (doc 05 §3.4).
pub fn effective(song: &Song, wanted: Difficulty) -> Option<&ChartEntry> {
    song.chart(wanted)
        .or_else(|| song.charts.iter().max_by_key(|c| c.difficulty))
}

#[derive(Debug)]
pub struct SongList {
    songs: Vec<Song>,
    /// Visible songs (indices into `songs`) in display order.
    order: Vec<usize>,
    cursor: usize,
    sort: Sort,
    /// Show only songs that have the wanted difficulty.
    only_available: bool,
    wanted: Difficulty,
}

impl SongList {
    /// Songs without any chart are dropped (doc 05 §3.5).
    pub fn new(mut songs: Vec<Song>) -> Self {
        songs.retain(|s| !s.charts.is_empty());
        let mut list = Self {
            songs,
            order: Vec::new(),
            cursor: 0,
            sort: Sort::Title,
            only_available: false,
            wanted: Difficulty::Expert,
        };
        list.refresh();
        list
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn only_available(&self) -> bool {
        self.only_available
    }

    pub fn wanted(&self) -> Difficulty {
        self.wanted
    }

    /// Every song of the catalog (with a chart), in catalog order.
    pub fn songs(&self) -> &[Song] {
        &self.songs
    }

    pub fn song_at(&self, i: usize) -> Option<&Song> {
        self.order.get(i).map(|&s| &self.songs[s])
    }

    pub fn current(&self) -> Option<&Song> {
        self.song_at(self.cursor)
    }

    /// The selected song and the chart that would be played.
    pub fn chart(&self) -> Option<(&Song, &ChartEntry)> {
        let s = self.current()?;
        Some((s, effective(s, self.wanted)?))
    }

    /// Move the cursor by `n` rows, wrapping around.
    pub fn move_by(&mut self, n: i32) {
        let len = self.order.len() as i64;
        if len > 0 {
            self.cursor = (self.cursor as i64 + i64::from(n)).rem_euclid(len) as usize;
        }
    }

    pub fn select(&mut self, i: usize) {
        if i < self.order.len() {
            self.cursor = i;
        }
    }

    /// Step to the next existing difficulty of the current song in direction `dir` (±1).
    pub fn shift_difficulty(&mut self, dir: i32) {
        let Some((song, entry)) = self.chart() else {
            return;
        };
        let mut have: Vec<_> = song.charts.iter().map(|c| c.difficulty).collect();
        have.sort();
        let Some(pos) = have.iter().position(|&d| d == entry.difficulty) else {
            return;
        };
        let next = pos as i64 + i64::from(dir.signum());
        if let Some(&d) = usize::try_from(next).ok().and_then(|n| have.get(n)) {
            self.set_difficulty(d);
        }
    }

    /// Set the wanted difficulty (kept when changing songs).
    pub fn set_difficulty(&mut self, d: Difficulty) {
        self.wanted = d;
        if self.only_available || self.sort == Sort::Level {
            self.refresh();
        }
    }

    pub fn next_sort(&mut self) {
        self.sort = self.sort.next();
        self.refresh();
    }

    pub fn toggle_filter(&mut self) {
        self.only_available = !self.only_available;
        self.refresh();
    }

    /// Rebuild the visible order, keeping the current song selected if still visible.
    fn refresh(&mut self) {
        let keep = self.current().map(|s| s.id);
        let wanted = self.wanted;
        let songs = &self.songs;
        let mut order: Vec<usize> = (0..songs.len())
            .filter(|&i| !self.only_available || songs[i].chart(wanted).is_some())
            .collect();
        let level = |s: &Song| effective(s, wanted).map_or(0.0, |c| c.level);
        let by_title = |a: &Song, b: &Song| {
            a.reading
                .cmp(&b.reading)
                .then_with(|| a.title.cmp(&b.title))
                .then(a.id.cmp(&b.id))
        };
        order.sort_by(|&a, &b| {
            let (a, b) = (&songs[a], &songs[b]);
            match self.sort {
                Sort::Title => by_title(a, b),
                Sort::Level => level(a)
                    .partial_cmp(&level(b))
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| by_title(a, b)),
                Sort::Version => b.version.cmp(&a.version).then(a.id.cmp(&b.id)),
            }
        });
        self.cursor = keep
            .and_then(|id| order.iter().position(|&i| songs[i].id == id))
            .unwrap_or(0);
        self.order = order;
    }
}

/// Rows shown in the song list.
const LIST_ROWS: usize = 9;

/// Song-select screen inside the 9:16 `area`.
pub fn select_view(list: &SongList, save: &SaveData, area: Rect) -> View {
    let u = area.w / 100.0;
    let mut v = View::default();
    let cx = area.x + area.w / 2.0;
    v.text_in(
        "SONG SELECT",
        [cx, area.y + 3.0 * u],
        6.0 * u,
        WHITE,
        Align::Center,
        None,
    );

    // Sort / filter buttons.
    let bar = |n: f32| Rect {
        x: area.x + 3.0 * u + n * 47.0 * u,
        y: area.y + 12.0 * u,
        w: 46.0 * u,
        h: 8.0 * u,
    };
    let filter = if list.only_available() {
        labels::difficulty(list.wanted())
    } else {
        "ALL"
    };
    for (n, text, action) in [
        (
            0.0,
            format!("SORT: {}", list.sort().label()),
            Action::NextSort,
        ),
        (1.0, format!("SHOW: {filter}"), Action::ToggleFilter),
    ] {
        let r = bar(n);
        v.fill(r, PANEL);
        v.text_in(
            text,
            [r.x + r.w / 2.0, r.y + 2.0 * u],
            4.0 * u,
            WHITE,
            Align::Center,
            None,
        );
        v.button(r, action);
    }

    // Difficulty: ◀ NAME ▶.
    let d = list.chart().map_or(list.wanted(), |(_, c)| c.difficulty);
    let dy = area.y + 22.0 * u;
    v.text_in(
        format!("◀  {}  ▶", labels::difficulty(d)),
        [cx, dy],
        5.5 * u,
        labels::difficulty_color(d),
        Align::Center,
        None,
    );
    let half = |x| Rect {
        x,
        y: dy,
        w: area.w / 2.0,
        h: 8.0 * u,
    };
    v.button(half(area.x), Action::Left);
    v.button(half(cx), Action::Right);

    // List, centred on the cursor and wrapping when it is longer than the screen.
    let top = area.y + 32.0 * u;
    let h = 12.0 * u;
    let n = list.len();
    if n == 0 {
        v.text_in(
            "NO SONGS",
            [cx, top + h],
            5.0 * u,
            GREY,
            Align::Center,
            None,
        );
    }
    let shown = n.min(LIST_ROWS);
    let first = if n > LIST_ROWS {
        (list.cursor() + n - LIST_ROWS / 2) % n
    } else {
        0
    };
    for k in 0..shown {
        let i = (first + k) % n;
        let Some(song) = list.song_at(i) else {
            continue;
        };
        let r = row(area, top, h, k);
        v.fill(r, if i == list.cursor() { HIGHLIGHT } else { PANEL });
        let j = Rect {
            x: r.x + 0.5 * u,
            y: r.y + 0.5 * u,
            w: r.h - u,
            h: r.h - u,
        };
        v.items.push(crate::ui::Item::Jacket {
            rect: j,
            key: song.jacket.clone(),
        });
        let tx = j.x + j.w + 2.0 * u;
        let text_w = r.x + r.w - tx - 16.0 * u;
        v.text_in(
            &song.title,
            [tx, r.y + 1.0 * u],
            4.2 * u,
            WHITE,
            Align::Left,
            Some(text_w),
        );
        if let Some(c) = effective(song, list.wanted()) {
            v.text_in(
                labels::record(save.record(song.id, c.difficulty)),
                [tx, r.y + 6.2 * u],
                3.2 * u,
                GREY,
                Align::Left,
                Some(text_w),
            );
            v.text_in(
                labels::level(c.level),
                [r.x + r.w - 2.0 * u, r.y + 2.5 * u],
                6.0 * u,
                labels::difficulty_color(c.difficulty),
                Align::Right,
                None,
            );
        }
        v.button(r, Action::Pick(i));
    }

    let foot = area.y + area.h - 7.0 * u;
    v.text_in(
        "↑↓ song   ←→ difficulty   Enter select   Esc quit",
        [cx, foot],
        3.0 * u,
        GREY,
        Align::Center,
        Some(area.w - 4.0 * u),
    );
    v
}

/// Difficulty screen: the four slots of the current song.
pub fn difficulty_view(list: &SongList, save: &SaveData, area: Rect) -> View {
    let u = area.w / 100.0;
    let mut v = View::default();
    let cx = area.x + area.w / 2.0;
    let Some((song, chosen)) = list.chart() else {
        return v;
    };
    let j = Rect {
        x: cx - 25.0 * u,
        y: area.y + 6.0 * u,
        w: 50.0 * u,
        h: 50.0 * u,
    };
    v.items.push(crate::ui::Item::Jacket {
        rect: j,
        key: song.jacket.clone(),
    });
    let max = Some(area.w - 6.0 * u);
    v.text_in(
        &song.title,
        [cx, area.y + 60.0 * u],
        6.0 * u,
        WHITE,
        Align::Center,
        max,
    );
    v.text_in(
        &song.artist,
        [cx, area.y + 68.0 * u],
        4.0 * u,
        GREY,
        Align::Center,
        max,
    );
    v.text_in(
        format!("BPM {}", song.bpm),
        [cx, area.y + 74.0 * u],
        3.5 * u,
        GREY,
        Align::Center,
        None,
    );
    let top = area.y + 82.0 * u;
    let h = 14.0 * u;
    for (k, d) in Difficulty::ALL.into_iter().enumerate() {
        let Some(c) = song.chart(d) else { continue };
        let r = row(area, top, h, k);
        v.fill(
            r,
            if d == chosen.difficulty {
                HIGHLIGHT
            } else {
                PANEL
            },
        );
        let color = labels::difficulty_color(d);
        v.fill(Rect { w: 1.5 * u, ..r }, color);
        v.text(
            labels::difficulty(d),
            [r.x + 4.0 * u, r.y + 1.5 * u],
            5.0 * u,
            color,
        );
        v.text(
            labels::record(save.record(song.id, d)),
            [r.x + 4.0 * u, r.y + 8.0 * u],
            3.5 * u,
            GREY,
        );
        v.text_in(
            format!("{} ({:.1})", labels::level(c.level), c.level),
            [r.x + r.w - 2.0 * u, r.y + 3.0 * u],
            6.0 * u,
            color,
            Align::Right,
            None,
        );
        v.button(r, Action::Pick(k));
    }
    buttons(&mut v, area, "BACK", "NEXT");
    v
}

/// Back / confirm buttons at the bottom of `area`.
pub fn buttons(v: &mut View, area: Rect, back: &str, confirm: &str) {
    let u = area.w / 100.0;
    let y = area.y + area.h - 16.0 * u;
    for (n, text, action) in [(0.0, back, Action::Back), (1.0, confirm, Action::Confirm)] {
        let r = Rect {
            x: area.x + 4.0 * u + n * 48.0 * u,
            y,
            w: 44.0 * u,
            h: 11.0 * u,
        };
        v.fill(r, if n > 0.0 { HIGHLIGHT } else { PANEL });
        v.text_in(
            text,
            [r.x + r.w / 2.0, r.y + 3.0 * u],
            5.0 * u,
            WHITE,
            Align::Center,
            None,
        );
        v.button(r, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(d: Difficulty, level: f32) -> ChartEntry {
        ChartEntry {
            difficulty: d,
            level,
            norma: 0.8,
            movie: None,
            chart_path: String::new(),
        }
    }

    fn song(id: u32, reading: &str, version: u8, charts: Vec<ChartEntry>) -> Song {
        Song {
            id,
            folder: format!("S00-{id:03}"),
            title: reading.to_uppercase(),
            artist: String::new(),
            reading: reading.into(),
            bpm: "150".into(),
            genre: 0,
            version,
            jacket: format!("J{id}"),
            preview_begin: 0.0,
            preview_seconds: 10.0,
            charts,
        }
    }

    fn list() -> SongList {
        use Difficulty::*;
        SongList::new(vec![
            song(1, "c", 1, vec![chart(Normal, 3.0), chart(Expert, 12.0)]),
            song(
                2,
                "a",
                3,
                vec![chart(Hard, 8.0), chart(Expert, 13.7), chart(Inferno, 14.0)],
            ),
            song(3, "b", 2, vec![chart(Expert, 11.0)]),
            song(4, "z", 5, vec![]),
        ])
    }

    fn ids(l: &SongList) -> Vec<u32> {
        (0..l.len()).map(|i| l.song_at(i).unwrap().id).collect()
    }

    #[test]
    fn sorts_and_drops_chartless_songs() {
        let mut l = list();
        assert_eq!(ids(&l), [2, 3, 1]);
        l.next_sort();
        assert_eq!(l.sort(), Sort::Level);
        assert_eq!(ids(&l), [3, 1, 2]);
        l.next_sort();
        assert_eq!(ids(&l), [2, 3, 1]); // version 3, 2, 1
        l.next_sort();
        assert_eq!(l.sort(), Sort::Title);
    }

    #[test]
    fn cursor_wraps_and_survives_resorting() {
        let mut l = list();
        l.move_by(-1);
        assert_eq!(l.current().unwrap().id, 1);
        l.move_by(2);
        assert_eq!(l.current().unwrap().id, 3);
        l.next_sort(); // level: [3, 1, 2]
        assert_eq!((l.cursor(), l.current().unwrap().id), (0, 3));
        l.select(2);
        assert_eq!(l.current().unwrap().id, 2);
        l.select(99);
        assert_eq!(l.current().unwrap().id, 2);
    }

    #[test]
    fn difficulty_falls_back_and_shifts_over_existing_charts() {
        let mut l = list();
        l.set_difficulty(Difficulty::Inferno);
        // Song 2 has Inferno; song 3 falls back to its highest (Expert).
        assert_eq!(l.chart().unwrap().1.difficulty, Difficulty::Inferno);
        l.move_by(1);
        assert_eq!(l.chart().unwrap().1.difficulty, Difficulty::Expert);
        assert_eq!(l.wanted(), Difficulty::Inferno);
        l.move_by(-1);
        l.shift_difficulty(-1);
        assert_eq!(l.wanted(), Difficulty::Expert);
        l.shift_difficulty(-1);
        assert_eq!(l.wanted(), Difficulty::Hard);
        l.shift_difficulty(-1); // no lower chart: unchanged
        assert_eq!(l.wanted(), Difficulty::Hard);
        l.shift_difficulty(1);
        assert_eq!(l.wanted(), Difficulty::Expert);
    }

    #[test]
    fn filter_shows_only_songs_with_the_difficulty() {
        let mut l = list();
        l.set_difficulty(Difficulty::Inferno);
        l.toggle_filter();
        assert_eq!(ids(&l), [2]);
        l.set_difficulty(Difficulty::Normal);
        assert_eq!(ids(&l), [1]);
        assert_eq!(l.current().unwrap().id, 1);
        l.toggle_filter();
        assert_eq!(ids(&l), [2, 3, 1]);
        assert_eq!(l.current().unwrap().id, 1);
    }

    fn area() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 900.0,
            h: 1600.0,
        }
    }

    #[test]
    fn select_view_lists_songs_with_pick_buttons() {
        let l = list();
        let v = select_view(&l, &SaveData::default(), area());
        let texts = v.texts();
        assert!(texts.contains(&"A") && texts.contains(&"13+") && texts.contains(&"NO PLAY"));
        assert!(texts.contains(&"SORT: TITLE"));
        let picks = v
            .buttons
            .iter()
            .filter(|(_, a)| matches!(a, Action::Pick(_)))
            .count();
        assert_eq!(picks, 3);
        // The first row's centre picks song index 0.
        let (r, _) = v
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Pick(0))
            .unwrap();
        assert_eq!(v.hit([r.x + 5.0, r.y + 5.0]), Some(Action::Pick(0)));
    }

    #[test]
    fn difficulty_view_has_existing_slots_only() {
        let l = list(); // song 2: Hard, Expert, Inferno
        let v = difficulty_view(&l, &SaveData::default(), area());
        let texts = v.texts();
        assert!(texts.contains(&"HARD") && texts.contains(&"INFERNO"));
        assert!(!texts.contains(&"NORMAL"));
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Pick(3)));
        assert!(!v.buttons.iter().any(|(_, a)| *a == Action::Pick(0)));
        assert!(v.buttons.iter().any(|(_, a)| *a == Action::Confirm));
    }
}
