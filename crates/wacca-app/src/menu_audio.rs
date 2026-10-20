//! Menu music and menu sounds. The cues are the game's own (`MER_BGM_SYS_*` in the BGM bank,
//! `MER_SE_UI_*` in the SE bank, all named in the 3.07.01 executable), but which one the game plays
//! where is not recorded.
//!
//! The BGM bank has three generations of system music, `00x` (WACCA), `20x` (Lily) and `30x`
//! (Reverse), each with a ~45 s track that does not loop (`x01`), a ~30 s loop (`x02`), a ~12 s
//! loop (`x03`) and a ~7 s sting (`x05`); the executable names the Reverse set.
// UNVERIFIED: x01 = title, x02 = menus, from their lengths and loop points only.
// UNVERIFIED: the three menu sounds below were picked by length (short clicks), not by ear.
// UI_037 has a sustain loop, so the game probably holds it while the wheel turns; here it plays
// once per step.

use std::collections::HashMap;
use std::time::Instant;

use wacca_audio::{BgmBank, Player, SeBank, Sfx};

use crate::screen::Screen;
use crate::ui::Action;

pub const TITLE_BGM: &str = "MER_BGM_SYS_301";
pub const MENU_BGM: &str = "MER_BGM_SYS_302";
/// Linear gain of the menu music (the song previews play at 1).
pub const BGM_GAIN: f32 = 0.8;
pub const SE_GAIN: f32 = 0.8;

/// A menu sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    /// Cursor, wheel, tabs, toggles.
    Move,
    Confirm,
    Back,
}

impl Sound {
    pub fn cue(self) -> &'static str {
        match self {
            Sound::Move => "MER_SE_UI_037",
            Sound::Confirm => "MER_SE_UI_025",
            Sound::Back => "MER_SE_UI_026",
        }
    }
}

/// The music `screen` plays. Song select and the options before play have the song preview
/// instead (`None`), as do play, results, the timing check and setup.
pub fn music_for(screen: Screen) -> Option<&'static str> {
    match screen {
        Screen::Title => Some(TITLE_BGM),
        Screen::ModeSelect | Screen::Category => Some(MENU_BGM),
        Screen::SongSelect
        | Screen::Options
        | Screen::Play
        | Screen::Results
        | Screen::Calibrate
        | Screen::Setup => None,
    }
}

/// The sound a menu action makes.
pub fn sound_for(a: Action) -> Option<Sound> {
    match a {
        Action::Up
        | Action::Down
        | Action::Left
        | Action::Right
        | Action::Pick(_)
        | Action::Scroll(_)
        | Action::NextSort
        | Action::ToggleFilter
        | Action::Difficulty
        | Action::Genre(_) => Some(Sound::Move),
        Action::Confirm | Action::Retry | Action::Setup | Action::Import => Some(Sound::Confirm),
        Action::Back => Some(Sound::Back),
    }
}

/// Menu music through the shared [`Player`]: loads the screen's cue, loops it, and lets go when
/// a screen without music comes (the preview or the song then loads its own audio).
#[derive(Debug, Default)]
pub struct MenuMusic {
    playing: Option<&'static str>,
    /// Loading this cue failed; do not retry every frame.
    failed: Option<&'static str>,
}

impl MenuMusic {
    /// Every frame on the title and the menus up to options: starts, switches or stops the music
    /// for `screen`.
    pub fn update(&mut self, screen: Screen, player: &Player, bgm: Option<&BgmBank>) {
        let want = music_for(screen);
        if want != self.playing {
            if self.playing.take().is_some() && screen_owns_player(screen) {
                player.pause();
            }
            if let (Some(cue), Some(bgm)) = (want, bgm)
                && self.failed != Some(cue)
            {
                match start(player, bgm, cue) {
                    Ok(()) => self.playing = Some(cue),
                    Err(e) => {
                        log::warn!("menu music {cue}: {e}");
                        self.failed = Some(cue);
                    }
                }
            }
        }
        let d = player.duration();
        if self.playing.is_some()
            && d > 0.0
            && player.position() >= d - 0.02
            && let Err(e) = player.seek(0.0)
        {
            log::warn!("menu music loop: {e}");
        }
    }

    /// Something else took the player (play, timing check): stop tracking the music.
    pub fn forget(&mut self) {
        self.playing = None;
    }
}

/// Screens where the menu music may pause the player when it stops: song select and options
/// (the preview starts by itself after a short rest). Play, the timing check and results load
/// or keep their own audio.
fn screen_owns_player(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::Title
            | Screen::ModeSelect
            | Screen::Category
            | Screen::SongSelect
            | Screen::Options
    )
}

fn start(player: &Player, bgm: &BgmBank, cue: &str) -> Result<(), Box<dyn std::error::Error>> {
    player.load(Box::new(bgm.open_cue(cue)?))?;
    player.set_volume(BGM_GAIN);
    player.play();
    log::debug!("menu music {cue}");
    Ok(())
}

/// Menu sounds, decoded on first use at the player's rate.
#[derive(Debug, Default)]
pub struct MenuSounds {
    cache: HashMap<Sound, Vec<Sfx>>,
    /// A sound that failed to decode stays silent.
    failed: Vec<Sound>,
    last: Option<(Sound, Instant)>,
}

impl MenuSounds {
    pub fn play(&mut self, sound: Sound, player: &Player, se: Option<&SeBank>) {
        // A held arrow key repeats ~30 times a second; one click per 40 ms is plenty.
        if let Some((s, t)) = self.last
            && s == sound
            && t.elapsed().as_secs_f32() < 0.04
        {
            return;
        }
        self.last = Some((sound, Instant::now()));
        let rate = player.sample_rate();
        if !self.cache.contains_key(&sound) && !self.failed.contains(&sound) {
            let Some(bank) = se else { return };
            match bank.decode_cue(sound.cue()) {
                Ok(tracks) => {
                    // Some UI cues have a sustain loop (e.g. UI_037), which would ring until
                    // stopped; menu sounds play once.
                    let tracks = tracks
                        .iter()
                        .map(|t| Sfx {
                            loop_range: None,
                            ..t.resampled(rate)
                        })
                        .collect();
                    self.cache.insert(sound, tracks);
                }
                Err(e) => {
                    log::warn!("menu sound {}: {e}", sound.cue());
                    self.failed.push(sound);
                }
            }
        }
        for t in self.cache.get(&sound).into_iter().flatten() {
            if let Err(e) = player.play_sfx(t, SE_GAIN) {
                log::debug!("menu sound: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_on_the_menus_preview_on_song_select() {
        assert_eq!(music_for(Screen::Title), Some(TITLE_BGM));
        assert_eq!(music_for(Screen::Category), Some(MENU_BGM));
        assert_eq!(music_for(Screen::SongSelect), None);
        assert_eq!(music_for(Screen::Play), None);
    }

    #[test]
    fn actions_make_sounds() {
        assert_eq!(sound_for(Action::Scroll(-2)), Some(Sound::Move));
        assert_eq!(sound_for(Action::Confirm), Some(Sound::Confirm));
        assert_eq!(sound_for(Action::Back), Some(Sound::Back));
    }
}
