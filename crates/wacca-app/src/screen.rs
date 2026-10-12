//! The core loop as a state machine: SongSelect → Difficulty → Options → Play → Results → SongSelect.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    SongSelect,
    Difficulty,
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
        (SongSelect, Confirm) => Next::Go(Difficulty),
        (SongSelect, Back) => Next::Quit,
        (Difficulty, Confirm) => Next::Go(Options),
        (Difficulty, Back) => Next::Go(SongSelect),
        (Options, Confirm) => Next::Go(Play),
        (Options, Back) => Next::Go(Difficulty),
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
        let mut s = Screen::SongSelect;
        for e in [
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
    fn back_walks_up_and_quits_from_song_select() {
        assert_eq!(
            next(Screen::Options, Event::Back),
            Next::Go(Screen::Difficulty)
        );
        assert_eq!(
            next(Screen::Difficulty, Event::Back),
            Next::Go(Screen::SongSelect)
        );
        assert_eq!(next(Screen::SongSelect, Event::Back), Next::Quit);
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
