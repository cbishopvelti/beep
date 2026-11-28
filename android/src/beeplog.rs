//! A port of the watch's `beeplog.js`: reads a test's log
//! (`beeptest.<start>.csv`, a line a second) and turns it into TCX (Garmin
//! Training Center XML, for Strava, Garmin Connect and the like) or plain
//! CSV, with the same file names.
//!
//! The output must be byte for byte what `beeplog.js` gives for the same
//! log: the tests compare it against files node made from `beeplog.js`
//! (`tests/fixtures/` at the repo root). So numbers are kept as `f64`, as
//! JavaScript has them, a missing GPS value is `NaN` as there, and they're
//! written the way JavaScript writes them ([`js_number`], `toFixed`).
//!
//! What the log holds (`lib.js`):
//!
//! - `start,<ms>,<tz offset min>,<course m>`: first.
//! - `<t s>,<bpm>,<confidence>,<level>,<shuttle>,<distance m>` and, with GPS,
//!   `,<speed km/h>,<lat>,<lon>` (empty without a fix): a sample.
//! - `clock,<s>`: the watch's clock was set by this much during the test.
//! - `end,<t>,<level>,<shuttle>,<distance>,<max hr>,<avg hr>`: last, once the
//!   test is over.
//!
//! Beside the port, what the result page draws: a row per level
//! ([`level_rows`]) and the heart rate over the test ([`hr_series`]).

/// A test's log, as `beeplog.js` `parse` reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Log {
    /// When the test started, ms since 1970, moved by every `clock` line.
    pub start_ms: f64,
    /// The watch's `getTimezoneOffset()` at the start: minutes *behind* UTC
    /// (-60 for UTC+1), 0 when absent.
    pub tz_offset: f64,
    /// The course, metres (20 when absent).
    pub course: f64,
    /// The samples up to the end (none after `end.t`), in time order
    /// (a stable sort, so equal times keep the log's order).
    pub samples: Vec<Sample>,
    /// The `end` line, once the test is over.
    pub end: Option<End>,
}

/// One sample line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds since the start.
    pub t: f64,
    pub bpm: f64,
    /// The heart rate's confidence, 0 to 100.
    pub conf: f64,
    pub level: f64,
    /// Shuttles completed in the level.
    pub shuttle: f64,
    /// Metres run since the start.
    pub dist: f64,
    /// GPS speed, km/h; `NaN` when not recorded or no fix.
    pub gps_speed: f64,
    /// `NaN` when not recorded or no fix.
    pub lat: f64,
    /// `NaN` when not recorded or no fix.
    pub lon: f64,
}

/// The `end` line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct End {
    /// Seconds since the start.
    pub t: f64,
    pub level: f64,
    pub shuttle: f64,
    pub dist: f64,
    pub max_hr: f64,
    pub avg_hr: f64,
}

/// Reads a log as `beeplog.js` `parse` does: lines split on `\r?\n`, blank
/// ones skipped, unknown ones ignored, so anything at all parses (an empty
/// text gives an empty log starting at 0 on a 20 m course).
pub fn parse(text: &str) -> Log {
    todo!()
}

/// "20m beep test result 9.4, 1480 m", or "20m beep test (incomplete log)"
/// without an `end`: `beeplog.js` `resultText`.
pub fn result_text(log: &Log) -> String {
    todo!()
}

/// The log as TCX, one lap per level: `beeplog.js` `toTCX(log,
/// {minConfidence})`. Heart rate readings below `min_confidence` (0 to 100;
/// the page offers 50, "leave out low-confidence readings") are left out.
/// An error, "Log has no samples", when there are none.
pub fn to_tcx(log: &Log, min_confidence: f64) -> Result<String, String> {
    todo!()
}

/// The log as CSV, a row a sample: `beeplog.js` `toCSV`.
pub fn to_csv(log: &Log) -> String {
    todo!()
}

/// What to call the exported files, without the extension:
/// "beeptest-20m-2026-11-28-14-05" in the watch's time zone at the start,
/// as `beeplog.js` `fileBase`.
pub fn file_base(log: &Log) -> String {
    todo!()
}

/// `v` as JavaScript's `String(v)` writes it: "12", "12.5", "0.1", "NaN",
/// "1e+21". What `beeplog.js` gets when it joins or adds a number to a
/// string without `toFixed`.
pub fn js_number(v: f64) -> String {
    todo!()
}

/// A level of the test, for the result page's table and the chart's level
/// marks. The levels are the TCX's laps: consecutive samples with the same
/// level, each lap running from its first sample (0 for the first) to the
/// first sample of the next lap (the last sample for the last).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelRow {
    pub level: u32,
    /// When the level started and ended, seconds since the start.
    pub start_s: f64,
    pub end_s: f64,
    /// The level's heart rate readings with bpm above 0 and confidence at
    /// least the `min_confidence` asked for: their average (rounded, as the
    /// TCX's) and highest. `None` with no readings.
    pub avg_hr: Option<u32>,
    pub max_hr: Option<u32>,
    /// The average of the level's GPS speeds, km/h; `None` when GPS wasn't
    /// recorded or had no fix all level.
    pub gps_kmh: Option<f64>,
}

/// A row per level, in order. Empty for a log with no samples.
pub fn level_rows(log: &Log, min_confidence: f64) -> Vec<LevelRow> {
    todo!()
}

/// The heart rate over the test, for the chart: `(seconds since the start,
/// bpm)` for every sample with bpm above 0 and confidence at least
/// `min_confidence`, in time order.
pub fn hr_series(log: &Log, min_confidence: f64) -> Vec<(f64, f64)> {
    todo!()
}
