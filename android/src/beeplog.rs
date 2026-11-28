//! A port of the watch's `beeplog.js`: reads a test's log
//! (`beeptest.<start>.csv`, a line a second) and turns it into TCX (Garmin
//! Training Center XML, for Strava, Garmin Connect and the like) or plain
//! CSV, with the same file names.
//!
//! The output must be byte for byte what `beeplog.js` gives for the same
//! log: the tests compare it against files node made from `beeplog.js`
//! (`tests/fixtures/beeplog/` at the repo root, with the script that makes
//! them). So numbers are kept as `f64`, as JavaScript has them, a missing
//! GPS value is `NaN` as there, fields are read as `+s` reads them
//! ([`js_to_number`]), and numbers are written the way JavaScript writes
//! them ([`js_number`], `toFixed`). Where `beeplog.js` throws (no samples,
//! or a time a `Date` can't hold), the port returns its message as an error.
//!
//! What the log holds (`lib.js`):
//!
//! - `start,<ms>,<tz offset min>,<course m>,<progression>`: first. The
//!   progression (the Speeds setting's index, [`crate::tables::PROGRESSIONS`])
//!   came with that setting: a log from before it has four fields.
//!   `beeplog.js` ignores it (so the TCX and CSV do too); [`parse`] reads it
//!   into [`Log::progression`].
//! - `<t s>,<bpm>,<confidence>,<level>,<shuttle>,<distance m>` and, with GPS,
//!   `,<speed km/h>,<lat>,<lon>` (empty without a fix): a sample.
//! - `clock,<s>`: the watch's clock was set by this much during the test.
//! - `end,<t>,<level>,<shuttle>,<distance>,<max hr>,<avg hr>`: last, once the
//!   test is over.
//!
//! Beside the port, what the result page draws: a row per level
//! ([`level_rows`]) and the heart rate over the test ([`hr_series`]).

use std::ops::Range;

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
    /// The speed progression the test ran (the start line's fifth field): 0,
    /// 1 or 2; 0 when absent (a log from before the Speeds setting) or
    /// anything else. Not in `beeplog.js`, which doesn't read it.
    pub progression: u32,
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
/// text gives an empty log starting at 0 on a 20 m course). Each field is
/// read as JavaScript's `+s` reads it ([`js_to_number`]).
pub fn parse(text: &str) -> Log {
    let mut out = Log {
        start_ms: 0.0,
        tz_offset: 0.0,
        course: 20.0,
        progression: 0,
        samples: Vec::new(),
        end: None,
    };
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let p: Vec<&str> = line.split(',').collect();
        // `+p[i]`: a missing field is undefined, which is NaN.
        let plus = |i: usize| p.get(i).map_or(f64::NAN, |s| js_to_number(s));
        match p[0] {
            "start" => {
                out.start_ms = plus(1);
                out.tz_offset = or(plus(2), 0.0);
                out.course = or(plus(3), 20.0);
                let p = plus(4);
                out.progression = if p == 1.0 || p == 2.0 { p as u32 } else { 0 };
            }
            // the watch's clock was set by this much (s) during the test
            "clock" => out.start_ms += plus(1) * 1000.0,
            "end" => {
                out.end = Some(End {
                    t: plus(1),
                    level: plus(2),
                    shuttle: plus(3),
                    dist: plus(4),
                    max_hr: plus(5),
                    avg_hr: plus(6),
                })
            }
            _ if p.len() >= 6 && !plus(0).is_nan() => {
                // `num`: missing or empty is NaN
                let num = |i: usize| match p.get(i) {
                    None | Some(&"") => f64::NAN,
                    Some(s) => js_to_number(s),
                };
                out.samples.push(Sample {
                    t: plus(0),
                    bpm: plus(1),
                    conf: plus(2),
                    level: plus(3),
                    shuttle: plus(4),
                    dist: plus(5),
                    gps_speed: num(6),
                    lat: num(7),
                    lon: num(8),
                });
            }
            _ => {}
        }
    }
    // What came after the end is left out (an end at NaN leaves nothing, as
    // there), and the rest is put in time order: a stable sort, and `t` is
    // never NaN, so this is the order beeplog.js's comparator gives.
    if let Some(end) = out.end {
        out.samples.retain(|s| s.t <= end.t);
    }
    out.samples
        .sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// JavaScript's `x || d` for a number: `d` when `x` is 0, -0 or NaN.
