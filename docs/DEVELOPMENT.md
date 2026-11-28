# Developing Beep Test

- User guide (also the App Loader's "Read more"): [README.md](../README.md)
- Tests: `npm test` in a headless emulator, see [tests/README.md](../tests/README.md)

## How it's built

| File | On the watch | What it does |
|---|---|---|
| `lib.js` | `beeptest` (module) | The test engine: level tables, cues, HR/GPS logging, results. The schedule is a pure function of time since the start, so it never drifts and can be resumed. |
| `boot.js` | `beeptest.boot.js` | On every app load, resumes a test in progress from `beeptest.state.json`, so it keeps running at the clock, and passes the phone app's messages to `beeptest.sync`. |
| `sync.js` | `beeptest.sync` (module) | The watch's side of the sync with the Beep Test phone app (The watch link, below). Only loaded when the phone sends something or connects, or there's something to tell it. |
| `widget.js` | `beeptest.wid.js` | Shows `level.shuttle` and heart rate on the clock while a test runs. |
| `app.js` | `beeptest.app.js` | The screens and menus. |
| `interface.html`, `beeplog.js` | (App Loader only) | The download page: turns a log into TCX or CSV. |
| `android/` | (the phone) | The Beep Test phone app: Rust and egui, its own tests, and the Gradle project for the APK. Its files are listed in [android/README.md](../android/README.md). |
| `tests/` | (not installed) | The emulator tests ([tests/README.md](../tests/README.md)), and in `tests/fixtures/` the files both the watch's and the phone's tests read (below). |

Data on the watch: `beeptest.json` (past results, the last 30),
`beeptest.settings.json` (course, GPS), `beeptest.state.json` (the test in
progress), `beeptest.sync.json` (results deleted on the watch, for the phone),
`beeptest.xfer` (only between a `load()` that cut a transfer to the phone off
and the boot code that follows it) and one `beeptest.<start time>.csv` StorageFile per test with a line a second.

## The watch link

The phone app (`android/`) keeps every result and heart rate log, and stays
in sync with the watch: results and logs come off the watch by themselves,
a delete on either side deletes on both, and the course and GPS speed can
be set from the phone. It talks to the watch through Bangle.js Gadgetbridge,
as blink does, or directly over Bluetooth (the same lines both ways).

**Phone to watch.** A line `\x10GB(<json>)\n`, ASCII only (anything outside
0x20..0x7e is written as `\uXXXX`): broadcast as `com.banglejs.uart.tx` with
the extra `line`, which Gadgetbridge passes on, or written to the UART over
the direct link. Android Integration sets up `GB()`, and `beeptest.boot.js`
wraps it (boot files run in name order, so `android` comes first): it takes
every message whose `t` starts with `beeptest-` and passes the rest on.
Android Integration must not see Beep Test's messages: it `console.log`s any
type it has no handler for ("GB Unknown", pretty-printed over several lines),
the console is the Bluetooth link, and Gadgetbridge pops up "Malformed JSON
from Bangle.js" for a printed line that starts with `{`
(`tests/console.test.js` checks the console stays quiet).

| `t` | Fields | What the watch does |
| --- | --- | --- |
| `beeptest-sync` | `del`: log file names of results deleted on the phone (may be absent) | Deletes each named result (its history entry and its log; never the running test's), then answers `state`. |
| `beeptest-get` | `f`: file name, `at`: byte offset to start from (default 0) | Sends that file from `at` as `log` pieces, then `logend`. `f` is a log (`beeptest.<digits>.csv`, a StorageFile) or `beeptest.json` (the history). One transfer at a time: a new `get` replaces one in progress (which ends without a `logend`). While a test is running it answers `busy` instead, since reading storage must not delay the cues, and a test that starts during a transfer stops it with `busy` too. A file that isn't there (or any other name) answers `logend` with `len` `-1`. |
| `beeptest-set` | `m`: 20 or 15, `gps`: true or false (each optional) | Saves them to `beeptest.settings.json` (a bad `m` is ignored), emits `Bangle.emit("beeptest", {type: "settings"})` so an open app shows them, then answers `state`. |

**Watch to phone.** A Gadgetbridge intent message, which Gadgetbridge
broadcasts when Allow Intents is on, sent as blink's watch app does:
`require("android").gbSend(m)` when Android Integration is there, else an
empty line and then the JSON with `Bluetooth.println`.

```json
{"t":"intent","target":"broadcastreceiver","package":"com.cbishopvelti.beep",
 "action":"com.cbishopvelti.beep.WATCH","extra":{"event":"state", ...}}
```

Extras are all strings, as Gadgetbridge puts them on the intent. Nothing
is sent when no phone is connected (`NRF.getSecurityStatus().connected`).

| `event` | Extras | When |
| --- | --- | --- |
| `state` | `v` (the version installed, from `beeptest.info`), `m` (`"20"` or `"15"`), `gps` (`"true"` or `"false"`), `run` (the running test's log file name, or `""`), `hist` (`"<length>:<crc>"` of the history as the phone gets it, `"0:0"` when there's none), `del` (log file names of results deleted on the watch, comma-separated, newest last, at most 30; `""` when none) | The answer to `beeptest-sync` and `beeptest-set`. Also sent unasked when a test starts, ends or is cancelled, when a result is deleted on the watch, when Course or GPS speed is changed on the watch, 3 s after a phone connects (after Android Integration's own hello), and half a second after a `load()` that cut a transfer off (below). |
| `log` | `f`, `at` (where this piece starts in the file), `crc` (`E.CRC32` of `d`), `d` (the piece: up to 1000 bytes of the file, as text) | A piece of the file asked for, in order, one per 50 ms timer tick so the watch stays responsive. |
| `logend` | `f`, `len` (the file's length in bytes, or `-1` if it isn't there) | After the last piece. Also `-1` when the file being sent is deleted (on the watch, or by a sync). A transfer stops with nothing sent when the phone disconnects. |
| `busy` | `f` | A `beeptest-get` while a test is running: nothing is sent; ask again after the next `state` (which comes when the test ends). |

CRCs are `E.CRC32` as an unsigned decimal, the same number as zlib's
`crc32` of the same bytes. The history goes as `JSON.stringify` of
`beeptest.json`, not the file's own bytes: `Storage.writeJSON` leaves the
quotes off keys (`{t:1781344800000,m:20,...}`), which a JSON parser on the
phone refuses. `hist` is the length and CRC of that same text, so the phone
compares it with what it fetched last. A history entry is
`{t (ms), m, l, s, d (m), dur (s), max, avg, f}` (see `lib.js` `stop`), and
results are known by their log's file name `f`.

Deleting a result on the watch (its Results menu, or Delete on the App
Loader's download page) records the log's file name in `beeptest.sync.json`
(`{"del":[...]}`, the last 30), which is what `state`'s `del` reports. A delete
the phone asked for isn't recorded there. The Results menu finds the result
to delete by its file name in the history as it is then, since the phone may
have deleted others while the menu was open.

A `load()` on the watch (back to the clock, a message opening) clears every
timer and `sync.js` with them, so a transfer stops with no `logend`. Its
`E.on("kill")` writes the file's name to `beeptest.xfer` first, and the boot
code that runs next erases it and sends `state` half a second later: that
starts the phone's next round, which asks for the rest from what it has.
Without it, a sync with the phone app closed (nothing ticks there) would wait
for the next test.

The phone's side (when it syncs, how it resumes a log from the size of what
it has, what it checks) is in `android/README.md`.

**Shared fixtures.** Both sides' tests read the same files in
`tests/fixtures/`, so a change to the link on one side that the other
doesn't expect fails a test:

| File | Written by | Read by |
| --- | --- | --- |
| `phone-lines.txt`: one of each line the phone sends, exactly as `android/src/link.rs` builds them | `android/tests/fixtures.rs`, which fails when it's out of date | `tests/fixtures.test.js`: the watch reads every line |
| `watch-lines.txt`: what the watch prints for a scripted exchange, one message a line | `tests/fixtures.test.js`, which fails when the watch prints something else | `android/tests/fixtures.rs`: fed through the phone's link and sync engine, it must leave the results and logs the exchange carried |
| `beeplog/`: logs (`<name>.log`) and what `beeplog.js` makes of them (TCX, TCX without low-confidence readings, CSV, and `expected.json`) | `node tests/fixtures/beeplog/make.js` | `android/tests/beeplog.rs`: the phone's port of `beeplog.js` must give the same, byte for byte |

After a change that's meant, write the file again on the side that
changed, then run the other side's tests:

```sh
# the watch changed what it prints
BEEP_BLESS=1 node tests/run.js tests/fixtures.test.js
(cd android && cargo test --test fixtures)
# the phone changed a line it sends
(cd android && BEEP_BLESS=1 cargo test --test fixtures)
node tests/run.js tests/fixtures.test.js
# beeplog.js changed
node tests/fixtures/beeplog/make.js
(cd android && cargo test --test beeplog)
```

The version in `watch-lines.txt`'s `state` lines is `metadata.json`'s, so
a new watch version means writing it again too.

**Trying it by hand.** With the phone on adb and Gadgetbridge connected:

```sh
# Does the watch answer? (a state intent arrives at the phone app)
adb shell "am broadcast -a com.banglejs.uart.tx --es line 'GB({\"t\":\"beeptest-sync\"})'"
# Does the phone app hear the watch? (as if the watch said state, with no history)
adb shell "am broadcast -a com.cbishopvelti.beep.WATCH -p com.cbishopvelti.beep --es event state \
  --es v 0.05 --es m 20 --es gps false --es run '' --es hist 0:0 --es del ''"
# What the phone app made of it (a refused event says why)
adb logcat -s beep
```

On the watch, with the Web IDE connected instead,
`GB({t:"beeptest-get",f:"beeptest.json"})` prints the history's pieces in
the console.

## Installing development builds

**One click (easiest).** Open
[cbishopvelti.github.io/beep](https://cbishopvelti.github.io/beep/) in
Chrome or Edge, press **Connect**, pick the watch and press the install
(or update) button on Beep Test. It's the normal Bangle.js App Loader with
only Beep Test on it, rebuilt from main on every push by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml)
(`sh tools/build-loader.sh` builds the same thing locally into `_site/`).
The download page for the heart rate logs is there too: the download icon
next to Beep Test under *My Apps*.

The first time, GitHub Pages has to be switched on: in the repository's
Settings > Pages, set Source to **GitHub Actions**.

**Trying a pull request before it's merged.** Every open PR from a branch in
this repository gets its own install page at
`cbishopvelti.github.io/beep/pr-<number>/`, with a yellow banner saying
which PR it is. A bot comment on the PR links to it and is kept up to date.
The preview's version is main's plus a fourth number (`0.01.12.3` is 3 PR
commits on top of main's `0.01.12`), so the App Loader offers it as an
update, and main's next build after the merge as an update again. To go back
without merging, open the main page and press reinstall on Beep Test.
Previews rebuild on every push to the PR or to main and disappear when the
PR closes.

**App Loader from files.** Download this repository (Code, then Download ZIP)
and unzip it. Open the [App Loader](https://banglejs.com/apps/) in Chrome or
Edge and connect to the watch. On the **More...** tab, press **Install App from
Files** and select every file in the folder (it only takes the ones
`metadata.json` lists).

**Espruino Web IDE.** Run `npm run bundle` (Node 18 or later, no packages
needed), or download `beeptest-install` from the latest
[Emulator tests run](https://github.com/cbishopvelti/beep/actions/workflows/test.yml) on GitHub. Open the
[Web IDE](https://www.espruino.com/ide/), connect to the watch, paste
`dist/beeptest-install.js` into the editor on the right and press **Send to
Espruino**. It writes the files, adds Beep Test to the launcher and restarts
the clock. Sending it again updates the app. (This route has no download
page for the logs: use one of the App Loaders for that.)

## Testing on the watch

The emulator tests cover the logic, but these need the real watch. To get
partway into a test without running it, connect the Web IDE and type a line
from the table in the console (left side). It starts a test that began a
while ago.

| To test | Type |
|---|---|
| Level 2 starts (2 buzzes) in 10 s | `require("beeptest").start(20,{start:Date.now()-53000})` |
| Level 8 starts in 10 s, 20m | `require("beeptest").start(20,{start:Date.now()-432000})` |
| The end of level 21 (test complete) in 10 s | `require("beeptest").start(20,{start:Date.now()-1311000})` |
| 15m course, with GPS speed | `require("beeptest").start(15,{gps:true})` |

`require("beeptest").cancel()` throws away a test started this way. Disconnect the Web IDE
before a check that needs the watch on your wrist.

1. **Install.** Beep Test is in the launcher with its icon. Opening it shows
   the course and "Tap to start". No widget on the clock while idle.
2. **Start.** Press the button on the start screen: a menu opens and nothing
   buzzes. Back out, then tap the screen: a 3 s countdown, two buzzes and the
   test starts.
3. **Cues.** With the watch on your wrist, the single shuttle buzz and the
   double level buzz are easy to tell apart without looking.
4. **Long press.** During a test hold the button about 1.5 s: the watch goes
   to the clock, the widget shows `level.shuttle` and heart rate, and the buzzes
   carry on. Unlock the watch and tap the widget: the test screen comes back. A short press (under 1 s) opens the menu instead.
5. **Stop & save.** Press the button, wait a few seconds, choose Stop & save:
   the result is the last shuttle completed when you pressed the button.
6. **Cancel test.** Asks first, then goes back to the start screen with no result.
7. **Export.** In the App Loader, the download icon lists the test; TCX
   imports into Strava or Garmin Connect with heart rate and one lap per level.
8. **GPS speed.** Outdoors, with GPS speed on, the bottom row shows a speed
   once there's a fix, and GPS goes off when the test ends.
9. **Phone app.** With the Beep Test phone app installed and Gadgetbridge
   connected (Allow Intents on), finish a test: it shows on the phone, with
   its heart rate, shortly after. Delete it on the watch (Results): it goes
   from the phone too. Change the course on the phone's Watch page: the
   watch's start screen shows it.

## Releasing to the Bangle.js App Loader

To list Beep Test in the official App Loader (banglejs.com/apps), it goes
into the [espruino/BangleApps](https://github.com/espruino/BangleApps)
repository as `apps/beeptest/`. This repository stays the place it is
developed; a release copies the app's files across.

1. Bump `version` in `metadata.json` (two numbers, such as `0.02`) and add a
   line for it at the end of `ChangeLog`, in the same `0.02: what changed`
   form. The App Loader shows that line as the release notes.
2. If the screens changed, `npm run screenshots` retakes the pictures in
   `screenshots/` in the emulator.
3. `npm run store-check` runs BangleApps' own checks (`bin/sanitycheck.js`
   and its ESLint rules, which allow no warnings) on the app. CI runs it on
   every pull request too.
4. In a fork of BangleApps, on a new branch from its `master`, copy the
   files in: `node tools/app-files.js <fork>/apps/beeptest` (it copies only
   what `metadata.json` uses, plus `ChangeLog`, `README.md`, the screenshots
   and the download page). Commit with an `AI-assisted` trailer if AI helped
   (their `AGENTS.md` asks for it, without naming the AI service), push, and
   open a pull request to `espruino/BangleApps` `master`.
5. Once it's merged it appears at
   [espruino.github.io/BangleApps](https://espruino.github.io/BangleApps/)
   (the development App Loader) straight away, and at
   [banglejs.com/apps](https://banglejs.com/apps/) when that is next
   refreshed.

Code in BangleApps is MIT licensed: submitting it there means agreeing to
that for the app.

The install page here (`cbishopvelti.github.io/beep`) keeps publishing
main as `<version>.<build>` (`0.01.12`), which the App Loader sees as newer
than the store's `0.01`, so a watch on development builds stays on them.
