//! The level tables, as the watch's `lib.js` has them (between its
//! `TABLES_START` and `TABLES_END` marks): the speed of each level, how long
//! a shuttle takes, how many shuttles each level has on a 20 m or 15 m
//! course, where a test is at a time since its start, and the result it
//! scores there.
//!
//! The phone uses them to draw a result (the levels on the heart rate chart,
//! the target speed in the table) and to compare results ("best"). They must
//! give what `lib.js` gives for the same numbers: the tests check them
//! against values worked out from it.

/// The courses the watch offers, in metres, in the order the phone shows
/// them.
pub const COURSES: [u32; 2] = [20, 15];

/// Shuttles per level on the standard 20 m test (Leger / UK beep test),
/// levels 1 to 21: `lib.js` `TESTS[20]`.
pub const SHUTTLES_20: [u32; 21] = [
    7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16, 16,
];

/// Shuttles per level on the 15 m version (FitnessGram 15 m PACER lap
/// counts): `lib.js` `TESTS[15]`.
pub const SHUTTLES_15: [u32; 21] = [
    9, 10, 11, 12, 12, 13, 13, 14, 14, 15, 15, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21,
];

/// The shuttles in each level for an `m` metre course, level 1 first. Any
/// course but 15 gets the 20 m table, as `lib.js` `shuttles(m)` does.
pub fn shuttles(m: u32) -> &'static [u32] {
    if m == 15 { &SHUTTLES_15 } else { &SHUTTLES_20 }
}

/// The running speed of `level`, km/h: 8.0 at level 1, then 8.0 + 0.5 per
/// level (9.0 at level 2), as `lib.js` `speed(l)`.
pub fn speed(level: u32) -> f64 {
    if level == 1 {
        8.0
    } else {
        8.0 + 0.5 * f64::from(level)
    }
}

/// How long one shuttle of `level` takes on an `m` metre course, seconds:
/// `m * 3.6 / speed(level)`, as `lib.js` `shuttleTime(m, l)`.
pub fn shuttle_time(m: u32, level: u32) -> f64 {
    f64::from(m) * 3.6 / speed(level)
}

/// When `level` starts, seconds after the start of the test on an `m` metre
/// course: 0 for level 1, the end of the last level for one past the table.
pub fn level_start(m: u32, level: u32) -> f64 {
    // The same sums as `position`, a shuttle at a time.
    let tab = shuttles(m);
    let mut at = 0.0;
    for l in 1..level.min(tab.len() as u32 + 1) {
        let d = shuttle_time(m, l);
        for _ in 0..tab[l as usize - 1] {
            at += d;
        }
    }
    at
}

/// Where a test is `t` seconds after its start: `lib.js` `position(m, t)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// The level being run (1 up; the last level once `done`).
    pub level: u32,
    /// Shuttles completed in that level.
    pub shuttle: u32,
    /// Shuttles completed in the whole test.
    pub total: u32,
    /// When the shuttle being run now started (s).
    pub shuttle_start: f64,
    /// When it ends (s).
    pub next_at: f64,
    /// Past the end of the last level.
    pub done: bool,
}

/// Where an `m` metre test is `t` seconds after its start, exactly as
/// `lib.js` `position(m, t)` (the same floating point sums, so a boundary
/// falls on the same side).
pub fn position(m: u32, t: f64) -> Position {
    let tab = shuttles(m);
    let mut at = 0.0;
    let mut total = 0;
    for (l, &n) in (1..).zip(tab) {
        let d = shuttle_time(m, l);
        for s in 0..n {
            if at + d > t {
                return Position {
                    level: l,
                    shuttle: s,
                    total,
                    shuttle_start: at,
                    next_at: at + d,
                    done: false,
                };
            }
            at += d;
            total += 1;
        }
    }
    Position {
        level: tab.len() as u32,
        shuttle: tab[tab.len() - 1],
        total,
        shuttle_start: at,
        next_at: at,
        done: true,
    }
}

/// A result: the last fully completed shuttle, as `level.shuttle` (`l` and
/// `s` in a history entry). `0.0` when not even one shuttle was completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Score {
    pub level: u32,
    pub shuttle: u32,
}

impl Score {
    /// "9.4".
    pub fn text(self) -> String {
        format!("{}.{}", self.level, self.shuttle)
    }
}

/// The result scored at position `p` on an `m` metre course, as `lib.js`
/// `result(m, p)`: the last fully completed shuttle (the end of the last
/// level once `done`; the end of the level before when none of this one is
/// done yet).
pub fn result(m: u32, p: &Position) -> Score {
    let tab = shuttles(m);
    if p.done {
        Score {
            level: tab.len() as u32,
            shuttle: tab[tab.len() - 1],
        }
    } else if p.shuttle > 0 {
        Score {
            level: p.level,
            shuttle: p.shuttle,
        }
    } else if p.level > 1 {
        Score {
            level: p.level - 1,
            shuttle: tab.get(p.level as usize - 2).copied().unwrap_or(0),
        }
    } else {
        Score::default()
    }
}