fn or(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() { d } else { x }
}

/// "20m beep test result 9.4, 1480 m", or "20m beep test (incomplete log)"
/// without an `end`: `beeplog.js` `resultText`.
pub fn result_text(log: &Log) -> String {
    let name = format!("{}m beep test", js_number(log.course));
    match log.end {
        None => format!("{name} (incomplete log)"),
        Some(e) => format!(
            "{name} result {}.{}, {} m",
            js_number(e.level),
            js_number(e.shuttle),
            js_number(e.dist)
        ),
    }
}

/// The samples' laps, one per level as `beeplog.js` groups them: a new lap
/// whenever a sample's level isn't the one before's (so a NaN level is a
/// lap of its own each time, as `!=` has it there).
fn laps(samples: &[Sample]) -> Vec<Range<usize>> {
    let mut laps: Vec<Range<usize>> = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        match laps.last_mut() {
            Some(lap) if samples[lap.start].level == s.level => lap.end = i + 1,
            _ => laps.push(i..i + 1),
        }
    }
    laps
}

/// Where lap `i` starts and ends, as `toTCX` works it out: from its first
/// sample (time and distance 0 for the first lap) to the first sample of
/// the next lap, or its own last for the last lap. `(start t, start dist,
/// end t, end dist)`.
fn lap_ends(samples: &[Sample], laps: &[Range<usize>], i: usize) -> (f64, f64, f64, f64) {
    let (first_t, first_dist) = if i > 0 {
        let s = samples[laps[i].start];
        (s.t, s.dist)
    } else {
        (0.0, 0.0)
    };
    let next = match laps.get(i + 1) {
        Some(lap) => samples[lap.start],
        None => samples[laps[i].end - 1],
    };
    (first_t, first_dist, next.t, next.dist)
}

/// Whether a sample's heart rate counts: `p.bpm > 0 && p.conf >= minConf`.
fn hr_in(s: &Sample, min_conf: f64) -> bool {
    s.bpm > 0.0 && s.conf >= min_conf
}

/// The average (as `reduce` sums, from 0) and `Math.max` of some readings.
fn avg_max(hrs: &[f64]) -> (f64, f64) {
    let avg = hrs.iter().fold(0.0, |a, b| a + b) / hrs.len() as f64;
    let max = hrs.iter().fold(f64::NEG_INFINITY, |a, &b| js_max(a, b));
    (avg, max)
}

