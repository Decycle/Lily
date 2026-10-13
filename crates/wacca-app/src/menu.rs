//! What each menu action does on each screen (pure): updates the list / options and returns
//! the screen [`Event`] it causes, if any.

use wacca_game::save::Settings;

use crate::calibrate::Calibration;
use crate::options::{self, OptionsMenu, Row};
use crate::screen::{Event, Screen};
use crate::select::SongList;
use crate::ui::Action;

/// Menu state that survives between plays.
#[derive(Debug)]
pub struct Menu {
    pub list: SongList,
    /// Index into [`SongList::genres`] on the category screen.
    pub category: usize,
    pub options: OptionsMenu,
    pub settings: Settings,
    pub autoplay: bool,
    /// Taps of the timing check (restarted each time it opens).
    pub calibration: Calibration,
}

impl Menu {
    /// Move the category cursor by `n` (wrapping); `open` also shows that genre's songs.
    fn move_category(&mut self, n: i32, open: bool) {
        let genres = self.list.genres();
        if genres.is_empty() {
            return;
        }
        let len = genres.len() as i64;
        self.category = (self.category as i64 + i64::from(n)).rem_euclid(len) as usize;
        if open {
            self.list.set_genre(Some(genres[self.category]));
        }
    }

    /// The options row `action` would change, if that row is the timing check.
    fn opens_check(&self, action: Action) -> bool {
        let row = match action {
            Action::Left | Action::Right => self.options.row,
            Action::Pick(n) => n / 2,
            _ => return false,
        };
        options::ROWS.get(row) == Some(&Row::Check)
    }

