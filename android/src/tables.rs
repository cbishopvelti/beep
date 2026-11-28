//! The level tables, as the watch's `lib.js` has them (between its
//! `TABLES_START` and `TABLES_END` marks): the speed of each level, how long
//! a shuttle takes, how many shuttles each level has on a 20 m or 15 m
//! course, where a test is at a time since its start, and the result it
//! scores there, for each of the watch's speed progressions (its Speeds
//! setting: `PROGS`, and a history entry's `p`).
//!
//! The phone uses them to compare results ("best", the progress chart) and
//! for the demo data. They must give what `lib.js` gives for the same
//! numbers: the tests check them against values worked out with node from
//! it. A watch app from before the Speeds setting has only the standard
//! progression, 0, which is what an entry without `p` ran, and every
//! function here gives what that `lib.js` gave for it.

/// The courses the watch offers, in metres, in the order the phone shows
/// them.
pub const COURSES: [u32; 2] = [20, 15];

/// The speed progressions, by index (`p`): the speed of level 1, then of
/// level 2, km/h; every level after adds 0.5. `lib.js` `PROGS`.
///
/// - 0: 8.0, 9.0, 9.5... the standard 20 m beep test (the default, and all
///   a watch from before the Speeds setting has);
/// - 1: 8.0, 8.5, 9.0... Leger & Lambert's original (1982);
/// - 2: 8.5, 9.5, 10.0...
pub const PROGRESSIONS: [[f64; 2]; 3] = [[8.0, 9.0], [8.0, 8.5], [8.5, 9.5]];

/// The levels in every table.
pub const LEVELS: u32 = 21;

/// Whether `p` is one of [`PROGRESSIONS`].
pub fn is_progression(p: u32) -> bool {
    (p as usize) < PROGRESSIONS.len()
}

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

/// Shuttles per level for the other progressions, which `lib.js` works out
/// from the speeds (`shuttles(m, p)`: the fewest that make a level last
/// over a minute, the rule the standard 20 m table follows). Written out
/// here; the tests check they follow the rule. `[p - 1][0]` is 20 m,
/// `[p - 1][1]` 15 m.
const SHUTTLES_OTHER: [[[u32; 21]; 2]; 2] = [
    [
        [
            7, 8, 8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16,
        ],
        [
            9, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 21,
        ],
    ],
    [
        [
            8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16, 16, 16,
        ],
        [
            10, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 21, 21, 22,
        ],
    ],
];

/// The shuttles in each level for an `m` metre course and progression `p`,
/// level 1 first, as `lib.js` `shuttles(m, p)`: any course but 15 gets the
/// 20 m table, and any `p` but 1 or 2 the standard one.
pub fn shuttles(m: u32, p: u32) -> &'static [u32] {
    let course = usize::from(m == 15);
    match p {
        1 | 2 => &SHUTTLES_OTHER[p as usize - 1][course],
        _ if m == 15 => &SHUTTLES_15,
        _ => &SHUTTLES_20,
    }
}

/// The running speed of `level` in progression `p`, km/h: the first speed
/// at level 1, the second at level 2, then 0.5 more a level, as `lib.js`
/// `speed(l, p)` (any `p` it hasn't got is the standard one: 8.0, 9.0,
/// 9.5...).
pub fn speed(level: u32, p: u32) -> f64 {
    let s = PROGRESSIONS.get(p as usize).unwrap_or(&PROGRESSIONS[0]);
    if level == 1 {
        s[0]
    } else {
        s[1] + 0.5 * (f64::from(level) - 2.0)
    }
}

/// The first speeds of progression `p`, as the watch shows them: "8.0, 8.5,
/// 9.0... km/h" (`app.js` `fmtSpeeds`). What a result's card and page say
/// when it ran on speeds other than the standard ones.
pub fn speeds_text(p: u32) -> String {
    let s: Vec<String> = (1..=3).map(|l| format!("{:.1}", speed(l, p))).collect();
    format!("{}... km/h", s.join(", "))
}

/// How long one shuttle of `level` takes on an `m` metre course in
/// progression `p`, seconds: `m * 3.6 / speed(level, p)`, as `lib.js`
/// `shuttleTime(m, l, p)`.
pub fn shuttle_time(m: u32, level: u32, p: u32) -> f64 {
    f64::from(m) * 3.6 / speed(level, p)
}