/// The log as TCX, one lap per level: `beeplog.js` `toTCX(log,
/// {minConfidence})`. Heart rate readings below `min_confidence` (0 to 100;
/// the page offers 50, "leave out low-confidence readings") are left out.
/// An error, "Log has no samples", when there are none, and "Invalid time
/// value" (JavaScript's) when a time is outside what a `Date` can hold.
pub fn to_tcx(log: &Log, min_confidence: f64) -> Result<String, String> {
    let min_conf = or(min_confidence, 0.0);
    let samples = &log.samples;
    if samples.is_empty() {
        return Err("Log has no samples".into());
    }
    let start = log.start_ms;
    let id = iso(start)?;
    let laps = laps(samples);

    let mut x: Vec<String> = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.into(),
        r#"<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:ns3="http://www.garmin.com/xmlschemas/ActivityExtension/v2" xsi:schemaLocation="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2 http://www.garmin.com/xmlschemas/TrainingCenterDatabasev2.xsd">"#.into(),
        " <Activities>".into(),
        "  <Activity Sport=\"Running\">".into(),
        format!("   <Id>{id}</Id>"),
    ];
    for (i, lap) in laps.iter().enumerate() {
        let pts = &samples[lap.clone()];
        let (lap_start_t, first_dist, lap_end_t, next_dist) = lap_ends(samples, &laps, i);
        let lap_dist = next_dist - first_dist;
        let hrs: Vec<f64> = pts
            .iter()
            .filter(|p| hr_in(p, min_conf))
            .map(|p| p.bpm)
            .collect();
        // pace is constant within a level
        let max_speed = if lap_end_t > lap_start_t {
            lap_dist / (lap_end_t - lap_start_t)
        } else {
            0.0
        };
        x.push(format!(
            "   <Lap StartTime=\"{}\">",
            iso(start + lap_start_t * 1000.0)?
        ));
        x.push(format!(
            "    <TotalTimeSeconds>{}</TotalTimeSeconds>",
            to_fixed(js_max(0.0, lap_end_t - lap_start_t), 1)
        ));
        x.push(format!(
            "    <DistanceMeters>{}</DistanceMeters>",
            to_fixed(js_max(0.0, lap_dist), 1)
        ));
        x.push(format!(
            "    <MaximumSpeed>{}</MaximumSpeed>",
            to_fixed(max_speed, 2)
        ));
        x.push("    <Calories>0</Calories>".into());
        if !hrs.is_empty() {
            let (avg, max) = avg_max(&hrs);
            x.push(format!(
                "    <AverageHeartRateBpm><Value>{}</Value></AverageHeartRateBpm>",
                js_number(js_round(avg))
            ));
            x.push(format!(
                "    <MaximumHeartRateBpm><Value>{}</Value></MaximumHeartRateBpm>",
                js_number(max)
            ));
        }
        x.push("    <Intensity>Active</Intensity>".into());
        x.push("    <TriggerMethod>Distance</TriggerMethod>".into());
        x.push("    <Track>".into());
        for p in pts {
            x.push("     <Trackpoint>".into());
            x.push(format!("      <Time>{}</Time>", iso(start + p.t * 1000.0)?));
            if !p.lat.is_nan() && !p.lon.is_nan() {
                x.push(format!(
                    "      <Position><LatitudeDegrees>{}</LatitudeDegrees><LongitudeDegrees>{}</LongitudeDegrees></Position>",
                    js_number(p.lat),
                    js_number(p.lon)
                ));
            }
            x.push(format!(
                "      <DistanceMeters>{}</DistanceMeters>",
                to_fixed(p.dist, 1)
            ));
            if hr_in(p, min_conf) {
                x.push(format!(
                    "      <HeartRateBpm><Value>{}</Value></HeartRateBpm>",
                    js_number(js_round(p.bpm))
                ));
            }
            // GPS speed, m/s
            if !p.gps_speed.is_nan() {
                x.push(format!(
                    "      <Extensions><ns3:TPX><ns3:Speed>{}</ns3:Speed></ns3:TPX></Extensions>",
                    to_fixed(p.gps_speed / 3.6, 2)
                ));
            }
            x.push("     </Trackpoint>".into());
        }
        x.push("    </Track>".into());
        x.push(format!(
            "    <Notes>Level {}</Notes>",
            js_number(samples[lap.start].level)
        ));
        x.push("   </Lap>".into());
    }
    x.push(format!("   <Notes>{}</Notes>", esc(&result_text(log))));
    x.push(
        "   <Creator xsi:type=\"Device_t\"><Name>Bangle.js Beep Test</Name><UnitId>0</UnitId>\
         <ProductID>0</ProductID><Version><VersionMajor>0</VersionMajor>\
         <VersionMinor>1</VersionMinor></Version></Creator>"
            .into(),
    );
    x.push("  </Activity>".into());
    x.push(" </Activities>".into());
    x.push("</TrainingCenterDatabase>".into());
    Ok(x.join("\n") + "\n")
}