/// Shuttles completed in all, for a result of `level.shuttle` on an `m`
/// metre course: every shuttle of the levels before, plus `shuttle`. What
/// the progress chart plots, and what ranks results on the same course.
pub fn total_shuttles(m: u32, level: u32, shuttle: u32) -> u32 {
    let tab = shuttles(m);
    let before = (level.saturating_sub(1) as usize).min(tab.len());
    tab[..before].iter().sum::<u32>().saturating_add(shuttle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `lib.js` gives, worked out with node from its tables
    /// (`position` and `result` at these times).
    #[test]
    fn position_and_result_are_lib_js() {
        let cases: [(u32, f64, Position, (u32, u32)); 10] = [
            (20, 0.0, pos(1, 0, 0, 0.0, 9.0, false), (0, 0)),
            (20, 8.9, pos(1, 0, 0, 0.0, 9.0, false), (0, 0)),
            (20, 9.0, pos(1, 1, 1, 9.0, 18.0, false), (1, 1)),
            (20, 62.9, pos(1, 6, 6, 54.0, 63.0, false), (1, 6)),
            (20, 63.0, pos(2, 0, 7, 63.0, 71.0, false), (1, 7)),
            (
                20,
                200.5,
                pos(4, 1, 24, 194.8315789473683, 202.0315789473683, false),
                (4, 1),
            ),
            (
                20,
                1e9,
                pos(21, 16, 247, 1321.1501661580282, 1321.1501661580282, true),
                (21, 16),
            ),
            (15, 6.75, pos(1, 1, 1, 6.75, 13.5, false), (1, 1)),
            (
                15,
                1234.5,
                pos(21, 0, 302, 1234.373306924307, 1237.2922258432259, false),
                (20, 20),
            ),
            (
                15,
                1e9,
                pos(21, 21, 323, 1295.6706042216051, 1295.6706042216051, true),
                (21, 21),
            ),
        ];
        for (m, t, want, (l, s)) in cases {
            let p = position(m, t);
            assert_eq!(p, want, "{m} m at {t} s");
            assert_eq!(
                result(m, &p),
                Score {
                    level: l,
                    shuttle: s
                },
                "{m} m at {t} s"
            );
        }
        // Before the start is the start.
        assert_eq!(position(20, -1.0), position(20, 0.0));
    }

    fn pos(level: u32, shuttle: u32, total: u32, start: f64, next: f64, done: bool) -> Position {
        Position {
            level,
            shuttle,
            total,
            shuttle_start: start,
            next_at: next,
            done,
        }
    }

    #[test]
    fn speeds_and_times() {
        assert_eq!(speed(1), 8.0);
        assert_eq!(speed(2), 9.0);
        assert_eq!(speed(3), 9.5);
        assert_eq!(shuttle_time(20, 1), 9.0);
        assert_eq!(shuttle_time(20, 2), 8.0);
        assert_eq!(shuttle_time(15, 21), 2.918918918918919);
        assert_eq!(shuttles(15), &SHUTTLES_15);
        assert_eq!(shuttles(20), &SHUTTLES_20);
        assert_eq!(shuttles(0), &SHUTTLES_20);
    }

    #[test]
    fn level_starts_are_positions_sums() {
        assert_eq!(level_start(20, 1), 0.0);
        assert_eq!(level_start(20, 2), 63.0);
        assert_eq!(level_start(20, 4), 187.63157894736833);
        assert_eq!(level_start(15, 2), 60.75);
        assert_eq!(level_start(15, 4), 183.2763157894736);
        assert_eq!(level_start(20, 22), 1321.1501661580282);
        assert_eq!(level_start(20, 99), 1321.1501661580282);
        assert_eq!(level_start(15, 22), 1295.6706042216051);
        assert_eq!(level_start(20, 0), 0.0);
        // A level starts where position says its first shuttle does.
        for m in COURSES {
            for l in 1..=21 {
                let p = position(m, level_start(m, l) + 1e-9);
                assert_eq!((p.level, p.shuttle), (l, 0), "{m} m level {l}");
            }
        }
    }

    #[test]
    fn scores_and_totals() {
        let s = Score {
            level: 9,
            shuttle: 4,
        };
        assert_eq!(s.text(), "9.4");
        assert_eq!(Score::default().text(), "0.0");
        assert_eq!(total_shuttles(20, 1, 3), 3);
        assert_eq!(total_shuttles(20, 2, 0), 7);
        assert_eq!(
            total_shuttles(20, 9, 4),
            7 + 8 + 8 + 9 + 9 + 10 + 10 + 11 + 4
        );
        assert_eq!(total_shuttles(15, 3, 5), 9 + 10 + 5);
        assert_eq!(total_shuttles(20, 0, 0), 0);
        // A whole test is every shuttle: what position counts at the end.
        assert_eq!(total_shuttles(20, 21, 16), position(20, 1e9).total);
        assert_eq!(total_shuttles(15, 21, 21), position(15, 1e9).total);
        assert_eq!(total_shuttles(20, 99, 0), 247);
        // Results on the same course rank by it.
        assert!(total_shuttles(20, 9, 1) > total_shuttles(20, 8, 11));
    }
}
