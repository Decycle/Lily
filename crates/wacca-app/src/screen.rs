//! The core loop as a state machine, following the original:
//! Title → ModeSelect → Category (genre folder) → SongSelect (song + difficulty) → Options → Play
//! → Results → SongSelect.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    ModeSelect,
    Category,
    SongSelect,
    Options,
    Play,
    Results,
}

/// Things that move between screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Confirm,
    Back,
    /// The song ended (played out or given up).
    PlayOver,
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Stay,
    Go(Screen),
    Quit,
}

pub fn next(screen: Screen, event: Event) -> Next {
    use Event::*;
    use Screen::*;
    match (screen, event) {
        (Title, Confirm) => Next::Go(ModeSelect),
        (Title, Back) => Next::Quit,
        (ModeSelect, Confirm) => Next::Go(Category),
        (ModeSelect, Back) => Next::Go(Title),
        (Category, Confirm) => Next::Go(SongSelect),
        (Category, Back) => Next::Go(ModeSelect),
        (SongSelect, Confirm) => Next::Go(Options),
        (SongSelect, Back) => Next::Go(Category),
        (Options, Confirm) => Next::Go(Play),
        (Options, Back) => Next::Go(SongSelect),
        (Play, PlayOver) => Next::Go(Results),
        (Results, Confirm | Back) => Next::Go(SongSelect),
        (Results, Retry) => Next::Go(Play),
        _ => Next::Stay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_loop() {
        let mut s = Screen::Title;
        for e in [
            Event::Confirm,
            Event::Confirm,
            Event::Confirm,
            Event::Confirm,
            Event::Confirm,
            Event::PlayOver,
        ] {
            let Next::Go(n) = next(s, e) else {
                panic!("{s:?} {e:?}")
            };
            s = n;
        }
        assert_eq!(s, Screen::Results);
        assert_eq!(next(s, Event::Confirm), Next::Go(Screen::SongSelect));
        assert_eq!(next(s, Event::Retry), Next::Go(Screen::Play));
    }

    #[test]
    fn back_walks_up_and_quits_from_the_title() {
        assert_eq!(
            next(Screen::Options, Event::Back),
            Next::Go(Screen::SongSelect)
        );
        assert_eq!(
            next(Screen::SongSelect, Event::Back),
            Next::Go(Screen::Category)
        );
        assert_eq!(
            next(Screen::Category, Event::Back),
            Next::Go(Screen::ModeSelect)
        );
        assert_eq!(
            next(Screen::ModeSelect, Event::Back),
            Next::Go(Screen::Title)
        );
        assert_eq!(next(Screen::Title, Event::Back), Next::Quit);
    }

    #[test]
    fn ignored_events_stay() {
        // Esc during play is handled as give-up (PlayOver), not Back.
        assert_eq!(next(Screen::Play, Event::Back), Next::Stay);
        assert_eq!(next(Screen::Play, Event::Confirm), Next::Stay);
        assert_eq!(next(Screen::SongSelect, Event::PlayOver), Next::Stay);
        assert_eq!(next(Screen::Options, Event::Retry), Next::Stay);
    }
}