/// The log as CSV, a row a sample: `beeplog.js` `toCSV`. An error, "Invalid
/// time value", when a sample's time is outside what a `Date` can hold
/// (where `beeplog.js` throws).
pub fn to_csv(log: &Log) -> Result<String, String> {
    let mut rows = vec![
        "time,elapsed_s,heart_rate_bpm,hr_confidence,level,shuttle,distance_m,\
         gps_speed_kmh,latitude,longitude"
            .to_string(),
    ];
    let or_empty = |v: f64| {
        if v.is_nan() {
            String::new()
        } else {
            js_number(v)
        }
    };
    for s in &log.samples {
        rows.push(
            [
                iso(log.start_ms + s.t * 1000.0)?,
                js_number(s.t),
                js_number(s.bpm),
                js_number(s.conf),
                js_number(s.level),
                js_number(s.shuttle),
                js_number(s.dist),
                or_empty(s.gps_speed),
                or_empty(s.lat),
                or_empty(s.lon),
            ]
            .join(","),
        );
    }
    Ok(rows.join("\n") + "\n")
}

/// What to call the exported files, without the extension:
/// "beeptest-20m-2026-11-28-14-05" in the watch's time zone at the start,
/// as `beeplog.js` `fileBase`. An error, "Invalid time value", for a start
/// outside what a `Date` can hold.
pub fn file_base(log: &Log) -> Result<String, String> {
    // local time for the name
    let d = date_iso(log.start_ms - log.tz_offset * 60000.0).ok_or_else(invalid)?;
    let stamp: String = d
        .chars()
        .take(16)
        .map(|c| if c == ':' || c == 'T' { '-' } else { c })
        .collect();
    Ok(format!("beeptest-{}m-{stamp}", js_number(log.course)))
}

/// What JavaScript's `Date` throws for a time it can't hold.
fn invalid() -> String {
    "Invalid time value".into()
}

/// `beeplog.js` `iso`: `new Date(Math.round(ms)).toISOString()` without the
/// milliseconds.
fn iso(ms: f64) -> Result<String, String> {
    let s = date_iso(js_round(ms)).ok_or_else(invalid)?;
    Ok(format!("{}Z", &s[..s.len() - 5]))
}

/// `new Date(ms).toISOString()`: "2026-11-28T14:05:12.345Z", the year as
/// `+010000` or `-000001` outside 0 to 9999. `None` where JavaScript throws
/// (NaN, or more than 8.64e15 ms either side of 1970).
fn date_iso(ms: f64) -> Option<String> {
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return None;
    }
    // TimeClip: towards zero, to a whole ms.
    let ms = ms.trunc() as i64;
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    let year = if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else {
        format!("{}{:06}", if y < 0 { '-' } else { '+' }, y.abs())
    };
    Some(format!(
        "{year}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    ))
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `beeplog.js` `esc`.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JavaScript's `Math.round`: the nearest whole number, a half going up.
fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}

/// JavaScript's `Math.max(a, b)`: NaN if either is, and +0 above -0.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else {
        a.max(b)
    }
}

