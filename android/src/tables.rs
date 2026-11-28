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
    todo!()
}

/// The running speed of `level`, km/h: 8.0 at level 1, then 8.0 + 0.5 per
/// level (9.0 at level 2), as `lib.js` `speed(l)`.
pub fn speed(level: u32) -> f64 {
    todo!()
}

/// How long one shuttle of `level` takes on an `m` metre course, seconds:
/// `m * 3.6 / speed(level)`, as `lib.js` `shuttleTime(m, l)`.
pub fn shuttle_time(m: u32, level: u32) -> f64 {
    todo!()
}

/// When `level` starts, seconds after the start of the test on an `m` metre
/// course: 0 for level 1, the end of the last level for one past the table.
pub fn level_start(m: u32, level: u32) -> f64 {
    todo!()
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
    todo!()
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
        todo!()
    }
}

/// The result scored at position `p` on an `m` metre course, as `lib.js`
/// `result(m, p)`: the last fully completed shuttle (the end of the last
/// level once `done`; the end of the level before when none of this one is
/// done yet).
pub fn result(m: u32, p: &Position) -> Score {
    todo!()
}

/// Shuttles completed in all, for a result of `level.shuttle` on an `m`
/// metre course: every shuttle of the levels before, plus `shuttle`. What
/// the progress chart plots, and what ranks results on the same course.
pub fn total_shuttles(m: u32, level: u32, shuttle: u32) -> u32 {
    todo!()
}
