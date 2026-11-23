# Bleep Test

The 20 metre multistage fitness test ("bleep test" / "beep test") for Bangle.js,
with heart rate logging you can export to your phone or fitness service.

## Usage

1. Mark out two lines 20 m apart.
2. Open **Bleep Test** and choose **Start test**. You get a 5 second countdown
   (press the button to cancel).
3. Run to the other line before each cue:
   * **1 buzz**: end of a shuttle, turn and go.
   * **3 buzzes**: new level, the pace gets faster.
4. When you can no longer reach the line in time, **press the button**. The test stops
   and your result is shown as `level.shuttle` (the last shuttle you completed),
   along with total distance, an estimated VO2max and average / max heart rate.
5. Past results are under **Results** on the watch, where you can also delete them.

Bangle.js 2 has no speaker, so cues are vibrations. On Bangle.js 1 the piezo
beeper sounds as well.

The screen stays on and unlocked for the whole test so a press of the button always
stops it. Your normal timeout settings are restored afterwards.

## Timings

Standard 20 m shuttle run table: level 1 is 8.0 km/h, level 2 is 9.0 km/h, then each
level adds 0.5 km/h up to level 21 (18.5 km/h). Each level lasts about a minute:

| Level | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Shuttles | 7 | 8 | 8 | 9 | 9 | 10 | 10 | 11 | 11 | 11 | 12 | 12 | 13 | 13 | 13 | 14 | 14 | 15 | 15 | 16 | 16 |

Cue times are scheduled from the test start time, so they do not drift.

VO2max is estimated with `3.46 × (level + shuttle / (0.4325 × level + 7.0048)) + 12.2`
(Flouris et al., 2005). Treat it as a rough guide.

## Heart rate log and export

While the test runs, the watch writes one line a second to a `bleep.<time>.csv` file
with elapsed time, heart rate, HRM confidence, level, shuttle and distance.

To get it off the watch, connect in the [Bangle.js App Loader](https://banglejs.com/apps/),
find **Bleep Test** under *My Apps* and click the **download (interface) icon**. Each test
can be downloaded as:

* **TCX** (Garmin Training Center XML): a standard activity format with time, distance
  and heart rate, split into one lap per level. Upload it to Strava (*Upload activity → File*),
  Garmin Connect (*Import data*), Runalyze, Intervals.icu, Golden Cheetah, or open it in
  most phone fitness apps. There is no GPS, so it shows as an indoor run.
* **CSV** for spreadsheets.

There is an option to drop low-confidence HR readings. The wrist HRM struggles with the
arm movement of hard running; for better data pair a chest strap with the
[BTHRM](https://banglejs.com/apps/?id=bthrm) app, which this app picks up automatically.

Make sure the watch's clock is set (the App Loader sets it when you connect) so the
exported activity has the right date.