/// JavaScript's `x.toFixed(digits)`, for 0 to 6 digits: the exact value of
/// `x` rounded to `digits` places, a half going up (away from zero: the
/// sign is set aside first, so -0.04 gives "-0.0"); "NaN"; and
/// [`js_number`] from 1e21 up.
fn to_fixed(x: f64, digits: u32) -> String {
    let digits = digits.min(6);
    if x.is_nan() {
        return "NaN".into();
    }
    let (sign, x) = if x < 0.0 { ("-", -x) } else { ("", x) };
    if x >= 1e21 {
        return format!("{sign}{}", js_number(x));
    }
    // x is mant * 2^exp exactly; n is x * 10^digits rounded.
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, exp) = if raw_exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), raw_exp - 1075)
    };
    let scale = 10u128.pow(digits);
    // Below 1e21 the whole part has at most 70 bits and 10^6 takes 20 more,
    // so everything fits in a u128.
    let n: u128 = if exp >= 0 {
        (u128::from(mant) << exp) * scale
    } else {
        let shift = (-exp) as u32;
        let num = u128::from(mant);
        if shift >= 128 {
            // Below 2^-75: rounds to 0 at any of these places.
            0
        } else {
            let whole = num >> shift;
            let rem = num & ((1u128 << shift) - 1);
            // rem < 2^53, so rem * scale < 2^73.
            let prod = rem * scale;
            let q = prod >> shift;
            let r = prod & ((1u128 << shift) - 1);
            let up = r >= 1u128 << (shift - 1);
            whole * scale + q + u128::from(up)
        }
    };
    let s = n.to_string();
    if digits == 0 {
        return format!("{sign}{s}");
    }
    let d = digits as usize;
    let s = if s.len() <= d {
        format!("{}{s}", "0".repeat(d + 1 - s.len()))
    } else {
        s
    };
    let (whole, part) = s.split_at(s.len() - d);
    format!("{sign}{whole}.{part}")
}

/// `v` as JavaScript's `String(v)` writes it: "12", "12.5", "0.1", "NaN",
/// "1e+21". What `beeplog.js` gets when it joins or adds a number to a
/// string without `toFixed`. The digits are the fewest that read back as
/// `v` (as many as Rust's `{:e}` gives) and, of those, the closest to `v`,
/// the even one on a tie, as ECMAScript's Number::toString picks them
/// (Rust's `{:e}` rounds a tie up). They're laid out as it lays them out.
pub fn js_number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v == 0.0 {
        return "0".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v < 0.0 {
        return format!("-{}", js_number(-v));
    }
    let shortest = format!("{v:e}");
    let k = shortest
        .split('e')
        .next()
        .unwrap_or("")
        .bytes()
        .filter(u8::is_ascii_digit)
        .count();
    // The exact value rounded to k digits, half to even.
    let nearest = format!("{v:.*e}", k.saturating_sub(1));
    let e = if nearest.parse::<f64>() == Ok(v) {
        nearest
    } else {
        shortest
    };
    let (mantissa, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let k = digits.len() as i32;
    // v = 0.digits * 10^n
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(n as usize);
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    }
}

/// What JavaScript's `+s` (`Number(s)`) makes of a string: surrounding
/// white space trimmed, "" is 0, "Infinity" with or without a sign, `0x`,
/// `0o` and `0b` integers, decimals such as `1`, `1.`, `.5`, `-1e3`; NaN
/// for anything else (Rust's own "inf" and "nan" too).
pub fn js_to_number(s: &str) -> f64 {
    let s = s.trim_matches(is_js_space);
    if s.is_empty() {
        return 0.0;
    }
    let b = s.as_bytes();
    if b.len() > 1 && b[0] == b'0' {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            return radix_number(&s[2..], radix);
        }
    }
    let (sign, rest) = match b[0] {
        b'+' => (1.0, &s[1..]),
        b'-' => (-1.0, &s[1..]),
        _ => (1.0, s),
    };
    if rest == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal(rest) {
        return f64::NAN;
    }
    // What is_decimal lets through is Rust's grammar too, and Rust rounds
    // to the nearest double as JavaScript does.
    s.parse().unwrap_or(f64::NAN)
}

/// ECMAScript's StrWhiteSpaceChar: its white space and line terminators.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `digits ['.' digits] [e [sign] digits]` with a digit before or after the
/// point: ECMAScript's StrUnsignedDecimalLiteral, less Infinity.
fn is_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let from = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - from
    };
    let mut n = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        i += 1;
        n += digits(&mut i);
    }
    if n == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// A `0x`, `0o` or `0b` integer's digits as JavaScript reads them: NaN
