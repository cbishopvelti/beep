# Emulator tests

Headless Bangle.js 2 emulator tests for Beep Test, run in Node 22 or later
(the sync tests check CRCs with `zlib.crc32`).

## Setup

The tests need EspruinoWebIDE and BangleApps (with its Android Integration
app) checked out side by side.
`tests/setup-emulator.sh` does that in `$BANGLE_DIR` (default `/opt/bangle`):

```sh
BANGLE_DIR=~/bangle sh tests/setup-emulator.sh
```

## Running

```sh
npm test                                   # all tests/*.test.js
node tests/run.js tests/engine.test.js -v  # one file, with the watch's console
node tests/run.js --grep=cancel            # tests whose name contains "cancel"
BEEP_TEST_TIME=2026-06-15T23:58:00Z npm test   # run at another time of day
```

The whole suite takes about 15 seconds.

Set `BANGLE_DIR` if the emulator isn't in `/opt/bangle`. GitHub Actions runs
the same on every pull request (`.github/workflows/test.yml`).

## What's where

| File | What it tests |
| --- | --- |
| `engine.test.js` | The test engine (`lib.js`): timings, cues, results, resuming after an app change, cancel, heart rate and GPS logging. |
| `ui.test.js` | The screens and menus (`app.js`): tap to start, the menus before and during a test, stop and cancel, the result screen. |
| `widget.test.js` | The clock widget (`widget.js`). |
| `export.test.js` | The App Loader's export (`beeplog.js`): a log the watch wrote, as TCX and CSV. |
| `install.test.js` | `metadata.json` for the App Loader, and the Web IDE installer (`npm run bundle`). |
| `sync.test.js` | The sync with the phone app (`sync.js` and the boot file's `GB()`), message by message, as `docs/DEVELOPMENT.md` "The watch link" lists them. |
| `console.test.js` | That nothing Beep Test sends or gets makes Android Integration print to the console, where Gadgetbridge would pop up "Malformed JSON". |
| `fixtures.test.js` | The files shared with the phone app's tests (below). |

The phone app (`android/`) has its own tests in Rust: `cd android && cargo
test` (see [android/README.md](../android/README.md)).

## Shared fixtures

`tests/fixtures/` holds files the phone app's tests read too, so a change
to the watch link on one side that the other doesn't expect fails a test
(the table is in `docs/DEVELOPMENT.md`, "The watch link"):

- `watch-lines.txt` is what the watch prints for the scripted exchange in
  `fixtures.test.js`. After a change to what the watch sends (or a new
  version in `metadata.json`), `BEEP_BLESS=1 node tests/run.js
  tests/fixtures.test.js` writes it again; then run the phone app's
  `cargo test --test fixtures`, which reads it.
- `phone-lines.txt` is lines exactly as the phone app builds them, which
  `fixtures.test.js` checks the watch reads. The phone app's side writes
  it: `cd android && BEEP_BLESS=1 cargo test --test fixtures`.
- `beeplog/` is logs and what `beeplog.js` makes of them, for the phone
  app's port of it. `node tests/fixtures/beeplog/make.js` makes them again
  after a change to `beeplog.js`; then run `cargo test --test beeplog` in
  `android/`.

## Writing tests

```js
const { test, assert } = require("./harness");
const { BT, startAgo, timeAt } = require("./helpers");

test("what it checks", async h => {
  startAgo(h, 20, timeAt(h, 20, 7, 4) + 2);         // a 20m test, 2 s into shuttle 5 of level 7
  assert.eq(h.eval(`${BT}.status().level`), 7);     // run code on the watch and get the value
  await h.load("beeptest.app.js");                 // load an app and wait for it
  await h.wait(1500);                               // let 1.5 s pass on the watch
});
```

- Each test starts from a freshly booted watch with Beep Test installed and
  its boot code run (`load()`). The harness boots once, snapshots the watch's
  RAM and flash, and restores that for every test.
- The watch's clock is virtual (`tests/emulator.js`). It stands still while
  the watch runs a command and only moves in `h.wait(ms)`, which runs the
  watch's timers as they fall due, so a 60 s wait takes no real time and every
  run sees exactly the same times. Every test starts at 10:00:00 UTC on
  15 June 2026 (plus the boot). `BEEP_TEST_TIME` (any time `Date.parse`
  reads) starts it elsewhere.
- `h.eval(js)` returns the value (through JSON, so `undefined` comes back as
  `null`). `h.tx(js)` ignores it.
- `h.load()` with no file loads the clock, which is how to simulate a long
  press or leaving the app.
- To test far into a test, start it in the past (`startAgo` in
  `tests/helpers.js`, which passes `start` to `require("beeptest").start`)
  rather than waiting. Don't use `setTime()` on the watch: the clock is the
  harness's.
- There's no GPS or heart rate sensor in the emulator. `GPS_USERS` and
  `HRM_USERS` record who asked for power (`GPS_USERS.beeptest == 1` while a
  test with GPS speed runs). Feed readings with
  `Bangle.emit("HRM", {bpm, confidence})` and
  `Bangle.emit("GPS", {fix:1, speed, lat, lon})`.
- The phone: Android Integration (`apps/android` from BangleApps) is
  installed too, as on a watch paired with Gadgetbridge. `await h.connect()`
  and `h.disconnect()` connect and disconnect it (the phone counts as
  connected while Storage has `test.bt`), `await h.gb({t: "beeptest-get",
  f: "beeptest.json"})` sends a message as Gadgetbridge would, and
  `h.sent()` is every message the watch sent the phone since the test
  started (`h.lines()` as printed). `ours(h)` in `tests/helpers.js` is the
  extras of those for the Beep Test phone app, `untilEnd(h, f)` waits for a
  transfer to end, and `makeLog(h, start, m, dur)` writes a past test's log
  the same in every run.
- A test that can't run here calls `skip(why)` (from `./harness`): it is
  listed as skipped, not passed or failed.
- `tests/helpers.js` has spies for the menu, drawing and buzzes, and presses
  the button by running the app's captured BTN1 watch (`captureButton`,
  `press(h, ms)`), and taps the screen (`tap`).
- An uncaught error on the watch fails the test.