    pub fn act(&mut self, screen: Screen, action: Action) -> Option<Event> {
        use Action::*;
        if screen == Screen::Options && self.opens_check(action) {
            if let Pick(n) = action {
                self.options.row = n / 2;
            }
            self.calibration = Calibration::default();
            return Some(Event::Calibrate);
        }
        let list = &mut self.list;
        match (screen, action) {
            (_, Back) => return Some(Event::Back),
            (Screen::Title | Screen::ModeSelect | Screen::Results, Confirm) => {
                return Some(Event::Confirm);
            }
            (Screen::Results, Retry) => return Some(Event::Retry),
            (Screen::Category, Confirm) => {
                let empty = list.genres().is_empty();
                self.move_category(0, true);
                return (!empty).then_some(Event::Confirm);
            }
            (Screen::SongSelect, Confirm) => {
                return list.chart().is_some().then_some(Event::Confirm);
            }
            (Screen::Options, Confirm) => return Some(Event::Confirm),
            // APPLY: only once there is a suggestion.
            (Screen::Calibrate, Confirm) => {
                let o = self.calibration.suggestion()?;
                self.settings.judge_offset = o;
                return Some(Event::Confirm);
            }

            (Screen::Category, Left) => self.move_category(-1, false),
            (Screen::Category, Right) => self.move_category(1, false),
            (Screen::Category, Scroll(n)) => self.move_category(n, false),
            (Screen::Category, Action::Difficulty) => list.cycle_wanted(),

            (Screen::SongSelect, Left) => list.move_by(-1),
            (Screen::SongSelect, Right) => list.move_by(1),
            (Screen::SongSelect, Scroll(n)) => list.move_by(n),
            (Screen::SongSelect, Action::Difficulty) => list.cycle_difficulty(),
            (Screen::SongSelect, Genre(n)) => self.move_category(n, true),
            (Screen::Category | Screen::SongSelect, Up) => list.shift_difficulty(1),
            (Screen::Category | Screen::SongSelect, Down) => list.shift_difficulty(-1),
            (Screen::Category | Screen::SongSelect, NextSort) => list.next_sort(),
            (Screen::Category | Screen::SongSelect, ToggleFilter) => list.toggle_filter(),

            (Screen::Options, Up) => self.options.move_by(-1),
            (Screen::Options, Down) => self.options.move_by(1),
            (Screen::Options, Left) => {
                self.options
                    .adjust(&mut self.settings, &mut self.autoplay, -1)
            }
            (Screen::Options, Right) => {
                self.options
                    .adjust(&mut self.settings, &mut self.autoplay, 1)
            }
            (Screen::Options, Pick(n)) => {
                options::pick(&mut self.options, n, &mut self.settings, &mut self.autoplay)
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacca_chart::Difficulty;
    use wacca_game::catalog::{ChartEntry, Song};

    fn song(id: u32, reading: &str, ds: &[Difficulty]) -> Song {
        Song {
            id,
            folder: String::new(),
            title: reading.into(),
            artist: String::new(),
            reading: reading.into(),
            bpm: String::new(),
            genre: 0,
            version: 1,
            jacket: String::new(),
            preview_begin: 0.0,
            preview_seconds: 0.0,
            charts: ds
                .iter()
                .map(|&difficulty| ChartEntry {
                    difficulty,
                    level: 10.0,
                    norma: 0.8,
                    movie: None,
                    chart_path: String::new(),
                })
                .collect(),
        }
    }

    fn menu() -> Menu {
        use Difficulty::*;
        Menu {
            list: SongList::new(vec![
                song(1, "a", &[Normal, Expert]),
                song(2, "b", &[Hard, Expert, Inferno]),
                Song {
                    genre: 3,
                    ..song(3, "c", &[Normal])
                },
            ]),
            category: 0,
            options: OptionsMenu::default(),
            settings: Settings::default(),
            autoplay: false,
            calibration: Calibration::default(),
        }
    }

    #[test]
    fn category_wheel_opens_a_genre() {
        let mut m = menu(); // genres [0, 3]
        assert_eq!(m.act(Screen::Category, Action::Right), None);
        assert_eq!(m.category, 1);
        assert_eq!(m.act(Screen::Category, Action::Scroll(-3)), None);
        assert_eq!(m.category, 0);
        assert_eq!(
            m.act(Screen::Category, Action::Confirm),
            Some(Event::Confirm)
        );
        assert_eq!((m.list.genre(), m.list.len()), (Some(0), 2));
        m.act(Screen::Category, Action::Difficulty); // Normal → Hard → Expert
        m.act(Screen::Category, Action::Difficulty);
        assert_eq!(m.list.wanted(), Difficulty::Expert);
        assert_eq!(m.act(Screen::Category, Action::Back), Some(Event::Back));
    }

    #[test]
    fn song_select_wheel_difficulty_and_genre_jump() {
        let mut m = menu();
        m.list.set_difficulty(Difficulty::Expert);
        m.act(Screen::Category, Action::Confirm);
        assert_eq!(m.list.current().unwrap().id, 1);
        assert_eq!(m.act(Screen::SongSelect, Action::Right), None);
        assert_eq!(m.list.current().unwrap().id, 2);
        assert_eq!(m.act(Screen::SongSelect, Action::Scroll(3)), None);
        assert_eq!(m.list.current().unwrap().id, 1);
        m.act(Screen::SongSelect, Action::Left);
        m.act(Screen::SongSelect, Action::Difficulty); // song 2: Expert → Inferno
        assert_eq!(m.list.wanted(), Difficulty::Inferno);
        m.act(Screen::SongSelect, Action::Down);
        assert_eq!(m.list.wanted(), Difficulty::Expert);
        m.act(Screen::SongSelect, Action::Genre(1));
        assert_eq!((m.category, m.list.genre()), (1, Some(3)));
        assert_eq!(
            m.act(Screen::SongSelect, Action::Confirm),
            Some(Event::Confirm)
        );
        assert_eq!(m.act(Screen::SongSelect, Action::Back), Some(Event::Back));
    }

    #[test]
    fn options_and_results() {
        let mut m = menu();
        m.act(Screen::Options, Action::Right);
        assert_eq!(m.settings.note_speed, 4.5);
        // Wraps to the last row (touch guide), then up past bar lines and video to autoplay.
        for _ in 0..4 {
            m.act(Screen::Options, Action::Up);
        }
        m.act(Screen::Options, Action::Left);
        assert!(m.autoplay);
        assert_eq!(
            m.act(Screen::Options, Action::Confirm),
            Some(Event::Confirm)
        );
        assert_eq!(m.act(Screen::Results, Action::Retry), Some(Event::Retry));
        for screen in [Screen::Title, Screen::ModeSelect] {
            assert_eq!(m.act(screen, Action::Confirm), Some(Event::Confirm));
            assert_eq!(m.act(screen, Action::Right), None);
        }
        assert_eq!(
            m.act(Screen::Results, Action::Confirm),
            Some(Event::Confirm)
        );
    }

    #[test]
    fn timing_check_opens_from_its_row_and_applies() {
        use crate::calibrate::{LEAD_IN, PERIOD};
        let mut m = menu();
        m.act(Screen::Options, Action::Down);
        assert_eq!(m.act(Screen::Options, Action::Right), None); // TIMING +0.1
        m.calibration.tap(LEAD_IN);
        m.act(Screen::Options, Action::Down);
        assert_eq!(m.act(Screen::Options, Action::Left), Some(Event::Calibrate));
        assert!(m.calibration.errors().is_empty());
        // Not enough taps yet: APPLY does nothing; Back leaves the setting alone.
        assert_eq!(m.act(Screen::Calibrate, Action::Confirm), None);
        for k in 0..16 {
            m.calibration.tap(LEAD_IN + k as f64 * PERIOD - 0.0125);
        }
        assert_eq!(m.act(Screen::Calibrate, Action::Back), Some(Event::Back));
        assert_eq!(m.settings.judge_offset, 0.1);
        assert_eq!(
            m.act(Screen::Calibrate, Action::Confirm),
            Some(Event::Confirm)
        );
        assert_eq!(m.settings.judge_offset, 1.5);
        // Tapping the row (either half) opens it too.
        m.options.row = 0;
        assert_eq!(
            m.act(Screen::Options, Action::Pick(5)),
            Some(Event::Calibrate)
        );
        assert_eq!(m.options.row, 2);
    }

    #[test]
    fn empty_list_cannot_confirm() {
        let mut m = menu();
        m.list = SongList::new(vec![]);
        assert_eq!(m.act(Screen::SongSelect, Action::Confirm), None);
    }
}