/// unless there is one and every one is a digit of `radix`; the value
/// rounded to the nearest double.
fn radix_number(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let bits_per = radix.trailing_zeros();
    let mut value: u128 = 0;
    // Past 120 bits, later digits only scale the value, and whether any of
    // them is set is all the rounding needs (a double keeps 53).
    let mut dropped = 0i32;
    let mut sticky = false;
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else {
            return f64::NAN;
        };
        if 128 - value.leading_zeros() > 120 {
            dropped = dropped.saturating_add(bits_per as i32);
            sticky |= d != 0;
        } else {
            value = (value << bits_per) | u128::from(d);
        }
    }
    (value | u128::from(sticky)) as f64 * 2f64.powi(dropped)
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
    let min_conf = or(min_confidence, 0.0);
    let samples = &log.samples;
    let laps = laps(samples);
    (0..laps.len())
        .map(|i| {
            let pts = &samples[laps[i].clone()];
            let (start_s, _, end_s, _) = lap_ends(samples, &laps, i);
            let hrs: Vec<f64> = pts
                .iter()
                .filter(|p| hr_in(p, min_conf))
                .map(|p| p.bpm)
                .collect();
            let (avg_hr, max_hr) = if hrs.is_empty() {
                (None, None)
            } else {
                let (avg, max) = avg_max(&hrs);
                (Some(js_round(avg) as u32), Some(js_round(max) as u32))
            };
            let speeds: Vec<f64> = pts
                .iter()
                .map(|p| p.gps_speed)
                .filter(|v| !v.is_nan())
                .collect();
            let gps_kmh = (!speeds.is_empty())
                .then(|| speeds.iter().fold(0.0, |a, b| a + b) / speeds.len() as f64);
            LevelRow {
                level: samples[laps[i].start].level as u32,
                start_s,
                end_s,
                avg_hr,
                max_hr,
                gps_kmh,
            }
        })
        .collect()
}

