# Emulator tests

Headless Bangle.js 2 emulator tests for Beep Test, run in Node 18 or later.

## Setup

The tests need EspruinoWebIDE and BangleApps checked out side by side.
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

The whole suite takes a few seconds.

Set `BANGLE_DIR` if the emulator isn't in `/opt/bangle`. GitHub Actions runs
the same on every pull request (`.github/workflows/test.yml`).

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
- `tests/helpers.js` has spies for the menu, drawing and buzzes, and presses
  the button by running the app's captured BTN1 watch (`captureButton`,
  `press(h, ms)`), and taps the screen (`tap`).
- An uncaught error on the watch fails the test.
