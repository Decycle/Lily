//! Song list: genre folder, sort, filter, cursor and difficulty (`docs/research/05-save-and-song-select.md`
//! §3). The screens drawing it are `category` and `select_ui`.

use std::cmp::Ordering;

use wacca_chart::Difficulty;
use wacca_game::catalog::{ChartEntry, Song};
use wacca_game::grade::{Grade, Lamp};
use wacca_game::save::SaveData;

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

/// Personal bests over a folder at one difficulty, as on the category screen: songs with at
/// least each lamp / grade, out of `total` songs that have the chart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderStats {
    pub total: usize,
    /// CLEAR, Missless, Full Combo.
    pub lamps: [usize; 3],
    /// S, SS, SSS, then S+, SS+, SSS+.
    pub grades: [usize; 6],
}

pub const STAT_LAMPS: [Lamp; 3] = [Lamp::Clear, Lamp::Missless, Lamp::FullCombo];
pub const STAT_GRADES: [Grade; 6] = [
    Grade::S,
    Grade::SS,
    Grade::SSS,
    Grade::SPlus,
    Grade::SsPlus,
    Grade::SssPlus,
];

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
    /// Genre folder (`None` = all songs).
    genre: Option<i8>,
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
            wanted: Difficulty::Normal,
            genre: None,
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

    pub fn wanted(&self) -> Difficulty {
        self.wanted
    }

    /// Every song of the catalog (with a chart), in catalog order.
    pub fn songs(&self) -> &[Song] {
        &self.songs
    }

    pub fn genre(&self) -> Option<i8> {
        self.genre
    }

    /// Genres that have songs, ascending (test songs with a negative genre are left out).
    pub fn genres(&self) -> Vec<i8> {
        let mut g: Vec<i8> = self
            .songs
            .iter()
            .map(|s| s.genre)
            .filter(|&g| g >= 0)
            .collect();
        g.sort_unstable();
        g.dedup();
        g
    }

    /// Show only `genre` (or everything), keeping the current song if it is still shown.
    pub fn set_genre(&mut self, genre: Option<i8>) {
        if self.genre != genre {
            self.genre = genre;
            self.refresh();
        }
    }

    pub fn song_at(&self, i: usize) -> Option<&Song> {
        self.order.get(i).map(|&s| &self.songs[s])
    }

    /// The song `k` places from the cursor, wrapping around.
    pub fn around(&self, k: i32) -> Option<&Song> {
        let len = self.order.len() as i64;
        (len > 0)
            .then(|| (self.cursor as i64 + i64::from(k)).rem_euclid(len) as usize)
            .and_then(|i| self.song_at(i))
    }

    pub fn current(&self) -> Option<&Song> {
        self.song_at(self.cursor)
    }

    /// The selected song and the chart that would be played.
    pub fn chart(&self) -> Option<(&Song, &ChartEntry)> {
        let s = self.current()?;
        Some((s, effective(s, self.wanted)?))
    }

    /// Move the cursor by `n` songs, wrapping around.
    pub fn move_by(&mut self, n: i32) {
        let len = self.order.len() as i64;
        if len > 0 {
            self.cursor = (self.cursor as i64 + i64::from(n)).rem_euclid(len) as usize;
        }
    }

    /// Step to the next existing difficulty of the current song in direction `dir` (±1).
    pub fn shift_difficulty(&mut self, dir: i32) {
        let Some((song, entry)) = self.chart() else {
            return;
        };
        let have = sorted_difficulties(song);
        let Some(pos) = have.iter().position(|&d| d == entry.difficulty) else {
            return;
        };
        let next = pos as i64 + i64::from(dir.signum());
        if let Some(&d) = usize::try_from(next).ok().and_then(|n| have.get(n)) {
            self.set_difficulty(d);
        }
    }

    /// The DIFFICULTY button: the next existing difficulty of the current song, wrapping from
    /// the highest back to the lowest.
    pub fn cycle_difficulty(&mut self) {
        let Some((song, entry)) = self.chart() else {
            return;
        };
        let have = sorted_difficulties(song);
        let next = have
            .iter()
            .copied()
            .find(|&d| d > entry.difficulty)
            .or(have.first().copied());
        if let Some(d) = next {
            self.set_difficulty(d);
        }
    }

    /// The DIFFICULTY button with no song in view: the next difficulty, wrapping.
    pub fn cycle_wanted(&mut self) {
        let next = Difficulty::ALL.into_iter().find(|&d| d > self.wanted);
        self.set_difficulty(next.unwrap_or(Difficulty::Normal));
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

    /// Bests over the songs of `genre` at difficulty `d`.
    pub fn stats(&self, genre: i8, d: Difficulty, save: &SaveData) -> FolderStats {
        let mut st = FolderStats::default();
        for s in self.songs.iter().filter(|s| s.genre == genre) {
            if s.chart(d).is_none() {
                continue;
            }
            st.total += 1;
            let Some(r) = save.record(s.id, d).filter(|r| r.play_count > 0) else {
                continue;
            };
            for (n, &l) in st.lamps.iter_mut().zip(&STAT_LAMPS) {
                *n += usize::from(r.best_lamp >= l);
            }
            for (n, &g) in st.grades.iter_mut().zip(&STAT_GRADES) {
                *n += usize::from(r.best_grade.is_some_and(|b| b >= g));
            }
        }
        st
    }

    /// Rebuild the visible order, keeping the current song selected if still visible.
    fn refresh(&mut self) {
        let keep = self.current().map(|s| s.id);
        let wanted = self.wanted;
        let songs = &self.songs;
        let mut order: Vec<usize> = (0..songs.len())
            .filter(|&i| self.genre.is_none_or(|g| songs[i].genre == g))
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

fn sorted_difficulties(song: &Song) -> Vec<Difficulty> {
    let mut have: Vec<_> = song.charts.iter().map(|c| c.difficulty).collect();
    have.sort();
    have
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wacca_game::save::ChartRecord;

    pub fn chart(d: Difficulty, level: f32) -> ChartEntry {
        ChartEntry {
            difficulty: d,
            level,
            norma: 0.8,
            movie: None,
            chart_path: String::new(),
        }
    }

    pub fn song(id: u32, reading: &str, version: u8, charts: Vec<ChartEntry>) -> Song {
        Song {
            id,
            folder: format!("S00-{id:03}"),
            title: reading.to_uppercase(),
            artist: format!("artist {id}"),
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

    pub fn list() -> SongList {
        use Difficulty::*;
        SongList::new(vec![
            song(1, "c", 1, vec![chart(Normal, 3.0), chart(Expert, 12.0)]),
            song(
                2,
                "a",
                3,
                vec![chart(Hard, 8.0), chart(Expert, 13.7), chart(Inferno, 14.0)],
            ),
            Song {
                genre: 2,
                ..song(3, "b", 2, vec![chart(Expert, 11.0)])
            },
            song(4, "z", 5, vec![]),
        ])
    }

    fn ids(l: &SongList) -> Vec<u32> {
        (0..l.len()).map(|i| l.song_at(i).unwrap().id).collect()
    }

    #[test]
    fn sorts_and_drops_chartless_songs() {
        let mut l = list();
        l.set_difficulty(Difficulty::Expert);
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
        l.set_difficulty(Difficulty::Expert);
        l.move_by(-1);
        assert_eq!(l.current().unwrap().id, 1);
        l.move_by(2);
        assert_eq!(l.current().unwrap().id, 3);
        assert_eq!(l.around(1).unwrap().id, 1);
        assert_eq!(l.around(-4).unwrap().id, 2);
        l.next_sort(); // level: [3, 1, 2]
        assert_eq!((l.cursor(), l.current().unwrap().id), (0, 3));
    }

    #[test]
    fn genre_folders() {
        let mut l = list();
        assert_eq!(l.wanted(), Difficulty::Normal);
        l.cycle_wanted();
        assert_eq!(l.wanted(), Difficulty::Hard);
        l.set_difficulty(Difficulty::Inferno);
        l.cycle_wanted();
        assert_eq!(l.wanted(), Difficulty::Normal);
        assert_eq!(l.genres(), [0, 2]);
        l.set_genre(Some(0));
        assert_eq!(ids(&l), [2, 1]);
        l.set_genre(Some(2));
        assert_eq!((ids(&l), l.cursor()), (vec![3], 0));
        l.set_genre(None);
        assert_eq!(ids(&l), [2, 3, 1]);
        assert_eq!(l.current().unwrap().id, 3);
        assert!(SongList::new(vec![]).around(0).is_none());
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
    fn difficulty_button_cycles_existing_charts() {
        let mut l = list(); // song 2: Hard, Expert, Inferno
        l.set_difficulty(Difficulty::Expert);
        l.cycle_difficulty();
        assert_eq!(l.wanted(), Difficulty::Inferno);
        l.cycle_difficulty();
        assert_eq!(l.wanted(), Difficulty::Hard);
        l.move_by(2); // song 1: Normal, Expert; Hard falls back to Expert
        l.cycle_difficulty();
        assert_eq!(l.wanted(), Difficulty::Normal);
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

    #[test]
    fn folder_stats_count_bests_at_least_each_lamp_and_grade() {
        let l = list();
        let mut save = SaveData::default();
        let fc = ChartRecord {
            best_lamp: Lamp::FullCombo,
            best_grade: Some(Grade::SsPlus),
            play_count: 1,
            ..ChartRecord::default()
        };
        save.records
            .insert(wacca_game::save::chart_key(2, Difficulty::Expert), fc);
        let st = l.stats(0, Difficulty::Expert, &save);
        assert_eq!(st.total, 2);
        assert_eq!(st.lamps, [1, 1, 1]);
        assert_eq!(st.grades, [1, 1, 0, 1, 1, 0]);
        assert_eq!(l.stats(0, Difficulty::Normal, &save).total, 1);
        assert_eq!(l.stats(2, Difficulty::Expert, &save).lamps, [0; 3]);
    }
}
