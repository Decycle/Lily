//! What each menu action does on each screen (pure): updates the list / options and returns
//! the screen [`Event`] it causes, if any.

use wacca_chart::Difficulty;
use wacca_game::save::Settings;

use crate::options::{self, OptionsMenu};
use crate::screen::{Event, Screen};
use crate::select::SongList;
use crate::ui::Action;

/// Menu state that survives between plays.
#[derive(Debug)]
pub struct Menu {
    pub list: SongList,
    pub options: OptionsMenu,
    pub settings: Settings,
    pub autoplay: bool,
}

impl Menu {
    pub fn act(&mut self, screen: Screen, action: Action) -> Option<Event> {
        use Action::*;
        let list = &mut self.list;
        match (screen, action) {
            (_, Back) => return Some(Event::Back),
            (Screen::Results, Confirm) => return Some(Event::Confirm),
            (Screen::Results, Retry) => return Some(Event::Retry),
            (Screen::SongSelect | Screen::Difficulty, Confirm) => {
                return list.chart().is_some().then_some(Event::Confirm);
            }
            (Screen::Options, Confirm) => return Some(Event::Confirm),

            (Screen::SongSelect, Up) => list.move_by(-1),
            (Screen::SongSelect, Down) => list.move_by(1),
            (Screen::SongSelect, Scroll(n)) => list.move_by(n),
            (Screen::SongSelect, Pick(i)) if i == list.cursor() => {
                return self.act(screen, Confirm);
            }
            (Screen::SongSelect, Pick(i)) => list.select(i),
            (Screen::SongSelect, NextSort) => list.next_sort(),
            (Screen::SongSelect, ToggleFilter) => list.toggle_filter(),
            (Screen::SongSelect | Screen::Difficulty, Left) => list.shift_difficulty(-1),
            (Screen::SongSelect | Screen::Difficulty, Right) => list.shift_difficulty(1),
            (Screen::Difficulty, Up) => list.shift_difficulty(-1),
            (Screen::Difficulty, Down) => list.shift_difficulty(1),
            (Screen::Difficulty, Pick(k)) => {
                let d = Difficulty::ALL.get(k).copied()?;
                if list.chart().is_some_and(|(_, c)| c.difficulty == d) {
                    return self.act(screen, Confirm);
                }
                if list.current().is_some_and(|s| s.chart(d).is_some()) {
                    list.set_difficulty(d);
                }
            }

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
            ]),
            options: OptionsMenu::default(),
            settings: Settings::default(),
            autoplay: false,
        }
    }

    #[test]
    fn song_select_navigation_and_tap_to_confirm() {
        let mut m = menu();
        assert_eq!(m.act(Screen::SongSelect, Action::Down), None);
        assert_eq!(m.list.current().unwrap().id, 2);
        assert_eq!(m.act(Screen::SongSelect, Action::Pick(0)), None);
        assert_eq!(m.list.current().unwrap().id, 1);
        // Tapping the selected row confirms.
        assert_eq!(
            m.act(Screen::SongSelect, Action::Pick(0)),
            Some(Event::Confirm)
        );
        assert_eq!(m.act(Screen::SongSelect, Action::Scroll(3)), None);
        assert_eq!(m.list.current().unwrap().id, 2);
        assert_eq!(m.act(Screen::SongSelect, Action::Back), Some(Event::Back));
    }

    #[test]
    fn difficulty_slots() {
        let mut m = menu(); // song 1: Normal, Expert; wanted Expert
        assert_eq!(m.act(Screen::Difficulty, Action::Pick(1)), None); // no Hard chart
        assert_eq!(m.list.wanted(), Difficulty::Expert);
        assert_eq!(m.act(Screen::Difficulty, Action::Pick(0)), None);
        assert_eq!(m.list.wanted(), Difficulty::Normal);
        assert_eq!(
            m.act(Screen::Difficulty, Action::Pick(0)),
            Some(Event::Confirm)
        );
        m.act(Screen::Difficulty, Action::Right);
        assert_eq!(m.list.wanted(), Difficulty::Expert);
        assert_eq!(m.act(Screen::Difficulty, Action::Pick(9)), None);
    }

    #[test]
    fn options_and_results() {
        let mut m = menu();
        m.act(Screen::Options, Action::Right);
        assert_eq!(m.settings.note_speed, 4.5);
        m.act(Screen::Options, Action::Up); // wraps to autoplay
        m.act(Screen::Options, Action::Left);
        assert!(m.autoplay);
        assert_eq!(
            m.act(Screen::Options, Action::Confirm),
            Some(Event::Confirm)
        );
        assert_eq!(m.act(Screen::Results, Action::Retry), Some(Event::Retry));
        assert_eq!(
            m.act(Screen::Results, Action::Confirm),
            Some(Event::Confirm)
        );
    }

    #[test]
    fn empty_list_cannot_confirm() {
        let mut m = menu();
        m.list = SongList::new(vec![]);
        assert_eq!(m.act(Screen::SongSelect, Action::Confirm), None);
    }
}