/// When `level` starts, seconds after the start of the test on an `m` metre
/// course in progression `p`: 0 for level 1, the end of the last level for
/// one past the table.
pub fn level_start(m: u32, level: u32, p: u32) -> f64 {
    // The same sums as `position`, a shuttle at a time.
    let tab = shuttles(m, p);
    let mut at = 0.0;
    for l in 1..level.min(tab.len() as u32 + 1) {
        let d = shuttle_time(m, l, p);
        for _ in 0..tab[l as usize - 1] {
            at += d;
        }
    }
    at
}

/// Where a test is `t` seconds after its start: `lib.js` `position(m, t, p)`.
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

/// Where an `m` metre test in progression `p` is `t` seconds after its
/// start, exactly as `lib.js` `position(m, t, p)` (the same floating point
/// sums, so a boundary falls on the same side).
pub fn position(m: u32, t: f64, p: u32) -> Position {
    let tab = shuttles(m, p);
    let mut at = 0.0;
    let mut total = 0;
    for (l, &n) in (1..).zip(tab) {
        let d = shuttle_time(m, l, p);
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

/// The result scored at position `pos` on an `m` metre course in
/// progression `p`, as `lib.js` `result(m, pos, p)`: the last fully
/// completed shuttle (the end of the last level once `done`; the end of the
/// level before when none of this one is done yet).
pub fn result(m: u32, pos: &Position, p: u32) -> Score {
    let tab = shuttles(m, p);
    if pos.done {
        Score {
            level: tab.len() as u32,
            shuttle: tab[tab.len() - 1],
        }
    } else if pos.shuttle > 0 {
        Score {
            level: pos.level,
            shuttle: pos.shuttle,
        }
    } else if pos.level > 1 {
        Score {
            level: pos.level - 1,
            shuttle: tab.get(pos.level as usize - 2).copied().unwrap_or(0),
        }
    } else {
        Score::default()
    }
}

/// Shuttles completed in all, for a result of `level.shuttle` on an `m`
/// metre course in progression `p`: every shuttle of the levels before,
/// plus `shuttle`. What the progress chart plots, for results on the same
/// course and progression.
pub fn total_shuttles(m: u32, level: u32, shuttle: u32, p: u32) -> u32 {
    let tab = shuttles(m, p);
    let before = (level.saturating_sub(1) as usize).min(tab.len());
    tab[..before].iter().sum::<u32>().saturating_add(shuttle)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// What `lib.js` gives, worked out with node from its tables
    /// (`position` and `result` at these times; the other progressions
    /// from the Speeds setting's `lib.js`, PR #6).
    #[test]
    fn position_and_result_are_lib_js() {
        /// (course, progression, time, where, result).
        type Case = (u32, u32, f64, Position, (u32, u32));
        #[rustfmt::skip]
        let cases: [Case; 22] = [
            (20, 0, 0.0, pos(1, 0, 0, 0.0, 9.0, false), (0, 0)),
            (20, 0, 8.9, pos(1, 0, 0, 0.0, 9.0, false), (0, 0)),
            (20, 0, 9.0, pos(1, 1, 1, 9.0, 18.0, false), (1, 1)),
            (20, 0, 62.9, pos(1, 6, 6, 54.0, 63.0, false), (1, 6)),
            (20, 0, 63.0, pos(2, 0, 7, 63.0, 71.0, false), (1, 7)),
            (20, 0, 200.5, pos(4, 1, 24, 194.8315789473683, 202.0315789473683, false), (4, 1)),
            (20, 0, 1e9, pos(21, 16, 247, 1321.1501661580282, 1321.1501661580282, true), (21, 16)),
            (15, 0, 6.75, pos(1, 1, 1, 6.75, 13.5, false), (1, 1)),
            (15, 0, 1234.5, pos(21, 0, 302, 1234.373306924307, 1237.2922258432259, false), (20, 20)),
            (15, 0, 1e9, pos(21, 21, 323, 1295.6706042216051, 1295.6706042216051, true), (21, 21)),
            // 8.0, 8.5, 9.0...
            (20, 1, 63.0, pos(2, 0, 7, 63.0, 71.47058823529412, false), (1, 7)),
            (20, 1, 200.5, pos(4, 0, 23, 194.76470588235293, 202.34365325077397, false), (3, 8)),
            (20, 1, 600.0, pos(10, 4, 84, 599.013811650726, 604.773811650726, false), (10, 4)),
            (20, 1, 1e9, pos(21, 16, 239, 1326.6446017701107, 1326.6446017701107, true), (21, 16)),
            (15, 1, 200.5, pos(4, 1, 31, 195.96362229102172, 201.6478328173375, false), (4, 1)),
            (15, 1, 1234.5, pos(20, 15, 289, 1232.474147260442, 1235.559861546156, false), (20, 15)),
            (15, 1, 1e9, pos(21, 21, 315, 1310.9027186890128, 1310.9027186890128, true), (21, 21)),
            // 8.5, 9.5, 10.0...
            (20, 2, 9.0, pos(1, 1, 1, 8.470588235294118, 16.941176470588236, false), (1, 1)),
            (20, 2, 63.0, pos(1, 7, 7, 59.29411764705882, 67.76470588235294, false), (1, 7)),
            (20, 2, 1234.5, pos(20, 8, 232, 1230.7797369052475, 1234.6716287971394, false), (20, 8)),
            (15, 2, 600.0, pos(10, 9, 123, 599.2193656552099, 603.2193656552099, false), (10, 9)),
            (15, 2, 1e9, pos(21, 22, 338, 1307.9763317757834, 1307.9763317757834, true), (21, 22)),
        ];
        for (m, p, t, want, (l, s)) in cases {
            let at = position(m, t, p);
            assert_eq!(at, want, "{m} m, progression {p}, at {t} s");
            assert_eq!(
                result(m, &at, p),
                Score {
                    level: l,
                    shuttle: s
                },
                "{m} m, progression {p}, at {t} s"
            );
        }
        // Before the start is the start.
        assert_eq!(position(20, -1.0, 0), position(20, 0.0, 0));
        // A progression lib.js hasn't got is the standard one.
        assert_eq!(position(20, 200.5, 7), position(20, 200.5, 0));
    }

    #[test]
    fn speeds_and_times() {
        let speeds = |p| [1, 2, 3, 4, 21, 22].map(|l| speed(l, p));
        assert_eq!(speeds(0), [8.0, 9.0, 9.5, 10.0, 18.5, 19.0]);
        assert_eq!(speeds(1), [8.0, 8.5, 9.0, 9.5, 18.0, 18.5]);
        assert_eq!(speeds(2), [8.5, 9.5, 10.0, 10.5, 19.0, 19.5]);
        assert_eq!(speeds(3), speeds(0));
        // As lib.js before the Speeds setting: 8.0 + 0.5 a level after 1.
        for l in 2..=22 {
            assert_eq!(speed(l, 0), 8.0 + 0.5 * f64::from(l), "level {l}");
        }
        assert_eq!(shuttle_time(20, 1, 0), 9.0);
        assert_eq!(shuttle_time(20, 2, 0), 8.0);
        assert_eq!(shuttle_time(15, 21, 0), 2.918918918918919);
        assert_eq!(shuttle_time(20, 2, 1), 8.470588235294118);
        assert_eq!(shuttle_time(20, 21, 1), 4.0);
        assert_eq!(shuttle_time(15, 2, 2), 5.684210526315789);
        assert_eq!(shuttle_time(15, 3, 2), 5.4);
        assert_eq!(shuttle_time(15, 21, 2), 2.8421052631578947);
        assert_eq!(shuttles(15, 0), &SHUTTLES_15);
        assert_eq!(shuttles(20, 0), &SHUTTLES_20);
        assert_eq!(shuttles(0, 0), &SHUTTLES_20);
        assert_eq!(shuttles(20, 9), &SHUTTLES_20);
        assert_eq!(shuttles(17, 1), shuttles(20, 1));
        assert!(is_progression(2) && !is_progression(3));
        assert_eq!(speeds_text(0), "8.0, 9.0, 9.5... km/h");
        assert_eq!(speeds_text(1), "8.0, 8.5, 9.0... km/h");
        assert_eq!(speeds_text(2), "8.5, 9.5, 10.0... km/h");
    }

    /// The other progressions' tables are what lib.js works out (node gave
    /// these), and follow its rule: the fewest shuttles that take over a
    /// minute.
    #[test]
    fn the_other_progressions_tables_are_lib_js() {
        assert_eq!(
            shuttles(20, 1),
            [
                7, 8, 8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16
            ]
        );
        assert_eq!(
            shuttles(15, 1),
            [
                9, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 21
            ]
        );
        assert_eq!(
            shuttles(20, 2),
            [
                8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 16, 16, 16
            ]
        );
        assert_eq!(
            shuttles(15, 2),
            [
                10, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 21, 21, 22
            ]
        );
        for p in [1, 2] {
            for m in COURSES {
                let tab = shuttles(m, p);
                assert_eq!(tab.len(), LEVELS as usize);
                for (l, &n) in (1..).zip(tab) {
                    let rule = (60.0 / shuttle_time(m, l, p)).floor() as u32 + 1;
                    assert_eq!(n, rule, "{m} m, progression {p}, level {l}");
                    let lasts = level_start(m, l + 1, p) - level_start(m, l, p);
                    assert!(lasts > 60.0 && lasts < 68.0, "{m} m {p} level {l}: {lasts}");
                }
            }
        }
    }

    #[test]
    fn level_starts_are_positions_sums() {
        assert_eq!(level_start(20, 1, 0), 0.0);
        assert_eq!(level_start(20, 2, 0), 63.0);
        assert_eq!(level_start(20, 4, 0), 187.63157894736833);
        assert_eq!(level_start(15, 2, 0), 60.75);
        assert_eq!(level_start(15, 4, 0), 183.2763157894736);
        assert_eq!(level_start(20, 22, 0), 1321.1501661580282);
        assert_eq!(level_start(20, 99, 0), 1321.1501661580282);
        assert_eq!(level_start(15, 22, 0), 1295.6706042216051);
        assert_eq!(level_start(20, 0, 0), 0.0);
        assert_eq!(level_start(20, 4, 1), 194.76470588235293);
        assert_eq!(level_start(15, 4, 1), 190.27941176470594);
        assert_eq!(level_start(15, 22, 1), 1310.9027186890128);
        assert_eq!(level_start(20, 2, 2), 67.76470588235294);
        assert_eq!(level_start(20, 4, 2), 193.19628482972126);
        assert_eq!(level_start(15, 22, 2), 1307.9763317757834);
        // A level starts where position says its first shuttle does.
        for p in 0..3 {
            for m in COURSES {
                for l in 1..=LEVELS {
                    let at = position(m, level_start(m, l, p) + 1e-9, p);
                    assert_eq!((at.level, at.shuttle), (l, 0), "{m} m {p} level {l}");
                }
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
        assert_eq!(total_shuttles(20, 1, 3, 0), 3);
        assert_eq!(total_shuttles(20, 2, 0, 0), 7);
        assert_eq!(
            total_shuttles(20, 9, 4, 0),
            7 + 8 + 8 + 9 + 9 + 10 + 10 + 11 + 4
        );
        assert_eq!(total_shuttles(15, 3, 5, 0), 9 + 10 + 5);
        assert_eq!(total_shuttles(20, 0, 0, 0), 0);
        // The other progressions count their own tables (node: 73, 95, 80, 103).
        assert_eq!(total_shuttles(20, 9, 4, 1), 73);
        assert_eq!(total_shuttles(15, 9, 4, 1), 95);
        assert_eq!(total_shuttles(20, 9, 4, 2), 80);
        assert_eq!(total_shuttles(15, 9, 4, 2), 103);
        // A whole test is every shuttle: what position counts at the end.
        for p in 0..3 {
            for m in COURSES {
                let last = *shuttles(m, p).last().unwrap();
                assert_eq!(
                    total_shuttles(m, LEVELS, last, p),
                    position(m, 1e9, p).total,
                    "{m} m {p}"
                );
            }
        }
        assert_eq!(total_shuttles(20, 99, 0, 0), 247);
        assert_eq!(total_shuttles(20, 99, 0, 2), 256);
        // Results on the same course rank by it.
        assert!(total_shuttles(20, 9, 1, 0) > total_shuttles(20, 8, 11, 0));
    }
}
