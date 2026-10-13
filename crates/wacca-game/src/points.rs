//! Reverse Points (RP) earned per play and the player level on the user plate.
//!
//! RP: the results screen's "Obtained Reverse Points" is `WaccaPointTable[grade][Level01..15]`
//! (the game's internal name for RP is WACCA Point). Verified on the E3 real-game results: B on
//! NORMAL 1 and NORMAL 3 → 210 (clear and failed alike), AA on NORMAL 3 → 660.

use crate::grade::Grade;

/// `WaccaPointTable`: RP per grade (worst → best, like [`Grade`]) and chart level 1..=15.
const WACCA_POINT: [[u16; 15]; 13] = [
    [90; 15],
    [150; 15],
    [210; 15],
    [240; 15],
    [
        660, 660, 660, 660, 660, 720, 720, 720, 780, 780, 780, 780, 840, 900, 1050,
    ],
    [
        750, 750, 750, 750, 750, 780, 780, 780, 840, 840, 840, 840, 900, 960, 1110,
    ],
    [
        900, 900, 900, 900, 900, 960, 960, 960, 1020, 1020, 1020, 1080, 1080, 1140, 1200,
    ],
    [
        900, 900, 900, 900, 900, 960, 960, 960, 1020, 1020, 1020, 1080, 1080, 1140, 1200,
    ],
    [
        960, 960, 960, 960, 960, 1020, 1020, 1020, 1080, 1080, 1080, 1140, 1140, 1200, 1260,
    ],
    [
        960, 960, 960, 960, 960, 1020, 1020, 1020, 1080, 1080, 1080, 1140, 1140, 1200, 1260,
    ],
    [
        1020, 1020, 1020, 1020, 1020, 1080, 1080, 1080, 1200, 1200, 1200, 1350, 1350, 1440, 1500,
    ],
    [
        1020, 1020, 1020, 1020, 1020, 1080, 1080, 1080, 1200, 1200, 1200, 1350, 1350, 1440, 1500,
    ],
    [
        1200, 1200, 1200, 1200, 1200, 1200, 1200, 1200, 1350, 1350, 1350, 1500, 1500, 1800, 2100,
    ],
];

/// RP of one play: grade × the chart's whole level (12.7 → 12), clamped to the table's 1..=15.
/// Boost items / campaign multipliers ("Earn 3x RP" on the attract screen) are left out.
pub fn reverse_points(grade: Grade, level: f32) -> u32 {
    let col = (level.trunc() as i32).clamp(1, 15) as usize - 1;
    u32::from(WACCA_POINT[grade as usize][col])
}

/// `LevelUpTable`: (from level, EXP per level-up), ascending.
const LEVEL_UP: [(u32, u32); 11] = [
    (1, 50),
    (3, 100),
    (5, 200),
    (10, 300),
    (15, 400),
    (20, 500),
    (30, 600),
    (40, 700),
    (50, 800),
    (100, 900),
    (200, 1000),
];

/// EXP credited per finished play.
/// UNVERIFIED: the client computes EXP and sends it as an item (ARTEMiS only adds it); the real
/// amount is unknown, so every play counts the same.
pub const EXP_PER_PLAY: u32 = 10;

/// Player level after `plays` plays, from `LevelUpTable` (level 1 at 0 plays, capped at 999).
pub fn player_level(plays: u32) -> u32 {
    let mut exp = plays.saturating_mul(EXP_PER_PLAY);
    let mut level = 1;
    while level < 999 {
        let need = LEVEL_UP
            .iter()
            .rev()
            .find(|&&(from, _)| level >= from)
            .map_or(LEVEL_UP[0].1, |&(_, n)| n);
        if exp < need {
            break;
        }
        exp -= need;
        level += 1;
    }
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rp_matches_the_real_results() {
        assert_eq!(reverse_points(Grade::B, 1.0), 210);
        assert_eq!(reverse_points(Grade::B, 3.0), 210);
        assert_eq!(reverse_points(Grade::AA, 3.0), 660);
        assert_eq!(reverse_points(Grade::AA, 13.7), 840);
        assert_eq!(reverse_points(Grade::Master, 15.0), 2100);
        assert_eq!(reverse_points(Grade::D, 0.0), 90);
        assert_eq!(reverse_points(Grade::SssPlus, 99.0), 1500);
    }

    #[test]
    fn level_follows_the_level_up_table() {
        assert_eq!(player_level(0), 1);
        assert_eq!(player_level(4), 1);
        // 50 EXP: level 2; another 50: level 3; then 100 per level.
        assert_eq!(player_level(5), 2);
        assert_eq!(player_level(10), 3);
        assert_eq!(player_level(19), 3);
        assert_eq!(player_level(20), 4);
        assert_eq!(player_level(u32::MAX), 999);
    }
}
