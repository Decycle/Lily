//! The core loop as a state machine, following the original:
//! Title → ModeSelect → Category (genre folder) → SongSelect (song + difficulty) → Options → Play
//! → Results → SongSelect. The timing check opens from (and returns to) Options. First-run setup
//! ([`crate::setup_flow`], its own steps) opens instead of the title when game files are missing,
//! or from the title, and ends on the title.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    ModeSelect,
    Category,
    SongSelect,
    Options,
    /// Timing check ([`crate::calibrate`]).
    Calibrate,
    Play,
    Results,
    /// First-run setup.
    Setup,
}

/// Things that move between screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Confirm,
    Back,
    /// The song ended (played out or given up).
    PlayOver,
    /// QUIT from the pause menu: back to song select without results.
    Abandon,
    Retry,
    /// Open the timing check.
    Calibrate,
    /// Open setup from the title.
    Setup,
    /// Setup finished or was left.
    SetupDone,
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
        (Options, Event::Calibrate) => Next::Go(Screen::Calibrate),
        (Screen::Calibrate, Confirm | Back) => Next::Go(Options),
        (Play, PlayOver) => Next::Go(Results),
        (Play, Abandon) => Next::Go(SongSelect),
        (Results, Confirm | Back) => Next::Go(SongSelect),
        (Results, Retry) => Next::Go(Play),
        (Title, Event::Setup) => Next::Go(Screen::Setup),
        (Screen::Setup, SetupDone) => Next::Go(Title),
        _ => Next::Stay,
    }
}

/// Seconds the arcade gives a menu screen before it moves on by itself (`LimitationTimeTable`:
/// mode select 30, song select 99, final check 25, results 15). The category wheel is part of the
/// game's song select screen, so the two share one countdown. Shown and enforced only with the
/// `menu_timers` setting; this home build has no time limit by default.
pub fn time_limit(screen: Screen) -> Option<f32> {
    match screen {
        Screen::ModeSelect => Some(30.0),
        Screen::Category | Screen::SongSelect => Some(99.0),
        Screen::Options => Some(25.0),
        Screen::Results => Some(15.0),
        Screen::Title | Screen::Calibrate | Screen::Play | Screen::Setup => None,
    }
}

/// Whether moving `from` → `to` keeps the running countdown (one song select screen).
pub fn keeps_timer(from: Screen, to: Screen) -> bool {
    matches!(
        (from, to),
        (Screen::Category, Screen::SongSelect) | (Screen::SongSelect, Screen::Category)
    )
}

/// Whole seconds shown by a countdown of `limit` after `elapsed` (the game counts 99 … 0).
pub fn countdown(limit: f32, elapsed: f32) -> u32 {
    (limit - elapsed).ceil().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_time_limits_follow_the_table() {
        assert_eq!(time_limit(Screen::ModeSelect), Some(30.0));
        assert_eq!(time_limit(Screen::SongSelect), Some(99.0));
        assert_eq!(time_limit(Screen::Results), Some(15.0));
        assert_eq!(time_limit(Screen::Play), None);
        assert!(keeps_timer(Screen::Category, Screen::SongSelect));
        assert!(!keeps_timer(Screen::SongSelect, Screen::Options));
        assert_eq!(countdown(30.0, 0.0), 30);
        assert_eq!(countdown(30.0, 3.5), 27);
        assert_eq!(countdown(15.0, 20.0), 0);
    }

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
    fn timing_check_returns_to_options() {
        let c = Screen::Calibrate;
        assert_eq!(next(Screen::Options, Event::Calibrate), Next::Go(c));
        assert_eq!(next(c, Event::Confirm), Next::Go(Screen::Options));
        assert_eq!(next(c, Event::Back), Next::Go(Screen::Options));
        assert_eq!(next(Screen::SongSelect, Event::Calibrate), Next::Stay);
    }

    #[test]
    fn setup_opens_from_and_returns_to_the_title() {
        assert_eq!(next(Screen::Title, Event::Setup), Next::Go(Screen::Setup));
        assert_eq!(
            next(Screen::Setup, Event::SetupDone),
            Next::Go(Screen::Title)
        );
        // Its own steps handle Confirm / Back.
        assert_eq!(next(Screen::Setup, Event::Back), Next::Stay);
        assert_eq!(next(Screen::ModeSelect, Event::Setup), Next::Stay);
        assert_eq!(time_limit(Screen::Setup), None);
    }

    #[test]
    fn ignored_events_stay() {
        // Esc during play is handled as give-up (PlayOver), not Back.
        assert_eq!(next(Screen::Play, Event::Back), Next::Stay);
        assert_eq!(next(Screen::Play, Event::Confirm), Next::Stay);
        assert_eq!(next(Screen::SongSelect, Event::PlayOver), Next::Stay);
        assert_eq!(next(Screen::Options, Event::Retry), Next::Stay);
        assert_eq!(
            next(Screen::Play, Event::Abandon),
            Next::Go(Screen::SongSelect)
        );
    }
}