/// The heart rate over the test, for the chart: `(seconds since the start,
/// bpm)` for every sample with bpm above 0 and confidence at least
/// `min_confidence`, in time order.
pub fn hr_series(log: &Log, min_confidence: f64) -> Vec<(f64, f64)> {
    let min_conf = or(min_confidence, 0.0);
    log.samples
        .iter()
        .filter(|s| hr_in(s, min_conf))
        .map(|s| (s.t, s.bpm))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // The ties below are written out exactly: that they're ties is the point.
    #[allow(clippy::excessive_precision)]
    fn numbers_print_as_javascript_prints_them() {
        let cases: [(f64, &str); 21] = [
            (12.0, "12"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "0"),
            (-3.25, "-3.25"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (51.501234, "51.501234"),
            // Two shortest candidates as close as each other: the even one.
            (203424739792675.125, "203424739792675.12"),
            (598558904416188.25, "598558904416188.2"),
            (2883437877377.03125, "2883437877377.0312"),
        ];
        for (v, want) in cases {
            assert_eq!(js_number(v), want, "{v:?}");
        }
    }

    #[test]
    fn to_fixed_rounds_the_exact_value_halves_up() {
        let cases: [(f64, u32, &str); 14] = [
            (1.25, 1, "1.3"),
            (0.25, 1, "0.3"),
            (2.5, 0, "3"),
            (1.005, 2, "1.00"),
            (0.05, 1, "0.1"),
            (1.45, 1, "1.4"),
            (-0.04, 1, "-0.0"),
            (-1.25, 1, "-1.3"),
            (0.0, 2, "0.00"),
            (-0.0, 1, "0.0"),
            (123.456, 2, "123.46"),
            (1e21, 1, "1e+21"),
            (f64::NAN, 1, "NaN"),
            (f64::NEG_INFINITY, 2, "-Infinity"),
        ];
        for (v, d, want) in cases {
            assert_eq!(to_fixed(v, d), want, "{v:?}.toFixed({d})");
        }
        assert_eq!(to_fixed(5e-324, 2), "0.00");
        assert_eq!(
            to_fixed(999999999999999900000.0, 1),
            "999999999999999868928.0"
        );
    }

    #[test]
    fn fields_read_as_plus_reads_them() {
        let cases: [(&str, f64); 16] = [
            ("12", 12.0),
            (" 12\t", 12.0),
            ("", 0.0),
            ("  ", 0.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("-1e3", -1000.0),
            ("1E+2", 100.0),
            ("0x1F", 31.0),
            ("0b101", 5.0),
            ("0o17", 15.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("1e99999", f64::INFINITY),
            ("\u{feff}7\u{2028}", 7.0),
            ("0", 0.0),
        ];
        for (s, want) in cases {
            assert_eq!(js_to_number(s), want, "{s:?}");
        }
        for s in [
            "abc", "inf", "nan", "NaN", "infinity", "1e", ".", "+", "-0x1", "0x", "1_000", "1,2",
            "\u{85}1", "12abc", "e5", "--1",
        ] {
            assert!(js_to_number(s).is_nan(), "{s:?}");
        }
        assert!(js_to_number("-0").is_sign_negative());
        assert_eq!(
            js_to_number("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"),
            2f64.powi(136)
        );
        assert_eq!(js_to_number("0x20000000000001"), 9007199254740992.0);
        assert_eq!(js_to_number("0x20000000000003"), 9007199254740996.0);
    }

    #[test]
    fn dates_as_to_iso_string() {
        assert_eq!(date_iso(0.0).unwrap(), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            date_iso(1_764_334_800_123.0).unwrap(),
            "2025-11-28T13:00:00.123Z"
        );
        assert_eq!(date_iso(-1.0).unwrap(), "1969-12-31T23:59:59.999Z");
        assert_eq!(date_iso(-1.7).unwrap(), "1969-12-31T23:59:59.999Z");
        assert_eq!(date_iso(8.64e15).unwrap(), "+275760-09-13T00:00:00.000Z");
        assert_eq!(date_iso(-8.64e15).unwrap(), "-271821-04-20T00:00:00.000Z");
        assert_eq!(
            date_iso(-62_198_755_200_000.0).unwrap(),
            "-000001-01-01T00:00:00.000Z"
        );
        assert_eq!(
            date_iso(253_402_300_800_000.0).unwrap(),
            "+010000-01-01T00:00:00.000Z"
        );
        assert_eq!(
            date_iso(951_782_400_000.0).unwrap(),
            "2000-02-29T00:00:00.000Z"
        );
        assert_eq!(date_iso(8.64e15 + 1.0), None);
        assert_eq!(date_iso(f64::NAN), None);
        assert_eq!(iso(1499.5).unwrap(), "1970-01-01T00:00:01Z");
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }

    const LOG: &str = "start,1781344800000,-60,20\n\
        0.0,100,20,1,0,0.0\n\
        5.0,110,95,1,0,11.1\n\
        9.0,120,95,1,1,20.0\n\
        63.0,140,95,2,0,140.0\n\
        70.0,150,40,2,0,157.5\n\
        71.0,0,0,2,1,160.0\n\
        end,71.0,2,1,160,150,130\n\
        72.0,155,95,2,1,165.0\n";

    #[test]
    fn reads_a_log() {
        let log = parse(LOG);
        assert_eq!(log.start_ms, 1_781_344_800_000.0);
        assert_eq!(log.tz_offset, -60.0);
        assert_eq!(log.course, 20.0);
        // The sample after the end is left out.
        assert_eq!(log.samples.len(), 6);
        assert!(log.samples[0].gps_speed.is_nan());
        assert_eq!(log.end.unwrap().level, 2.0);
        assert_eq!(result_text(&log), "20m beep test result 2.1, 160 m");
        assert_eq!(file_base(&log).unwrap(), "beeptest-20m-2026-06-13-11-00");
        let empty = parse("");
        assert_eq!(empty.course, 20.0);
        assert_eq!(result_text(&empty), "20m beep test (incomplete log)");
        assert_eq!(to_tcx(&empty, 0.0), Err("Log has no samples".into()));
        assert_eq!(
            to_csv(&empty).unwrap(),
            "time,elapsed_s,heart_rate_bpm,hr_confidence,level,shuttle,distance_m,gps_speed_kmh,latitude,longitude\n"
        );
        // Clock lines move the start; CRLF and junk lines are fine.
        let log = parse("start,1000,0,15\r\nclock,2.5\r\n\r\nhello\n1,2,3\nclock,-0.5\n");
        assert_eq!(log.start_ms, 3000.0);
        assert_eq!(log.course, 15.0);
        assert!(log.samples.is_empty());
        // The speed progression after the course: read, and nothing else
        // changes (beeplog.js ignores it). Without one, or one the phone
        // doesn't know, the standard speeds.
        assert_eq!(parse(LOG).progression, 0);
        let with = LOG.replacen("-60,20\n", "-60,20,2\n", 1);
        let log = parse(&with);
        assert_eq!(log.progression, 2);
        // (Debug, since a NaN isn't equal to itself.)
        assert_eq!(
            format!(
                "{:?}",
                Log {
                    progression: 0,
                    ..log.clone()
                }
            ),
            format!("{:?}", parse(LOG))
        );
        assert_eq!(to_tcx(&log, 0.0), to_tcx(&parse(LOG), 0.0));
        assert_eq!(to_csv(&log), to_csv(&parse(LOG)));
        for odd in ["3", "1.5", "x", "", "-1", "0x1"] {
            let p = parse(&format!("start,0,0,20,{odd}\n")).progression;
            assert_eq!(p, if odd == "0x1" { 1 } else { 0 }, "{odd:?}");
        }
        // A start a Date can't hold.
        let bad = parse("start,abc\n0,1,2,3,4,5\n");
        assert_eq!(to_tcx(&bad, 0.0), Err("Invalid time value".into()));
        assert_eq!(to_csv(&bad), Err("Invalid time value".into()));
        assert_eq!(file_base(&bad), Err("Invalid time value".into()));
    }

    #[test]
    fn rows_per_level_and_the_heart_rate() {
        let log = parse(LOG);
        let rows = level_rows(&log, 0.0);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            LevelRow {
                level: 1,
                start_s: 0.0,
                end_s: 63.0,
                avg_hr: Some(110),
                max_hr: Some(120),
                gps_kmh: None,
            }
        );
        assert_eq!((rows[1].start_s, rows[1].end_s), (63.0, 71.0));
        assert_eq!((rows[1].avg_hr, rows[1].max_hr), (Some(145), Some(150)));
        // Leaving out low confidence: the first reading (20%) and 70 s (40%).
        let rows = level_rows(&log, 50.0);
        assert_eq!((rows[0].avg_hr, rows[0].max_hr), (Some(115), Some(120)));
        assert_eq!((rows[1].avg_hr, rows[1].max_hr), (Some(140), Some(140)));
        assert_eq!(hr_series(&log, 0.0).len(), 5);
        assert_eq!(
            hr_series(&log, 50.0),
            vec![(5.0, 110.0), (9.0, 120.0), (63.0, 140.0)]
        );
        assert!(level_rows(&parse(""), 0.0).is_empty());
        let gps = parse(
            "start,0,0,20\n0,100,90,1,0,0,9.0,51.5,-0.1\n1,100,90,1,0,2,,,\n2,100,90,1,0,4,11.0,51.5,-0.1\n",
        );
        assert_eq!(level_rows(&gps, 0.0)[0].gps_kmh, Some(10.0));
    }
}
